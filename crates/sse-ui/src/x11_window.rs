//! X11 window backend over a Unix socket: setup, one window, PutImage presentation, a reader thread for events.
//!
//! Only `std` is used: the socket is `UnixStream`, the reader thread blocks in `read` and feeds the loop channel, so
//! the UI thread sleeps until something happens. MIT-SHM presentation comes with the `sse-sys` shared-memory calls.

use crate::event_loop::{Present, Proxy, WindowEvent};
use crate::raster::Rect;
use crate::x11::{
    find_mit_cookie, keysym_to_char, parse_setup_reply, parse_xauthority, Atom, ByteOrder, Connection, Drawable, Event,
    Gc, Packet, ShmSeg, Transport, Window,
};
use sse_core::{Error, Result};
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
use sse_sys::shm::SharedMemory;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
use std::sync::mpsc;

const ORDER: ByteOrder = ByteOrder::Little;
const CW_BACK_PIXEL: u32 = 1 << 1;
const CW_BIT_GRAVITY: u32 = 1 << 4;
const CW_EVENT_MASK: u32 = 1 << 11;
const NORTH_WEST_GRAVITY: u32 = 1;
const EVENT_MASK: u32 = 1 // KeyPress
    | 1 << 1 // KeyRelease
    | 1 << 2 // ButtonPress
    | 1 << 3 // ButtonRelease
    | 1 << 5 // LeaveWindow
    | 1 << 6 // PointerMotion
    | 1 << 15 // Exposure
    | 1 << 17 // StructureNotify
    | 1 << 21; // FocusChange
const ZPIXMAP: u8 = 2;
const LEAVE_NOTIFY: u8 = 8;
const SHIFT_MASK: u16 = 1;
const CONTROL_MASK: u16 = 4;
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
const MAX_SHM_SEGMENT_BYTES: usize = 128 * 1024 * 1024;

struct Stream(UnixStream);

impl Transport for Stream {
    fn send(&mut self, data: &[u8]) {
        // A failed write surfaces as a disconnect on the reader thread.
        let _ = self.0.write_all(data);
    }

    fn receive(&mut self, out: &mut [u8]) {
        if self.0.read_exact(out).is_err() {
            out.fill(0);
        }
    }
}

/// Decoder-only transport for the reader thread.
struct Silent;

impl Transport for Silent {
    fn send(&mut self, _: &[u8]) {}
    fn receive(&mut self, out: &mut [u8]) {
        out.fill(0);
    }
}

/// An open X11 window.
pub struct X11Window {
    connection: Connection<Stream>,
    window: Window,
    gc: Gc,
    depth: u8,
    scanline_pad: u8,
    scratch: Vec<u8>,
    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
    shm: Option<ShmBuffer>,
}

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
struct ShmBuffer {
    memory: SharedMemory,
    segment: ShmSeg,
    event_base: u8,
    completion_rx: mpsc::Receiver<u32>,
    capacity_width: u16,
    capacity_height: u16,
    scanline_pad: u8,
    offsets: [u32; 2],
    pending: [usize; 2],
    next_slot: usize,
}

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
struct FrameView<'a> {
    pixels: &'a [u32],
    stride: usize,
    width: u32,
    height: u32,
}

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
struct X11Target<'a> {
    connection: &'a mut Connection<Stream>,
    window: Window,
    gc: Gc,
    depth: u8,
}

impl X11Window {
    /// Connects to `$DISPLAY`, opens a mapped window and starts the reader thread that sends events to `proxy`.
    ///
    /// # Errors
    /// Returns an error when there is no local X server, setup is refused or the screen has no 32-bit TrueColor
    /// format.
    pub fn open<U: Send + 'static>(
        title: &str,
        width: u16,
        height: u16,
        background: u32,
        proxy: Proxy<U>,
    ) -> Result<Self> {
        let display = std::env::var("DISPLAY").map_err(|_| Error::Refused("DISPLAY is not set".to_owned()))?;
        let number = display_number(&display)?;
        let stream = connect(number)?;
        let mut reader = stream.try_clone().map_err(io)?;
        let mut writer = Stream(stream);

        let auth = xauthority_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|data| parse_xauthority(&data).ok())
            .and_then(|entries| find_mit_cookie(&entries, number));
        writer.send(&crate::x11::encode_setup(ORDER, auth.as_ref())?);
        let mut head = [0_u8; 8];
        reader.read_exact(&mut head).map_err(io)?;
        let extra = usize::from(u16::from_le_bytes([head[6], head[7]])).saturating_mul(4);
        let mut reply = head.to_vec();
        reply.resize(8_usize.saturating_add(extra), 0);
        reader.read_exact(reply.get_mut(8..).unwrap_or_default()).map_err(io)?;
        let setup = parse_setup_reply(&reply, ORDER)?;
        let (min_keycode, max_keycode) = (
            reply.get(34).copied().unwrap_or(8),
            reply.get(35).copied().unwrap_or(255),
        );
        let screen = setup
            .screens
            .first()
            .ok_or_else(|| Error::Refused("X server has no screens".to_owned()))?;
        let depth = screen.root_depth;
        let image_format = setup
            .pixmap_formats
            .iter()
            .find(|format| format.depth == depth && format.bits_per_pixel == 32)
            .ok_or_else(|| Error::Refused("X screen has no 32-bit pixel format".to_owned()))?;
        if depth < 24 {
            return Err(Error::Refused("X screen is not 24/32-bit TrueColor".to_owned()));
        }
        let scanline_pad = image_format.scanline_pad;
        let (root, visual) = (screen.root, screen.root_visual);

        let mut connection = Connection::new(writer, ORDER, &setup);
        let atoms = intern(
            &mut connection,
            &mut reader,
            &[
                b"WM_PROTOCOLS".as_slice(),
                b"WM_DELETE_WINDOW",
                b"_NET_WM_NAME",
                b"UTF8_STRING",
                b"ATOM",
                b"STRING",
                b"WM_NAME",
            ],
        )?;
        let [protocols, delete, net_name, utf8, atom_type, string, wm_name] = atoms;

        let count = max_keycode.saturating_sub(min_keycode).saturating_add(1);
        connection.get_keyboard_mapping(min_keycode, count)?;
        let keymap_reply = read_reply(&mut reader)?;
        let keymap = connection.parse_keyboard_mapping_reply(&keymap_reply, usize::from(count))?;

        let base = setup.resource_id_base;
        #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
        let (completion_tx, completion_rx) = mpsc::channel();
        #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
        let shm = try_enable_shm(
            &mut connection,
            &mut reader,
            base,
            width,
            height,
            scanline_pad,
            completion_rx,
        );
        let window = Window(base | 1);
        let gc = Gc(base | 2);
        connection.create_window(
            depth,
            window,
            Window(root),
            0,
            0,
            width,
            height,
            0,
            1,
            visual,
            &[
                (CW_BACK_PIXEL, background),
                (CW_BIT_GRAVITY, NORTH_WEST_GRAVITY),
                (CW_EVENT_MASK, EVENT_MASK),
            ],
        )?;
        connection.set_text_property(window, wm_name, string, title)?;
        connection.set_text_property(window, net_name, utf8, title)?;
        connection.set_wm_delete_window(window, protocols, delete, atom_type)?;
        connection.create_gc(gc, Drawable(window.0), &[])?;
        connection.map_window(window)?;

        let decoder = Connection::new(Silent, ORDER, &setup);
        #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
        let shm_event_base = shm.as_ref().map(|buffer| buffer.event_base);
        let thread = Reader {
            stream: reader,
            decoder,
            window,
            delete,
            keymap,
            min_keycode,
            size: (width, height),
            #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
            completion_tx,
            #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
            shm_event_base,
        };
        std::thread::Builder::new()
            .name("x11-events".to_owned())
            .spawn(move || thread.run(&proxy))
            .map_err(io)?;
        Ok(Self {
            connection,
            window,
            gc,
            depth,
            scanline_pad,
            scratch: Vec::new(),
            #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
            shm,
        })
    }

    /// Returns whether MIT-SHM is active for this window.
    #[must_use]
    pub fn uses_mit_shm(&self) -> bool {
        #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            self.shm.is_some()
        }
        #[cfg(not(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64"))))]
        {
            false
        }
    }
}

impl Present for X11Window {
    fn present(&mut self, frame: &[u32], stride: usize, width: u32, height: u32, rects: &[Rect]) -> Result<()> {
        #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
        if let Some(shm) = self.shm.as_mut() {
            if shm.supports(width, height) {
                let frame = FrameView {
                    pixels: frame,
                    stride,
                    width,
                    height,
                };
                let mut target = X11Target {
                    connection: &mut self.connection,
                    window: self.window,
                    gc: self.gc,
                    depth: self.depth,
                };
                return shm.present(frame, rects, &mut target);
            }
        }

        for rect in rects {
            let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
                usize::try_from(rect.x),
                usize::try_from(rect.y),
                usize::try_from(rect.width),
                usize::try_from(rect.height),
            ) else {
                continue;
            };
            if w == 0 || h == 0 || x.saturating_add(w) > usize::try_from(width).unwrap_or(0) {
                continue;
            }
            if y.saturating_add(h) > usize::try_from(height).unwrap_or(0) {
                continue;
            }
            let bytes_per_row = padded_row_bytes(w, self.scanline_pad)
                .ok_or_else(|| Error::damaged("unsupported X11 scanline padding"))?;
            self.scratch.clear();
            for row in y..y.saturating_add(h) {
                let start = row.saturating_mul(stride).saturating_add(x);
                let Some(pixels) = frame.get(start..start.saturating_add(w)) else {
                    return Err(Error::damaged("frame shorter than its size"));
                };
                for pixel in pixels {
                    self.scratch.extend_from_slice(&pixel.to_le_bytes());
                }
                let row_end = self
                    .scratch
                    .len()
                    .checked_add(bytes_per_row.saturating_sub(w.saturating_mul(4)))
                    .ok_or_else(|| Error::damaged("X11 row size overflow"))?;
                self.scratch.resize(row_end, 0);
            }
            self.connection.put_image(
                ZPIXMAP,
                Drawable(self.window.0),
                self.gc,
                u16::try_from(w).unwrap_or(u16::MAX),
                u16::try_from(h).unwrap_or(u16::MAX),
                i16::try_from(x).unwrap_or(i16::MAX),
                i16::try_from(y).unwrap_or(i16::MAX),
                0,
                self.depth,
                bytes_per_row,
                &self.scratch,
            )?;
        }
        Ok(())
    }
}

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
impl ShmBuffer {
    fn supports(&self, width: u32, height: u32) -> bool {
        width <= u32::from(self.capacity_width) && height <= u32::from(self.capacity_height)
    }

    fn present(&mut self, frame: FrameView<'_>, rects: &[Rect], target: &mut X11Target<'_>) -> Result<()> {
        if rects.is_empty() {
            return Ok(());
        }
        let width_usize = usize::try_from(frame.width).map_err(|_| Error::damaged("X11 width overflow"))?;
        let height_usize = usize::try_from(frame.height).map_err(|_| Error::damaged("X11 height overflow"))?;
        if frame.stride < width_usize {
            return Err(Error::damaged("frame stride shorter than its width"));
        }
        let image_stride = padded_row_bytes(width_usize, self.scanline_pad)
            .ok_or_else(|| Error::damaged("unsupported X11 scanline padding"))?;
        let slot = self.next_slot;
        self.wait_until_free(slot)?;
        let shm_offset = self
            .offsets
            .get(slot)
            .copied()
            .ok_or_else(|| Error::damaged("MIT-SHM slot out of range"))?;
        let base_offset = usize::try_from(shm_offset).map_err(|_| Error::damaged("MIT-SHM offset overflow"))?;
        let mut sent = false;

        for rect in rects {
            let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
                usize::try_from(rect.x),
                usize::try_from(rect.y),
                usize::try_from(rect.width),
                usize::try_from(rect.height),
            ) else {
                continue;
            };
            if w == 0 || h == 0 || x.checked_add(w).is_none_or(|end| end > width_usize) {
                continue;
            }
            if y.checked_add(h).is_none_or(|end| end > height_usize) {
                continue;
            }
            let (Ok(src_x), Ok(src_y), Ok(src_width), Ok(src_height), Ok(dst_x), Ok(dst_y)) = (
                u16::try_from(x),
                u16::try_from(y),
                u16::try_from(w),
                u16::try_from(h),
                i16::try_from(x),
                i16::try_from(y),
            ) else {
                continue;
            };

            for row in y..y.saturating_add(h) {
                let start = row
                    .checked_mul(frame.stride)
                    .and_then(|value| value.checked_add(x))
                    .ok_or_else(|| Error::damaged("frame pixel offset overflow"))?;
                let end = start
                    .checked_add(w)
                    .ok_or_else(|| Error::damaged("frame row length overflow"))?;
                let pixels = frame
                    .pixels
                    .get(start..end)
                    .ok_or_else(|| Error::damaged("frame shorter than its size"))?;
                let pixel_offset = row
                    .checked_mul(image_stride)
                    .and_then(|value| x.checked_mul(4).and_then(|column| value.checked_add(column)))
                    .and_then(|value| value.checked_add(base_offset))
                    .ok_or_else(|| Error::damaged("MIT-SHM row offset overflow"))?;
                self.memory.write_u32_le(pixel_offset, pixels).map_err(io)?;
            }
            target.connection.shm_put_image(
                Drawable(target.window.0),
                target.gc,
                u16::try_from(frame.width).map_err(|_| Error::damaged("X11 width exceeds protocol limit"))?,
                u16::try_from(frame.height).map_err(|_| Error::damaged("X11 height exceeds protocol limit"))?,
                src_x,
                src_y,
                src_width,
                src_height,
                dst_x,
                dst_y,
                target.depth,
                ZPIXMAP,
                true,
                self.segment,
                shm_offset,
            )?;
            let pending = self
                .pending
                .get_mut(slot)
                .ok_or_else(|| Error::damaged("MIT-SHM slot out of range"))?;
            *pending = pending.saturating_add(1);
            sent = true;
        }
        if sent {
            self.next_slot = if slot == 0 { 1 } else { 0 };
        }
        Ok(())
    }

    fn wait_until_free(&mut self, slot: usize) -> Result<()> {
        while self.pending.get(slot).copied().unwrap_or_default() > 0 {
            let offset = self
                .completion_rx
                .recv()
                .map_err(|error| Error::System(format!("MIT-SHM completion channel: {error}")))?;
            if let Some(completed_slot) = self.offsets.iter().position(|base| *base == offset) {
                if let Some(pending) = self.pending.get_mut(completed_slot) {
                    *pending = pending.saturating_sub(1);
                }
            }
        }
        Ok(())
    }
}

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
fn try_enable_shm(
    connection: &mut Connection<Stream>,
    reader: &mut UnixStream,
    resource_base: u32,
    width: u16,
    height: u16,
    scanline_pad: u8,
    completion_rx: mpsc::Receiver<u32>,
) -> Option<ShmBuffer> {
    let image_stride = padded_row_bytes(usize::from(width), scanline_pad)?;
    let slot_bytes = image_stride.checked_mul(usize::from(height))?;
    let segment_bytes = slot_bytes.checked_mul(2)?;
    if slot_bytes == 0 || segment_bytes > MAX_SHM_SEGMENT_BYTES {
        return None;
    }
    let second_offset = u32::try_from(slot_bytes).ok()?;

    connection.query_extension(b"MIT-SHM").ok()?;
    let reply = read_reply(reader).ok()?;
    let (_, event_base) = connection
        .parse_query_extension_reply_details(b"MIT-SHM", &reply)
        .ok()??;
    if event_base == 0 {
        return None;
    }

    let segment = ShmSeg(resource_base | 3);
    let mut memory = SharedMemory::new(segment_bytes).ok()?;
    let shmid = u32::try_from(memory.id()).ok()?;
    if connection.shm_attach(segment, shmid, false).is_err() {
        return None;
    }
    if connection.get_input_focus().is_err() || read_reply(reader).is_err() {
        let _ = connection.shm_detach(segment);
        return None;
    }
    if memory.request_removal().is_err() {
        let _ = connection.shm_detach(segment);
        return None;
    }

    Some(ShmBuffer {
        memory,
        segment,
        event_base,
        completion_rx,
        capacity_width: width,
        capacity_height: height,
        scanline_pad,
        offsets: [0, second_offset],
        pending: [0, 0],
        next_slot: 0,
    })
}

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
impl Drop for X11Window {
    fn drop(&mut self) {
        if let Some(shm) = self.shm.as_ref() {
            let _ = self.connection.shm_detach(shm.segment);
        }
    }
}

struct Reader {
    stream: UnixStream,
    decoder: Connection<Silent>,
    window: Window,
    delete: Atom,
    keymap: Vec<Vec<u32>>,
    min_keycode: u8,
    size: (u16, u16),
    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
    completion_tx: mpsc::Sender<u32>,
    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
    shm_event_base: Option<u8>,
}

impl Reader {
    fn run<U>(mut self, proxy: &Proxy<U>) {
        loop {
            let mut packet = [0_u8; 32];
            if self.stream.read_exact(&mut packet).is_err() {
                proxy.window(WindowEvent::Disconnected);
                return;
            }
            let kind = packet[0] & 0x7f;
            #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
            if let Some(event_base) = self.shm_event_base {
                if let Some(offset) = shm_completion_offset(&packet, event_base) {
                    let _ = self.completion_tx.send(offset);
                    continue;
                }
            }
            // Replies and generic events carry a payload after the fixed 32 bytes; skip it.
            if kind == 1 || kind == 35 {
                let extra = u32::from_le_bytes([packet[4], packet[5], packet[6], packet[7]]);
                let mut rest = vec![0_u8; usize::try_from(extra).unwrap_or(0).saturating_mul(4)];
                if self.stream.read_exact(&mut rest).is_err() {
                    proxy.window(WindowEvent::Disconnected);
                    return;
                }
                continue;
            }
            let Ok(Packet::Event(event)) = self.decoder.decode_packet(&packet) else {
                continue;
            };
            if let Some(converted) = self.convert(event) {
                if !proxy.window(converted) {
                    return;
                }
            }
        }
    }

    fn convert(&mut self, event: Event) -> Option<WindowEvent> {
        match event {
            Event::Expose { damage, .. } => Some(WindowEvent::Exposed(Rect::new(
                i32::from(damage.x),
                i32::from(damage.y),
                u32::from(damage.width),
                u32::from(damage.height),
            ))),
            Event::ConfigureNotify {
                window, width, height, ..
            } if window == self.window => {
                if (width, height) == self.size {
                    return None;
                }
                self.size = (width, height);
                Some(WindowEvent::Resized {
                    width: u32::from(width),
                    height: u32::from(height),
                })
            }
            Event::Motion { x, y, .. } => Some(WindowEvent::PointerMoved {
                x: i32::from(x),
                y: i32::from(y),
            }),
            Event::Button {
                pressed, detail, x, y, ..
            } => match detail {
                4 | 5 if pressed => Some(WindowEvent::Wheel {
                    delta: if detail == 4 { -1 } else { 1 },
                }),
                4..=7 => None,
                button => Some(WindowEvent::Button {
                    button,
                    pressed,
                    x: i32::from(x),
                    y: i32::from(y),
                }),
            },
            Event::Key {
                pressed, detail, state, ..
            } => {
                let shift = state & SHIFT_MASK != 0;
                let row = self.keymap.get(usize::from(detail.saturating_sub(self.min_keycode)))?;
                let lower = row.first().copied().unwrap_or(0);
                let keysym = if shift {
                    row.get(1).copied().filter(|k| *k != 0).unwrap_or(lower)
                } else {
                    lower
                };
                Some(WindowEvent::Key {
                    pressed,
                    keysym,
                    text: keysym_to_char(keysym),
                    ctrl: state & CONTROL_MASK != 0,
                    shift,
                })
            }
            Event::Focus { focused, .. } => Some(WindowEvent::Focus(focused)),
            Event::ClientMessage { data, .. } if data.get(0..4) == Some(&self.delete.0.to_le_bytes()[..]) => {
                Some(WindowEvent::CloseRequested)
            }
            Event::Unknown { response_type, .. } if response_type & 0x7f == LEAVE_NOTIFY => {
                Some(WindowEvent::PointerLeft)
            }
            _ => None,
        }
    }
}

fn intern<const N: usize>(
    connection: &mut Connection<Stream>,
    reader: &mut UnixStream,
    names: &[&[u8]; N],
) -> Result<[Atom; N]> {
    for name in names {
        connection.intern_atom(false, name)?;
    }
    let mut atoms = [Atom(0); N];
    for atom in &mut atoms {
        *atom = connection.parse_intern_atom_reply(&read_reply(reader)?)?;
    }
    Ok(atoms)
}

/// Reads the next reply during setup, skipping events; an X error is returned as an error.
fn read_reply(reader: &mut UnixStream) -> Result<Vec<u8>> {
    loop {
        let mut packet = [0_u8; 32];
        reader.read_exact(&mut packet).map_err(io)?;
        match packet[0] {
            0 => return Err(Error::System(format!("X11 error {} during setup", packet[1]))),
            1 => {
                let extra = u32::from_le_bytes([packet[4], packet[5], packet[6], packet[7]]);
                let mut reply = packet.to_vec();
                reply.resize(
                    32_usize.saturating_add(usize::try_from(extra).unwrap_or(0).saturating_mul(4)),
                    0,
                );
                reader.read_exact(reply.get_mut(32..).unwrap_or_default()).map_err(io)?;
                return Ok(reply);
            }
            _ => {}
        }
    }
}

fn display_number(display: &str) -> Result<u16> {
    let rest = display.strip_prefix("unix").unwrap_or(display);
    let Some(rest) = rest.strip_prefix(':') else {
        return Err(Error::Refused(format!("remote X display {display} is not supported")));
    };
    rest.split('.')
        .next()
        .and_then(|number| number.parse().ok())
        .ok_or_else(|| Error::Refused(format!("bad DISPLAY {display}")))
}

fn connect(number: u16) -> Result<UnixStream> {
    let path = format!("/tmp/.X11-unix/X{number}");
    match UnixStream::connect(&path) {
        Ok(stream) => Ok(stream),
        #[cfg(target_os = "linux")]
        Err(_) => {
            use std::os::linux::net::SocketAddrExt;
            let address = std::os::unix::net::SocketAddr::from_abstract_name(path.as_bytes()).map_err(io)?;
            UnixStream::connect_addr(&address).map_err(io)
        }
        #[cfg(not(target_os = "linux"))]
        Err(error) => Err(io(error)),
    }
}

fn xauthority_path() -> Option<PathBuf> {
    std::env::var_os("XAUTHORITY")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".Xauthority")))
}

fn io(error: std::io::Error) -> Error {
    Error::System(format!("X11 connection: {error}"))
}

fn shm_completion_offset(packet: &[u8], event_base: u8) -> Option<u32> {
    if packet.first().copied()? & 0x7f != event_base {
        return None;
    }
    let bytes: [u8; 4] = packet.get(16..20)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn padded_row_bytes(width: usize, scanline_pad: u8) -> Option<usize> {
    let pad_bits = usize::from(scanline_pad);
    if pad_bits == 0 || pad_bits.checked_rem(8)? != 0 {
        return None;
    }
    let pad_bytes = pad_bits.checked_div(8)?;
    let row_bytes = width.checked_mul(4)?;
    let rounded = row_bytes.checked_add(pad_bytes.checked_sub(1)?)?;
    rounded.checked_div(pad_bytes)?.checked_mul(pad_bytes)
}

#[cfg(test)]
mod tests {
    use super::{padded_row_bytes, shm_completion_offset};

    #[test]
    fn completion_event_reads_offset_and_ignores_sent_event_bit() {
        let mut packet = [0_u8; 32];
        if let Some(kind) = packet.first_mut() {
            *kind = 0xc0;
        }
        for (slot, byte) in packet.iter_mut().skip(16).take(4).zip(0x1234_5678_u32.to_le_bytes()) {
            *slot = byte;
        }
        assert_eq!(shm_completion_offset(&packet, 64), Some(0x1234_5678));
    }

    #[test]
    fn completion_event_rejects_other_types_and_short_packets() {
        assert_eq!(shm_completion_offset(&[64_u8; 19], 64), None);
        assert_eq!(shm_completion_offset(&[63_u8; 32], 64), None);
    }

    #[test]
    fn image_rows_follow_the_server_scanline_padding() {
        assert_eq!(padded_row_bytes(3, 32), Some(12));
        assert_eq!(padded_row_bytes(3, 64), Some(16));
        assert_eq!(padded_row_bytes(3, 0), None);
    }
}
