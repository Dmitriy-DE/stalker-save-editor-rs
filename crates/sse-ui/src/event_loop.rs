//! Event loop: one channel carries window events and worker messages while the UI thread sleeps between updates.
//!
//! Channel-backed platforms block on the receiver while idle. A platform with a main-thread native event pump may
//! override [`Present::wait_for_message`]; workers wake it through a [`Proxy`] callback. Every wake drains queued
//! messages, lets the app react, lays out and repaints only the damaged rectangles, then presents them.

use crate::raster::Rect;
use crate::widget::Tree;
use sse_core::Result;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Platform-independent window input.
#[derive(Clone, Debug, PartialEq)]
pub enum WindowEvent {
    /// The native backing scale changed (for example, after moving between DPI-scaled monitors).
    DpiChanged {
        /// Logical-to-framebuffer scale, where `1.0` is 96 DPI.
        scale: f32,
    },
    /// The window has a new size in pixels.
    Resized {
        /// Width.
        width: u32,
        /// Height.
        height: u32,
    },
    /// Part of the window must be drawn again (uncovered).
    Exposed(Rect),
    /// Pointer position in window pixels.
    PointerMoved {
        /// X.
        x: i32,
        /// Y.
        y: i32,
    },
    /// Pointer left the window.
    PointerLeft,
    /// Button 1 = primary, 2 = middle, 3 = secondary.
    Button {
        /// Button number.
        button: u8,
        /// Pressed or released.
        pressed: bool,
        /// X.
        x: i32,
        /// Y.
        y: i32,
    },
    /// Wheel steps, positive = down.
    Wheel {
        /// Steps.
        delta: i32,
    },
    /// Keyboard key.
    Key {
        /// Pressed or released.
        pressed: bool,
        /// X11 keysym value (also used as the portable key code).
        keysym: u32,
        /// Text produced by the key, if any.
        text: Option<char>,
        /// Ctrl held.
        ctrl: bool,
        /// Shift held.
        shift: bool,
    },
    /// Window gained or lost keyboard focus.
    Focus(bool),
    /// The user asked to close the window.
    CloseRequested,
    /// The connection to the display is gone.
    Disconnected,
}

/// Anything that wakes the UI thread.
#[derive(Debug)]
pub enum Message<U> {
    /// Input from the window backend.
    Window(WindowEvent),
    /// A message from a worker thread.
    User(U),
}

/// Callback run after a message is queued to wake a platform event loop.
pub type WakeCallback = Arc<dyn Fn() + Send + Sync>;
type SharedWakeCallback = Arc<Mutex<Option<WakeCallback>>>;

/// Cloneable sender for worker threads.
pub struct Proxy<U> {
    sender: Sender<Message<U>>,
    wake_callback: SharedWakeCallback,
}

impl<U> Clone for Proxy<U> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            wake_callback: Arc::clone(&self.wake_callback),
        }
    }
}

impl<U> std::fmt::Debug for Proxy<U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proxy").finish_non_exhaustive()
    }
}

impl<U> Proxy<U> {
    /// Sends a worker message to the UI thread. Returns false when the UI is gone.
    pub fn send(&self, message: U) -> bool {
        let sent = self.sender.send(Message::User(message)).is_ok();
        if sent {
            self.wake();
        }
        sent
    }

    /// Sends a window event (used by backends).
    pub fn window(&self, event: WindowEvent) -> bool {
        let sent = self.sender.send(Message::Window(event)).is_ok();
        if sent {
            self.wake();
        }
        sent
    }

    /// Installs a callback that wakes a native event loop after a message is queued.
    pub fn set_wake_callback(&self, callback: Option<WakeCallback>) {
        if let Ok(mut slot) = self.wake_callback.lock() {
            *slot = callback;
        }
    }

    fn wake(&self) {
        let callback = self.wake_callback.lock().ok().and_then(|slot| slot.clone());
        if let Some(callback) = callback {
            callback();
        }
    }
}

/// Creates the loop channel.
#[must_use]
pub fn channel_pair<U>() -> (Proxy<U>, Receiver<Message<U>>) {
    let (sender, receiver) = channel();
    (
        Proxy {
            sender,
            wake_callback: Arc::new(Mutex::new(None)),
        },
        receiver,
    )
}

/// Shows frames. Implemented by the X11, Wayland, Win32 and macOS backends and by the headless test backend.
pub trait Present {
    /// Copies `rects` of `frame` (`0xAARRGGBB`, `stride` pixels per row, size `width` × `height`) to the screen.
    ///
    /// # Errors
    /// Returns an error when the display connection fails.
    fn present(&mut self, frame: &[u32], stride: usize, width: u32, height: u32, rects: &[Rect]) -> Result<()>;

    /// Waits for a worker or window message. Backends with a main-thread native event pump may override this.
    ///
    /// The default blocks on the channel used by [`Proxy`].
    fn wait_for_message<U>(&mut self, receiver: &Receiver<Message<U>>) -> Result<Option<Message<U>>> {
        Ok(receiver.recv().ok())
    }
}

/// What the app wants after handling a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Keep running.
    Continue,
    /// Leave the loop.
    Exit,
}

/// Application callbacks. All of them run on the UI thread and must not block.
pub trait App<U> {
    /// Handles one message. Default window handling (resize, expose, hover, clicks) already happened; `clicked` is
    /// the widget a primary click landed on.
    fn message(&mut self, tree: &mut Tree, message: &Message<U>, clicked: Option<crate::widget::WidgetId>) -> Flow;

    /// Handles a native close request. Apps close by default; one with a pending operation may defer closing.
    fn close_requested(&mut self, tree: &mut Tree, message: &Message<U>) -> Flow {
        let _ = self.message(tree, message, None);
        Flow::Exit
    }
}

/// Per-loop statistics, for budgets and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Times the thread woke up.
    pub wakes: u64,
    /// Frames painted.
    pub frames: u64,
    /// Pixels repainted in total.
    pub pixels: u64,
}

/// Runs until the app returns [`Flow::Exit`], the window closes or every sender is gone.
///
/// # Errors
/// Returns an error from layout, painting or the backend.
pub fn run<U, A: App<U>, P: Present>(
    receiver: &Receiver<Message<U>>,
    tree: &mut Tree,
    app: &mut A,
    backend: &mut P,
) -> Result<Stats> {
    let mut stats = Stats::default();
    let mut frame: Vec<u32> = Vec::new();
    while let Some(first) = backend.wait_for_message(receiver)? {
        stats.wakes = stats.wakes.saturating_add(1);
        let mut next = Some(first);
        while let Some(message) = next {
            if handle(tree, app, &message) == Flow::Exit {
                return Ok(stats);
            }
            next = receiver.try_recv().ok();
        }
        if !tree.is_dirty() {
            continue;
        }
        let (width, height) = tree.size();
        let stride = usize::try_from(width).unwrap_or(0);
        let pixels = stride.saturating_mul(usize::try_from(height).unwrap_or(0));
        if frame.len() != pixels {
            frame.clear();
            frame.resize(pixels, 0);
            tree.damage_all();
        }
        let rects = tree.paint(&mut frame, stride)?;
        if rects.is_empty() {
            continue;
        }
        backend.present(&frame, stride, width, height, &rects)?;
        stats.frames = stats.frames.saturating_add(1);
        for rect in &rects {
            stats.pixels = stats
                .pixels
                .saturating_add(u64::from(rect.width).saturating_mul(u64::from(rect.height)));
        }
    }
    Ok(stats)
}

/// Applies one message without entering the blocking native receiver loop.
/// Browser hosts use this to feed events from JavaScript into the same retained UI dispatcher.
pub fn dispatch<U, A: App<U>>(tree: &mut Tree, app: &mut A, message: &Message<U>) -> Flow {
    handle(tree, app, message)
}

fn handle<U, A: App<U>>(tree: &mut Tree, app: &mut A, message: &Message<U>) -> Flow {
    let mut clicked = None;
    if let Message::Window(event) = message {
        match *event {
            WindowEvent::DpiChanged { scale } => tree.set_scale(scale),
            WindowEvent::Resized { width, height } => tree.resize(width, height),
            WindowEvent::Exposed(rect) => tree.add_damage(rect),
            WindowEvent::PointerMoved { x, y } => {
                tree.pointer_moved(x, y);
            }
            WindowEvent::PointerLeft => tree.pointer_left(),
            WindowEvent::Button {
                button: 1,
                pressed,
                x,
                y,
            } => clicked = tree.pointer_button(pressed, x, y),
            WindowEvent::Key {
                pressed: true,
                keysym,
                text,
                ctrl: false,
                ..
            } => {
                let text_buffer = text.map(|ch| ch.to_string());
                let _ = tree.edit_focused_input(keysym, text_buffer.as_deref());
            }
            WindowEvent::CloseRequested => return app.close_requested(tree, message),
            WindowEvent::Disconnected => {
                let _ = app.message(tree, message, None);
                return Flow::Exit;
            }
            _ => {}
        }
    }
    app.message(tree, message, clicked)
}

#[cfg(test)]
mod tests {
    use super::{channel_pair, App, Flow, Message, WindowEvent};
    use crate::glyphs::Fonts;
    use crate::raster::Color;
    use crate::widget::Tree;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn proxy_wakes_a_registered_native_event_loop_after_sending() {
        let (proxy, receiver) = channel_pair::<u8>();
        let calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = Arc::clone(&calls);
        proxy.set_wake_callback(Some(Arc::new(move || {
            callback_calls.fetch_add(1, Ordering::Relaxed);
        })));

        assert!(proxy.send(7));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(matches!(receiver.recv(), Ok(Message::User(7))));
    }

    #[test]
    fn close_request_respects_the_apps_deferred_close_decision() -> sse_core::Result<()> {
        struct KeepOpen;

        impl App<()> for KeepOpen {
            fn message(
                &mut self,
                _tree: &mut Tree,
                _message: &Message<()>,
                _clicked: Option<crate::widget::WidgetId>,
            ) -> Flow {
                Flow::Continue
            }

            fn close_requested(&mut self, _tree: &mut Tree, _message: &Message<()>) -> Flow {
                Flow::Continue
            }
        }

        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut app = KeepOpen;
        assert_eq!(
            super::handle(&mut tree, &mut app, &Message::Window(WindowEvent::CloseRequested)),
            Flow::Continue
        );
        Ok(())
    }
}
