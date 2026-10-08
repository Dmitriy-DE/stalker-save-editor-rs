//! X17 implementation.
//! Native Win32 software window.
//!
//! The backend uses only hand-written Win32 ABI declarations. Every native call is isolated behind a documented
//! SAFETY boundary. Rendering is BGRA8 through SetDIBitsToDevice and only invalidated damage rectangles are copied.

use crate::win32_ffi as w;
use sse_core::{Error, Result};
use std::{
    collections::VecDeque,
    ffi::c_void,
    mem, ptr,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

const WM_DESTROY: u32 = 2;
const WM_SIZE: u32 = 5;
const WM_SETFOCUS: u32 = 7;
const WM_KILLFOCUS: u32 = 8;
const WM_PAINT: u32 = 15;
const WM_CLOSE: u32 = 16;
const WM_ERASEBKGND: u32 = 20;
const WM_SETCURSOR: u32 = 32;
const WM_GETMINMAXINFO: u32 = 36;
const WM_KEYDOWN: u32 = 0x100;
const WM_KEYUP: u32 = 0x101;
const WM_CHAR: u32 = 0x102;
const WM_IME_STARTCOMPOSITION: u32 = 0x10d;
const WM_IME_ENDCOMPOSITION: u32 = 0x10e;
const WM_IME_COMPOSITION: u32 = 0x10f;
const WM_IME_CHAR: u32 = 0x286;
const GCS_COMPSTR: u32 = 0x0008;
const GCS_RESULTSTR: u32 = 0x0800;
const WM_MOUSEMOVE: u32 = 0x200;
const WM_LDOWN: u32 = 0x201;
const WM_LUP: u32 = 0x202;
const WM_RDOWN: u32 = 0x204;
const WM_RUP: u32 = 0x205;
const WM_MDOWN: u32 = 0x207;
const WM_MUP: u32 = 0x208;
const WM_WHEEL: u32 = 0x20a;
const WM_HWHEEL: u32 = 0x20e;
const WM_DPICHANGED: u32 = 0x2e0;
const WM_HOTKEY: u32 = 0x312;
const PM_REMOVE: u32 = 1;
const QS_ALLINPUT: u32 = 0x4ff;
const WAIT_TIMEOUT: u32 = 258;
const INFINITE: u32 = u32::MAX;
const WS_OVERLAPPEDWINDOW: u32 = 0x00cf0000;
const WS_VISIBLE: u32 = 0x10000000;
const GWLP_USERDATA: i32 = -21;
const DIB_RGB_COLORS: u32 = 0;
const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 2;
const MOD_NOREPEAT: u32 = 0x4000;

/// Rectangle in physical framebuffer pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Windows client rectangle in pixels.
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
    /// Middle.
    Middle,
}
/// System cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    /// Arrow.
    Arrow,
    /// Text.
    Text,
    /// Hand.
    Hand,
    /// Horizontal resize.
    ResizeHorizontal,
    /// Vertical resize.
    ResizeVertical,
    /// Busy.
    Wait,
}
/// Native event.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Wait timed out.
    Timeout,
    /// Close requested.
    Close,
    /// Client size/DPI changed.
    Resized {
        /// Pixel width.
        width: u32,
        /// Pixel height.
        height: u32,
        /// DPI/96.
        scale: f32,
    },
    /// Focus state.
    Focus(bool),
    /// Key transition.
    Key {
        /// Virtual key.
        code: u32,
        /// Pressed.
        down: bool,
        /// Repeat.
        repeat: bool,
    },
    /// Decoded WM_CHAR text.
    Text(char),
    /// Input method started a composition.
    ImeStart,
    /// Input method changed its preedit text.
    ImeUpdate(String),
    /// Input method committed text.
    ImeCommit(String),
    /// Input method cancelled its composition.
    ImeCancel,
    /// Pointer position.
    PointerMoved {
        /// X.
        x: i32,
        /// Y.
        y: i32,
    },
    /// Button transition.
    PointerButton {
        /// Button.
        button: MouseButton,
        /// Pressed.
        down: bool,
    },
    /// Wheel.
    Wheel {
        /// Horizontal.
        x: i32,
        /// Vertical.
        y: i32,
    },
    /// Hotkey ID.
    HotKey(i32),
    /// Worker wake.
    Wake,
}
/// Creation settings.
#[derive(Clone, Debug)]
pub struct WindowOptions {
    /// Title.
    pub title: String,
    /// Width.
    pub width: u32,
    /// Height.
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
    /// Presents BGRA8 pixels.
    fn present(&mut self, frame: &[u8], width: u32, height: u32, damage: &[Rect]) -> Result<()>;
    /// Sleeps until one event or timeout.
    fn next_event(&mut self, timeout: Option<Duration>) -> Event;
    /// Sets cursor.
    fn set_cursor(&mut self, cursor: CursorShape);
    /// Sets clipboard text.
    fn set_clipboard_text(&mut self, text: &str) -> Result<()>;
    /// Gets clipboard text.
    fn clipboard_text(&mut self) -> Result<Option<String>>;
    /// Registers hotkey.
    fn register_hotkey(&mut self, id: i32, modifiers: u32, key: u32) -> Result<()>;
    /// Native file picker.
    fn open_file(&mut self) -> Result<Option<String>>;
    /// Native folder picker.
    fn open_folder(&mut self) -> Result<Option<String>>;
    /// High contrast preference.
    fn high_contrast(&self) -> bool;
    /// Reduced motion preference.
    fn reduced_motion(&self) -> bool;
}

struct WakeInner(usize);
// SAFETY: a Win32 event HANDLE may be signalled from any thread.
unsafe impl Send for WakeInner {}
// SAFETY: SetEvent is thread-safe and does not expose Rust memory.
unsafe impl Sync for WakeInner {}
impl Drop for WakeInner {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: this Arc is the final owner of a CreateEventW handle.
            let _ = unsafe { w::CloseHandle(self.0 as w::Handle) };
        }
    }
}
/// Cloneable worker wake primitive.
#[derive(Clone)]
pub struct WakeHandle(Arc<WakeInner>);
impl WakeHandle {
    /// Wakes the UI wait.
    pub fn wake(&self) {
        if self.0 .0 != 0 {
            // SAFETY: live event handle, SetEvent is thread-safe.
            let _ = unsafe { w::SetEvent(self.0 .0 as w::Handle) };
        }
    }
}

struct State {
    frame: Vec<u8>,
    width: u32,
    height: u32,
    events: VecDeque<Event>,
    min_w: u32,
    min_h: u32,
    high: Option<u16>,
    ime_active: bool,
    cursor: CursorShape,
}
/// Win32 implementation.
pub struct Win32Window {
    hwnd: w::Hwnd,
    state: *mut Mutex<State>,
    wake: WakeHandle,
    hotkeys: Vec<i32>,
    icons: Vec<w::Hicon>,
    com: bool,
}
impl Win32Window {
    /// Creates a visible per-monitor-v2-DPI window with a dark title bar.
    pub fn new(options: WindowOptions) -> Result<Self> {
        // SAFETY: -4 is DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2; fallback is PROCESS_PER_MONITOR_DPI_AWARE.
        if unsafe { w::SetProcessDpiAwarenessContext(-4) } == 0 {
            // SAFETY: this process-wide API takes only the documented awareness enum and no pointers.
            let _ = unsafe { w::SetProcessDpiAwareness(2) };
        }
        // SAFETY: null reserved pointer and COINIT_APARTMENTTHREADED initialize COM for this thread.
        let com = unsafe { w::CoInitializeEx(ptr::null_mut(), 2) } >= 0;
        // SAFETY: null security/name pointers request an unnamed auto-reset event with an initially nonsignaled state.
        let event = unsafe { w::CreateEventW(ptr::null(), 0, 0, ptr::null()) };
        if event.is_null() {
            return Err(Error::System("CreateEventW failed".to_owned()));
        }
        let wake = WakeHandle(Arc::new(WakeInner(event as usize)));
        let class = wide("SseWindow");
        let title = wide(&options.title);
        let wc = w::WndClass {
            size: u32::try_from(mem::size_of::<w::WndClass>()).unwrap_or_default(),
            style: 3,
            proc: Some(proc),
            class_extra: 0,
            window_extra: 0,
            instance: ptr::null_mut(),
            icon: ptr::null_mut(),
            cursor: cursor_handle(CursorShape::Arrow),
            background: ptr::null_mut(),
            menu: ptr::null(),
            name: class.as_ptr(),
            small_icon: ptr::null_mut(),
        }; // SAFETY: WNDCLASSEX and UTF-16 name are valid for this call.
        let _ = unsafe { w::RegisterClassExW(&wc) };
        let state = Box::into_raw(Box::new(Mutex::new(State {
            frame: Vec::new(),
            width: options.width,
            height: options.height,
            events: VecDeque::new(),
            min_w: options.min_width,
            min_h: options.min_height,
            high: None,
            ime_active: false,
            cursor: CursorShape::Arrow,
        })));
        let width = i32::try_from(options.width).map_err(|_| Error::Refused("window width too large".to_owned()))?;
        let height = i32::try_from(options.height).map_err(|_| Error::Refused("window height too large".to_owned()))?; // SAFETY: registered class, stable Box pointer, NUL-terminated strings.
        let hwnd = unsafe {
            w::CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                i32::MIN,
                i32::MIN,
                width,
                height,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                state.cast::<c_void>(),
            )
        };
        if hwnd.is_null() {
            // SAFETY: CreateWindowExW failed, so no HWND can retain the raw Box allocation.
            drop(unsafe { Box::from_raw(state) });
            return Err(Error::System("CreateWindowExW failed".to_owned()));
        }
        // SAFETY: state came from Box::into_raw and remains allocated until Win32Window::drop.
        unsafe { w::SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
        let dark: i32 = 1; // SAFETY: attribute 20 consumes a BOOL-sized value.
        let _ = unsafe {
            w::DwmSetWindowAttribute(
                hwnd,
                20,
                (&dark as *const i32).cast(),
                u32::try_from(mem::size_of::<i32>()).unwrap_or_default(),
            )
        };
        // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
        unsafe {
            w::ShowWindow(hwnd, 5);
            w::UpdateWindow(hwnd);
        }
        Ok(Self {
            hwnd,
            state,
            wake,
            hotkeys: Vec::new(),
            icons: Vec::new(),
            com,
        })
    }
    fn state(&self) -> MutexGuard<'_, State> {
        // SAFETY: state is a Box::into_raw allocation owned by this window and reclaimed only after HWND destruction.
        unsafe { &*self.state }
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Handle suitable for worker threads.
    #[must_use]
    pub fn wake_handle(&self) -> WakeHandle {
        self.wake.clone()
    }
    /// Creates a window icon from tightly packed RGBA8 pixels.
    pub fn set_icon_rgba(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
        let pixels = usize::try_from(width)
            .ok()
            .and_then(|a| usize::try_from(height).ok().and_then(|b| a.checked_mul(b)))
            .and_then(|v| v.checked_mul(4))
            .ok_or_else(|| Error::Refused("icon dimensions overflow".to_owned()))?;
        if rgba.len() != pixels {
            return Err(Error::Refused("icon byte count mismatch".to_owned()));
        }
        let mut bgra = Vec::with_capacity(pixels);
        for p in rgba.chunks_exact(4) {
            bgra.push(*p.get(2).unwrap_or(&0));
            bgra.push(*p.get(1).unwrap_or(&0));
            bgra.push(*p.first().unwrap_or(&0));
            bgra.push(*p.get(3).unwrap_or(&0));
        }
        let wi = i32::try_from(width).map_err(|_| Error::Refused("icon too wide".to_owned()))?;
        let hi = i32::try_from(height).map_err(|_| Error::Refused("icon too tall".to_owned()))?; // SAFETY: CreateBitmap copies supplied pixels.
        let color = unsafe { w::CreateBitmap(wi, hi, 1, 32, bgra.as_ptr().cast()) };
        let mask = vec![
            0u8;
            usize::try_from(height).unwrap_or_default().saturating_mul(
                usize::try_from(width.saturating_add(7))
                    .unwrap_or_default()
                    .checked_div(8)
                    .unwrap_or(0)
            )
        ];
        // SAFETY: CreateBitmap copies the live monochrome mask bytes during the call.
        let mono = unsafe { w::CreateBitmap(wi, hi, 1, 1, mask.as_ptr().cast()) };
        if color.is_null() || mono.is_null() {
            if !color.is_null() {
                // SAFETY: color is an owned HBITMAP returned by CreateBitmap and was not transferred.
                let _ = unsafe { w::DeleteObject(color) };
            }
            if !mono.is_null() {
                // SAFETY: mono is an owned HBITMAP returned by CreateBitmap and was not transferred.
                let _ = unsafe { w::DeleteObject(mono) };
            }
            return Err(Error::System("icon bitmap creation failed".to_owned()));
        }
        let info = w::IconInfo {
            icon: 1,
            x: 0,
            y: 0,
            mask: mono,
            color,
        };
        // SAFETY: both bitmap handles are live and their metadata is valid for CreateIconIndirect.
        let icon = unsafe { w::CreateIconIndirect(&info) };
        // SAFETY: CreateIconIndirect copies the bitmap data; these handles remain owned by this function.
        unsafe {
            w::DeleteObject(color);
            w::DeleteObject(mono);
        }
        if icon.is_null() {
            return Err(Error::System("CreateIconIndirect failed".to_owned()));
        }
        // SAFETY: WM_SETICON accepts the live HICON in lParam; ownership remains with this window.
        unsafe {
            w::PostMessageW(self.hwnd, 0x80, 1, icon as isize);
            w::PostMessageW(self.hwnd, 0x80, 0, icon as isize);
        }
        self.icons.push(icon);
        Ok(())
    }
    fn dialog(&mut self, folders: bool) -> Result<Option<String>> {
        file_dialog(self.hwnd, folders)
    }
}
impl Drop for Win32Window {
    fn drop(&mut self) {
        for id in &self.hotkeys {
            // SAFETY: hwnd is live until DestroyWindow below and each id was registered on it.
            let _ = unsafe { w::UnregisterHotKey(self.hwnd, *id) };
        }
        if !self.hwnd.is_null() {
            // SAFETY: clearing GWLP_USERDATA prevents destruction callbacks from observing the state allocation.
            unsafe { w::SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0) };
            // SAFETY: hwnd is owned by this Win32Window and is destroyed exactly once.
            let _ = unsafe { w::DestroyWindow(self.hwnd) };
        }
        for icon in self.icons.drain(..) {
            if !icon.is_null() {
                // SAFETY: each icon was created by CreateIconIndirect and ownership was retained by this window.
                let _ = unsafe { w::DestroyIcon(icon) };
            }
        }
        if !self.state.is_null() {
            // SAFETY: state came from Box::into_raw, GWLP_USERDATA is cleared, and the HWND is already destroyed.
            drop(unsafe { Box::from_raw(self.state) });
            self.state = ptr::null_mut();
        }
        if self.com {
            // SAFETY: this thread successfully initialized COM in Win32Window::new.
            unsafe { w::CoUninitialize() };
        }
    }
}
impl Window for Win32Window {
    fn present(&mut self, frame: &[u8], width: u32, height: u32, damage: &[Rect]) -> Result<()> {
        let bytes = usize::try_from(width)
            .ok()
            .and_then(|a| usize::try_from(height).ok().and_then(|b| a.checked_mul(b)))
            .and_then(|v| v.checked_mul(4))
            .ok_or_else(|| Error::Refused("frame dimensions overflow".to_owned()))?;
        if frame.len() != bytes {
            return Err(Error::Refused("BGRA frame size mismatch".to_owned()));
        }
        {
            let mut state = self.state();
            state.frame.clear();
            state.frame.extend_from_slice(frame);
            state.width = width;
            state.height = height;
        }
        for d in damage {
            let r = w::Rect {
                left: i32::try_from(d.x).unwrap_or_default(),
                top: i32::try_from(d.y).unwrap_or_default(),
                right: i32::try_from(d.x.saturating_add(d.width)).unwrap_or(i32::MAX),
                bottom: i32::try_from(d.y.saturating_add(d.height)).unwrap_or(i32::MAX),
            }; // SAFETY: live HWND; no erase prevents resize flicker.
            unsafe {
                w::InvalidateRect(self.hwnd, &r, 0);
            }
        }
        if damage.is_empty() {
            // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
            unsafe {
                w::InvalidateRect(self.hwnd, ptr::null(), 0);
            }
        }
        Ok(())
    }
    fn next_event(&mut self, timeout: Option<Duration>) -> Event {
        if let Some(e) = self.state().events.pop_front() {
            return e;
        }
        let ms = timeout.map_or(INFINITE, |d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX));
        let handle = self.wake.0 .0 as w::Handle; // SAFETY: event handle is live; wait sleeps without polling.
        let wait = unsafe { w::MsgWaitForMultipleObjects(1, &handle, 0, ms, QS_ALLINPUT) };
        if wait == WAIT_TIMEOUT {
            return Event::Timeout;
        }
        if wait == 0 {
            return Event::Wake;
        }
        let mut msg = w::Msg {
            hwnd: ptr::null_mut(),
            message: 0,
            w_param: 0,
            l_param: 0,
            time: 0,
            point: w::Point { x: 0, y: 0 },
            private: 0,
        };
        loop {
            // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
            if unsafe { w::PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) } == 0 {
                break;
            }
            // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
            unsafe {
                w::TranslateMessage(&msg);
                w::DispatchMessageW(&msg);
            }
            if let Some(e) = self.state().events.pop_front() {
                return e;
            }
        }
        Event::Timeout
    }
    fn set_cursor(&mut self, c: CursorShape) {
        self.state().cursor = c;
        let h = cursor_handle(c);
        if !h.is_null() {
            // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
            unsafe {
                w::SetCursor(h);
            }
        }
    }
    fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        let data = wide(text);
        let bytes = data
            .len()
            .checked_mul(2)
            .ok_or_else(|| Error::Refused("clipboard too large".to_owned()))?;
        // SAFETY: hwnd is live and clipboard ownership is released on every return path below.
        // SAFETY: hwnd is live and clipboard ownership is released on every return path below.
        if unsafe { w::OpenClipboard(self.hwnd) } == 0 {
            return Err(Error::System("OpenClipboard failed".to_owned()));
        }
        // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
        unsafe { w::EmptyClipboard() };
        // SAFETY: the clipboard is open and `bytes` was checked against overflow for the UTF-16 payload.
        let mem = unsafe { w::GlobalAlloc(GMEM_MOVEABLE, bytes) };
        if mem.is_null() {
            // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
            unsafe { w::CloseClipboard() };
            return Err(Error::System("GlobalAlloc failed".to_owned()));
        }
        // SAFETY: mem is the movable global allocation created above.
        let dst = unsafe { w::GlobalLock(mem) };
        if dst.is_null() {
            // SAFETY: SetClipboardData has not taken ownership, so this allocation is still ours.
            let _ = unsafe { w::GlobalFree(mem) };
            // SAFETY: this call closes the clipboard opened above.
            unsafe { w::CloseClipboard() };
            return Err(Error::System("GlobalLock failed".to_owned()));
        }
        // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr().cast::<u8>(), dst.cast::<u8>(), bytes);
            w::GlobalUnlock(mem);
        }
        // SAFETY: mem contains a NUL-terminated UTF-16 buffer and remains owned by us until this succeeds.
        let set = unsafe { w::SetClipboardData(CF_UNICODETEXT, mem) };
        // SAFETY: this call closes the clipboard opened above.
        unsafe { w::CloseClipboard() };
        if set.is_null() {
            // SAFETY: failed SetClipboardData did not transfer ownership of mem.
            let _ = unsafe { w::GlobalFree(mem) };
            Err(Error::System("SetClipboardData failed".to_owned()))
        } else {
            Ok(())
        }
    }
    fn clipboard_text(&mut self) -> Result<Option<String>> {
        // SAFETY: querying clipboard format availability has no pointer preconditions.
        if unsafe { w::IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
            return Ok(None);
        }
        // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
        if unsafe { w::OpenClipboard(self.hwnd) } == 0 {
            return Err(Error::System("OpenClipboard failed".to_owned()));
        }
        // SAFETY: clipboard is open and CF_UNICODETEXT availability was checked above.
        let mem = unsafe { w::GetClipboardData(CF_UNICODETEXT) };
        if mem.is_null() {
            // SAFETY: this call balances the successful OpenClipboard above.
            unsafe { w::CloseClipboard() };
            return Ok(None);
        }
        // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
        let units = unsafe { w::GlobalSize(mem) }.checked_div(2).unwrap_or(0);
        // SAFETY: mem is the live global-memory handle returned by GetClipboardData while the clipboard is open.
        let raw = unsafe { w::GlobalLock(mem) };
        if raw.is_null() {
            // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
            unsafe { w::CloseClipboard() };
            return Err(Error::System("GlobalLock failed".to_owned()));
        }
        // SAFETY: GlobalLock returned a buffer of GlobalSize(mem) bytes, so units u16 values are readable.
        let slice = unsafe { std::slice::from_raw_parts(raw.cast::<u16>(), units) };
        let end = slice.iter().position(|v| *v == 0).unwrap_or(slice.len());
        let text = String::from_utf16_lossy(slice.get(..end).unwrap_or_default());
        // SAFETY: this Win32 FFI boundary uses the live handles/pointers established by the surrounding checks.
        unsafe {
            w::GlobalUnlock(mem);
            w::CloseClipboard();
        }
        Ok(Some(text))
    }
    fn register_hotkey(&mut self, id: i32, modifiers: u32, key: u32) -> Result<()> {
        // SAFETY: hwnd is live; id/modifier/key are plain Win32 hotkey values.
        if unsafe { w::RegisterHotKey(self.hwnd, id, modifiers | MOD_NOREPEAT, key) } == 0 {
            return Err(Error::System("RegisterHotKey failed".to_owned()));
        }
        self.hotkeys.push(id);
        Ok(())
    }
    fn open_file(&mut self) -> Result<Option<String>> {
        self.dialog(false)
    }
    fn open_folder(&mut self) -> Result<Option<String>> {
        self.dialog(true)
    }
    fn high_contrast(&self) -> bool {
        let mut v = w::HighContrast {
            size: u32::try_from(mem::size_of::<w::HighContrast>()).unwrap_or_default(),
            flags: 0,
            scheme: ptr::null_mut(),
        };
        // SAFETY: v points to writable HIGHCONTRAST storage with the documented size.
        (unsafe { w::SystemParametersInfoW(0x42, v.size, (&mut v as *mut w::HighContrast).cast(), 0) }) != 0
            && v.flags & 1 != 0
    }
    fn reduced_motion(&self) -> bool {
        let mut enabled: i32 = 1;
        // SAFETY: enabled points to writable BOOL-sized storage for SPI_GETCLIENTAREAANIMATION.
        (unsafe { w::SystemParametersInfoW(0x1042, 0, (&mut enabled as *mut i32).cast(), 0) }) != 0 && enabled == 0
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
fn word(v: isize, high: bool) -> i32 {
    let raw = if high {
        (v as usize).checked_shr(16).unwrap_or(0) & 0xffff
    } else {
        v as usize & 0xffff
    };
    i32::from(i16::from_le_bytes(u16::try_from(raw).unwrap_or_default().to_le_bytes()))
}
fn wheel(v: usize) -> i32 {
    i32::from(i16::from_le_bytes(
        u16::try_from(v.checked_shr(16).unwrap_or(0) & 0xffff)
            .unwrap_or_default()
            .to_le_bytes(),
    ))
}
fn cursor_handle(c: CursorShape) -> w::Hcursor {
    let id = match c {
        CursorShape::Arrow => 32512,
        CursorShape::Text => 32513,
        CursorShape::Wait => 32514,
        CursorShape::ResizeHorizontal => 32644,
        CursorShape::ResizeVertical => 32645,
        CursorShape::Hand => 32649,
    };
    // SAFETY: MAKEINTRESOURCE-style cursor IDs are documented for LoadCursorW with a null instance.
    unsafe { w::LoadCursorW(ptr::null_mut(), id as usize as *const u16) }
}
unsafe extern "system" fn proc(hwnd: w::Hwnd, msg: u32, wp: usize, lp: isize) -> isize {
    // SAFETY: hwnd is supplied by Windows to this registered window procedure.
    let raw = unsafe { w::GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Mutex<State>;
    if raw.is_null() {
        // SAFETY: forwarding unhandled messages to the default procedure is required by Win32.
        return unsafe { w::DefWindowProcW(hwnd, msg, wp, lp) };
    }
    // SAFETY: GWLP_USERDATA is set from Box::into_raw after window creation and cleared before destruction.
    let state = unsafe { &*raw };
    match msg {
        WM_CLOSE => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::Close);
            0
        }
        WM_DESTROY => {
            // SAFETY: posting WM_QUIT does not retain hwnd or Rust references.
            unsafe { w::PostQuitMessage(0) };
            0
        }
        WM_ERASEBKGND => 1,
        WM_SETFOCUS => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::Focus(true));
            0
        }
        WM_KILLFOCUS => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::Focus(false));
            0
        }
        WM_SIZE => {
            let width = u32::try_from(word(lp, false)).unwrap_or_default();
            let height = u32::try_from(word(lp, true)).unwrap_or_default();
            // SAFETY: hwnd is the live window receiving WM_SIZE.
            let dpi = unsafe { w::GetDpiForWindow(hwnd) };
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::Resized {
                    width,
                    height,
                    scale: (dpi as f32).mul_add(0.010416667, 0.0),
                });
            0
        }
        WM_DPICHANGED => {
            // SAFETY: WM_DPICHANGED lParam points to a RECT valid for the duration of this callback.
            let rect = unsafe { &*(lp as *const w::Rect) };
            let width = rect.right.saturating_sub(rect.left);
            let height = rect.bottom.saturating_sub(rect.top);
            // SAFETY: hwnd and the suggested rectangle are valid for this WM_DPICHANGED callback.
            unsafe { w::SetWindowPos(hwnd, ptr::null_mut(), rect.left, rect.top, width, height, 0x14) };
            0
        }
        WM_GETMINMAXINFO => {
            let (min_w, min_h) = {
                let state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                (state.min_w, state.min_h)
            };
            // SAFETY: WM_GETMINMAXINFO lParam points to writable MINMAXINFO for this callback.
            let limits = unsafe { &mut *(lp as *mut w::MinMax) };
            limits.min_track.x = i32::try_from(min_w).unwrap_or(i32::MAX);
            limits.min_track.y = i32::try_from(min_h).unwrap_or(i32::MAX);
            0
        }
        WM_KEYDOWN => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::Key {
                    code: u32::try_from(wp).unwrap_or_default(),
                    down: true,
                    repeat: (lp & (1isize.checked_shl(30).unwrap_or(0))) != 0,
                });
            0
        }
        WM_KEYUP => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::Key {
                    code: u32::try_from(wp).unwrap_or_default(),
                    down: false,
                    repeat: false,
                });
            0
        }
        WM_IME_STARTCOMPOSITION => {
            let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.ime_active = true;
            state.events.push_back(Event::ImeStart);
            0
        }
        WM_IME_COMPOSITION => {
            let flags = u32::try_from(lp).unwrap_or_default();
            if flags & GCS_RESULTSTR != 0 {
                if let Some(text) = ime_composition_string(hwnd, GCS_RESULTSTR) {
                    let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    state.ime_active = false;
                    state.events.push_back(Event::ImeCommit(text));
                }
            } else if flags & GCS_COMPSTR != 0 {
                if let Some(text) = ime_composition_string(hwnd, GCS_COMPSTR) {
                    state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .events
                        .push_back(Event::ImeUpdate(text));
                }
            }
            0
        }
        WM_IME_ENDCOMPOSITION => {
            let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.ime_active {
                state.events.push_back(Event::ImeCancel);
            }
            state.ime_active = false;
            0
        }
        // WM_IME_CHAR's default processing generates WM_CHAR. The UTF-16 result was already delivered
        // as one ImeCommit from WM_IME_COMPOSITION, so consume this message to avoid duplicate insertion.
        WM_IME_CHAR => 0,
        WM_CHAR => {
            let u = u16::try_from(wp).unwrap_or_default();
            let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if (0xd800..=0xdbff).contains(&u) {
                state.high = Some(u);
            } else if (0xdc00..=0xdfff).contains(&u) {
                if let Some(high) = state.high.take() {
                    if let Some(Ok(character)) = char::decode_utf16([high, u]).next() {
                        state.events.push_back(Event::Text(character));
                    }
                }
            } else if let Some(character) = char::from_u32(u32::from(u)) {
                state.high = None;
                state.events.push_back(Event::Text(character));
            }
            0
        }
        WM_MOUSEMOVE => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::PointerMoved {
                    x: word(lp, false),
                    y: word(lp, true),
                });
            0
        }
        WM_LDOWN | WM_RDOWN | WM_MDOWN => {
            // SAFETY: hwnd is the live window receiving this mouse-button message.
            unsafe { w::SetCapture(hwnd) };
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::PointerButton {
                    button: if msg == WM_LDOWN {
                        MouseButton::Left
                    } else if msg == WM_RDOWN {
                        MouseButton::Right
                    } else {
                        MouseButton::Middle
                    },
                    down: true,
                });
            0
        }
        WM_LUP | WM_RUP | WM_MUP => {
            // SAFETY: releasing mouse capture is valid while handling a button-up message.
            unsafe { w::ReleaseCapture() };
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::PointerButton {
                    button: if msg == WM_LUP {
                        MouseButton::Left
                    } else if msg == WM_RUP {
                        MouseButton::Right
                    } else {
                        MouseButton::Middle
                    },
                    down: false,
                });
            0
        }
        WM_WHEEL => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::Wheel { x: 0, y: wheel(wp) });
            0
        }
        WM_HWHEEL => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::Wheel { x: wheel(wp), y: 0 });
            0
        }
        WM_HOTKEY => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .events
                .push_back(Event::HotKey(i32::try_from(wp).unwrap_or_default()));
            0
        }
        WM_SETCURSOR => {
            let cursor = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).cursor;
            // SAFETY: cursor_handle returns a shared system cursor handle valid for SetCursor.
            unsafe { w::SetCursor(cursor_handle(cursor)) };
            1
        }
        WM_PAINT => {
            paint(hwnd, state);
            0
        }
        _ => {
            // SAFETY: forwarding unhandled messages to the default procedure is required by Win32.
            unsafe { w::DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}

fn ime_composition_string(hwnd: w::Hwnd, index: u32) -> Option<String> {
    const MAX_IME_STRING_BYTES: i32 = 1_048_576;
    // SAFETY: hwnd is the live window receiving the IME message on this UI thread.
    let context = unsafe { w::ImmGetContext(hwnd) };
    if context.is_null() {
        return None;
    }
    // SAFETY: a null buffer with length zero is the documented size-query form for ImmGetCompositionStringW.
    let byte_count = unsafe { w::ImmGetCompositionStringW(context, index, ptr::null_mut(), 0) };
    let result = (|| {
        if !(0..=MAX_IME_STRING_BYTES).contains(&byte_count) {
            return None;
        }
        let mut bytes = vec![0_u8; usize::try_from(byte_count).ok()?];
        if byte_count == 0 {
            Some(String::new())
        } else {
            let length = u32::try_from(bytes.len()).ok()?;
            // SAFETY: bytes is writable for `length` bytes and remains alive for the synchronous native call.
            let written = unsafe { w::ImmGetCompositionStringW(context, index, bytes.as_mut_ptr().cast(), length) };
            if written >= 0 && written <= byte_count {
                bytes.truncate(usize::try_from(written).ok()?);
                let units = bytes
                    .chunks_exact(2)
                    .filter_map(|pair| <[u8; 2]>::try_from(pair).ok())
                    .map(u16::from_le_bytes)
                    .collect::<Vec<_>>();
                Some(String::from_utf16_lossy(&units))
            } else {
                None
            }
        }
    })();
    // SAFETY: context was returned by ImmGetContext for this live hwnd and is released exactly once.
    let _ = unsafe { w::ImmReleaseContext(hwnd, context) };
    result
}

fn paint(hwnd: w::Hwnd, state: &Mutex<State>) {
    let mut paint = w::Paint {
        hdc: ptr::null_mut(),
        erase: 0,
        paint: w::Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        restore: 0,
        inc_update: 0,
        reserved: [0; 32],
    };
    // SAFETY: hwnd is live and paint points to writable PAINTSTRUCT storage for this WM_PAINT.
    let dc = unsafe { w::BeginPaint(hwnd, &mut paint) };
    if !dc.is_null() {
        let state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.frame.is_empty() {
            let info = w::Bmi {
                header: w::BmiHeader {
                    size: u32::try_from(mem::size_of::<w::BmiHeader>()).unwrap_or_default(),
                    width: i32::try_from(state.width).unwrap_or_default(),
                    height: i32::try_from(state.height).unwrap_or_default().saturating_neg(),
                    planes: 1,
                    bit_count: 32,
                    compression: 0,
                    size_image: 0,
                    x: 0,
                    y: 0,
                    used: 0,
                    important: 0,
                },
                colors: [0],
            };
            let left = paint.paint.left.max(0);
            let top = paint.paint.top.max(0);
            let right = paint.paint.right.max(left);
            let bottom = paint.paint.bottom.max(top);
            let width = u32::try_from(right.saturating_sub(left)).unwrap_or_default();
            let height = u32::try_from(bottom.saturating_sub(top)).unwrap_or_default();
            // SAFETY: dc is from BeginPaint; frame and info remain live and immutable for the duration of this call.
            unsafe {
                w::SetDIBitsToDevice(
                    dc,
                    left,
                    top,
                    width,
                    height,
                    left,
                    top,
                    0,
                    state.height,
                    state.frame.as_ptr().cast(),
                    &info,
                    DIB_RGB_COLORS,
                );
            }
        }
    }
    // SAFETY: every successful BeginPaint for this PAINTSTRUCT is paired with EndPaint before returning.
    unsafe { w::EndPaint(hwnd, &paint) };
}

// IFileOpenDialog and IShellItem vtable prefixes, in documented COM order.
#[repr(C)]
struct DialogV {
    qi: usize,
    add: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    show: unsafe extern "system" fn(*mut c_void, w::Hwnd) -> i32,
    set_types: usize,
    set_type: usize,
    get_type: usize,
    advise: usize,
    unadvise: usize,
    set_options: unsafe extern "system" fn(*mut c_void, u32) -> i32,
    get_options: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
    default_folder: usize,
    set_folder: usize,
    get_folder: usize,
    current: usize,
    set_name: usize,
    get_name: usize,
    set_title: unsafe extern "system" fn(*mut c_void, *const u16) -> i32,
    set_ok: usize,
    set_label: usize,
    get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
}
#[repr(C)]
struct OpenDialogV {
    base: DialogV,
    add_place: usize,
    set_default_extension: usize,
    close: usize,
    set_client_guid: usize,
    clear_client_data: usize,
    set_filter: usize,
    get_results: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
}
#[repr(C)]
struct ItemArrayV {
    qi: usize,
    add: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    bind: usize,
    property_store: usize,
    property_description_list: usize,
    attributes: usize,
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
    get_item_at: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32,
}
#[repr(C)]
struct ItemV {
    qi: usize,
    add: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    bind: usize,
    parent: usize,
    name: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> i32,
}
const CLSID: w::Guid = w::Guid {
    a: 0xdc1c5a9c,
    b: 0xe88a,
    c: 0x4dde,
    d: [0xa5, 0xa1, 0x60, 0xf8, 0x2a, 0x20, 0xae, 0xf7],
};
const IID: w::Guid = w::Guid {
    a: 0xd57c7288,
    b: 0xd4ad,
    c: 0x4768,
    d: [0xbe, 0x02, 0x9d, 0x96, 0x95, 0x32, 0xd9, 0x60],
};

struct ComOwned {
    pointer: *mut c_void,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

impl Drop for ComOwned {
    fn drop(&mut self) {
        // SAFETY: pointer is one owned COM reference and release is from that interface's live vtable.
        unsafe { (self.release)(self.pointer) };
    }
}

struct CoTaskMem(*mut c_void);

impl Drop for CoTaskMem {
    fn drop(&mut self) {
        // SAFETY: this pointer was returned by IShellItem::GetDisplayName and is freed exactly once.
        unsafe { w::CoTaskMemFree(self.0) };
    }
}

struct ComApartment;

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: this thread successfully initialized COM in open_files.
        unsafe { w::CoUninitialize() };
    }
}

/// Opens the Win32 file picker with multi-select enabled.
///
/// # Errors
/// Returns an error when COM or the native file picker fails.
pub fn open_files() -> Result<Option<Vec<std::path::PathBuf>>> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    // SAFETY: COM is initialized for this thread with the documented STA apartment model.
    let initialized = unsafe { w::CoInitializeEx(ptr::null_mut(), 2) };
    if initialized < 0 {
        return Err(Error::System(format!(
            "CoInitializeEx for file picker failed with HRESULT 0x{:08x}",
            initialized as u32
        )));
    }
    let _apartment = ComApartment;

    let mut raw: *mut c_void = ptr::null_mut();
    // SAFETY: CLSID/IID identify IFileOpenDialog and raw is a writable COM out pointer.
    let created = unsafe { w::CoCreateInstance(&CLSID, ptr::null_mut(), 1, &IID, &mut raw) };
    if created < 0 || raw.is_null() {
        return Err(Error::System(format!(
            "IFileOpenDialog unavailable (HRESULT 0x{:08x})",
            created as u32
        )));
    }
    // SAFETY: CoCreateInstance returned IFileOpenDialog with the documented vtable prefix.
    let dialog_v = unsafe { &**(raw.cast::<*mut OpenDialogV>()) };
    let dialog = ComOwned {
        pointer: raw,
        release: dialog_v.base.release,
    };

    let mut options = 0_u32;
    // SAFETY: dialog is a live IFileOpenDialog and options is writable.
    let got_options = unsafe { (dialog_v.base.get_options)(raw, &mut options) };
    if got_options < 0 {
        return Err(Error::System(format!(
            "IFileOpenDialog::GetOptions failed with HRESULT 0x{:08x}",
            got_options as u32
        )));
    }
    // FOS_FORCEFILESYSTEM | FOS_ALLOWMULTISELECT | FOS_PATHMUSTEXIST | FOS_FILEMUSTEXIST | FOS_NOCHANGEDIR.
    // SAFETY: dialog is live and these bits are documented IFileDialogOptions flags.
    let set_options = unsafe { (dialog_v.base.set_options)(raw, options | 0x40 | 0x200 | 0x800 | 0x1000 | 0x8) };
    if set_options < 0 {
        return Err(Error::System(format!(
            "IFileOpenDialog::SetOptions failed with HRESULT 0x{:08x}",
            set_options as u32
        )));
    }
    let title: Vec<u16> = "Открыть сохранение".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: title is NUL-terminated UTF-16 and remains alive for this COM call.
    let set_title = unsafe { (dialog_v.base.set_title)(raw, title.as_ptr()) };
    if set_title < 0 {
        return Err(Error::System(format!(
            "IFileOpenDialog::SetTitle failed with HRESULT 0x{:08x}",
            set_title as u32
        )));
    }
    // SAFETY: dialog is live; a null owner is valid for an unparented modal picker.
    let shown = unsafe { (dialog_v.base.show)(raw, ptr::null_mut()) };
    if shown == -2_147_023_673 {
        return Ok(None);
    }
    if shown < 0 {
        return Err(Error::System(format!(
            "IFileOpenDialog::Show failed with HRESULT 0x{:08x}",
            shown as u32
        )));
    }

    let mut raw_items: *mut c_void = ptr::null_mut();
    // SAFETY: dialog is live and raw_items is a writable IShellItemArray out pointer.
    let got_items = unsafe { (dialog_v.get_results)(raw, &mut raw_items) };
    if got_items < 0 || raw_items.is_null() {
        return Err(Error::System(format!(
            "IFileOpenDialog::GetResults failed with HRESULT 0x{:08x}",
            got_items as u32
        )));
    }
    // SAFETY: GetResults returned a live IShellItemArray with the documented vtable prefix.
    let items_v = unsafe { &**(raw_items.cast::<*mut ItemArrayV>()) };
    let items = ComOwned {
        pointer: raw_items,
        release: items_v.release,
    };
    let mut count = 0_u32;
    // SAFETY: items is live and count is writable.
    let got_count = unsafe { (items_v.get_count)(raw_items, &mut count) };
    if got_count < 0 {
        return Err(Error::System(format!(
            "IShellItemArray::GetCount failed with HRESULT 0x{:08x}",
            got_count as u32
        )));
    }
    if count == 0 {
        return Ok(None);
    }
    if usize::try_from(count).unwrap_or(usize::MAX) > crate::file_dialog::MAX_SELECTED_FILES {
        return Err(Error::Refused(
            "native file picker selected more than 512 files".to_owned(),
        ));
    }
    let mut paths = Vec::with_capacity(usize::try_from(count).unwrap_or_default());
    for index in 0..count {
        let mut raw_item: *mut c_void = ptr::null_mut();
        // SAFETY: items is live, index is less than the reported count, and raw_item is writable.
        let got_item = unsafe { (items_v.get_item_at)(raw_items, index, &mut raw_item) };
        if got_item < 0 || raw_item.is_null() {
            return Err(Error::System(format!(
                "IShellItemArray::GetItemAt failed with HRESULT 0x{:08x}",
                got_item as u32
            )));
        }
        // SAFETY: GetItemAt returned one owned IShellItem with the documented vtable prefix.
        let item_v = unsafe { &**(raw_item.cast::<*mut ItemV>()) };
        let item = ComOwned {
            pointer: raw_item,
            release: item_v.release,
        };
        let mut raw_path: *mut u16 = ptr::null_mut();
        // SAFETY: item is live and raw_path is a writable PWSTR out pointer.
        let got_path = unsafe { (item_v.name)(raw_item, 0x80058000, &mut raw_path) };
        if got_path < 0 || raw_path.is_null() {
            return Err(Error::System(format!(
                "IShellItem::GetDisplayName failed with HRESULT 0x{:08x}",
                got_path as u32
            )));
        }
        let path_memory = CoTaskMem(raw_path.cast());
        let mut length = 0_usize;
        while length < 32_768 {
            // SAFETY: GetDisplayName returned a live NUL-terminated UTF-16 string; length is bounded below 32 KiB.
            if unsafe { *raw_path.add(length) } == 0 {
                break;
            }
            length = length.saturating_add(1);
        }
        if length == 32_768 {
            return Err(Error::Refused(
                "native file path exceeds the Windows path limit".to_owned(),
            ));
        }
        // SAFETY: the preceding bounded scan found the terminator in the system-owned allocation.
        let wide = unsafe { std::slice::from_raw_parts(raw_path, length) };
        let path = std::path::PathBuf::from(OsString::from_wide(wide));
        if !path.is_absolute() {
            return Err(Error::Refused("native file picker returned a relative path".to_owned()));
        }
        paths.push(path);
        drop(path_memory);
        drop(item);
    }
    drop(items);
    drop(dialog);
    Ok(Some(paths))
}

fn file_dialog(owner: w::Hwnd, folders: bool) -> Result<Option<String>> {
    let mut raw: *mut c_void = ptr::null_mut();
    // SAFETY: COM is initialized by Win32Window::new; CLSID/IID and out pointer match IFileOpenDialog.
    let hr = unsafe { w::CoCreateInstance(&CLSID, ptr::null_mut(), 1, &IID, &mut raw) };
    if hr < 0 || raw.is_null() {
        return Err(Error::System("IFileOpenDialog unavailable".to_owned()));
    }
    // SAFETY: successful CoCreateInstance returned an IFileOpenDialog pointer with this vtable prefix.
    let v = unsafe { &**(raw as *mut *mut DialogV) };
    let mut options = 0;
    // SAFETY: raw is a live IFileOpenDialog and options is writable.
    let _ = unsafe { (v.get_options)(raw, &mut options) };
    // SAFETY: raw is a live IFileOpenDialog and the option bits are documented FOS flags.
    let _ = unsafe { (v.set_options)(raw, options | 0x40 | if folders { 0x20 } else { 0 }) };
    // SAFETY: raw is live and owner is the live parent HWND.
    let shown = unsafe { (v.show)(raw, owner) };
    if shown < 0 {
        // SAFETY: release balances the CoCreateInstance reference exactly once on this path.
        unsafe { (v.release)(raw) };
        return Ok(None);
    }
    let mut item: *mut c_void = ptr::null_mut();
    // SAFETY: raw is live and item is a writable COM out pointer.
    if unsafe { (v.get_result)(raw, &mut item) } < 0 || item.is_null() {
        // SAFETY: raw still owns the reference returned by CoCreateInstance and is released once on this path.
        unsafe { (v.release)(raw) };
        return Ok(None);
    }
    // SAFETY: get_result returned a live IShellItem pointer with this vtable prefix.
    let iv = unsafe { &**(item as *mut *mut ItemV) };
    let mut path: *mut u16 = ptr::null_mut();
    // SAFETY: item is live and path is a writable PWSTR out pointer.
    let ok = unsafe { (iv.name)(item, 0x80058000, &mut path) };
    let result = if ok >= 0 && !path.is_null() {
        let mut n = 0usize;
        // SAFETY: SIGDN_FILESYSPATH returns a NUL-terminated CoTaskMemAlloc UTF-16 string.
        while unsafe { *path.add(n) } != 0 {
            n = n.saturating_add(1);
        }
        // SAFETY: n was found before the terminating NUL in the live UTF-16 allocation.
        Some(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(path, n) }))
    } else {
        None
    };
    if !path.is_null() {
        // SAFETY: path was allocated by the shell for SIGDN_FILESYSPATH and is freed exactly once.
        unsafe { w::CoTaskMemFree(path.cast()) }
    }
    // SAFETY: item and raw each own one live COM reference and are released exactly once.
    unsafe {
        (iv.release)(item);
        (v.release)(raw);
    }
    Ok(result)
}

/// Opens a native picker for one existing folder.
///
/// The caller initializes a single-threaded COM apartment for this invocation.
///
/// # Errors
/// Returns an error when COM or the native folder picker fails.
pub fn choose_directory() -> Result<Option<std::path::PathBuf>> {
    // SAFETY: COM is initialized for this thread with the documented STA apartment model.
    let initialized = unsafe { w::CoInitializeEx(ptr::null_mut(), 2) };
    if initialized < 0 {
        return Err(Error::System(format!(
            "CoInitializeEx for folder picker failed with HRESULT 0x{:08x}",
            initialized as u32
        )));
    }
    let _apartment = ComApartment;
    let Some(selected) = file_dialog(ptr::null_mut(), true)? else {
        return Ok(None);
    };
    let path = std::path::PathBuf::from(selected);
    if !path.is_absolute() {
        return Err(Error::Refused(
            "native folder picker returned a relative path".to_owned(),
        ));
    }
    Ok(Some(path))
}
