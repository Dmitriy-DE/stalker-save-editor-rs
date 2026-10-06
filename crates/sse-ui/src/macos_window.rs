//! macOS window adapter for the event-loop `Present` contract.

#[cfg(any(test, target_os = "macos"))]
use crate::raster::Rect;
#[cfg(any(test, target_os = "macos"))]
use sse_core::{Error, Result};
#[cfg(any(test, target_os = "macos"))]
use std::collections::HashSet;

#[cfg(any(test, target_os = "macos"))]
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
        let source = frame
            .get(start..end)
            .ok_or_else(|| Error::damaged("frame buffer is shorter than its dimensions"))?;
        for pixel in source {
            bytes.extend_from_slice(&pixel.to_le_bytes());
        }
    }
    Ok(())
}

#[cfg(any(test, target_os = "macos"))]
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

#[cfg(any(test, target_os = "macos"))]
fn macos_keysym(code: u16) -> u32 {
    match code {
        0 => u32::from(b'a'),
        1 => u32::from(b's'),
        2 => u32::from(b'd'),
        3 => u32::from(b'f'),
        4 => u32::from(b'h'),
        5 => u32::from(b'g'),
        6 => u32::from(b'z'),
        7 => u32::from(b'x'),
        8 => u32::from(b'c'),
        9 => u32::from(b'v'),
        11 => u32::from(b'b'),
        12 => u32::from(b'q'),
        13 => u32::from(b'w'),
        14 => u32::from(b'e'),
        15 => u32::from(b'r'),
        16 => u32::from(b'y'),
        17 => u32::from(b't'),
        18..=29 => b"123465=97-80"
            .get(usize::from(code.saturating_sub(18)))
            .map_or(0, |key| u32::from(*key)),
        31 => u32::from(b'o'),
        32 => u32::from(b'u'),
        34 => u32::from(b'i'),
        35 => u32::from(b'p'),
        36 => 0xff0d,
        37 => u32::from(b'l'),
        38 => u32::from(b'j'),
        40 => u32::from(b'k'),
        45 => u32::from(b'n'),
        46 => u32::from(b'm'),
        48 => 0xff09,
        49 => 0x20,
        51 => 0xff08,
        53 => 0xff1b,
        54 => 0xffeb,
        55 => 0xffeb,
        56 => 0xffe1,
        58 => 0xffe9,
        59 => 0xffe3,
        60 => 0xffe2,
        61 => 0xffea,
        62 => 0xffe4,
        122 => 0xffbe,
        120 => 0xffbf,
        99 => 0xffc0,
        118 => 0xffc1,
        96 => 0xffc2,
        97 => 0xffc3,
        98 => 0xffc4,
        100 => 0xffc5,
        101 => 0xffc6,
        109 => 0xffc7,
        103 => 0xffc8,
        111 => 0xffc9,
        115 => 0xff50,
        116 => 0xff55,
        117 => 0xffff,
        119 => 0xff57,
        121 => 0xff56,
        123 => 0xff51,
        124 => 0xff53,
        125 => 0xff54,
        126 => 0xff52,
        _ => u32::from(code),
    }
}

#[cfg(any(test, target_os = "macos"))]
fn point_to_pixels(x: f64, y: f64, logical_height: f64, scale: f64) -> (i32, i32) {
    if !scale.is_finite() || scale <= 0.0 || !logical_height.is_finite() {
        return (0, 0);
    }
    (rounded_i32(x * scale), rounded_i32((logical_height - y) * scale))
}

#[cfg(any(test, target_os = "macos"))]
fn rounded_i32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    let value = value.round();
    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    let exponent_bits = u16::try_from((bits >> 52) & 0x7ff).unwrap_or_default();
    if exponent_bits == 0 {
        return 0;
    }
    let exponent = i32::from(exponent_bits).saturating_sub(1023);
    if exponent < 0 {
        return 0;
    }
    if exponent > 31 {
        return if negative { i32::MIN } else { i32::MAX };
    }
    let fraction_mask = 0x000f_ffff_ffff_ffff_u64;
    let significand = (bits & fraction_mask) | 0x0010_0000_0000_0000;
    let shift = exponent.saturating_sub(52);
    let magnitude = if shift >= 0 {
        significand
            .checked_shl(u32::try_from(shift).unwrap_or(u32::MAX))
            .unwrap_or(u64::MAX)
    } else {
        significand
            .checked_shr(u32::try_from(shift.saturating_neg()).unwrap_or(u32::MAX))
            .unwrap_or_default()
    };
    let signed = i64::try_from(magnitude).unwrap_or(i64::MAX);
    let signed = if negative { signed.saturating_neg() } else { signed };
    i32::try_from(signed).unwrap_or(if negative { i32::MIN } else { i32::MAX })
}

#[cfg(any(test, target_os = "macos"))]
fn scale_to_f32(value: f64) -> Option<f32> {
    if !value.is_finite() || !(0.25..=16.0).contains(&value) {
        return None;
    }
    let bits = value.to_bits();
    let exponent64 = u16::try_from((bits >> 52) & 0x7ff).ok()?;
    let exponent_unbiased = i32::from(exponent64).saturating_sub(1023);
    let exponent32 = u32::try_from(exponent_unbiased.saturating_add(127)).ok()?;
    let fraction64 = bits & 0x000f_ffff_ffff_ffff;
    let fraction_hi = u32::try_from(fraction64 >> 29).ok()?;
    let fraction_lo = fraction64 & 0x1fff_ffff;
    let halfway = 0x1000_0000_u64;
    let rounded = if fraction_lo > halfway || (fraction_lo == halfway && fraction_hi & 1 != 0) {
        fraction_hi.checked_add(1)?
    } else {
        fraction_hi
    };
    let (exponent32, fraction32) = if rounded >= 0x0080_0000 {
        (exponent32.checked_add(1)?, 0)
    } else {
        (exponent32, rounded)
    };
    Some(f32::from_bits(exponent32.checked_shl(23)? | fraction32))
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Default)]
struct KeyModifiers {
    pressed: HashSet<u16>,
}

#[cfg(any(test, target_os = "macos"))]
impl KeyModifiers {
    fn update(&mut self, code: u16, pressed: bool) {
        if pressed {
            self.pressed.insert(code);
        } else {
            self.pressed.remove(&code);
        }
    }

    fn control(&self) -> bool {
        [54, 55, 59, 62].iter().any(|code| self.pressed.contains(code))
    }

    fn shift(&self) -> bool {
        [56, 60].iter().any(|code| self.pressed.contains(code))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{
        clip_rect, frame_to_bgra_into, macos_keysym, point_to_pixels, rounded_i32, scale_to_f32, KeyModifiers,
    };
    use crate::event_loop::{ImeEvent, Message, Present, Proxy, WindowEvent};
    use crate::raster::Rect;
    use sse_core::Result;
    use sse_sys::window_macos::{Event, MacWindow, MouseButton, Rect as NativeRect, Window, WindowOptions};
    use std::sync::mpsc::{Receiver, TryRecvError};
    use std::sync::Arc;

    /// macOS presenter. AppKit event pumping and frame submission stay on the thread that calls `open` and `run`.
    pub struct MacPresenter<U> {
        window: MacWindow,
        proxy: Proxy<U>,
        bytes: Vec<u8>,
        damage: Vec<NativeRect>,
        logical_height: f64,
        scale: f64,
        pointer: (i32, i32),
        modifiers: KeyModifiers,
    }

    impl<U: Send + 'static> MacPresenter<U> {
        /// Creates the native window on the current thread, which must be the process main thread.
        pub fn open(title: impl Into<String>, width: u32, height: u32, proxy: Proxy<U>) -> Result<Self> {
            let options = WindowOptions {
                title: title.into(),
                width,
                height,
                min_width: 640,
                min_height: 400,
            };
            let window = MacWindow::new(options)?;
            window.set_icon_png(&crate::window_icon::app_icon_png()?)?;
            let wake = window.wake_handle();
            let ui_thread = std::thread::current().id();
            proxy.set_wake_callback(Some(Arc::new(move || {
                if std::thread::current().id() != ui_thread {
                    wake.wake();
                }
            })));
            Ok(Self {
                window,
                proxy,
                bytes: Vec::new(),
                damage: Vec::new(),
                logical_height: f64::from(height),
                scale: 1.0,
                pointer: (0, 0),
                modifiers: KeyModifiers::default(),
            })
        }

        fn forward_event(&mut self, event: Event) -> bool {
            match event {
                Event::Timeout | Event::Wake => true,
                Event::Close => self.proxy.window(WindowEvent::CloseRequested),
                Event::Resized { width, height, scale } => {
                    let Some(scale32) = scale_to_f32(scale) else {
                        return true;
                    };
                    self.scale = scale;
                    let mut sent = self.proxy.window(WindowEvent::DpiChanged { scale: scale32 });
                    if width > 0 && height > 0 {
                        self.logical_height = f64::from(height) / scale;
                        sent &= self.proxy.window(WindowEvent::Resized { width, height });
                    }
                    sent
                }
                Event::ImeStart => self.proxy.window(WindowEvent::Ime(ImeEvent::Start)),
                Event::ImeUpdate(text) => self.proxy.window(WindowEvent::Ime(ImeEvent::Update(text))),
                Event::ImeCommit(text) => self.proxy.window(WindowEvent::Ime(ImeEvent::Commit(text))),
                Event::ImeCancel => self.proxy.window(WindowEvent::Ime(ImeEvent::Cancel)),
                Event::Key { code, down, .. } => {
                    self.modifiers.update(code, down);
                    self.proxy.window(WindowEvent::Key {
                        pressed: down,
                        keysym: macos_keysym(code),
                        text: None,
                        ctrl: self.modifiers.control(),
                        shift: self.modifiers.shift(),
                    })
                }
                Event::Text(text) => {
                    for character in text.chars() {
                        if !self.proxy.window(WindowEvent::Key {
                            pressed: true,
                            keysym: u32::from(character),
                            text: Some(character),
                            ctrl: self.modifiers.control(),
                            shift: self.modifiers.shift(),
                        }) {
                            return false;
                        }
                    }
                    true
                }
                Event::PointerMoved { x, y } => {
                    self.pointer = point_to_pixels(x, y, self.logical_height, self.scale);
                    self.proxy.window(WindowEvent::PointerMoved {
                        x: self.pointer.0,
                        y: self.pointer.1,
                    })
                }
                Event::PointerButton { button, down } => self.proxy.window(WindowEvent::Button {
                    button: match button {
                        MouseButton::Left => 1,
                        MouseButton::Other => 2,
                        MouseButton::Right => 3,
                    },
                    pressed: down,
                    x: self.pointer.0,
                    y: self.pointer.1,
                }),
                Event::Wheel { x: _, y } => self.proxy.window(WindowEvent::Wheel { delta: rounded_i32(-y) }),
            }
        }
    }

    impl<U> Drop for MacPresenter<U> {
        fn drop(&mut self) {
            self.proxy.set_wake_callback(None);
        }
    }

    impl<U: Send + 'static> Present for MacPresenter<U> {
        fn present(&mut self, frame: &[u32], stride: usize, width: u32, height: u32, damage: &[Rect]) -> Result<()> {
            self.damage.clear();
            self.damage.reserve(damage.len());
            for rect in damage {
                if let Some(rect) = clip_rect(*rect, width, height) {
                    self.damage.push(NativeRect {
                        x: u32::try_from(rect.x).unwrap_or_default(),
                        y: u32::try_from(rect.y).unwrap_or_default(),
                        width: rect.width,
                        height: rect.height,
                    });
                }
            }
            if self.damage.is_empty() || width == 0 || height == 0 {
                return Ok(());
            }
            frame_to_bgra_into(frame, stride, width, height, &mut self.bytes)?;
            self.window.present(&self.bytes, width, height, &self.damage)
        }

        fn wait_for_message<V>(&mut self, receiver: &Receiver<Message<V>>) -> Result<Option<Message<V>>> {
            loop {
                match receiver.try_recv() {
                    Ok(message) => return Ok(Some(message)),
                    Err(TryRecvError::Disconnected) => return Ok(None),
                    Err(TryRecvError::Empty) => {}
                }
                let event = self.window.next_event(None);
                if !self.forward_event(event) {
                    return Ok(None);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use platform::MacPresenter;

#[cfg(test)]
mod tests {
    use super::{clip_rect, frame_to_bgra_into, macos_keysym, point_to_pixels, KeyModifiers};
    use crate::raster::Rect;

    #[test]
    fn converts_strided_argb_frame_to_reusable_bgra_bytes() {
        let frame = [
            0x1122_3344,
            0x5566_7788,
            0xdead_beef,
            0x99aa_bbcc,
            0xddee_ff00,
            0xcafe_babe,
        ];
        let mut bytes = Vec::new();
        assert!(frame_to_bgra_into(&frame, 3, 2, 2, &mut bytes).is_ok());
        assert_eq!(
            bytes,
            [0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66, 0x55, 0xcc, 0xbb, 0xaa, 0x99, 0x00, 0xff, 0xee, 0xdd,]
        );
        let allocation = bytes.as_ptr();
        assert!(frame_to_bgra_into(&frame, 3, 2, 2, &mut bytes).is_ok());
        assert_eq!(bytes.as_ptr(), allocation);
    }

    #[test]
    fn clips_damage_and_rejects_offscreen_rectangles() {
        assert_eq!(clip_rect(Rect::new(-2, 1, 5, 4), 4, 3), Some(Rect::new(0, 1, 3, 2)));
        assert_eq!(clip_rect(Rect::new(8, 8, 3, 2), 4, 3), None);
    }

    #[test]
    fn maps_common_mac_virtual_keys_to_portable_keysyms() {
        assert_eq!(macos_keysym(126), 0xff52);
        assert_eq!(macos_keysym(0), u32::from(b'a'));
        assert_eq!(macos_keysym(36), 0xff0d);
    }

    #[test]
    fn translates_cocoa_bottom_left_points_to_scaled_top_left_pixels() {
        assert_eq!(point_to_pixels(10.0, 20.0, 600.0, 2.0), (20, 1160));
        assert_eq!(point_to_pixels(f64::NAN, 20.0, 600.0, 2.0), (0, 1160));
    }

    #[test]
    fn converts_supported_backing_scales_without_lossy_casts() {
        assert_eq!(super::scale_to_f32(1.5), Some(1.5));
        assert_eq!(super::scale_to_f32(f64::NAN), None);
        assert_eq!(super::scale_to_f32(128.0), None);
    }

    #[test]
    fn tracks_left_and_right_control_shift_and_command_modifiers() {
        let mut modifiers = KeyModifiers::default();
        modifiers.update(59, true);
        modifiers.update(62, true);
        modifiers.update(59, false);
        assert!(modifiers.control());
        modifiers.update(55, true);
        modifiers.update(60, true);
        modifiers.update(56, true);
        assert!(modifiers.shift());
        modifiers.update(55, false);
        modifiers.update(62, false);
        modifiers.update(60, false);
        modifiers.update(56, false);
        assert!(!modifiers.control());
        assert!(!modifiers.shift());
    }
}
