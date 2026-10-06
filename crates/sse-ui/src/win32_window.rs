//! Win32 window adapter for the event-loop `Present` contract.

#[cfg(any(test, windows))]
use crate::raster::Rect;
#[cfg(any(test, windows))]
use sse_core::{Error, Result};
#[cfg(any(test, windows))]
use std::collections::HashSet;

#[cfg(test)]
fn frame_to_bgra(frame: &[u32], stride: usize, width: u32, height: u32) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    frame_to_bgra_into(frame, stride, width, height, &mut bytes)?;
    Ok(bytes)
}

#[cfg(any(test, windows))]
fn frame_to_bgra_into(frame: &[u32], stride: usize, width: u32, height: u32, bytes: &mut Vec<u8>) -> Result<()> {
    let row_width = usize::try_from(width).map_err(|_| Error::Refused("frame width is too large".to_owned()))?;
    let rows = usize::try_from(height).map_err(|_| Error::Refused("frame height is too large".to_owned()))?;
    if row_width > stride {
        return Err(Error::Refused("frame stride is shorter than its width".to_owned()));
    }
    let source_len = if rows == 0 {
        0
    } else {
        rows.saturating_sub(1)
            .checked_mul(stride)
            .and_then(|start| start.checked_add(row_width))
            .ok_or_else(|| Error::Refused("frame dimensions overflow".to_owned()))?
    };
    if frame.len() < source_len {
        return Err(Error::damaged("frame buffer is shorter than its dimensions"));
    }
    let pixels = row_width
        .checked_mul(rows)
        .ok_or_else(|| Error::Refused("frame dimensions overflow".to_owned()))?;
    let byte_len = pixels
        .checked_mul(4)
        .ok_or_else(|| Error::Refused("frame byte count overflow".to_owned()))?;
    bytes.clear();
    bytes.reserve(byte_len);
    for row in 0..rows {
        let start = row
            .checked_mul(stride)
            .ok_or_else(|| Error::Refused("frame row offset overflow".to_owned()))?;
        let end = start
            .checked_add(row_width)
            .ok_or_else(|| Error::Refused("frame row end overflow".to_owned()))?;
        let pixels = frame
            .get(start..end)
            .ok_or_else(|| Error::damaged("frame buffer is shorter than its dimensions"))?;
        for pixel in pixels {
            bytes.extend_from_slice(&pixel.to_le_bytes());
        }
    }
    Ok(())
}

#[cfg(test)]
fn clip_damage(rects: &[Rect], width: u32, height: u32) -> Vec<Rect> {
    let mut clipped = Vec::with_capacity(rects.len());
    for rect in rects {
        if let Some(rect) = clip_rect(*rect, width, height) {
            clipped.push(rect);
        }
    }
    clipped
}

#[cfg(any(test, windows))]
fn clip_rect(rect: Rect, width: u32, height: u32) -> Option<Rect> {
    let left = i64::from(rect.x).max(0);
    let top = i64::from(rect.y).max(0);
    let right = i64::from(rect.x)
        .saturating_add(i64::from(rect.width))
        .min(i64::from(width));
    let bottom = i64::from(rect.y)
        .saturating_add(i64::from(rect.height))
        .min(i64::from(height));
    if right <= left || bottom <= top {
        return None;
    }
    let (Ok(x), Ok(y), Ok(rect_width), Ok(rect_height)) = (
        i32::try_from(left),
        i32::try_from(top),
        u32::try_from(right.saturating_sub(left)),
        u32::try_from(bottom.saturating_sub(top)),
    ) else {
        return None;
    };
    Some(Rect::new(x, y, rect_width, rect_height))
}

#[cfg(any(test, windows))]
fn win32_keysym(code: u32) -> u32 {
    match code {
        0x08 => 0xff08,        // Backspace
        0x09 => 0xff09,        // Tab
        0x0d => 0xff0d,        // Return
        0x1b => 0xff1b,        // Escape
        0x20 => 0x20,          // Space
        0x10 | 0xa0 => 0xffe1, // Left Shift
        0xa1 => 0xffe2,        // Right Shift
        0x11 | 0xa2 => 0xffe3, // Left Control
        0xa3 => 0xffe4,        // Right Control
        0x12 | 0xa4 => 0xffe9, // Left Alt
        0xa5 => 0xffea,        // Right Alt
        0x21 => 0xff55,        // Page Up
        0x22 => 0xff56,        // Page Down
        0x23 => 0xff57,        // End
        0x24 => 0xff50,        // Home
        0x25 => 0xff51,        // Left
        0x26 => 0xff52,        // Up
        0x27 => 0xff53,        // Right
        0x28 => 0xff54,        // Down
        0x2d => 0xff63,        // Insert
        0x2e => 0xffff,        // Delete
        0x41..=0x5a => code.saturating_add(0x20),
        0x70..=0x87 => 0xffbe_u32.saturating_add(code.saturating_sub(0x70)),
        _ => code,
    }
}

#[cfg(any(test, windows))]
#[derive(Default)]
struct KeyModifiers {
    pressed: HashSet<u32>,
}

#[cfg(any(test, windows))]
impl KeyModifiers {
    fn update(&mut self, code: u32, pressed: bool) {
        if pressed {
            self.pressed.insert(code);
        } else {
            self.pressed.remove(&code);
        }
    }

    fn control(&self) -> bool {
        [0x11, 0xa2, 0xa3].iter().any(|code| self.pressed.contains(code))
    }

    fn shift(&self) -> bool {
        [0x10, 0xa0, 0xa1].iter().any(|code| self.pressed.contains(code))
    }
}

#[cfg(windows)]
mod platform {
    use super::{clip_rect, frame_to_bgra_into, win32_keysym, KeyModifiers};
    use crate::event_loop::{ImeEvent, Present, Proxy, WindowEvent};
    use crate::raster::Rect;
    use sse_core::{Error, Result};
    use sse_sys::window_win32::{Event, Rect as NativeRect, WakeHandle, Win32Window, Window, WindowOptions};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};

    struct Frame {
        bytes: Vec<u8>,
        width: u32,
        height: u32,
        damage: Vec<NativeRect>,
    }

    /// A Win32 presenter whose native message pump owns the window on a dedicated thread.
    pub struct Win32Presenter {
        mailbox: Arc<Mutex<Frame>>,
        wake: WakeHandle,
        shutdown: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }

    impl Win32Presenter {
        /// Creates the native window and starts its message-pump thread.
        pub fn open<U: Send + 'static>(
            title: impl Into<String>,
            width: u32,
            height: u32,
            proxy: Proxy<U>,
        ) -> Result<Self> {
            let options = WindowOptions {
                title: title.into(),
                width,
                height,
                min_width: 640,
                min_height: 400,
            };
            let mailbox = Arc::new(Mutex::new(Frame {
                bytes: Vec::new(),
                width: 0,
                height: 0,
                damage: Vec::new(),
            }));
            let shutdown = Arc::new(AtomicBool::new(false));
            let event_mailbox = Arc::clone(&mailbox);
            let event_shutdown = Arc::clone(&shutdown);
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let worker = thread::Builder::new()
                .name("sse-win32-window".to_owned())
                .spawn(move || {
                    let mut window = match Win32Window::new(options) {
                        Ok(window) => window,
                        Err(error) => {
                            let _ = ready_tx.send(Err(error));
                            return;
                        }
                    };
                    let wake = window.wake_handle();
                    if ready_tx.send(Ok(wake)).is_err() {
                        return;
                    }
                    let mut modifiers = KeyModifiers::default();
                    let mut pointer = (0, 0);
                    while !event_shutdown.load(Ordering::Acquire) {
                        let event = window.next_event(None);
                        if event_shutdown.load(Ordering::Acquire) {
                            break;
                        }
                        if matches!(event, Event::Wake) {
                            let presented = event_mailbox
                                .lock()
                                .map(|frame| window.present(&frame.bytes, frame.width, frame.height, &frame.damage));
                            if !matches!(presented, Ok(Ok(()))) {
                                proxy.window(WindowEvent::Disconnected);
                                break;
                            }
                            continue;
                        }
                        if !forward_event(event, &proxy, &mut modifiers, &mut pointer) {
                            break;
                        }
                    }
                })
                .map_err(|error| Error::System(error.to_string()))?;

            let wake = match ready_rx.recv() {
                Ok(Ok(wake)) => wake,
                Ok(Err(error)) => {
                    let _ = worker.join();
                    return Err(error);
                }
                Err(error) => {
                    let _ = worker.join();
                    return Err(Error::System(format!("Win32 window thread did not start: {error}")));
                }
            };
            Ok(Self {
                mailbox,
                wake,
                shutdown,
                worker: Some(worker),
            })
        }
    }

    impl Present for Win32Presenter {
        fn present(&mut self, frame: &[u32], stride: usize, width: u32, height: u32, damage: &[Rect]) -> Result<()> {
            let mut mailbox = self
                .mailbox
                .lock()
                .map_err(|_| Error::System("Win32 frame mailbox was poisoned".to_owned()))?;
            mailbox.damage.clear();
            mailbox.damage.reserve(damage.len());
            for rect in damage {
                if let Some(rect) = clip_rect(*rect, width, height) {
                    mailbox.damage.push(NativeRect {
                        x: u32::try_from(rect.x).unwrap_or_default(),
                        y: u32::try_from(rect.y).unwrap_or_default(),
                        width: rect.width,
                        height: rect.height,
                    });
                }
            }
            if mailbox.damage.is_empty() {
                return Ok(());
            }
            frame_to_bgra_into(frame, stride, width, height, &mut mailbox.bytes)?;
            mailbox.width = width;
            mailbox.height = height;
            drop(mailbox);
            self.wake.wake();
            Ok(())
        }
    }

    impl Drop for Win32Presenter {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Release);
            self.wake.wake();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn forward_event<U: Send + 'static>(
        event: Event,
        proxy: &Proxy<U>,
        modifiers: &mut KeyModifiers,
        pointer: &mut (i32, i32),
    ) -> bool {
        match event {
            Event::Timeout | Event::Wake | Event::HotKey(_) => true,
            Event::Close => {
                proxy.window(WindowEvent::CloseRequested);
                false
            }
            Event::Resized { width, height, scale } => {
                if scale.is_finite() && scale > 0.0 {
                    proxy.window(WindowEvent::DpiChanged { scale });
                }
                proxy.window(WindowEvent::Resized { width, height })
            }
            Event::Focus(focused) => proxy.window(WindowEvent::Focus(focused)),
            Event::Key { code, down, .. } => {
                modifiers.update(code, down);
                proxy.window(WindowEvent::Key {
                    pressed: down,
                    keysym: win32_keysym(code),
                    text: None,
                    ctrl: modifiers.control(),
                    shift: modifiers.shift(),
                })
            }
            Event::Text(text) => proxy.window(WindowEvent::Key {
                pressed: true,
                keysym: u32::from(text),
                text: Some(text),
                ctrl: modifiers.control(),
                shift: modifiers.shift(),
            }),
            Event::ImeStart => proxy.window(WindowEvent::Ime(ImeEvent::Start)),
            Event::ImeUpdate(text) => proxy.window(WindowEvent::Ime(ImeEvent::Update(text))),
            Event::ImeCommit(text) => proxy.window(WindowEvent::Ime(ImeEvent::Commit(text))),
            Event::ImeCancel => proxy.window(WindowEvent::Ime(ImeEvent::Cancel)),
            Event::PointerMoved { x, y } => {
                *pointer = (x, y);
                proxy.window(WindowEvent::PointerMoved { x, y })
            }
            Event::PointerButton { button, down } => proxy.window(WindowEvent::Button {
                button: match button {
                    sse_sys::window_win32::MouseButton::Left => 1,
                    sse_sys::window_win32::MouseButton::Middle => 2,
                    sse_sys::window_win32::MouseButton::Right => 3,
                },
                pressed: down,
                x: pointer.0,
                y: pointer.1,
            }),
            Event::Wheel { x, y } => {
                let delta = if y != 0 {
                    y.saturating_div(-120)
                } else {
                    x.saturating_div(120)
                };
                proxy.window(WindowEvent::Wheel { delta })
            }
        }
    }
}

#[cfg(windows)]
pub use platform::Win32Presenter;

#[cfg(test)]
mod tests {
    use super::{clip_damage, frame_to_bgra, win32_keysym, KeyModifiers};
    use crate::raster::Rect;

    #[test]
    fn converts_a_strided_argb_frame_to_tight_bgra_bytes() {
        let frame = [
            0x1122_3344,
            0x5566_7788,
            0xdead_beef,
            0x99aa_bbcc,
            0xddee_ff00,
            0xcafe_babe,
        ];
        let bytes = frame_to_bgra(&frame, 3, 2, 2).unwrap_or_default();
        assert_eq!(
            bytes,
            [0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66, 0x55, 0xcc, 0xbb, 0xaa, 0x99, 0x00, 0xff, 0xee, 0xdd,]
        );
    }

    #[test]
    fn clips_damage_to_frame_bounds_and_discards_empty_rectangles() {
        let damage = [Rect::new(-2, 1, 5, 4), Rect::new(8, 8, 3, 2), Rect::new(2, 0, 2, 1)];
        assert_eq!(
            clip_damage(&damage, 4, 3),
            [Rect::new(0, 1, 3, 2), Rect::new(2, 0, 2, 1)]
        );
    }

    #[test]
    fn maps_win32_navigation_and_letter_keys_to_portable_keysyms() {
        assert_eq!(win32_keysym(0x26), 0xff52);
        assert_eq!(win32_keysym(0x41), u32::from(b'a'));
        assert_eq!(win32_keysym(0xa2), 0xffe3);
    }

    #[test]
    fn tracks_left_and_right_control_and_shift_until_both_are_released() {
        let mut modifiers = KeyModifiers::default();
        modifiers.update(0xa2, true);
        modifiers.update(0xa3, true);
        modifiers.update(0xa2, false);
        assert!(modifiers.control());
        modifiers.update(0x10, true);
        assert!(modifiers.shift());
        modifiers.update(0xa3, false);
        modifiers.update(0x10, false);
        assert!(!modifiers.control());
        assert!(!modifiers.shift());
    }

    #[test]
    fn retains_bgra_buffer_capacity_between_frames() {
        let frame = [0x1122_3344_u32];
        let mut bytes = frame_to_bgra(&frame, 1, 1, 1).unwrap_or_default();
        let allocation = bytes.as_ptr();
        let result = super::frame_to_bgra_into(&frame, 1, 1, 1, &mut bytes);
        assert!(result.is_ok());
        assert_eq!(bytes.as_ptr(), allocation);
        assert_eq!(bytes, [0x44, 0x33, 0x22, 0x11]);
    }
}
