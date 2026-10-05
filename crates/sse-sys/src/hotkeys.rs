//! Native global hotkey registration used by the companion helper process.
//!
//! Unsafe platform calls stay in sse-sys. The UI/helper sees only checked bindings and pressed IDs.

use sse_core::{Error, Result};
use std::time::Duration;

/// One modifier-plus-letter binding registered by the helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyBinding {
    /// Stable caller-owned identifier returned by poll.
    pub id: u32,
    /// Uppercase ASCII A-Z.
    pub key: u8,
    /// Control modifier.
    pub control: bool,
    /// Alt modifier.
    pub alt: bool,
    /// Shift modifier.
    pub shift: bool,
}

impl HotkeyBinding {
    fn validate(self) -> Result<Self> {
        if self.id == 0 || !self.key.is_ascii_uppercase() {
            return Err(Error::Refused(
                "hotkey binding must have a non-zero ID and A-Z key".to_owned(),
            ));
        }
        if !self.control && !self.alt && !self.shift {
            return Err(Error::Refused(
                "hotkey binding requires at least one modifier".to_owned(),
            ));
        }
        Ok(self)
    }
}

/// A platform hotkey session. Bindings are grabbed only while an X-Ray game window has focus.
pub struct HotkeySession {
    inner: platform::Session,
}

impl HotkeySession {
    /// Opens the native backend and validates all bindings.
    ///
    /// # Errors
    /// Returns an error when the platform/display is unsupported or a binding is invalid.
    pub fn open(bindings: &[HotkeyBinding]) -> Result<Self> {
        let checked = bindings
            .iter()
            .copied()
            .map(HotkeyBinding::validate)
            .collect::<Result<Vec<_>>>()?;
        if checked.is_empty() {
            return Err(Error::Refused("at least one hotkey binding is required".to_owned()));
        }
        Ok(Self {
            inner: platform::Session::open(checked)?,
        })
    }

    /// Waits up to timeout for one pressed binding ID.
    ///
    /// # Errors
    /// Returns an error when the native event source fails.
    pub fn poll(&mut self, timeout: Duration) -> Result<Option<u32>> {
        self.inner.poll(timeout)
    }
}

#[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
fn is_game_name(name: &str) -> bool {
    let value = name.trim().to_ascii_lowercase();
    let value = value.strip_suffix(".exe").unwrap_or(&value);
    [
        "xr_3da",
        "xrengine",
        "steam_app_4500",
        "steam_app_20510",
        "steam_app_41700",
        "steam_app_2427410",
        "steam_app_2427420",
        "steam_app_2427430",
    ]
    .iter()
    .any(|expected| {
        value == *expected
            || value
                .strip_suffix(expected)
                .is_some_and(|prefix| prefix.ends_with('/') || prefix.ends_with('\\'))
    })
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{is_game_name, HotkeyBinding};
    use sse_core::Result;
    use std::ffi::c_void;
    use std::ptr;
    use std::thread;
    use std::time::{Duration, Instant};

    type Hwnd = *mut c_void;
    const WM_HOTKEY: u32 = 0x0312;
    const PM_REMOVE: u32 = 1;
    const MOD_ALT: u32 = 0x0001;
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_NOREPEAT: u32 = 0x4000;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    struct Msg {
        hwnd: Hwnd,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        point: Point,
        private: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn PeekMessageW(message: *mut Msg, hwnd: Hwnd, min: u32, max: u32, remove: u32) -> i32;
        fn RegisterHotKey(hwnd: Hwnd, id: i32, modifiers: u32, key: u32) -> i32;
        fn UnregisterHotKey(hwnd: Hwnd, id: i32) -> i32;
        fn GetForegroundWindow() -> Hwnd;
        fn GetClassNameW(hwnd: Hwnd, class_name: *mut u16, maximum: i32) -> i32;
    }

    pub(super) struct Session {
        bindings: Vec<HotkeyBinding>,
        active: bool,
        retry: u8,
        last_focus: Instant,
    }

    impl Session {
        pub(super) fn open(bindings: Vec<HotkeyBinding>) -> Result<Self> {
            let mut message = empty_message();
            // SAFETY: message is writable storage; PM_NOREMOVE initializes the thread message queue.
            let _ = unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, 0) };
            Ok(Self {
                bindings,
                active: false,
                retry: 0,
                last_focus: Instant::now()
                    .checked_sub(Duration::from_secs(1))
                    .unwrap_or_else(Instant::now),
            })
        }

        pub(super) fn poll(&mut self, timeout: Duration) -> Result<Option<u32>> {
            let deadline = Instant::now().checked_add(timeout).unwrap_or_else(Instant::now);
            loop {
                if self.last_focus.elapsed() >= Duration::from_millis(300) {
                    self.sync_focus();
                    self.last_focus = Instant::now();
                }
                let mut message = empty_message();
                // SAFETY: message is writable and this helper owns its thread message queue.
                while unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
                    if message.message == WM_HOTKEY {
                        let id = u32::try_from(message.w_param).unwrap_or_default();
                        if self.bindings.iter().any(|binding| binding.id == id) {
                            return Ok(Some(id));
                        }
                    }
                }
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn sync_focus(&mut self) {
            let wanted = game_has_focus();
            if wanted == self.active {
                self.retry = 0;
                return;
            }
            if self.retry != 0 {
                self.retry = self.retry.saturating_sub(1);
                return;
            }
            if self.set_registered(wanted) {
                self.active = wanted;
            } else {
                self.retry = 10;
            }
        }

        fn set_registered(&self, register: bool) -> bool {
            if !register {
                for binding in &self.bindings {
                    if let Ok(id) = i32::try_from(binding.id) {
                        // SAFETY: IDs belong to this helper thread; unregister is idempotent for absent IDs.
                        let _ = unsafe { UnregisterHotKey(ptr::null_mut(), id) };
                    }
                }
                return true;
            }
            let mut done = Vec::new();
            for binding in &self.bindings {
                let Ok(id) = i32::try_from(binding.id) else {
                    return false;
                };
                let mut modifiers = MOD_NOREPEAT;
                if binding.control {
                    modifiers |= MOD_CONTROL;
                }
                if binding.alt {
                    modifiers |= MOD_ALT;
                }
                if binding.shift {
                    modifiers |= MOD_SHIFT;
                }
                // SAFETY: null HWND registers on this helper thread; ID and virtual-key values are checked.
                if unsafe { RegisterHotKey(ptr::null_mut(), id, modifiers, u32::from(binding.key)) } == 0 {
                    for registered in done {
                        // SAFETY: registered IDs were successfully acquired by this helper thread above.
                        let _ = unsafe { UnregisterHotKey(ptr::null_mut(), registered) };
                    }
                    return false;
                }
                done.push(id);
            }
            true
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            let _ = self.set_registered(false);
        }
    }

    fn game_has_focus() -> bool {
        // SAFETY: GetForegroundWindow returns either null or a borrowed HWND owned by the desktop.
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() {
            return false;
        }
        let mut name = [0_u16; 256];
        // SAFETY: name is writable UTF-16 storage and window is a live borrowed foreground HWND.
        let length = unsafe { GetClassNameW(window, name.as_mut_ptr(), 256) };
        let Ok(length) = usize::try_from(length) else {
            return false;
        };
        is_game_name(&String::from_utf16_lossy(name.get(..length).unwrap_or_default()))
    }

    fn empty_message() -> Msg {
        Msg {
            hwnd: ptr::null_mut(),
            message: 0,
            w_param: 0,
            l_param: 0,
            time: 0,
            point: Point { x: 0, y: 0 },
            private: 0,
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{is_game_name, HotkeyBinding};
    use sse_core::{Error, Result};
    use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void, CStr};
    use std::mem::ManuallyDrop;
    use std::ptr;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    const KEY_PRESS: c_int = 2;
    const SHIFT_MASK: c_uint = 1;
    const LOCK_MASK: c_uint = 2;
    const CONTROL_MASK: c_uint = 4;
    const MOD1_MASK: c_uint = 8;
    const MOD2_MASK: c_uint = 16;
    static LAST_X_ERROR: AtomicI32 = AtomicI32::new(0);

    #[repr(C)]
    struct XClassHint {
        name: *mut c_char,
        class: *mut c_char,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct XKeyEvent {
        event_type: c_int,
        serial: c_ulong,
        send_event: c_int,
        display: *mut c_void,
        window: c_ulong,
        root: c_ulong,
        subwindow: c_ulong,
        time: c_ulong,
        x: c_int,
        y: c_int,
        x_root: c_int,
        y_root: c_int,
        state: c_uint,
        keycode: c_uint,
        same_screen: c_int,
    }
    #[repr(C)]
    struct XErrorEvent {
        event_type: c_int,
        display: *mut c_void,
        resource_id: c_ulong,
        serial: c_ulong,
        error_code: u8,
        request_code: u8,
        minor_code: u8,
    }
    #[repr(C)]
    union XEvent {
        event_type: c_int,
        key: ManuallyDrop<XKeyEvent>,
        pad: [isize; 24],
    }
    type ErrorHandler = unsafe extern "C" fn(*mut c_void, *mut XErrorEvent) -> c_int;

    #[link(name = "X11")]
    unsafe extern "C" {
        fn XOpenDisplay(name: *const c_char) -> *mut c_void;
        fn XDefaultRootWindow(display: *mut c_void) -> c_ulong;
        fn XKeysymToKeycode(display: *mut c_void, keysym: c_ulong) -> u8;
        fn XGrabKey(
            display: *mut c_void,
            keycode: c_int,
            modifiers: c_uint,
            window: c_ulong,
            owner_events: c_int,
            pointer_mode: c_int,
            keyboard_mode: c_int,
        );
        fn XUngrabKey(display: *mut c_void, keycode: c_int, modifiers: c_uint, window: c_ulong);
        fn XSync(display: *mut c_void, discard: c_int) -> c_int;
        fn XPending(display: *mut c_void) -> c_int;
        fn XNextEvent(display: *mut c_void, event: *mut XEvent) -> c_int;
        fn XCloseDisplay(display: *mut c_void) -> c_int;
        fn XGetInputFocus(display: *mut c_void, focus: *mut c_ulong, revert: *mut c_int) -> c_int;
        fn XGetClassHint(display: *mut c_void, window: c_ulong, hint: *mut XClassHint) -> c_int;
        fn XQueryTree(
            display: *mut c_void,
            window: c_ulong,
            root: *mut c_ulong,
            parent: *mut c_ulong,
            children: *mut *mut c_ulong,
            child_count: *mut c_uint,
        ) -> c_int;
        fn XFree(data: *mut c_void) -> c_int;
        fn XSetErrorHandler(handler: Option<ErrorHandler>) -> Option<ErrorHandler>;
    }

    struct NativeBinding {
        binding: HotkeyBinding,
        keycode: c_int,
        modifiers: c_uint,
    }

    pub(super) struct Session {
        display: *mut c_void,
        root: c_ulong,
        bindings: Vec<NativeBinding>,
        active: bool,
        retry: u8,
        last_focus: Instant,
        previous_handler: Option<ErrorHandler>,
    }

    impl Session {
        pub(super) fn open(bindings: Vec<HotkeyBinding>) -> Result<Self> {
            // SAFETY: null asks Xlib to use DISPLAY from the environment.
            let display = unsafe { XOpenDisplay(ptr::null()) };
            if display.is_null() {
                let reason = if std::env::var("XDG_SESSION_TYPE")
                    .ok()
                    .is_some_and(|value| value.eq_ignore_ascii_case("wayland"))
                    && std::env::var_os("DISPLAY").is_none()
                {
                    "Горячие клавиши требуют X11 или XWayland. Сеанс Wayland без XWayland не поддерживается."
                } else {
                    "Global hotkeys require an X11 display, but DISPLAY is not set or could not be opened."
                };
                return Err(Error::System(reason.to_owned()));
            }
            // SAFETY: display is live after XOpenDisplay succeeds.
            let root = unsafe { XDefaultRootWindow(display) };
            let mut native = Vec::with_capacity(bindings.len());
            for binding in bindings {
                // SAFETY: ASCII A-Z keysyms equal their Unicode/ASCII value.
                let keycode = c_int::from(unsafe { XKeysymToKeycode(display, c_ulong::from(binding.key)) });
                if keycode == 0 {
                    // SAFETY: display is live and owned by this constructor on the failure path.
                    let _ = unsafe { XCloseDisplay(display) };
                    return Err(Error::System(format!(
                        "X11 could not resolve hotkey {}",
                        char::from(binding.key)
                    )));
                }
                let mut modifiers = 0;
                if binding.control {
                    modifiers |= CONTROL_MASK;
                }
                if binding.alt {
                    modifiers |= MOD1_MASK;
                }
                if binding.shift {
                    modifiers |= SHIFT_MASK;
                }
                native.push(NativeBinding {
                    binding,
                    keycode,
                    modifiers,
                });
            }
            LAST_X_ERROR.store(0, Ordering::Release);
            // SAFETY: helper process owns Xlib error handling for its private display connection.
            let previous_handler = unsafe { XSetErrorHandler(Some(capture_error)) };
            Ok(Self {
                display,
                root,
                bindings: native,
                active: false,
                retry: 0,
                last_focus: Instant::now()
                    .checked_sub(Duration::from_secs(1))
                    .unwrap_or_else(Instant::now),
                previous_handler,
            })
        }

        pub(super) fn poll(&mut self, timeout: Duration) -> Result<Option<u32>> {
            let deadline = Instant::now().checked_add(timeout).unwrap_or_else(Instant::now);
            loop {
                if self.last_focus.elapsed() >= Duration::from_millis(300) {
                    self.sync_focus();
                    self.last_focus = Instant::now();
                }
                // SAFETY: display is live for this Session.
                while unsafe { XPending(self.display) } > 0 {
                    let mut event = XEvent { pad: [0; 24] };
                    // SAFETY: event is writable union storage large enough for XEvent.
                    let _ = unsafe { XNextEvent(self.display, &mut event) };
                    // SAFETY: event_type is the common leading field of every XEvent variant.
                    if unsafe { event.event_type } != KEY_PRESS {
                        continue;
                    }
                    // SAFETY: KEY_PRESS means the XKeyEvent union field is initialized.
                    let key = unsafe { *event.key };
                    let state = key.state & !(LOCK_MASK | MOD2_MASK);
                    if let Some(binding) = self.bindings.iter().find(|binding| {
                        binding.keycode == c_int::try_from(key.keycode).unwrap_or_default()
                            && binding.modifiers == state
                    }) {
                        return Ok(Some(binding.binding.id));
                    }
                }
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn sync_focus(&mut self) {
            let wanted = self.game_has_focus();
            if wanted == self.active {
                self.retry = 0;
                return;
            }
            if self.retry != 0 {
                self.retry = self.retry.saturating_sub(1);
                return;
            }
            if self.set_grabbed(wanted) {
                self.active = wanted;
            } else {
                self.retry = 10;
            }
        }

        fn set_grabbed(&self, grab: bool) -> bool {
            LAST_X_ERROR.store(0, Ordering::Release);
            for binding in &self.bindings {
                for extra in [0, LOCK_MASK, MOD2_MASK, LOCK_MASK | MOD2_MASK] {
                    let modifiers = binding.modifiers | extra;
                    if grab {
                        // SAFETY: display/root are live and keycode/modifier values were resolved by Xlib.
                        unsafe { XGrabKey(self.display, binding.keycode, modifiers, self.root, 0, 1, 1) };
                    } else {
                        // SAFETY: same tuple used for XGrabKey; ungrabbing an absent tuple is harmless.
                        unsafe { XUngrabKey(self.display, binding.keycode, modifiers, self.root) };
                    }
                }
            }
            // SAFETY: synchronizes this live display so asynchronous BadAccess is observed before returning.
            let _ = unsafe { XSync(self.display, 0) };
            LAST_X_ERROR.swap(0, Ordering::AcqRel) == 0
        }

        fn game_has_focus(&self) -> bool {
            let mut window = 0;
            let mut revert = 0;
            // SAFETY: output pointers are valid and display is live.
            let _ = unsafe { XGetInputFocus(self.display, &mut window, &mut revert) };
            for _ in 0..8 {
                if window <= 1 || window == self.root {
                    break;
                }
                let mut hint = XClassHint {
                    name: ptr::null_mut(),
                    class: ptr::null_mut(),
                };
                // SAFETY: hint is writable and window belongs to this display.
                if unsafe { XGetClassHint(self.display, window, &mut hint) } != 0 {
                    let name = c_string(hint.name);
                    let class = c_string(hint.class);
                    if !hint.name.is_null() {
                        // SAFETY: XGetClassHint allocated this pointer with Xlib.
                        let _ = unsafe { XFree(hint.name.cast()) };
                    }
                    if !hint.class.is_null() {
                        // SAFETY: XGetClassHint allocated this pointer with Xlib.
                        let _ = unsafe { XFree(hint.class.cast()) };
                    }
                    if name.as_deref().is_some_and(is_game_name) || class.as_deref().is_some_and(is_game_name) {
                        return true;
                    }
                }
                let mut root = 0;
                let mut parent = 0;
                let mut children = ptr::null_mut();
                let mut child_count = 0;
                // SAFETY: all output pointers are valid and window belongs to this display.
                if unsafe {
                    XQueryTree(
                        self.display,
                        window,
                        &mut root,
                        &mut parent,
                        &mut children,
                        &mut child_count,
                    )
                } == 0
                {
                    break;
                }
                if !children.is_null() {
                    // SAFETY: XQueryTree allocated children with Xlib.
                    let _ = unsafe { XFree(children.cast()) };
                }
                window = parent;
            }
            false
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            let _ = self.set_grabbed(false);
            // SAFETY: restores the process handler saved when this helper session opened.
            let _ = unsafe { XSetErrorHandler(self.previous_handler) };
            // SAFETY: display is owned by this Session and closed exactly once.
            let _ = unsafe { XCloseDisplay(self.display) };
        }
    }

    unsafe extern "C" fn capture_error(_display: *mut c_void, event: *mut XErrorEvent) -> c_int {
        if !event.is_null() {
            // SAFETY: Xlib passes a valid XErrorEvent pointer for the duration of this callback.
            let code = unsafe { (*event).error_code };
            LAST_X_ERROR.store(i32::from(code), Ordering::Release);
        }
        0
    }

    fn c_string(pointer: *mut c_char) -> Option<String> {
        if pointer.is_null() {
            return None;
        }
        // SAFETY: XGetClassHint returns NUL-terminated strings owned by Xlib.
        Some(unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
mod platform {
    use super::HotkeyBinding;
    use sse_core::{Error, Result};
    use std::time::Duration;

    pub(super) struct Session;

    impl Session {
        pub(super) fn open(_bindings: Vec<HotkeyBinding>) -> Result<Self> {
            Err(Error::System(
                "Global companion hotkeys are supported on Windows and X11/XWayland Linux only.".to_owned(),
            ))
        }

        pub(super) fn poll(&mut self, _timeout: Duration) -> Result<Option<u32>> {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_game_name;

    #[test]
    fn matches_xray_process_and_window_names_only() {
        assert!(is_game_name("xr_3da.exe"));
        assert!(is_game_name("steam_app_41700"));
        assert!(!is_game_name("code"));
        assert!(!is_game_name("notepad.exe"));
    }
}
