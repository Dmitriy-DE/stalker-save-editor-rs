//! Event loop: one channel carries window events and worker messages; the UI thread sleeps in `recv` while idle.
//!
//! The UI thread never waits for work. Backends push [`WindowEvent`]s from their reader thread, workers push their
//! own messages through a [`Proxy`]. Every wake drains the channel, lets the app react, lays out and repaints only
//! the damaged rectangles, then presents them.

use crate::raster::Rect;
use crate::widget::Tree;
use sse_core::Result;
use std::sync::mpsc::{channel, Receiver, Sender};

/// Platform-independent window input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowEvent {
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

/// Cloneable sender for worker threads.
#[derive(Debug)]
pub struct Proxy<U> {
    sender: Sender<Message<U>>,
}

impl<U> Clone for Proxy<U> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl<U> Proxy<U> {
    /// Sends a worker message to the UI thread. Returns false when the UI is gone.
    pub fn send(&self, message: U) -> bool {
        self.sender.send(Message::User(message)).is_ok()
    }

    /// Sends a window event (used by backends).
    pub fn window(&self, event: WindowEvent) -> bool {
        self.sender.send(Message::Window(event)).is_ok()
    }
}

/// Creates the loop channel.
#[must_use]
pub fn channel_pair<U>() -> (Proxy<U>, Receiver<Message<U>>) {
    let (sender, receiver) = channel();
    (Proxy { sender }, receiver)
}

/// Shows frames. Implemented by the X11, Wayland, Win32 and macOS backends and by the headless test backend.
pub trait Present {
    /// Copies `rects` of `frame` (`0xAARRGGBB`, `stride` pixels per row, size `width` × `height`) to the screen.
    ///
    /// # Errors
    /// Returns an error when the display connection fails.
    fn present(&mut self, frame: &[u32], stride: usize, width: u32, height: u32, rects: &[Rect]) -> Result<()>;
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
    while let Ok(first) = receiver.recv() {
        stats.wakes = stats.wakes.saturating_add(1);
        let mut next = Some(first);
        while let Some(message) = next {
            if dispatch(tree, app, &message) == Flow::Exit {
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

/// Applies one message to the retained tree and application without entering the blocking receiver loop.
///
/// This is used by hosts such as browsers that receive one event at a time from an external event loop.
pub fn dispatch<U, A: App<U>>(tree: &mut Tree, app: &mut A, message: &Message<U>) -> Flow {
    let mut clicked = None;
    if let Message::Window(event) = message {
        match *event {
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
            WindowEvent::CloseRequested | WindowEvent::Disconnected => {
                app.message(tree, message, None);
                return Flow::Exit;
            }
            _ => {}
        }
    }
    app.message(tree, message, clicked)
}
