//! X18 implementation.
//! Cocoa software window using Objective-C runtime calls only.
//!
//! NSApplication is created without a nib. A runtime-allocated NSView receives text through interpretKeyEvents /
//! insertText:, an NSWindow delegate reports close/backing changes, and a layer-backed view displays a CGImage
//! backed by an owned BGRA copy. No third-party crate is used.

use crate::macos_objc as o;
use sse_core::{Error, Result};
use std::{
    collections::{HashMap, VecDeque},
    ffi::c_void,
    mem,
    path::PathBuf,
    ptr,
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Duration,
};

const YES: o::Bool = 1;
const NO: o::Bool = 0;
const ANY: u64 = u64::MAX;
/// Rectangle in physical framebuffer pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Left pixel.
    pub x: u32,
    /// Top pixel.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}
/// Pointer button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    /// Left.
    Left,
    /// Right.
    Right,
    /// Other.
    Other,
}
/// Native cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    /// Arrow.
    Arrow,
    /// Text beam.
    Text,
    /// Hand.
    Hand,
    /// Horizontal resize.
    ResizeHorizontal,
    /// Vertical resize.
    ResizeVertical,
}
/// Cocoa event.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Timeout.
    Timeout,
    /// Close request.
    Close,
    /// Backing size/scale changed.
    Resized {
        /// Pixel width.
        width: u32,
        /// Pixel height.
        height: u32,
        /// Backing scale.
        scale: f64,
    },
    /// Key transition.
    Key {
        /// Hardware key code.
        code: u16,
        /// Pressed.
        down: bool,
        /// Repeat.
        repeat: bool,
    },
    /// Cocoa text input.
    Text(String),
    /// Input method began marked-text composition.
    ImeStart,
    /// Input method changed marked preedit text.
    ImeUpdate(String),
    /// Input method committed text.
    ImeCommit(String),
    /// Input method cancelled marked text.
    ImeCancel,
    /// Pointer movement.
    PointerMoved {
        /// X.
        x: f64,
        /// Y.
        y: f64,
    },
    /// Button.
    PointerButton {
        /// Button.
        button: MouseButton,
        /// Pressed.
        down: bool,
    },
    /// Scroll.
    Wheel {
        /// Horizontal.
        x: f64,
        /// Vertical.
        y: f64,
    },
    /// Worker wake.
    Wake,
}
/// Creation settings.
#[derive(Clone, Debug)]
pub struct WindowOptions {
    /// Title.
    pub title: String,
    /// Logical width.
    pub width: u32,
    /// Logical height.
    pub height: u32,
    /// Minimum width.
    pub min_width: u32,
    /// Minimum height.
    pub min_height: u32,
}
impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "S.T.A.L.K.E.R. Save Editor".to_owned(),
            width: 1100,
            height: 720,
            min_width: 640,
            min_height: 400,
        }
    }
}
/// UI-facing window contract.
pub trait Window {
    /// Presents BGRA8.
    fn present(&mut self, frame: &[u8], width: u32, height: u32, damage: &[Rect]) -> Result<()>;
    /// Waits for an event.
    fn next_event(&mut self, timeout: Option<Duration>) -> Event;
    /// Sets cursor.
    fn set_cursor(&mut self, cursor: CursorShape);
    /// Sets clipboard text.
    fn set_clipboard_text(&mut self, text: &str) -> Result<()>;
    /// Gets clipboard text.
    fn clipboard_text(&mut self) -> Result<Option<String>>;
    /// File picker.
    fn open_file(&mut self) -> Result<Option<String>>;
    /// Folder picker.
    fn open_folder(&mut self) -> Result<Option<String>>;
    /// Accessibility reduced-motion preference.
    fn reduced_motion(&self) -> bool;
}

struct Shared {
    events: Mutex<VecDeque<Event>>,
    ime_active: Mutex<bool>,
    ime_text_len: Mutex<usize>,
}
static OBJECTS: OnceLock<Mutex<HashMap<usize, Weak<Shared>>>> = OnceLock::new();
fn objects() -> &'static Mutex<HashMap<usize, Weak<Shared>>> {
    OBJECTS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn attach(object: o::Id, shared: &Arc<Shared>) {
    if let Ok(mut map) = objects().lock() {
        map.insert(object as usize, Arc::downgrade(shared));
    }
}
fn shared(object: o::Id) -> Option<Arc<Shared>> {
    objects().lock().ok()?.get(&(object as usize))?.upgrade()
}
fn push(object: o::Id, event: Event) {
    if let Some(s) = shared(object) {
        if let Ok(mut q) = s.events.lock() {
            q.push_back(event);
        }
    }
}

/// Worker-thread wake handle.
#[derive(Clone)]
pub struct WakeHandle {
    shared: Arc<Shared>,
    app: usize,
}
impl WakeHandle {
    /// Posts an application event so nextEventMatchingMask returns immediately.
    pub fn wake(&self) {
        if let Ok(mut q) = self.shared.events.lock() {
            q.push_back(Event::Wake);
        }
        let app = self.app as o::Id;
        if app.is_null() {
            return;
        }
        // Worker threads do not inherit AppKit's main-thread autorelease pool.
        // SAFETY: NSAutoreleasePool alloc/init follows the Objective-C object creation contract.
        let pool = unsafe { o::id(o::id(o::class(c"NSAutoreleasePool"), o::sel(c"alloc")), o::sel(c"init")) };
        let cls = o::class(c"NSEvent");
        type F = unsafe extern "C" fn(o::Id, o::Sel, usize, o::Point, usize, f64, isize, o::Id, isize, isize) -> o::Id;
        // SAFETY: objc_msgSend is cast to the exact NSEvent otherEventWithType selector ABI.
        let f: F = unsafe { mem::transmute(objc_msg_send_ptr()) };
        // SAFETY: selector, receiver and argument ABI match +otherEventWithType:... exactly.
        let event = unsafe {
            f(
                cls,
                o::sel(
                    c"otherEventWithType:location:modifierFlags:timestamp:windowNumber:context:subtype:data1:data2:",
                ),
                15,
                o::Point { x: 0.0, y: 0.0 },
                0,
                0.0,
                0,
                ptr::null_mut(),
                0,
                0,
            )
        };
        if !event.is_null() {
            // SAFETY: app is the live NSApplication and event is a live autoreleased NSEvent.
            unsafe { o::post_event(app, o::sel(c"postEvent:atStart:"), event, YES) };
        }
        if !pool.is_null() {
            // SAFETY: pool was allocated and initialized in this worker-thread call and is drained exactly once.
            unsafe { o::void(pool, o::sel(c"drain")) };
        }
    }
}
fn objc_msg_send_ptr() -> *const c_void {
    unsafe extern "C" {
        fn objc_msgSend();
    }
    objc_msgSend as *const c_void
}

/// Native macOS window.
pub struct MacWindow {
    app: o::Id,
    window: o::Id,
    view: o::Id,
    shared: Arc<Shared>,
}
impl MacWindow {
    /// Creates NSApplication, menu bar, dark NSWindow, runtime view/delegate classes, and a layer-backed content view.
    pub fn new(options: WindowOptions) -> Result<Self> {
        register_classes()?;
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let app = unsafe { o::id(o::class(c"NSApplication"), o::sel(c"sharedApplication")) };
        if app.is_null() {
            return Err(Error::System("NSApplication unavailable".to_owned()));
        }
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        unsafe { o::void_isize(app, o::sel(c"setActivationPolicy:"), 0) };
        install_menu(app)?;
        let rect = o::Rect {
            origin: o::Point { x: 0.0, y: 0.0 },
            size: o::Size {
                width: f64::from(options.width),
                height: f64::from(options.height),
            },
        };
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let allocated = unsafe { o::id(o::class(c"NSWindow"), o::sel(c"alloc")) };
        let window = unsafe {
            o::window_init(
                allocated,
                o::sel(c"initWithContentRect:styleMask:backing:defer:"),
                rect,
                15,
                2,
                NO,
            )
        };
        if window.is_null() {
            return Err(Error::System("NSWindow creation failed".to_owned()));
        }
        let title = o::string(&options.title).ok_or_else(|| Error::Refused("title contains NUL".to_owned()))?;
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        unsafe {
            o::void_id(window, o::sel(c"setTitle:"), title);
            o::void_size(
                window,
                o::sel(c"setContentMinSize:"),
                o::Size {
                    width: f64::from(options.min_width),
                    height: f64::from(options.min_height),
                },
            )
        };
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let view = unsafe {
            o::id_rect(
                o::id(o::class(c"SseFramebufferView"), o::sel(c"alloc")),
                o::sel(c"initWithFrame:"),
                rect,
            )
        };
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let delegate = unsafe { o::id(o::id(o::class(c"SseWindowDelegate"), o::sel(c"alloc")), o::sel(c"init")) };
        let shared = Arc::new(Shared {
            events: Mutex::new(VecDeque::new()),
            ime_active: Mutex::new(false),
            ime_text_len: Mutex::new(0),
        });
        attach(view, &shared);
        attach(delegate, &shared);
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        unsafe {
            o::void_bool(view, o::sel(c"setWantsLayer:"), YES);
            o::void_id(window, o::sel(c"setContentView:"), view);
            o::void_id(window, o::sel(c"setDelegate:"), delegate)
        };
        if let Some(name) = o::string("NSAppearanceNameDarkAqua") {
            // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
            let appearance = unsafe { o::id_id(o::class(c"NSAppearance"), o::sel(c"appearanceNamed:"), name) };
            unsafe { o::void_id(window, o::sel(c"setAppearance:"), appearance) }
        }
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        unsafe {
            o::void(window, o::sel(c"center"));
            o::void_id(window, o::sel(c"makeKeyAndOrderFront:"), ptr::null_mut());
            let _ = o::bool_id(window, o::sel(c"makeFirstResponder:"), view);
            o::void_bool(app, o::sel(c"activateIgnoringOtherApps:"), YES)
        };
        Ok(Self {
            app,
            window,
            view,
            shared,
        })
    }
    /// Sets the application icon using PNG data decoded by AppKit.
    pub fn set_icon_png(&self, png: &[u8]) -> Result<()> {
        if png.is_empty() {
            return Err(Error::Refused("application icon PNG is empty".to_owned()));
        }
        // SAFETY: NSData copies the live byte slice synchronously, and the class/selector match `+dataWithBytes:length:`.
        let data = unsafe {
            o::id_bytes_len(
                o::class(c"NSData"),
                o::sel(c"dataWithBytes:length:"),
                png.as_ptr().cast(),
                png.len(),
            )
        };
        if data.is_null() {
            return Err(Error::System("NSData could not load application icon bytes".to_owned()));
        }
        // SAFETY: NSImage alloc/initWithData: has the object/object ABI; AppKit owns `data` for this synchronous decode.
        let image = unsafe {
            o::id_id(
                o::id(o::class(c"NSImage"), o::sel(c"alloc")),
                o::sel(c"initWithData:"),
                data,
            )
        };
        if image.is_null() {
            return Err(Error::System("AppKit could not decode application icon PNG".to_owned()));
        }
        // SAFETY: `self.app` is the live NSApplication and `image` is a live NSImage; the setter retains the image.
        unsafe {
            o::void_id(self.app, o::sel(c"setApplicationIconImage:"), image);
            o::void(image, o::sel(c"release"));
        }
        Ok(())
    }
    /// Worker wake handle.
    #[must_use]
    pub fn wake_handle(&self) -> WakeHandle {
        WakeHandle {
            shared: self.shared.clone(),
            app: self.app as usize,
        }
    }
    fn panel(&mut self, folder: bool) -> Result<Option<String>> {
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let panel = unsafe { o::id(o::class(c"NSOpenPanel"), o::sel(c"openPanel")) };
        unsafe {
            o::void_bool(panel, o::sel(c"setCanChooseFiles:"), if folder { NO } else { YES });
            o::void_bool(
                panel,
                o::sel(c"setCanChooseDirectories:"),
                if folder { YES } else { NO },
            );
            o::void_bool(panel, o::sel(c"setAllowsMultipleSelection:"), NO)
        };
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        if unsafe { o::isize_(panel, o::sel(c"runModal")) } != 1 {
            return Ok(None);
        }
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let url = unsafe { o::id(panel, o::sel(c"URL")) };
        let path = unsafe { o::id(url, o::sel(c"path")) };
        Ok(o::rust_string(path))
    }
}
impl Drop for MacWindow {
    fn drop(&mut self) {
        if let Ok(mut map) = objects().lock() {
            map.remove(&(self.view as usize));
        }
        if !self.window.is_null() {
            // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
            unsafe {
                o::void(self.window, o::sel(c"close"));
                o::void(self.window, o::sel(c"release"))
            };
        }
    }
}
unsafe extern "C" fn release_pixels(info: *mut c_void, _data: *const c_void, _size: usize) {
    if !info.is_null() {
        // SAFETY: info was produced by Box::into_raw in present and provider calls release exactly once.
        drop(unsafe { Box::from_raw(info.cast::<Vec<u8>>()) });
    }
}
impl Window for MacWindow {
    fn present(&mut self, frame: &[u8], width: u32, height: u32, _damage: &[Rect]) -> Result<()> {
        let row = usize::try_from(width)
            .ok()
            .and_then(|v| v.checked_mul(4))
            .ok_or_else(|| Error::Refused("row overflow".to_owned()))?;
        let needed = row
            .checked_mul(usize::try_from(height).unwrap_or(usize::MAX))
            .ok_or_else(|| Error::Refused("frame overflow".to_owned()))?;
        if frame.len() != needed {
            return Err(Error::Refused("BGRA frame size mismatch".to_owned()));
        }
        let owned = Box::new(frame.to_vec());
        let data = owned.as_ptr();
        let info = Box::into_raw(owned).cast::<c_void>(); // SAFETY: provider owns Box through release_pixels and data remains valid until provider destruction.
        let provider = unsafe { o::CGDataProviderCreateWithData(info, data.cast(), needed, Some(release_pixels)) };
        if provider.is_null() {
            // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
            unsafe { drop(Box::from_raw(info.cast::<Vec<u8>>())) };
            return Err(Error::System("CGDataProviderCreateWithData failed".to_owned()));
        }
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let space = unsafe { o::CGColorSpaceCreateDeviceRGB() };
        if space.is_null() {
            // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
            unsafe { o::CGDataProviderRelease(provider) };
            return Err(Error::System("CGColorSpaceCreateDeviceRGB failed".to_owned()));
        }
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let image = unsafe {
            o::CGImageCreate(
                usize::try_from(width).unwrap_or_default(),
                usize::try_from(height).unwrap_or_default(),
                8,
                32,
                row,
                space,
                0x201,
                provider,
                ptr::null(),
                NO,
                0,
            )
        };
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        unsafe {
            o::CGColorSpaceRelease(space);
            o::CGDataProviderRelease(provider)
        };
        if image.is_null() {
            return Err(Error::System("CGImageCreate failed".to_owned()));
        }
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let layer = unsafe { o::id(self.view, o::sel(c"layer")) };
        unsafe {
            o::void_id(layer, o::sel(c"setContents:"), image);
            let scale = o::f64_(self.window, o::sel(c"backingScaleFactor"));
            type F = unsafe extern "C" fn(o::Id, o::Sel, f64);
            let f: F = mem::transmute(objc_msg_send_ptr());
            f(layer, o::sel(c"setContentsScale:"), scale);
            o::CGImageRelease(image)
        };
        Ok(())
    }
    fn next_event(&mut self, timeout: Option<Duration>) -> Event {
        if let Ok(mut q) = self.shared.events.lock() {
            if let Some(e) = q.pop_front() {
                return e;
            }
        }
        let date = match timeout {
            // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
            Some(d) => unsafe {
                o::date(
                    o::class(c"NSDate"),
                    o::sel(c"dateWithTimeIntervalSinceNow:"),
                    d.as_secs_f64(),
                )
            },
            // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
            None => unsafe { o::id(o::class(c"NSDate"), o::sel(c"distantFuture")) },
        };
        let mode = o::string("kCFRunLoopDefaultMode").unwrap_or(ptr::null_mut());
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let event = unsafe {
            o::event(
                self.app,
                o::sel(c"nextEventMatchingMask:untilDate:inMode:dequeue:"),
                ANY,
                date,
                mode,
                YES,
            )
        };
        if event.is_null() {
            return Event::Timeout;
        }
        decode_event(event, self.view, &self.shared);
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        unsafe {
            o::void_id(self.app, o::sel(c"sendEvent:"), event);
            o::void(self.app, o::sel(c"updateWindows"))
        };
        if let Ok(mut q) = self.shared.events.lock() {
            q.pop_front().unwrap_or(Event::Timeout)
        } else {
            Event::Timeout
        }
    }
    fn set_cursor(&mut self, c: CursorShape) {
        let name = match c {
            CursorShape::Arrow => c"arrowCursor",
            CursorShape::Text => c"IBeamCursor",
            CursorShape::Hand => c"pointingHandCursor",
            CursorShape::ResizeHorizontal => c"resizeLeftRightCursor",
            CursorShape::ResizeVertical => c"resizeUpDownCursor",
        };
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let cur = unsafe { o::id(o::class(c"NSCursor"), o::sel(name)) };
        if !cur.is_null() {
            // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
            unsafe { o::void(cur, o::sel(c"set")) }
        }
    }
    fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let board = unsafe { o::id(o::class(c"NSPasteboard"), o::sel(c"generalPasteboard")) };
        unsafe { o::void(board, o::sel(c"clearContents")) };
        let value = o::string(text).ok_or_else(|| Error::Refused("clipboard contains NUL".to_owned()))?;
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let array = unsafe { o::id_id(o::class(c"NSArray"), o::sel(c"arrayWithObject:"), value) };
        unsafe { o::void_id(board, o::sel(c"writeObjects:"), array) };
        Ok(())
    }
    fn clipboard_text(&mut self) -> Result<Option<String>> {
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let board = unsafe { o::id(o::class(c"NSPasteboard"), o::sel(c"generalPasteboard")) };
        let ty = o::string("public.utf8-plain-text").ok_or_else(|| Error::System("NSString failed".to_owned()))?;
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        Ok(o::rust_string(unsafe {
            o::id_id(board, o::sel(c"stringForType:"), ty)
        }))
    }
    fn open_file(&mut self) -> Result<Option<String>> {
        self.panel(false)
    }
    fn open_folder(&mut self) -> Result<Option<String>> {
        self.panel(true)
    }
    fn reduced_motion(&self) -> bool {
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let workspace = unsafe { o::id(o::class(c"NSWorkspace"), o::sel(c"sharedWorkspace")) };
        (unsafe { o::bool_(workspace, o::sel(c"accessibilityDisplayShouldReduceMotion")) }) != 0
    }
}

/// Opens the Cocoa file picker with multi-select enabled.
///
/// # Errors
/// Returns an error when the native picker returns an invalid selection.
pub fn open_files() -> Result<Option<Vec<PathBuf>>> {
    // SAFETY: NSOpenPanel::openPanel returns a live autoreleased panel on the caller's UI thread.
    let panel = unsafe { o::id(o::class(c"NSOpenPanel"), o::sel(c"openPanel")) };
    // SAFETY: panel is a live NSOpenPanel and these selectors take one Objective-C BOOL.
    unsafe {
        o::void_bool(panel, o::sel(c"setCanChooseFiles:"), YES);
        o::void_bool(panel, o::sel(c"setCanChooseDirectories:"), NO);
        o::void_bool(panel, o::sel(c"setAllowsMultipleSelection:"), YES);
    }
    if let Some(title) = o::string("Открыть сохранение") {
        // SAFETY: panel is live and title is a live NSString retained by the panel during this call.
        unsafe { o::void_id(panel, o::sel(c"setTitle:"), title) };
    }
    // SAFETY: NSOpenPanel::runModal takes no arguments and returns NSModalResponse.
    if unsafe { o::isize_(panel, o::sel(c"runModal")) } != 1 {
        return Ok(None);
    }
    // SAFETY: panel is live and URLs returns its selected NSArray of NSURL objects.
    let urls = unsafe { o::id(panel, o::sel(c"URLs")) };
    // SAFETY: URLs is a live NSArray and count returns NSUInteger.
    let count = unsafe { o::usize_(urls, o::sel(c"count")) };
    if count > crate::file_dialog::MAX_SELECTED_FILES {
        return Err(Error::Refused(
            "native file picker selected more than 512 files".to_owned(),
        ));
    }
    let mut paths = Vec::with_capacity(count);
    for index in 0..count {
        // SAFETY: urls is live and index is less than its reported count.
        let url = unsafe { o::id_usize(urls, o::sel(c"objectAtIndex:"), index) };
        // SAFETY: URL is live and NSURL::path returns its filesystem path string.
        let path = unsafe { o::id(url, o::sel(c"path")) };
        let path = o::rust_string(path)
            .ok_or_else(|| Error::Refused("native file picker returned an invalid path".to_owned()))?;
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(Error::Refused("native file picker returned a relative path".to_owned()));
        }
        paths.push(path);
    }
    Ok(Some(paths))
}

fn register_classes() -> Result<()> {
    if o::class(c"SseFramebufferView").is_null() {
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let cls = unsafe { o::objc_allocateClassPair(o::class(c"NSView"), c"SseFramebufferView".as_ptr(), 0) };
        if cls.is_null() {
            return Err(Error::System("view class allocation failed".to_owned()));
        }
        let text_input_client = o::protocol(c"NSTextInputClient");
        if text_input_client.is_null() {
            return Err(Error::System("NSTextInputClient protocol unavailable".to_owned()));
        }
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        unsafe {
            if o::class_addProtocol(cls, text_input_client) == 0 {
                return Err(Error::System("could not register NSTextInputClient".to_owned()));
            }
            o::class_addMethod(
                cls,
                o::sel(c"acceptsFirstResponder"),
                accepts_first_responder as *const c_void,
                c"c@:".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"insertText:replacementRange:"),
                insert_text as *const c_void,
                c"v@:@{_NSRange=QQ}".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"setMarkedText:selectedRange:replacementRange:"),
                set_marked_text as *const c_void,
                c"v@:@{_NSRange=QQ}{_NSRange=QQ}".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"unmarkText"),
                unmark_text as *const c_void,
                c"v@:".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"hasMarkedText"),
                has_marked_text as *const c_void,
                c"c@:".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"markedRange"),
                marked_range as *const c_void,
                c"{_NSRange=QQ}@:".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"selectedRange"),
                selected_range as *const c_void,
                c"{_NSRange=QQ}@:".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"validAttributesForMarkedText"),
                valid_attributes as *const c_void,
                c"@@:".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"attributedSubstringForProposedRange:actualRange:"),
                attributed_substring as *const c_void,
                c"@@:{_NSRange=QQ}^{_NSRange=QQ}".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"characterIndexForPoint:"),
                character_index_for_point as *const c_void,
                c"Q@:{CGPoint=dd}".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"firstRectForCharacterRange:actualRange:"),
                first_rect_for_range as *const c_void,
                c"{CGRect={CGPoint=dd}{CGSize=dd}}@:@{_NSRange=QQ}^{_NSRange=QQ}".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"doCommandBySelector:"),
                do_command as *const c_void,
                c"v@::".as_ptr(),
            );
            o::objc_registerClassPair(cls)
        }
    }
    if o::class(c"SseWindowDelegate").is_null() {
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        let cls = unsafe { o::objc_allocateClassPair(o::class(c"NSObject"), c"SseWindowDelegate".as_ptr(), 0) };
        if cls.is_null() {
            return Err(Error::System("delegate class allocation failed".to_owned()));
        }
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        unsafe {
            o::class_addMethod(
                cls,
                o::sel(c"windowShouldClose:"),
                should_close as *const c_void,
                c"c@:@".as_ptr(),
            );
            o::class_addMethod(
                cls,
                o::sel(c"windowDidChangeBackingProperties:"),
                backing_changed as *const c_void,
                c"v@:@".as_ptr(),
            );
            o::objc_registerClassPair(cls)
        }
    }
    Ok(())
}
unsafe extern "C" fn should_close(this: o::Id, _: o::Sel, _: o::Id) -> o::Bool {
    push(this, Event::Close);
    NO
}
unsafe extern "C" fn accepts_first_responder(_: o::Id, _: o::Sel) -> o::Bool {
    YES
}
unsafe extern "C" fn backing_changed(this: o::Id, _: o::Sel, note: o::Id) {
    // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
    let window = unsafe { o::id(note, o::sel(c"object")) };
    let scale = if window.is_null() {
        0.0
    } else {
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        unsafe { o::f64_(window, o::sel(c"backingScaleFactor")) }
    };
    push(
        this,
        Event::Resized {
            width: 0,
            height: 0,
            scale,
        },
    );
}
fn object_responds_to(object: o::Id, selector: o::Sel) -> bool {
    type F = unsafe extern "C" fn(o::Id, o::Sel, o::Sel) -> o::Bool;
    // SAFETY: objc_msgSend is called using the exact `respondsToSelector:` ABI.
    let f: F = unsafe { mem::transmute(objc_msg_send_ptr()) };
    // SAFETY: object is a live Objective-C object and selector is a registered selector.
    unsafe { f(object, o::sel(c"respondsToSelector:"), selector) != 0 }
}

fn marked_text_string(text: o::Id) -> Option<String> {
    if text.is_null() {
        return None;
    }
    let string_selector = o::sel(c"string");
    if object_responds_to(text, string_selector) {
        // SAFETY: the object reports the NSAttributedString-compatible `string` selector.
        o::rust_string(unsafe { o::id(text, string_selector) })
    } else {
        o::rust_string(text)
    }
}

unsafe extern "C" fn insert_text(this: o::Id, _: o::Sel, text: o::Id, _: o::Range) {
    let Some(value) = marked_text_string(text) else { return };
    let event = shared(this).map(|state| {
        let mut active = state
            .ime_active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let event = if *active {
            *active = false;
            if let Ok(mut length) = state.ime_text_len.lock() {
                *length = 0;
            }
            Event::ImeCommit(value)
        } else {
            Event::Text(value)
        };
        event
    });
    if let Some(event) = event {
        push(this, event);
    }
}

unsafe extern "C" fn set_marked_text(this: o::Id, _: o::Sel, text: o::Id, _: o::Range, _: o::Range) {
    let Some(value) = marked_text_string(text) else { return };
    let Some(state) = shared(this) else { return };
    let mut active = state
        .ime_active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !*active {
        *active = true;
        push(this, Event::ImeStart);
    }
    if let Ok(mut length) = state.ime_text_len.lock() {
        *length = value.encode_utf16().count();
    }
    push(this, Event::ImeUpdate(value));
}

unsafe extern "C" fn unmark_text(this: o::Id, _: o::Sel) {
    let Some(state) = shared(this) else { return };
    let mut active = state
        .ime_active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *active {
        *active = false;
        if let Ok(mut length) = state.ime_text_len.lock() {
            *length = 0;
        }
        push(this, Event::ImeCancel);
    }
}

unsafe extern "C" fn has_marked_text(this: o::Id, _: o::Sel) -> o::Bool {
    shared(this)
        .and_then(|state| state.ime_active.lock().ok().map(|active| *active))
        .is_some_and(|active| active) as o::Bool
}

unsafe extern "C" fn marked_range(this: o::Id, _: o::Sel) -> o::Range {
    let Some(state) = shared(this) else {
        return o::Range {
            location: usize::MAX,
            length: 0,
        };
    };
    let active = state.ime_active.lock().map(|value| *value).unwrap_or(false);
    let length = state.ime_text_len.lock().map(|value| *value).unwrap_or(0);
    o::Range {
        location: if active { 0 } else { usize::MAX },
        length: if active { length } else { 0 },
    }
}

unsafe extern "C" fn selected_range(_: o::Id, _: o::Sel) -> o::Range {
    o::Range { location: 0, length: 0 }
}

unsafe extern "C" fn valid_attributes(_: o::Id, _: o::Sel) -> o::Id {
    // SAFETY: NSArray::array returns a valid retained/autoreleased empty array for the synchronous call.
    unsafe { o::id(o::class(c"NSArray"), o::sel(c"array")) }
}

unsafe extern "C" fn attributed_substring(_: o::Id, _: o::Sel, _: o::Range, _: *mut o::Range) -> o::Id {
    ptr::null_mut()
}

unsafe extern "C" fn character_index_for_point(_: o::Id, _: o::Sel, _: o::Point) -> usize {
    usize::MAX
}

unsafe extern "C" fn first_rect_for_range(this: o::Id, _: o::Sel, _: o::Range, _: *mut o::Range) -> o::Rect {
    let window = unsafe { o::id(this, o::sel(c"window")) };
    if window.is_null() {
        return o::Rect {
            origin: o::Point { x: 0.0, y: 0.0 },
            size: o::Size {
                width: 1.0,
                height: 18.0,
            },
        };
    }
    // SAFETY: `window` is the live NSWindow returned by the view's `window` selector; `frame` returns NSRect.
    let frame = unsafe { o::rect(window, o::sel(c"frame")) };
    o::Rect {
        origin: o::Point {
            x: frame.origin.x + 16.0,
            y: frame.origin.y + 56.0,
        },
        size: o::Size {
            width: 1.0,
            height: 18.0,
        },
    }
}

unsafe extern "C" fn do_command(_: o::Id, _: o::Sel, _: o::Sel) {}

fn install_menu(app: o::Id) -> Result<()> {
    // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
    let menu = unsafe { o::id(o::id(o::class(c"NSMenu"), o::sel(c"alloc")), o::sel(c"init")) };
    let root = unsafe { o::id(o::id(o::class(c"NSMenuItem"), o::sel(c"alloc")), o::sel(c"init")) };
    // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
    unsafe { o::void_id(menu, o::sel(c"addItem:"), root) };
    let sub = unsafe { o::id(o::id(o::class(c"NSMenu"), o::sel(c"alloc")), o::sel(c"init")) };
    for (title, action, key) in [
        ("Copy", c"copy:", "c"),
        ("Paste", c"paste:", "v"),
        ("Quit", c"terminate:", "q"),
    ] {
        let item = menu_item(title, action, key)?;
        // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
        unsafe { o::void_id(sub, o::sel(c"addItem:"), item) }
    }
    // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
    unsafe {
        o::void_id(root, o::sel(c"setSubmenu:"), sub);
        o::void_id(app, o::sel(c"setMainMenu:"), menu)
    };
    Ok(())
}
fn menu_item(title: &str, action: &'static std::ffi::CStr, key: &str) -> Result<o::Id> {
    let title = o::string(title).ok_or_else(|| Error::System("menu title failed".to_owned()))?;
    let key = o::string(key).ok_or_else(|| Error::System("menu key failed".to_owned()))?;
    // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
    let item = unsafe {
        o::menu_init(
            o::id(o::class(c"NSMenuItem"), o::sel(c"alloc")),
            o::sel(c"initWithTitle:action:keyEquivalent:"),
            title,
            o::sel(action),
            key,
        )
    };
    Ok(item)
}
fn decode_event(event: o::Id, view: o::Id, shared: &Arc<Shared>) {
    // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
    let ty = unsafe { o::usize_(event, o::sel(c"type")) };
    let emit = |e| {
        if let Ok(mut q) = shared.events.lock() {
            q.push_back(e);
        }
    };
    match ty {
        10 | 11 => {
            // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
            let code = unsafe { o::usize_(event, o::sel(c"keyCode")) };
            let repeat = unsafe { o::bool_(event, o::sel(c"isARepeat")) } != 0;
            emit(Event::Key {
                code: u16::try_from(code).unwrap_or_default(),
                down: ty == 10,
                repeat,
            });
            if ty == 10 {
                // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
                let array = unsafe { o::id_id(o::class(c"NSArray"), o::sel(c"arrayWithObject:"), event) };
                unsafe { o::void_id(view, o::sel(c"interpretKeyEvents:"), array) }
            }
        }
        1..=4 | 25 | 26 => {
            let button = if matches!(ty, 1 | 2) {
                MouseButton::Left
            } else if matches!(ty, 3 | 4) {
                MouseButton::Right
            } else {
                MouseButton::Other
            };
            emit(Event::PointerButton {
                button,
                down: matches!(ty, 1 | 3 | 25),
            });
        }
        5..=7 => {
            // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
            let p = unsafe { o::point(event, o::sel(c"locationInWindow")) };
            emit(Event::PointerMoved { x: p.x, y: p.y });
        }
        22 => {
            emit(Event::Wheel {
                // SAFETY: Objective-C receiver, selector, and argument ABI are validated by the surrounding backend code.
                x: unsafe { o::f64_(event, o::sel(c"scrollingDeltaX")) },
                y: unsafe { o::f64_(event, o::sel(c"scrollingDeltaY")) },
            });
        }
        _ => {}
    }
}
