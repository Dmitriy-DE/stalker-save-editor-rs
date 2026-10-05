//! Native Wayland presenter for the retained software UI.
//!
//! The project keeps its own widget/raster/event layers.  This adapter only owns the native
//! Wayland surface and copies the already-rendered 0xAARRGGBB framebuffer to it.

use crate::event_loop::{Message, Present, Proxy, WindowEvent};
use crate::raster::Rect;
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use sse_core::{Error, Result};
use std::sync::mpsc::Receiver;
use std::time::Duration;

pub struct WaylandWindow {
    window: Window,
    last_size: (usize, usize),
    last_mouse: Option<(i32, i32)>,
    last_left: bool,
    proxy_wake: ProxyWake,
}

struct ProxyWake;

impl WaylandWindow {
    pub fn open<U: Send + 'static>(title: &str, width: u32, height: u32, proxy: Proxy<U>) -> Result<Self> {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Err(Error::Refused("WAYLAND_DISPLAY is not set".to_owned()));
        }
        let mut options = WindowOptions::default();
        options.resize = true;
        let mut window = Window::new(
            title,
            usize::try_from(width).map_err(|_| Error::Refused("Wayland width is too large".to_owned()))?,
            usize::try_from(height).map_err(|_| Error::Refused("Wayland height is too large".to_owned()))?,
            options,
        )
        .map_err(|error| Error::System(format!("Wayland window: {error}")))?;
        // Pump native events without busy-spinning. The retained event loop still repaints only damage.
        window.set_target_fps(120);
        proxy.set_wake_callback(None);
        Ok(Self {
            window,
            last_size: (usize::try_from(width).unwrap_or(0), usize::try_from(height).unwrap_or(0)),
            last_mouse: None,
            last_left: false,
            proxy_wake: ProxyWake,
        })
    }

    fn native_event<U>(&mut self) -> Option<Message<U>> {
        let _ = &self.proxy_wake;
        if !self.window.is_open() {
            return Some(Message::Window(WindowEvent::CloseRequested));
        }
        self.window.update();
        let size = self.window.get_size();
        if size != self.last_size {
            self.last_size = size;
            return Some(Message::Window(WindowEvent::Resized {
                width: u32::try_from(size.0).unwrap_or(u32::MAX),
                height: u32::try_from(size.1).unwrap_or(u32::MAX),
            }));
        }
        let mouse = self.window.get_mouse_pos(MouseMode::Discard).map(|(x, y)| (x as i32, y as i32));
        if mouse != self.last_mouse {
            self.last_mouse = mouse;
            return Some(Message::Window(match mouse {
                Some((x, y)) => WindowEvent::PointerMoved { x, y },
                None => WindowEvent::PointerLeft,
            }));
        }
        let left = self.window.get_mouse_down(MouseButton::Left);
        if left != self.last_left {
            self.last_left = left;
            let (x, y) = self.last_mouse.unwrap_or((0, 0));
            return Some(Message::Window(WindowEvent::Button { button: 1, pressed: left, x, y }));
        }
        if let Some((_, vertical)) = self.window.get_scroll_wheel() {
            if vertical != 0.0 {
                return Some(Message::Window(WindowEvent::Wheel { delta: if vertical < 0.0 { 1 } else { -1 } }));
            }
        }
        if let Some(key) = self.window.get_keys_pressed(KeyRepeat::No).into_iter().next() {
            return Some(Message::Window(key_event(&self.window, key, true)));
        }
        if let Some(key) = self.window.get_keys_released().into_iter().next() {
            return Some(Message::Window(key_event(&self.window, key, false)));
        }
        None
    }
}

impl Present for WaylandWindow {
    fn present(&mut self, frame: &[u32], stride: usize, width: u32, height: u32, rects: &[Rect]) -> Result<()> {
        if rects.is_empty() {
            return Ok(());
        }
        let width = usize::try_from(width).map_err(|_| Error::damaged("Wayland frame width overflow"))?;
        let height = usize::try_from(height).map_err(|_| Error::damaged("Wayland frame height overflow"))?;
        if stride < width || frame.len() < stride.saturating_mul(height) {
            return Err(Error::damaged("Wayland frame is shorter than its declared size"));
        }
        if stride == width {
            self.window.update_with_buffer(&frame[..width.saturating_mul(height)], width, height)
                .map_err(|error| Error::System(format!("Wayland present: {error}")))?;
        } else {
            let mut tight = Vec::with_capacity(width.saturating_mul(height));
            for row in 0..height {
                let start = row.saturating_mul(stride);
                tight.extend_from_slice(&frame[start..start.saturating_add(width)]);
            }
            self.window.update_with_buffer(&tight, width, height)
                .map_err(|error| Error::System(format!("Wayland present: {error}")))?;
        }
        Ok(())
    }

    fn wait_for_message<U>(&mut self, receiver: &Receiver<Message<U>>) -> Result<Option<Message<U>>> {
        loop {
            if let Ok(message) = receiver.try_recv() {
                return Ok(Some(message));
            }
            if let Some(message) = self.native_event() {
                return Ok(Some(message));
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    }
}

fn key_event(window: &Window, key: Key, pressed: bool) -> WindowEvent {
    let ctrl = window.is_key_down(Key::LeftCtrl) || window.is_key_down(Key::RightCtrl);
    let shift = window.is_key_down(Key::LeftShift) || window.is_key_down(Key::RightShift);
    let keysym = match key {
        Key::A => b'a' as u32, Key::B => b'b' as u32, Key::C => b'c' as u32, Key::D => b'd' as u32,
        Key::E => b'e' as u32, Key::F => b'f' as u32, Key::G => b'g' as u32, Key::H => b'h' as u32,
        Key::I => b'i' as u32, Key::J => b'j' as u32, Key::K => b'k' as u32, Key::L => b'l' as u32,
        Key::M => b'm' as u32, Key::N => b'n' as u32, Key::O => b'o' as u32, Key::P => b'p' as u32,
        Key::Q => b'q' as u32, Key::R => b'r' as u32, Key::S => b's' as u32, Key::T => b't' as u32,
        Key::U => b'u' as u32, Key::V => b'v' as u32, Key::W => b'w' as u32, Key::X => b'x' as u32,
        Key::Y => b'y' as u32, Key::Z => b'z' as u32,
        Key::Enter => 0xff0d, Key::Escape => 0xff1b, Key::Backspace => 0xff08, Key::Tab => 0xff09,
        Key::Up => 0xff52, Key::Down => 0xff54, Key::Left => 0xff51, Key::Right => 0xff53,
        _ => 0,
    };
    let text = if pressed && !ctrl {
        char::from_u32(keysym).map(|value| if shift { value.to_ascii_uppercase() } else { value })
    } else { None };
    WindowEvent::Key { pressed, keysym, text, ctrl, shift }
}
