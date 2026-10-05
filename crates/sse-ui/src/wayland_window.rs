//! Minimal dependency-free Wayland client for the editor surface.
//!
//! It speaks the core Wayland wire protocol over the compositor Unix socket.  The compositor,
//! xdg-shell and wl_shm objects are discovered from wl_registry; pixels live in a memfd mapping.

use crate::event_loop::{Present, Proxy, WindowEvent};
use crate::raster::Rect;
use sse_core::{Error, Result};
use sse_sys::memfd::MappedFile;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const WL_DISPLAY: u32 = 1;
const WL_REGISTRY: u32 = 2;
const WL_SHM_FORMAT_XRGB8888: u32 = 1;
#[derive(Clone, Copy, Default)]
struct Globals {
    compositor: Option<(u32, u32)>,
    shm: Option<(u32, u32)>,
    wm_base: Option<(u32, u32)>,
    seat: Option<(u32, u32)>,
}

#[derive(Clone, Copy)]
struct ReaderObjects {
    xdg_surface: u32,
    toplevel: u32,
    wm: u32,
    seat: Option<u32>,
    surface: u32,
}

/// Native Wayland presenter. Input support currently uses wl_seat discovery and falls back to X11 when the
/// compositor cannot provide the required core objects.
pub struct WaylandWindow {
    stream: UnixStream,
    surface: u32,
    buffer: u32,
    pool: u32,
    memory: MappedFile,
    size: (u32, u32),
    closed: Arc<Mutex<bool>>,
}

impl WaylandWindow {
    /// Opens a top-level xdg-shell surface on the current Wayland display.
    pub fn open<U: Send + 'static>(title: &str, width: u32, height: u32, proxy: Proxy<U>) -> Result<Self> {
        let mut stream = connect()?;
        send(&mut stream, WL_DISPLAY, 1, &u32s(&[WL_REGISTRY]))?; // get_registry
        send(&mut stream, WL_DISPLAY, 0, &u32s(&[3]))?; // sync callback id 3
        let mut globals = Globals::default();
        read_registry_until_done(&mut stream, 3, &mut globals)?;
        let compositor = bind(&mut stream, globals.compositor, "wl_compositor", 4, 4)?;
        let shm = bind(&mut stream, globals.shm, "wl_shm", 1, 5)?;
        let wm = bind(&mut stream, globals.wm_base, "xdg_wm_base", 1, 6)?;
        let seat = if let Some(seat_global) = globals.seat {
            Some(bind(
                &mut stream,
                Some(seat_global),
                "wl_seat",
                seat_global.1.min(7),
                7,
            )?)
        } else {
            None
        };

        let surface = 8;
        send(&mut stream, compositor, 0, &u32s(&[surface]))?;
        let xdg_surface = 9;
        send(&mut stream, wm, 2, &u32s(&[xdg_surface, surface]))?;
        let toplevel = 10;
        send(&mut stream, xdg_surface, 1, &u32s(&[toplevel]))?;
        send_string(&mut stream, toplevel, 2, title)?;
        send_string(&mut stream, toplevel, 3, "stalker-save-editor")?;
        send(&mut stream, surface, 6, &[])?; // initial commit
        stream.flush().map_err(io)?;

        let byte_len = frame_bytes(width, height)?;
        let memory = MappedFile::new(byte_len).map_err(io)?;
        let pool = 11;
        send_with_fd(
            &stream,
            shm,
            0,
            &u32s(&[
                pool,
                u32::try_from(byte_len).map_err(|_| Error::Refused("Wayland buffer too large".to_owned()))?,
            ]),
            memory.raw_fd(),
        )?;
        let buffer = 12;
        let stride = width
            .checked_mul(4)
            .ok_or_else(|| Error::Refused("Wayland stride overflow".to_owned()))?;
        send(
            &mut stream,
            pool,
            0,
            &u32s(&[buffer, 0, width, height, stride, WL_SHM_FORMAT_XRGB8888]),
        )?;
        stream.flush().map_err(io)?;

        let closed = Arc::new(Mutex::new(false));
        let reader = stream.try_clone().map_err(io)?;
        let reader_closed = Arc::clone(&closed);
        std::thread::Builder::new()
            .name("wayland-events".to_owned())
            .spawn(move || event_reader(reader, xdg_surface, toplevel, wm, seat, surface, proxy, reader_closed))
            .map_err(io)?;
        Ok(Self {
            stream,
            surface,
            buffer,
            pool,
            memory,
            size: (width, height),
            closed,
        })
    }
}

impl Present for WaylandWindow {
    fn present(&mut self, frame: &[u32], stride: usize, width: u32, height: u32, rects: &[Rect]) -> Result<()> {
        if rects.is_empty() {
            return Ok(());
        }
        if (width, height) != self.size || stride != usize::try_from(width).unwrap_or(0) {
            return Err(Error::Refused(
                "Wayland resize requires buffer recreation; using XWayland fallback is recommended".to_owned(),
            ));
        }
        self.memory.write_u32_le(frame).map_err(io)?;
        send(&mut self.stream, self.surface, 1, &u32s(&[self.buffer, 0, 0]))?; // attach
        for rect in rects {
            send(
                &mut self.stream,
                self.surface,
                2,
                &i32s(&[
                    rect.x,
                    rect.y,
                    i32::try_from(rect.width).unwrap_or(i32::MAX),
                    i32::try_from(rect.height).unwrap_or(i32::MAX),
                ]),
            )?;
        }
        send(&mut self.stream, self.surface, 6, &[])?;
        self.stream.flush().map_err(io)
    }
}

impl Drop for WaylandWindow {
    fn drop(&mut self) {
        let _ = send(&mut self.stream, self.buffer, 0, &[]);
        let _ = send(&mut self.stream, self.pool, 1, &[]);
        let _ = self.closed.lock().map(|mut value| *value = true);
    }
}

fn connect() -> Result<UnixStream> {
    let runtime =
        std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| Error::Refused("XDG_RUNTIME_DIR is not set".to_owned()))?;
    let display =
        std::env::var_os("WAYLAND_DISPLAY").ok_or_else(|| Error::Refused("WAYLAND_DISPLAY is not set".to_owned()))?;
    let path = PathBuf::from(runtime).join(display);
    UnixStream::connect(path).map_err(io)
}

fn event_reader<U: Send + 'static>(
    mut stream: UnixStream,
    objects: ReaderObjects,
    proxy: Proxy<U>,
    closed: Arc<Mutex<bool>>,
) {
    let ReaderObjects {
        xdg_surface,
        toplevel,
        wm,
        seat,
        surface,
    } = objects;
    const POINTER_ID: u32 = 13;
    const KEYBOARD_ID: u32 = 14;
    const SEAT_CAP_POINTER: u32 = 1;
    const SEAT_CAP_KEYBOARD: u32 = 2;

    let mut pointer = None;
    let mut keyboard = None;
    let mut pointer_position = (0_i32, 0_i32);
    let mut ctrl = false;
    let mut shift = false;

    loop {
        if closed.lock().map(|value| *value).unwrap_or(true) {
            return;
        }
        let Ok((object, opcode, payload)) = read_message(&mut stream) else {
            let _ = proxy.window(WindowEvent::Disconnected);
            return;
        };

        if object == xdg_surface && opcode == 0 {
            if let Some(serial) = read_u32(&payload, 0) {
                let _ = send(&mut stream, xdg_surface, 4, &u32s(&[serial]));
            }
        } else if object == toplevel && opcode == 1 {
            let _ = proxy.window(WindowEvent::CloseRequested);
        } else if object == wm && opcode == 0 {
            if let Some(serial) = read_u32(&payload, 0) {
                let _ = send(&mut stream, wm, 3, &u32s(&[serial]));
            }
        } else if Some(object) == seat && opcode == 0 {
            if let Some(capabilities) = read_u32(&payload, 0) {
                if capabilities & SEAT_CAP_POINTER != 0
                    && pointer.is_none()
                    && send(&mut stream, object, 0, &u32s(&[POINTER_ID])).is_ok()
                {
                    pointer = Some(POINTER_ID);
                }
                if capabilities & SEAT_CAP_KEYBOARD != 0
                    && keyboard.is_none()
                    && send(&mut stream, object, 1, &u32s(&[KEYBOARD_ID])).is_ok()
                {
                    keyboard = Some(KEYBOARD_ID);
                }
            }
        } else if Some(object) == pointer {
            if let Some(event) = parse_pointer_event(opcode, &payload, surface, &mut pointer_position) {
                let _ = proxy.window(event);
            }
        } else if Some(object) == keyboard {
            match opcode {
                1 => {
                    if read_u32(&payload, 4) == Some(surface) {
                        let _ = proxy.window(WindowEvent::Focus(true));
                    }
                }
                2 => {
                    if read_u32(&payload, 4) == Some(surface) {
                        let _ = proxy.window(WindowEvent::Focus(false));
                    }
                }
                3 => {
                    if let (Some(key), Some(state)) = (read_u32(&payload, 8), read_u32(&payload, 12)) {
                        if let Some(event) = keyboard_event(key, state != 0, ctrl, shift) {
                            let _ = proxy.window(event);
                        }
                    }
                }
                4 => {
                    let depressed = read_u32(&payload, 4).unwrap_or(0);
                    let latched = read_u32(&payload, 8).unwrap_or(0);
                    let locked = read_u32(&payload, 12).unwrap_or(0);
                    let active = depressed | latched | locked;
                    shift = active & 1 != 0;
                    ctrl = active & 4 != 0;
                }
                _ => {}
            }
        }

        let _ = stream.flush();
    }
}

fn parse_pointer_event(opcode: u16, payload: &[u8], surface: u32, position: &mut (i32, i32)) -> Option<WindowEvent> {
    match opcode {
        0 if read_u32(payload, 4) == Some(surface) => {
            let x = fixed_to_pixel(read_i32(payload, 8)?);
            let y = fixed_to_pixel(read_i32(payload, 12)?);
            *position = (x, y);
            Some(WindowEvent::PointerMoved { x, y })
        }
        1 if read_u32(payload, 4) == Some(surface) => Some(WindowEvent::PointerLeft),
        2 => {
            let x = fixed_to_pixel(read_i32(payload, 4)?);
            let y = fixed_to_pixel(read_i32(payload, 8)?);
            *position = (x, y);
            Some(WindowEvent::PointerMoved { x, y })
        }
        3 => {
            let button = match read_u32(payload, 8)? {
                272 => 1,
                274 => 2,
                273 => 3,
                _ => return None,
            };
            Some(WindowEvent::Button {
                button,
                pressed: read_u32(payload, 12)? != 0,
                x: position.0,
                y: position.1,
            })
        }
        4 if read_u32(payload, 4) == Some(0) => {
            let raw = read_i32(payload, 8)?;
            let delta = match raw.cmp(&0) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
            (delta != 0).then_some(WindowEvent::Wheel { delta })
        }
        _ => None,
    }
}

fn keyboard_event(key: u32, pressed: bool, ctrl: bool, shift: bool) -> Option<WindowEvent> {
    let (keysym, character) = evdev_key(key, shift)?;
    Some(WindowEvent::Key {
        pressed,
        keysym,
        text: (pressed && !ctrl).then_some(character).flatten(),
        ctrl,
        shift,
    })
}

fn evdev_key(key: u32, shift: bool) -> Option<(u32, Option<char>)> {
    let special = match key {
        1 => Some((0xff1b, None)),
        14 => Some((0xff08, None)),
        15 => Some((0xff09, Some('\t'))),
        28 => Some((0xff0d, Some('\n'))),
        102 => Some((0xff50, None)),
        103 => Some((0xff52, None)),
        104 => Some((0xff55, None)),
        105 => Some((0xff51, None)),
        106 => Some((0xff53, None)),
        107 => Some((0xff57, None)),
        108 => Some((0xff54, None)),
        109 => Some((0xff56, None)),
        111 => Some((0xffff, None)),
        _ => None,
    };
    if special.is_some() {
        return special;
    }

    let base = match key {
        2 => '1',
        3 => '2',
        4 => '3',
        5 => '4',
        6 => '5',
        7 => '6',
        8 => '7',
        9 => '8',
        10 => '9',
        11 => '0',
        16 => 'q',
        17 => 'w',
        18 => 'e',
        19 => 'r',
        20 => 't',
        21 => 'y',
        22 => 'u',
        23 => 'i',
        24 => 'o',
        25 => 'p',
        30 => 'a',
        31 => 's',
        32 => 'd',
        33 => 'f',
        34 => 'g',
        35 => 'h',
        36 => 'j',
        37 => 'k',
        38 => 'l',
        44 => 'z',
        45 => 'x',
        46 => 'c',
        47 => 'v',
        48 => 'b',
        49 => 'n',
        50 => 'm',
        57 => ' ',
        _ => return None,
    };
    let character = if shift { base.to_ascii_uppercase() } else { base };
    Some((u32::from(character), Some(character)))
}

fn fixed_to_pixel(raw: i32) -> i32 {
    raw.checked_div(256).unwrap_or_default()
}

fn bind(
    stream: &mut UnixStream,
    global: Option<(u32, u32)>,
    interface: &str,
    max_version: u32,
    id: u32,
) -> Result<u32> {
    let (name, version) = global.ok_or_else(|| Error::Refused(format!("Wayland compositor has no {interface}")))?;
    let version = version.min(max_version);
    let mut body = u32s(&[name]);
    body.extend_from_slice(&wire_string(interface));
    body.extend_from_slice(&u32s(&[version, id]));
    send(stream, WL_REGISTRY, 0, &body)?;
    Ok(id)
}

fn read_registry_until_done(stream: &mut UnixStream, callback: u32, globals: &mut Globals) -> Result<()> {
    loop {
        let (object, opcode, payload) = read_message(stream)?;
        if object == callback && opcode == 0 {
            return Ok(());
        }
        if object == WL_REGISTRY && opcode == 0 {
            if let Some((name, iface, version)) = parse_global(&payload) {
                match iface.as_str() {
                    "wl_compositor" => globals.compositor = Some((name, version)),
                    "wl_shm" => globals.shm = Some((name, version)),
                    "xdg_wm_base" => globals.wm_base = Some((name, version)),
                    "wl_seat" => globals.seat = Some((name, version)),
                    _ => {}
                }
            }
        }
    }
}

fn parse_global(payload: &[u8]) -> Option<(u32, String, u32)> {
    let name = read_u32(payload, 0)?;
    let len = usize::try_from(read_u32(payload, 4)?).ok()?;
    let end = 8usize.checked_add(align4(len))?;
    let raw = payload.get(8..8usize.checked_add(len)?.saturating_sub(1))?;
    let iface = String::from_utf8(raw.to_vec()).ok()?;
    Some((name, iface, read_u32(payload, end)?))
}
fn read_message(stream: &mut UnixStream) -> Result<(u32, u16, Vec<u8>)> {
    let mut head = [0u8; 8];
    stream.read_exact(&mut head).map_err(io)?;
    let object = u32::from_ne_bytes(head[..4].try_into().map_err(|_| Error::damaged("Wayland header"))?);
    let word = u32::from_ne_bytes(head[4..].try_into().map_err(|_| Error::damaged("Wayland header"))?);
    let size = usize::try_from(word >> 16).map_err(|_| Error::damaged("Wayland size"))?;
    if !(8..=1_048_576).contains(&size) {
        return Err(Error::damaged("invalid Wayland message size"));
    }
    let payload_len = size
        .checked_sub(8)
        .ok_or_else(|| Error::damaged("Wayland payload size underflow"))?;
    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload).map_err(io)?;
    let opcode = u16::try_from(word & 0xffff).map_err(|_| Error::damaged("Wayland opcode"))?;
    Ok((object, opcode, payload))
}
fn send(stream: &mut UnixStream, object: u32, opcode: u16, payload: &[u8]) -> Result<()> {
    let size = 8usize
        .checked_add(payload.len())
        .ok_or_else(|| Error::damaged("Wayland message overflow"))?;
    let size_word = u32::try_from(size)
        .map_err(|_| Error::damaged("Wayland message too large"))?
        .checked_shl(16)
        .ok_or_else(|| Error::damaged("Wayland message size shift overflow"))?;
    let word = size_word | u32::from(opcode);
    stream.write_all(&object.to_ne_bytes()).map_err(io)?;
    stream.write_all(&word.to_ne_bytes()).map_err(io)?;
    stream.write_all(payload).map_err(io)
}
fn send_string(stream: &mut UnixStream, object: u32, opcode: u16, value: &str) -> Result<()> {
    send(stream, object, opcode, &wire_string(value))
}
fn send_with_fd(stream: &UnixStream, object: u32, opcode: u16, payload: &[u8], fd: std::os::fd::RawFd) -> Result<()> {
    let size = 8usize
        .checked_add(payload.len())
        .ok_or_else(|| Error::damaged("Wayland fd message overflow"))?;
    let size_word = u32::try_from(size)
        .map_err(|_| Error::damaged("Wayland message too large"))?
        .checked_shl(16)
        .ok_or_else(|| Error::damaged("Wayland message size shift overflow"))?;
    let word = size_word | u32::from(opcode);
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(&object.to_ne_bytes());
    bytes.extend_from_slice(&word.to_ne_bytes());
    bytes.extend_from_slice(payload);
    sse_sys::unix_fd::send_fd(stream, &bytes, fd).map_err(io)
}
fn wire_string(value: &str) -> Vec<u8> {
    let len = value.len().saturating_add(1);
    let mut out = u32s(&[u32::try_from(len).unwrap_or(u32::MAX)]);
    out.extend_from_slice(value.as_bytes());
    out.push(0);
    let remainder = out.len().checked_rem(4).unwrap_or(0);
    let padding = 4_usize.saturating_sub(remainder).checked_rem(4).unwrap_or(0);
    out.resize(out.len().saturating_add(padding), 0);
    out
}
fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_ne_bytes()).collect()
}
fn i32s(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_ne_bytes()).collect()
}
fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_ne_bytes(bytes.get(offset..end)?.try_into().ok()?))
}
fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    let end = offset.checked_add(4)?;
    Some(i32::from_ne_bytes(bytes.get(offset..end)?.try_into().ok()?))
}
fn align4(value: usize) -> usize {
    value.saturating_add(3) & !3
}
fn frame_bytes(width: u32, height: u32) -> Result<usize> {
    usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|p| p.checked_mul(4))
        .filter(|n| *n > 0 && *n <= 134_217_728)
        .ok_or_else(|| Error::Refused("Wayland frame size outside limit".to_owned()))
}
fn io(error: std::io::Error) -> Error {
    Error::System(format!("Wayland: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{keyboard_event, parse_global, parse_pointer_event, wire_string};
    use crate::event_loop::WindowEvent;

    #[test]
    fn registry_global_decodes() {
        let mut p = 1u32.to_ne_bytes().to_vec();
        p.extend_from_slice(&wire_string("wl_compositor"));
        p.extend_from_slice(&4u32.to_ne_bytes());
        assert_eq!(parse_global(&p), Some((1, "wl_compositor".to_owned(), 4)));
    }

    #[test]
    fn pointer_motion_decodes_wayland_fixed_coordinates() {
        let mut payload = 7_u32.to_ne_bytes().to_vec();
        payload.extend_from_slice(&(12_i32.checked_mul(256).unwrap_or_default()).to_ne_bytes());
        payload.extend_from_slice(&(34_i32.checked_mul(256).unwrap_or_default()).to_ne_bytes());
        let mut position = (0, 0);
        assert_eq!(
            parse_pointer_event(2, &payload, 8, &mut position),
            Some(WindowEvent::PointerMoved { x: 12, y: 34 })
        );
        assert_eq!(position, (12, 34));
    }

    #[test]
    fn keyboard_event_preserves_ctrl_shortcut_without_text() {
        assert_eq!(
            keyboard_event(31, true, true, false),
            Some(WindowEvent::Key {
                pressed: true,
                keysym: u32::from('s'),
                text: None,
                ctrl: true,
                shift: false,
            })
        );
    }
}
