//! Safe browser host for the shared S.T.A.L.K.E.R. editor shell.

use sse_core::{Error, Result};
use sse_ui::event_loop::{self, Flow, Message, WindowEvent};
use sse_ui::glyphs::Fonts;
use sse_ui::screens::shell::Shell;
use sse_ui::screens::style::{rgb, BG_BASE};
use sse_ui::screens::AppMessage;
use sse_ui::widget::Tree;

const MAX_FRAME_PIXELS: u64 = 8_294_400;
const MAX_FRAME_EDGE: u32 = 8192;

/// Browser input normalized to the portable shell event model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebEvent {
    /// Canvas backing-store size in pixels.
    Resize {
        /// Canvas width in pixels.
        width: u32,
        /// Canvas height in pixels.
        height: u32,
    },
    /// Pointer location in canvas pixels.
    PointerMoved {
        /// Horizontal canvas coordinate.
        x: i32,
        /// Vertical canvas coordinate.
        y: i32,
    },
    /// Pointer left the canvas.
    PointerLeft,
    /// Pointer button state and location.
    Button {
        /// Button number: 1 primary, 2 middle, 3 secondary.
        button: u8,
        /// True for press and false for release.
        pressed: bool,
        /// Horizontal canvas coordinate.
        x: i32,
        /// Vertical canvas coordinate.
        y: i32,
    },
    /// Wheel steps; positive values mean down.
    Wheel {
        /// Wheel steps; positive values mean down.
        delta: i32,
    },
    /// Keyboard key and modifiers.
    Key {
        /// Pressed or released.
        pressed: bool,
        /// Portable key code used by the shell.
        keysym: u32,
        /// Text produced by the key, if any.
        text: Option<char>,
        /// Ctrl held.
        ctrl: bool,
        /// Shift held.
        shift: bool,
    },
    /// Window focus state.
    Focus(bool),
    /// Request to close the shell.
    CloseRequested,
}

/// Decodes the scalar ABI used by the small browser script.
///
/// Codes are 0 resize, 1 pointer move, 2 pointer left, 3 button, 4 wheel, 5 key, 6 focus, and 7 close request.
#[must_use]
pub fn decode_event(code: u32, a: i32, b: i32, c: i32, d: i32, e: i32) -> Option<WebEvent> {
    match code {
        0 => Some(WebEvent::Resize {
            width: u32::try_from(a).ok()?,
            height: u32::try_from(b).ok()?,
        }),
        1 => Some(WebEvent::PointerMoved { x: a, y: b }),
        2 => Some(WebEvent::PointerLeft),
        3 => {
            let button = u8::try_from(a).ok()?;
            if !(1..=3).contains(&button) {
                return None;
            }
            Some(WebEvent::Button {
                button,
                pressed: b != 0,
                x: c,
                y: d,
            })
        }
        4 => Some(WebEvent::Wheel { delta: a }),
        5 => Some(WebEvent::Key {
            pressed: a != 0,
            keysym: u32::try_from(b).ok()?,
            text: if c == 0 {
                None
            } else {
                Some(char::from_u32(u32::try_from(c).ok()?)?)
            },
            ctrl: d != 0,
            shift: e != 0,
        }),
        6 => Some(WebEvent::Focus(a != 0)),
        7 => Some(WebEvent::CloseRequested),
        _ => None,
    }
}

/// Retained shell, UI state, and the frame shared with the JavaScript host.
pub struct WebRuntime {
    tree: Tree,
    shell: Shell,
    frame: Vec<u32>,
}

impl WebRuntime {
    /// Creates the editor shell. The first frame is painted on the next call to [`Self::frame`].
    ///
    /// # Errors
    /// Returns an error if a bundled font or the shell tree cannot be initialized.
    pub fn new() -> Result<Self> {
        let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
        let shell = Shell::build(&mut tree, None)?;
        Ok(Self {
            tree,
            shell,
            frame: Vec::new(),
        })
    }

    /// Dispatches browser input through the same event path as native windows.
    pub fn dispatch(&mut self, event: WindowEvent) -> bool {
        event_loop::dispatch(&mut self.tree, &mut self.shell, &Message::<AppMessage>::Window(event)) == Flow::Exit
    }

    /// Dispatches an ABI-decoded browser event through the native window event path.
    pub fn dispatch_browser(&mut self, event: WebEvent) -> bool {
        let window_event = match event {
            WebEvent::Resize { width, height } => WindowEvent::Resized { width, height },
            WebEvent::PointerMoved { x, y } => WindowEvent::PointerMoved { x, y },
            WebEvent::PointerLeft => WindowEvent::PointerLeft,
            WebEvent::Button { button, pressed, x, y } => WindowEvent::Button { button, pressed, x, y },
            WebEvent::Wheel { delta } => WindowEvent::Wheel { delta },
            WebEvent::Key {
                pressed,
                keysym,
                text,
                ctrl,
                shift,
            } => WindowEvent::Key {
                pressed,
                keysym,
                text,
                ctrl,
                shift,
            },
            WebEvent::Focus(focused) => WindowEvent::Focus(focused),
            WebEvent::CloseRequested => WindowEvent::CloseRequested,
        };
        self.dispatch(window_event)
    }

    /// Resizes and paints the retained frame, returning its `0xAARRGGBB` pixels in shared wasm memory.
    ///
    /// Repeated calls without damage return the same buffer without repainting it. The returned slice remains
    /// valid until the next mutating call on this runtime.
    ///
    /// # Errors
    /// Returns an error for a zero-sized or over-budget frame, or a failed layout/paint operation.
    pub fn frame(&mut self, width: u32, height: u32) -> Result<&[u32]> {
        let required = frame_len(width, height)?;
        if self.tree.size() != (width, height) {
            self.tree.resize(width, height);
        }
        if self.frame.len() != required {
            self.frame.resize(required, 0);
        }
        if self.tree.is_dirty() {
            let stride = usize::try_from(width).map_err(|_| Error::Refused("frame width is too large".to_owned()))?;
            self.tree.paint(&mut self.frame, stride)?;
        }
        Ok(&self.frame)
    }
}

/// Returns the bounded number of pixels in a browser frame.
///
/// # Errors
/// Rejects zero dimensions, overflow, an edge above 8192 pixels, or more than 4K UHD pixels.
pub fn frame_len(width: u32, height: u32) -> Result<usize> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| Error::Refused("frame dimensions overflow".to_owned()))?;
    if width == 0 || height == 0 || width > MAX_FRAME_EDGE || height > MAX_FRAME_EDGE || pixels > MAX_FRAME_PIXELS {
        return Err(Error::Refused(format!(
            "frame dimensions {width}x{height} exceed the browser frame limit"
        )));
    }
    usize::try_from(pixels).map_err(|_| Error::Refused("frame is too large for this target".to_owned()))
}

#[cfg(target_arch = "wasm32")]
mod wasm_abi {
    use super::{decode_event, WebRuntime};
    use std::cell::RefCell;

    const CONTINUE: u32 = 0;
    const EXIT: u32 = 1;
    const INVALID_EVENT: u32 = 2;
    const RUNTIME_UNAVAILABLE: u32 = 3;

    thread_local! {
        static RUNTIME: RefCell<Option<WebRuntime>> = const { RefCell::new(None) };
    }

    #[no_mangle]
    pub extern "C" fn sse_web_init() -> u32 {
        RUNTIME.with(|runtime| {
            let Ok(mut runtime) = runtime.try_borrow_mut() else { return RUNTIME_UNAVAILABLE; };
            match WebRuntime::new() {
                Ok(shell) => { *runtime = Some(shell); CONTINUE }
                Err(_) => RUNTIME_UNAVAILABLE,
            }
        })
    }

    #[no_mangle]
    pub extern "C" fn sse_web_event(code: u32, a: i32, b: i32, c: i32, d: i32, e: i32) -> u32 {
        let Some(event) = decode_event(code, a, b, c, d, e) else { return INVALID_EVENT; };
        RUNTIME.with(|runtime| {
            let Ok(mut runtime) = runtime.try_borrow_mut() else { return RUNTIME_UNAVAILABLE; };
            let Some(runtime) = runtime.as_mut() else { return RUNTIME_UNAVAILABLE; };
            if runtime.dispatch_browser(event) { EXIT } else { CONTINUE }
        })
    }

    #[no_mangle]
    pub extern "C" fn sse_web_render(width: u32, height: u32) -> u32 {
        RUNTIME.with(|runtime| {
            let Ok(mut runtime) = runtime.try_borrow_mut() else { return 0; };
            let Some(runtime) = runtime.as_mut() else { return 0; };
            let Ok(frame) = runtime.frame(width, height) else { return 0; };
            u32::try_from(frame.as_ptr() as usize).unwrap_or(0)
        })
    }
}
