//! X17 implementation.
//! Native Win32 software window.
//!
//! The backend uses only hand-written Win32 ABI declarations. Every native call is isolated behind a documented
//! SAFETY boundary. Rendering is BGRA8 through SetDIBitsToDevice and only invalidated damage rectangles are copied.

use crate::win32_ffi as w;
use sse_core::{Error, Result};
use std::{collections::VecDeque, ffi::c_void, mem, ptr, sync::Arc, time::Duration};

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
    cursor: CursorShape,
}
/// Win32 implementation.
pub struct Win32Window {
    hwnd: w::Hwnd,
    state: *mut State,
    wake: WakeHandle,
    hotkeys: Vec<i32>,
    com: bool,
}
impl Win32Window {
    /// Creates a visible per-monitor-v2-DPI window with a dark title bar.
    pub fn new(options: WindowOptions) -> Result<Self> {
        // SAFETY: -4 is DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2; fallback is PROCESS_PER_MONITOR_DPI_AWARE.
        if unsafe { w::SetProcessDpiAwarenessContext(-4) } == 0 {
            // SAFETY: PROCESS_PER_MONITOR_DPI_AWARE is the documented fallback on older Windows.
            let _ = unsafe { w::SetProcessDpiAwareness(2) };
        }
        // SAFETY: initializes COM apartment state for this window thread; balanced in Drop on success.
        let com = unsafe { w::CoInitializeEx(ptr::null_mut(), 2) } >= 0;
        // SAFETY: null security/name pointers request an unnamed auto-reset event owned by this process.
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
        };
        // SAFETY: WNDCLASSEX fields and the NUL-terminated class name remain valid for the duration of the call.
        let _ = unsafe { w::RegisterClassExW(&wc) };
        let state = Box::into_raw(Box::new(State {
            frame: Vec::new(),
            width: options.width,
            height: options.height,
            events: VecDeque::new(),
            min_w: options.min_width,
            min_h: options.min_height,
            high: None,
            cursor: CursorShape::Arrow,
        }));
        let width = i32::try_from(options.width).map_err(|_| Error::Refused("window width too large".to_owned()))?;
        let height = i32::try_from(options.height).map_err(|_| Error::Refused("window height too large".to_owned()))?;
        // SAFETY: class/title are NUL-terminated, dimensions are checked, and state is a stable Box::into_raw pointer.
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
            // SAFETY: CreateWindowExW failed, so no HWND retained the Box::into_raw pointer.
            drop(unsafe { Box::from_raw(state) });
            return Err(Error::System("CreateWindowExW failed".to_owned()));
        }
        // SAFETY: state is a live Box::into_raw allocation and remains valid until Win32Window::drop.
        unsafe { w::SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
        let dark: i32 = 1;
        // SAFETY: attribute 20 consumes a pointer to a live BOOL-sized value for the duration of the call.
        let _ = unsafe {
            w::DwmSetWindowAttribute(
                hwnd,
                20,
                (&dark as *const i32).cast(),
                u32::try_from(mem::size_of::<i32>()).unwrap_or_default(),
            )
        };
        // SAFETY: hwnd was returned successfully by CreateWindowExW and is live here.
        unsafe {
            w::ShowWindow(hwnd, 5);
            w::UpdateWindow(hwnd);
        }
        Ok(Self {
            hwnd,
            state,
            wake,
            hotkeys: Vec::new(),
            com,
        })
    }
    fn state_mut(&mut self) -> &mut State {
        // SAFETY: Win32Window exclusively owns this Box::into_raw allocation on the window thread.
        unsafe { &mut *self.state }
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
        let hi = i32::try_from(height).map_err(|_| Error::Refused("icon too tall".to_owned()))?;
        // SAFETY: BGRA storage is contiguous and live for CreateBitmap, which copies the supplied pixels.
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
        // SAFETY: mask storage is contiguous and live for CreateBitmap, which copies the supplied bits.
        let mono = unsafe { w::CreateBitmap(wi, hi, 1, 1, mask.as_ptr().cast()) };
        if color.is_null() || mono.is_null() {
            return Err(Error::System("icon bitmap creation failed".to_owned()));
        }
        let info = w::IconInfo {
            icon: 1,
            x: 0,
            y: 0,
            mask: mono,
            color,
        };
        // SAFETY: both bitmaps are live and referenced by a fully initialized ICONINFO.
        let icon = unsafe { w::CreateIconIndirect(&info) };
        // SAFETY: CreateIconIndirect copied the bitmap data; these owned bitmap handles can now be released.
        unsafe {
            w::DeleteObject(color);
            w::DeleteObject(mono);
        }
        if icon.is_null() {
            return Err(Error::System("CreateIconIndirect failed".to_owned()));
        }
        // SAFETY: WM_SETICON accepts this live HICON in lParam for the live window.
        unsafe {
            w::PostMessageW(self.hwnd, 0x80, 1, icon as isize);
            w::PostMessageW(self.hwnd, 0x80, 0, icon as isize);
        }
        Ok(())
    }
    fn dialog(&mut self, folders: bool) -> Result<Option<String>> {
        file_dialog(self.hwnd, folders)
    }
}
impl Drop for Win32Window {
    fn drop(&mut self) {
        for id in &self.hotkeys {
            // SAFETY: hwnd is live here and each ID was registered by this window.
            let _ = unsafe { w::UnregisterHotKey(self.hwnd, *id) };
        }
        if !self.hwnd.is_null() {
            // SAFETY: clearing user data prevents callbacks during DestroyWindow from borrowing State.
            unsafe { w::SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0) };
            // SAFETY: hwnd is owned by this Win32Window and is destroyed exactly once.
            let _ = unsafe { w::DestroyWindow(self.hwnd) };
        }
        if !self.state.is_null() {
            // SAFETY: state came from Box::into_raw in new and no callback can reach it after user data is cleared.
            drop(unsafe { Box::from_raw(self.state) });
            self.state = ptr::null_mut();
        }
        if self.com {
            // SAFETY: this thread successfully called CoInitializeEx in new.
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
            let state = self.state_mut();
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
            };
            // SAFETY: hwnd is live and r is a valid stack RECT for the duration of InvalidateRect.
            unsafe {
                w::InvalidateRect(self.hwnd, &r, 0);
            }
        }
        if damage.is_empty() {
            // SAFETY: hwnd is live; a null RECT invalidates the whole client area without erasing.
            unsafe {
                w::InvalidateRect(self.hwnd, ptr::null(), 0);
            }
        }
        Ok(())
    }
    fn next_event(&mut self, timeout: Option<Duration>) -> Event {
        if let Some(e) = self.state_mut().events.pop_front() {
            return e;
        }
        let ms = timeout.map_or(INFINITE, |d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX));
        let handle = self.wake.0 .0 as w::Handle;
        // SAFETY: handle is the live event owned by WakeHandle; the pointer to it is valid for this synchronous wait.
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
            // SAFETY: msg is writable storage and a null HWND requests messages for the current thread.
            if unsafe { w::PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) } == 0 {
                break;
            }
            // SAFETY: msg was initialized by PeekMessageW and remains live for translation/dispatch.
            unsafe {
                w::TranslateMessage(&msg);
                w::DispatchMessageW(&msg);
            }
            if let Some(e) = self.state_mut().events.pop_front() {
                return e;
            }
        }
        Event::Timeout
    }
    fn set_cursor(&mut self, c: CursorShape) {
        self.state_mut().cursor = c;
        let h = cursor_handle(c);
        if !h.is_null() {
            // SAFETY: h is a shared system cursor handle returned by LoadCursorW.
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
        // SAFETY: hwnd is the live owner window for this clipboard transaction.
        if unsafe { w::OpenClipboard(self.hwnd) } == 0 {
            return Err(Error::System("OpenClipboard failed".to_owned()));
        }
        // SAFETY: clipboard is open on this thread.
        unsafe { w::EmptyClipboard() };
        // SAFETY: requests a movable global-memory block of the checked byte size.
        let mem = unsafe { w::GlobalAlloc(GMEM_MOVEABLE, bytes) };
        if mem.is_null() {
            // SAFETY: clipboard was opened successfully above and is still owned by this thread.
            unsafe { w::CloseClipboard() };
            return Err(Error::System("GlobalAlloc failed".to_owned()));
        }
        // SAFETY: mem is a live movable global-memory handle returned by GlobalAlloc.
        let dst = unsafe { w::GlobalLock(mem) };
        if dst.is_null() {
            // SAFETY: clipboard was opened successfully above and is still owned by this thread.
            unsafe { w::CloseClipboard() };
            return Err(Error::System("GlobalLock failed".to_owned()));
        }
        // SAFETY: source and locked destination each contain at least bytes bytes and do not overlap.
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr().cast::<u8>(), dst.cast::<u8>(), bytes);
            w::GlobalUnlock(mem);
        }
        // SAFETY: clipboard is open and mem contains NUL-terminated UTF-16 in movable global memory.
        let set = unsafe { w::SetClipboardData(CF_UNICODETEXT, mem) };
        // SAFETY: closes the clipboard transaction opened above.
        unsafe { w::CloseClipboard() };
        if set.is_null() {
            Err(Error::System("SetClipboardData failed".to_owned()))
        } else {
            Ok(())
        }
    }
    fn clipboard_text(&mut self) -> Result<Option<String>> {
        // SAFETY: format query has no pointer arguments and does not retain state.
        if unsafe { w::IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
            return Ok(None);
        }
        // SAFETY: hwnd is the live owner window for this clipboard transaction.
        if unsafe { w::OpenClipboard(self.hwnd) } == 0 {
            return Err(Error::System("OpenClipboard failed".to_owned()));
        }
        // SAFETY: clipboard is open and the requested format was reported available.
        let mem = unsafe { w::GetClipboardData(CF_UNICODETEXT) };
        if mem.is_null() {
            // SAFETY: clipboard was opened successfully above and is still owned by this thread.
            unsafe { w::CloseClipboard() };
            return Ok(None);
        }
        // SAFETY: mem is the live clipboard-owned global-memory handle returned above.
        let units = unsafe { w::GlobalSize(mem) }.checked_div(2).unwrap_or(0);
        // SAFETY: mem remains live while the clipboard is open.
        let raw = unsafe { w::GlobalLock(mem) };
        if raw.is_null() {
            // SAFETY: clipboard was opened successfully above and is still owned by this thread.
            unsafe { w::CloseClipboard() };
            return Err(Error::System("GlobalLock failed".to_owned()));
        }
        // SAFETY: GlobalSize bounds the locked allocation; interpreting it as u16 matches CF_UNICODETEXT.
        let slice = unsafe { std::slice::from_raw_parts(raw.cast::<u16>(), units) };
        let end = slice.iter().position(|v| *v == 0).unwrap_or(slice.len());
        let text = String::from_utf16_lossy(slice.get(..end).unwrap_or_default());
        // SAFETY: balances the successful GlobalLock/OpenClipboard calls above.
        unsafe {
            w::GlobalUnlock(mem);
            w::CloseClipboard();
        }
        Ok(Some(text))
    }
    fn register_hotkey(&mut self, id: i32, modifiers: u32, key: u32) -> Result<()> {
        // SAFETY: hwnd is live and the ID/modifier/key values are passed by value.
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
        // SAFETY: v is writable HIGHCONTRAST storage with its size field initialized.
        (unsafe { w::SystemParametersInfoW(0x42, v.size, (&mut v as *mut w::HighContrast).cast(), 0) }) != 0
            && v.flags & 1 != 0
    }
    fn reduced_motion(&self) -> bool {
        let mut enabled: i32 = 1;
        // SAFETY: enabled is writable BOOL-sized storage for the documented client-area animation query.
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
    // SAFETY: MAKEINTRESOURCE-style predefined cursor IDs are accepted with a null module handle.
    unsafe { w::LoadCursorW(ptr::null_mut(), id as usize as *const u16) }
}
unsafe extern "system" fn proc(hwnd: w::Hwnd, msg: u32, wp: usize, lp: isize) -> isize {
    // SAFETY: hwnd is supplied by Win32; GWLP_USERDATA is either zero or the State pointer installed by new.
    let raw = unsafe { w::GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    if raw.is_null() {
        // SAFETY: forwarding an unhandled message with the original Win32 arguments is required by the ABI.
        return unsafe { w::DefWindowProcW(hwnd, msg, wp, lp) };
    }
    match msg {
        WM_CLOSE => {
            // SAFETY: callbacks run on the owning window thread and user data points at the live State.
            unsafe { (&mut *raw).events.push_back(Event::Close) };
            0
        }
        WM_DESTROY => {
            // SAFETY: called on the window thread while processing WM_DESTROY.
            unsafe { w::PostQuitMessage(0) };
            0
        }
        WM_ERASEBKGND => 1,
        WM_SETFOCUS => {
            // SAFETY: callbacks run on the owning window thread and user data points at the live State.
            unsafe { (&mut *raw).events.push_back(Event::Focus(true)) };
            0
        }
        WM_KILLFOCUS => {
            // SAFETY: callbacks run on the owning window thread and user data points at the live State.
            unsafe { (&mut *raw).events.push_back(Event::Focus(false)) };
            0
        }
        WM_SIZE => {
            let width = u32::try_from(word(lp, false)).unwrap_or_default();
            let height = u32::try_from(word(lp, true)).unwrap_or_default();
            // SAFETY: hwnd is the live window currently dispatching WM_SIZE.
            let dpi = unsafe { w::GetDpiForWindow(hwnd) };
            let event = Event::Resized {
                width,
                height,
                scale: (dpi as f32).mul_add(0.010416667, 0.0),
            };
            // SAFETY: no State reference is held across GetDpiForWindow; raw remains owned by the window.
            unsafe { (&mut *raw).events.push_back(event) };
            0
        }
        WM_DPICHANGED => {
            // SAFETY: WM_DPICHANGED lParam points to the suggested RECT for the duration of this callback.
            let rect = unsafe { &*(lp as *const w::Rect) };
            let width = rect.right.saturating_sub(rect.left);
            let height = rect.bottom.saturating_sub(rect.top);
            // SAFETY: hwnd is live and the suggested RECT coordinates came from this WM_DPICHANGED message.
            unsafe { w::SetWindowPos(hwnd, ptr::null_mut(), rect.left, rect.top, width, height, 0x14) };
            0
        }
        WM_GETMINMAXINFO => {
            // SAFETY: raw points at live State; copy values before touching the Win32-owned output structure.
            let (min_w, min_h) = unsafe { ((*raw).min_w, (*raw).min_h) };
            // SAFETY: WM_GETMINMAXINFO lParam points to writable MINMAXINFO for this callback.
            let m = unsafe { &mut *(lp as *mut w::MinMax) };
            m.min_track.x = i32::try_from(min_w).unwrap_or(i32::MAX);
            m.min_track.y = i32::try_from(min_h).unwrap_or(i32::MAX);
            0
        }
        WM_KEYDOWN => {
            let event = Event::Key {
                code: u32::try_from(wp).unwrap_or_default(),
                down: true,
                repeat: (lp & (1isize.checked_shl(30).unwrap_or(0))) != 0,
            };
            // SAFETY: callbacks run on the owning window thread and user data points at the live State.
            unsafe { (&mut *raw).events.push_back(event) };
            0
        }
        WM_KEYUP => {
            let event = Event::Key {
                code: u32::try_from(wp).unwrap_or_default(),
                down: false,
                repeat: false,
            };
            // SAFETY: callbacks run on the owning window thread and user data points at the live State.
            unsafe { (&mut *raw).events.push_back(event) };
            0
        }
        WM_CHAR => {
            let u = u16::try_from(wp).unwrap_or_default();
            // SAFETY: this short borrow ends before the callback returns and crosses no Win32 call.
            let state = unsafe { &mut *raw };
            if (0xd800..=0xdbff).contains(&u) {
                state.high = Some(u);
            } else if (0xdc00..=0xdfff).contains(&u) {
                if let Some(h) = state.high.take() {
                    if let Some(Ok(character)) = char::decode_utf16([h, u]).next() {
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
            let event = Event::PointerMoved {
                x: word(lp, false),
                y: word(lp, true),
            };
            // SAFETY: callbacks run on the owning window thread and user data points at the live State.
            unsafe { (&mut *raw).events.push_back(event) };
            0
        }
        WM_LDOWN | WM_RDOWN | WM_MDOWN => {
            // SAFETY: hwnd is the live window receiving this mouse-button message.
            unsafe { w::SetCapture(hwnd) };
            let button = if msg == WM_LDOWN {
                MouseButton::Left
            } else if msg == WM_RDOWN {
                MouseButton::Right
            } else {
                MouseButton::Middle
            };
            // SAFETY: no State reference is held across SetCapture; raw remains live afterwards.
            unsafe { (&mut *raw).events.push_back(Event::PointerButton { button, down: true }) };
            0
        }
        WM_LUP | WM_RUP | WM_MUP => {
            // SAFETY: releasing capture is valid on the window thread after a captured button transition.
            unsafe { w::ReleaseCapture() };
            let button = if msg == WM_LUP {
                MouseButton::Left
            } else if msg == WM_RUP {
                MouseButton::Right
            } else {
                MouseButton::Middle
            };
            // SAFETY: no State reference is held across ReleaseCapture; raw remains live afterwards.
            unsafe { (&mut *raw).events.push_back(Event::PointerButton { button, down: false }) };
            0
        }
        WM_WHEEL => {
            let event = Event::Wheel { x: 0, y: wheel(wp) };
            // SAFETY: callbacks run on the owning window thread and user data points at the live State.
            unsafe { (&mut *raw).events.push_back(event) };
            0
        }
        WM_HWHEEL => {
            let event = Event::Wheel { x: wheel(wp), y: 0 };
            // SAFETY: callbacks run on the owning window thread and user data points at the live State.
            unsafe { (&mut *raw).events.push_back(event) };
            0
        }
        WM_HOTKEY => {
            let event = Event::HotKey(i32::try_from(wp).unwrap_or_default());
            // SAFETY: callbacks run on the owning window thread and user data points at the live State.
            unsafe { (&mut *raw).events.push_back(event) };
            0
        }
        WM_SETCURSOR => {
            // SAFETY: copy the cursor value from live State before calling back into Win32.
            let cursor = unsafe { (*raw).cursor };
            // SAFETY: cursor_handle returns a shared system cursor suitable for SetCursor.
            unsafe { w::SetCursor(cursor_handle(cursor)) };
            1
        }
        WM_PAINT => {
            // SAFETY: paint only takes a shared view of live State while Win32 reads the framebuffer bytes.
            paint(hwnd, unsafe { &*raw });
            0
        }
        _ => {
            // SAFETY: forwarding an unhandled message with the original Win32 arguments is required by the ABI.
            unsafe { w::DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}
fn paint(hwnd: w::Hwnd, s: &State) {
    let mut p = w::Paint {
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
    // SAFETY: hwnd is processing WM_PAINT and p is writable PAINTSTRUCT-compatible storage.
    let dc = unsafe { w::BeginPaint(hwnd, &mut p) };
    if !dc.is_null() && !s.frame.is_empty() {
        let info = w::Bmi {
            header: w::BmiHeader {
                size: u32::try_from(mem::size_of::<w::BmiHeader>()).unwrap_or_default(),
                width: i32::try_from(s.width).unwrap_or_default(),
                height: i32::try_from(s.height).unwrap_or_default().saturating_neg(),
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
        let left = p.paint.left.max(0);
        let top = p.paint.top.max(0);
        let right = p.paint.right.max(left);
        let bottom = p.paint.bottom.max(top);
        let width = u32::try_from(right.saturating_sub(left)).unwrap_or_default();
        let height = u32::try_from(bottom.saturating_sub(top)).unwrap_or_default();
        // SAFETY: frame contains the advertised BGRA dimensions and info describes that live buffer.
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
                s.height,
                s.frame.as_ptr().cast(),
                &info,
                DIB_RGB_COLORS,
            );
        }
    }
    // SAFETY: balances the successful BeginPaint call for this WM_PAINT dispatch.
    unsafe {
        w::EndPaint(hwnd, &p);
    }
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
    set_title: usize,
    set_ok: usize,
    set_label: usize,
    get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
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
fn file_dialog(owner: w::Hwnd, folders: bool) -> Result<Option<String>> {
    let mut raw: *mut c_void = ptr::null_mut();
    let hr = unsafe { w::CoCreateInstance(&CLSID, ptr::null_mut(), 1, &IID, &mut raw) };
    if hr < 0 || raw.is_null() {
        return Err(Error::System("IFileOpenDialog unavailable".to_owned()));
    }
    let v = unsafe { &**(raw as *mut *mut DialogV) };
    let mut options = 0;
    let _ = unsafe { (v.get_options)(raw, &mut options) };
    let _ = unsafe { (v.set_options)(raw, options | 0x40 | if folders { 0x20 } else { 0 }) };
    let shown = unsafe { (v.show)(raw, owner) };
    if shown < 0 {
        unsafe { (v.release)(raw) };
        return Ok(None);
    }
    let mut item: *mut c_void = ptr::null_mut();
    if unsafe { (v.get_result)(raw, &mut item) } < 0 || item.is_null() {
        unsafe { (v.release)(raw) };
        return Ok(None);
    }
    let iv = unsafe { &**(item as *mut *mut ItemV) };
    let mut path: *mut u16 = ptr::null_mut();
    let ok = unsafe { (iv.name)(item, 0x80058000, &mut path) };
    let result = if ok >= 0 && !path.is_null() {
        let mut n = 0usize;
        while unsafe { *path.add(n) } != 0 {
            n = n.saturating_add(1);
        }
        Some(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(path, n) }))
    } else {
        None
    };
    if !path.is_null() {
        unsafe { w::CoTaskMemFree(path.cast()) }
    }
    unsafe {
        (iv.release)(item);
        (v.release)(raw);
    }
    Ok(result)
}
