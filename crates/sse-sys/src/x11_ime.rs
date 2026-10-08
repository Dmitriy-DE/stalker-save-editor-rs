//! Optional Xlib input-method bridge for the std-only X11 window protocol client.

use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CString};
use std::mem;
use std::ptr;

const RTLD_NOW: c_int = 2;
const XIM_PREEDIT_CALLBACKS: c_ulong = 0x0002;
const XIM_PREEDIT_NOTHING: c_ulong = 0x0008;
const XIM_STATUS_NOTHING: c_ulong = 0x0400;
const X_LOOKUP_CHARS: c_int = 2;
const X_LOOKUP_BOTH: c_int = 4;
const X_BUFFER_OVERFLOW: c_int = -1;
const MAX_LOOKUP_BYTES: usize = 1_048_576;
const MAX_PREEDIT_CHARS: usize = 4096;

#[link(name = "c")]
// SAFETY: this matches the C locale conversion ABI; caller bounds input by XIMText.length and output by its capacity.
unsafe extern "C" {
    fn mbstowcs(destination: *mut c_int, source: *const c_char, count: usize) -> usize;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XKeyEvent {
    type_: c_int,
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
union XEvent {
    type_: c_int,
    key: XKeyEvent,
    pad: [c_long; 24],
}

type XOpenDisplay = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type XCloseDisplay = unsafe extern "C" fn(*mut c_void) -> c_int;
type XDefaultRootWindow = unsafe extern "C" fn(*mut c_void) -> c_ulong;
type XSetLocaleModifiers = unsafe extern "C" fn(*const c_char) -> *const c_char;
type XOpenIm = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> *mut c_void;
type XCloseIm = unsafe extern "C" fn(*mut c_void) -> c_int;
type XCreateIc = unsafe extern "C" fn(*mut c_void, ...) -> *mut c_void;
type XDestroyIc = unsafe extern "C" fn(*mut c_void);
type XSetIcFocus = unsafe extern "C" fn(*mut c_void);
type XUnsetIcFocus = unsafe extern "C" fn(*mut c_void);
type XFilterEvent = unsafe extern "C" fn(*mut XEvent, c_ulong) -> c_int;
type XUtf8LookupString =
    unsafe extern "C" fn(*mut c_void, *mut XKeyEvent, *mut c_char, c_int, *mut c_ulong, *mut c_int) -> c_int;
type XVaCreateNestedList = unsafe extern "C" fn(c_int, ...) -> *mut c_void;
type XFree = unsafe extern "C" fn(*mut c_void) -> c_int;

#[repr(C)]
struct XimCallback {
    client_data: *mut c_void,
    callback: *const c_void,
}

#[repr(C)]
union XimTextString {
    multi_byte: *const c_char,
    wide_char: *const c_int,
}

#[repr(C)]
struct XimText {
    length: u16,
    feedback: *mut c_ulong,
    encoding_is_wchar: c_int,
    string: XimTextString,
}

#[repr(C)]
struct XimPreeditDraw {
    caret: c_int,
    chg_first: c_int,
    chg_length: c_int,
    text: *mut XimText,
}

/// Preedit notifications produced by an XIM context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreeditEvent {
    /// Composition started.
    Start,
    /// Full current composition text.
    Update(String),
    /// Composition was cancelled.
    Cancel,
}

#[derive(Default)]
struct CallbackState {
    preedit: String,
    active: bool,
    events: Vec<PreeditEvent>,
}

struct Api {
    open_display: XOpenDisplay,
    close_display: XCloseDisplay,
    default_root_window: XDefaultRootWindow,
    set_locale_modifiers: XSetLocaleModifiers,
    open_im: XOpenIm,
    close_im: XCloseIm,
    create_ic: XCreateIc,
    destroy_ic: XDestroyIc,
    set_ic_focus: XSetIcFocus,
    unset_ic_focus: XUnsetIcFocus,
    filter_event: XFilterEvent,
    utf8_lookup_string: XUtf8LookupString,
    create_nested_list: XVaCreateNestedList,
    free: XFree,
}

struct Library(*mut c_void);

impl Drop for Library {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this handle came from dlopen and remains live until the Xlib-backed objects are destroyed.
            let _ = unsafe { dlclose(self.0) };
        }
    }
}

/// Best-effort XIM context with bounded preedit callbacks and a commit-only fallback.
///
/// It is optional so a missing XIM server keeps ordinary X11 typing available.
pub struct X11Ime {
    _library: Library,
    api: Api,
    display: *mut c_void,
    input_method: *mut c_void,
    input_context: *mut c_void,
    window: c_ulong,
    root: c_ulong,
    callback_state: Box<CallbackState>,
}

// SAFETY: after construction the value is moved to and used only by the X11 reader thread; no Xlib handle is
// accessed concurrently, and Drop runs on that same owner thread.
unsafe impl Send for X11Ime {}

impl X11Ime {
    /// Opens Xlib and creates an XIM context for an existing X11 window.
    #[must_use]
    pub fn open(display_name: &str, window: u32) -> Option<Self> {
        let library_name = c"libX11.so.6";
        // SAFETY: a static NUL-terminated library name and valid RTLD flags are passed to the dynamic loader.
        let handle = unsafe { dlopen(library_name.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return None;
        }
        let library = Library(handle);
        macro_rules! load {
            ($symbol:literal, $ty:ty) => {{
                // SAFETY: `handle` is the live libX11 handle retained in `library`.
                let symbol = unsafe { dlsym(handle, concat!($symbol, "\0").as_ptr().cast()) };
                if symbol.is_null() {
                    return None;
                }
                // SAFETY: the symbol name and ABI type correspond to the public Xlib declaration.
                unsafe { mem::transmute::<*mut c_void, $ty>(symbol) }
            }};
        }
        let api = Api {
            open_display: load!("XOpenDisplay", XOpenDisplay),
            close_display: load!("XCloseDisplay", XCloseDisplay),
            default_root_window: load!("XDefaultRootWindow", XDefaultRootWindow),
            set_locale_modifiers: load!("XSetLocaleModifiers", XSetLocaleModifiers),
            open_im: load!("XOpenIM", XOpenIm),
            close_im: load!("XCloseIM", XCloseIm),
            create_ic: load!("XCreateIC", XCreateIc),
            destroy_ic: load!("XDestroyIC", XDestroyIc),
            set_ic_focus: load!("XSetICFocus", XSetIcFocus),
            unset_ic_focus: load!("XUnsetICFocus", XUnsetIcFocus),
            filter_event: load!("XFilterEvent", XFilterEvent),
            utf8_lookup_string: load!("Xutf8LookupString", XUtf8LookupString),
            create_nested_list: load!("XVaCreateNestedList", XVaCreateNestedList),
            free: load!("XFree", XFree),
        };
        let display_name = CString::new(display_name).ok()?;
        // SAFETY: a null locale selector requests the user's process locale; this is done before XOpenIM.
        let _ = unsafe { setlocale(0, c"".as_ptr()) };
        // SAFETY: XSetLocaleModifiers receives a static NUL-terminated modifiers string.
        if unsafe { (api.set_locale_modifiers)(c"".as_ptr()) }.is_null() {
            return None;
        }
        // SAFETY: display_name is NUL-terminated and remains alive during the call.
        let display = unsafe { (api.open_display)(display_name.as_ptr()) };
        if display.is_null() {
            return None;
        }
        // SAFETY: display is live; null resource arguments request the locale's default input method.
        let input_method = unsafe { (api.open_im)(display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()) };
        if input_method.is_null() {
            // SAFETY: display was returned by XOpenDisplay and is not used after this close.
            let _ = unsafe { (api.close_display)(display) };
            return None;
        }
        let window = c_ulong::from(window);
        let mut callback_state = Box::<CallbackState>::default();
        let callback_data = (&mut *callback_state as *mut CallbackState).cast::<c_void>();
        let start_callback = XimCallback {
            client_data: callback_data,
            callback: xim_preedit_start as *const c_void,
        };
        let done_callback = XimCallback {
            client_data: callback_data,
            callback: xim_preedit_done as *const c_void,
        };
        let draw_callback = XimCallback {
            client_data: callback_data,
            callback: xim_preedit_draw as *const c_void,
        };
        // SAFETY: XVaCreateNestedList is variadic; attribute names and callback pointers match Xlib's preedit
        // callback declarations, and the list terminates with a null pointer.
        let preedit_attributes = unsafe {
            (api.create_nested_list)(
                0,
                c"preeditStartCallback".as_ptr(),
                &start_callback as *const XimCallback,
                c"preeditDoneCallback".as_ptr(),
                &done_callback as *const XimCallback,
                c"preeditDrawCallback".as_ptr(),
                &draw_callback as *const XimCallback,
                ptr::null::<c_char>(),
            )
        };
        let input_context = if preedit_attributes.is_null() {
            ptr::null_mut()
        } else {
            // SAFETY: XCreateIC is variadic; the attributes match the documented XIMPreeditCallbacks style.
            unsafe {
                (api.create_ic)(
                    input_method,
                    c"inputStyle".as_ptr(),
                    XIM_PREEDIT_CALLBACKS | XIM_STATUS_NOTHING,
                    c"clientWindow".as_ptr(),
                    window,
                    c"focusWindow".as_ptr(),
                    window,
                    c"preeditAttributes".as_ptr(),
                    preedit_attributes,
                    ptr::null::<c_char>(),
                )
            }
        };
        // SAFETY: preedit_attributes came from XVaCreateNestedList and is no longer needed after XCreateIC.
        if !preedit_attributes.is_null() {
            // SAFETY: XVaCreateNestedList returns Xlib-owned storage that must be released with XFree.
            let _ = unsafe { (api.free)(preedit_attributes) };
        }
        let input_context = if input_context.is_null() {
            // Some input methods do not support inline callbacks; retain commit-only XIM as a fallback.
            // SAFETY: XCreateIC is variadic; these attributes match XIMPreeditNothing.
            unsafe {
                (api.create_ic)(
                    input_method,
                    c"inputStyle".as_ptr(),
                    XIM_PREEDIT_NOTHING | XIM_STATUS_NOTHING,
                    c"clientWindow".as_ptr(),
                    window,
                    c"focusWindow".as_ptr(),
                    window,
                    ptr::null::<c_char>(),
                )
            }
        } else {
            input_context
        };
        if input_context.is_null() {
            // SAFETY: input_method and display were returned by XOpenIM and XOpenDisplay, respectively.
            let _ = unsafe { (api.close_im)(input_method) };
            // SAFETY: display was returned by XOpenDisplay and no Xlib object retains it after close_im.
            let _ = unsafe { (api.close_display)(display) };
            return None;
        }
        // SAFETY: display is live and XDefaultRootWindow returns a root XID for this connection.
        let root = unsafe { (api.default_root_window)(display) };
        Some(Self {
            _library: library,
            api,
            display,
            input_method,
            input_context,
            window,
            root,
            callback_state,
        })
    }

    /// Tells XIM whether this client window owns keyboard focus.
    pub fn set_focus(&mut self, focused: bool) {
        // SAFETY: input_context is live and owned by this X11Ime instance.
        unsafe {
            if focused {
                (self.api.set_ic_focus)(self.input_context);
            } else {
                (self.api.unset_ic_focus)(self.input_context);
            }
        }
    }

    /// Takes composition notifications queued by the input-method callbacks.
    pub fn take_preedit_events(&mut self) -> Vec<PreeditEvent> {
        mem::take(&mut self.callback_state.events)
    }

    /// Runs XIM's filter and UTF-8 lookup for one X11 key press.
    #[must_use]
    pub fn lookup(&mut self, keycode: u8, state: u16, time: u32, x: i16, y: i16) -> Option<String> {
        let mut event = XKeyEvent {
            type_: 2,
            serial: 0,
            send_event: 0,
            display: self.display,
            window: self.window,
            root: self.root,
            subwindow: 0,
            time: c_ulong::from(time),
            x: c_int::from(x),
            y: c_int::from(y),
            x_root: c_int::from(x),
            y_root: c_int::from(y),
            state: c_uint::from(state),
            keycode: c_uint::from(keycode),
            same_screen: 1,
        };
        let mut xevent = XEvent { key: event };
        // SAFETY: xevent has the Xlib XEvent layout, and both window XIDs are valid server-side identifiers.
        if unsafe { (self.api.filter_event)(&mut xevent, self.window) } != 0 {
            return None;
        }
        let mut bytes = [0_u8; 64];
        let mut keysym: c_ulong = 0;
        let mut status: c_int = 0;
        // SAFETY: event and output pointers are valid for the duration of the Xlib call; buffer capacity is explicit.
        let count = unsafe {
            (self.api.utf8_lookup_string)(
                self.input_context,
                &mut event,
                bytes.as_mut_ptr().cast(),
                c_int::try_from(bytes.len()).unwrap_or(64),
                &mut keysym,
                &mut status,
            )
        };
        if status == X_BUFFER_OVERFLOW && count > 0 {
            return self.lookup_overflow(event, usize::try_from(count).ok()?, keysym, status);
        }
        decode_lookup(bytes.get(..usize::try_from(count).ok()?)?, status)
    }

    fn lookup_overflow(
        &mut self,
        mut event: XKeyEvent,
        required: usize,
        mut keysym: c_ulong,
        mut status: c_int,
    ) -> Option<String> {
        if required > MAX_LOOKUP_BYTES {
            return None;
        }
        let mut bytes = vec![0_u8; required];
        // SAFETY: event and output pointers are valid; the bounded buffer prevents an untrusted IM from allocating
        // without a limit. Xutf8LookupString reports a negative result if this capacity is insufficient.
        let count = unsafe {
            (self.api.utf8_lookup_string)(
                self.input_context,
                &mut event,
                bytes.as_mut_ptr().cast(),
                c_int::try_from(bytes.len()).unwrap_or(i32::MAX),
                &mut keysym,
                &mut status,
            )
        };
        decode_lookup(bytes.get(..usize::try_from(count).ok()?)?, status)
    }
}

unsafe extern "C" fn xim_preedit_start(_: *mut c_void, client_data: *mut c_void, _: *mut c_void) -> c_int {
    if client_data.is_null() {
        return 0;
    }
    // SAFETY: XIM callbacks retain this pointer from XCreateIC; the boxed state outlives the input context and
    // callbacks are only invoked on its owning X11 reader thread.
    let state = unsafe { &mut *client_data.cast::<CallbackState>() };
    state.preedit.clear();
    state.active = true;
    state.events.push(PreeditEvent::Start);
    c_int::try_from(MAX_PREEDIT_CHARS).unwrap_or(4096)
}

unsafe extern "C" fn xim_preedit_done(_: *mut c_void, client_data: *mut c_void, _: *mut c_void) {
    if client_data.is_null() {
        return;
    }
    // SAFETY: the callback data pointer remains valid until after XDestroyIC on the owning reader thread.
    let state = unsafe { &mut *client_data.cast::<CallbackState>() };
    if state.active {
        state.active = false;
        state.preedit.clear();
        state.events.push(PreeditEvent::Cancel);
    }
}

unsafe extern "C" fn xim_preedit_draw(_: *mut c_void, client_data: *mut c_void, call_data: *mut c_void) {
    if client_data.is_null() || call_data.is_null() {
        return;
    }
    // SAFETY: Xlib passes the callback data pointer registered for this XIC; both state and call data are live
    // for the duration of the synchronous callback on the X11 reader thread.
    let state = unsafe { &mut *client_data.cast::<CallbackState>() };
    // SAFETY: XIMPreeditDrawCallback passes an XIMPreeditDrawCallbackStruct at call_data.
    let draw = unsafe { &*call_data.cast::<XimPreeditDraw>() };
    if !state.active {
        state.active = true;
        state.events.push(PreeditEvent::Start);
    }
    let replacement = if draw.text.is_null() {
        Some(String::new())
    } else {
        // SAFETY: non-null draw.text is the XIM-owned XIMText argument valid for this callback.
        xim_text_to_string(unsafe { &*draw.text })
    };
    let Some(replacement) = replacement else {
        return;
    };
    let Some(preedit) = replace_preedit(&state.preedit, draw.chg_first, draw.chg_length, &replacement) else {
        state.active = false;
        state.preedit.clear();
        state.events.push(PreeditEvent::Cancel);
        return;
    };
    state.preedit = preedit;
    state.events.push(PreeditEvent::Update(state.preedit.clone()));
}

fn xim_text_to_string(text: &XimText) -> Option<String> {
    let length = usize::from(text.length);
    if length > MAX_PREEDIT_CHARS {
        return None;
    }
    if length == 0 {
        return Some(String::new());
    }
    let mut wide = vec![0; length];
    if text.encoding_is_wchar != 0 {
        // SAFETY: XIMText.length bounds the Xlib-owned wide-character sequence for the duration of the callback.
        let source = unsafe { text.string.wide_char };
        if source.is_null() {
            return None;
        }
        // SAFETY: the XIMText contract provides `length` wchar_t units at this pointer during the callback.
        let units = unsafe { std::slice::from_raw_parts(source, length) };
        wide.copy_from_slice(units);
    } else {
        // SAFETY: XIM owns a multibyte string for the duration of this callback; XIMText.length limits the
        // requested output characters and the output buffer is exactly that bounded size.
        let source = unsafe { text.string.multi_byte };
        if source.is_null() {
            return None;
        }
        // SAFETY: `source` is the XIM-owned multibyte string and `wide` can hold `length` converted wchar_t values.
        let converted = unsafe { mbstowcs(wide.as_mut_ptr(), source, length) };
        if converted == usize::MAX || converted > length {
            return None;
        }
        wide.truncate(converted);
    }
    Some(
        wide.into_iter()
            .map(|unit| u32::try_from(unit).ok().and_then(char::from_u32).unwrap_or('\u{fffd}'))
            .collect(),
    )
}

fn replace_preedit(current: &str, first: c_int, length: c_int, replacement: &str) -> Option<String> {
    let first = usize::try_from(first).ok()?;
    let length = usize::try_from(length).ok()?;
    let mut characters: Vec<char> = current.chars().collect();
    let end = first.checked_add(length)?;
    if first > characters.len() || end > characters.len() {
        return None;
    }
    let final_length = characters
        .len()
        .checked_sub(length)?
        .checked_add(replacement.chars().count())?;
    if final_length > MAX_PREEDIT_CHARS {
        return None;
    }
    characters.splice(first..end, replacement.chars());
    Some(characters.into_iter().collect())
}

impl Drop for X11Ime {
    fn drop(&mut self) {
        // SAFETY: these handles were created by Xlib and are destroyed once in dependency order.
        unsafe {
            (self.api.destroy_ic)(self.input_context);
            let _ = (self.api.close_im)(self.input_method);
            let _ = (self.api.close_display)(self.display);
        }
    }
}

fn decode_lookup(bytes: &[u8], status: c_int) -> Option<String> {
    if !matches!(status, X_LOOKUP_CHARS | X_LOOKUP_BOTH) || bytes.is_empty() {
        return None;
    }
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

#[link(name = "c")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
}

#[cfg(test)]
mod tests {
    use super::{decode_lookup, replace_preedit, MAX_PREEDIT_CHARS, X_LOOKUP_BOTH, X_LOOKUP_CHARS};

    const X_LOOKUP_KEYSYM: i32 = 1;

    #[test]
    fn xutf8_lookup_only_returns_committed_text_statuses() {
        assert_eq!(
            decode_lookup("日本語".as_bytes(), X_LOOKUP_CHARS).as_deref(),
            Some("日本語")
        );
        assert_eq!(decode_lookup("한글".as_bytes(), X_LOOKUP_BOTH).as_deref(), Some("한글"));
        assert_eq!(decode_lookup(b"a", X_LOOKUP_KEYSYM), None);
        assert_eq!(decode_lookup(&[0xff], X_LOOKUP_CHARS), None);
    }

    #[test]
    fn preedit_draw_applies_character_ranges_and_bounds_untrusted_lengths() {
        assert_eq!(replace_preedit("かな", 1, 1, "漢字"), Some("か漢字".to_owned()));
        assert_eq!(replace_preedit("かな", -1, 1, "x"), None);
        assert_eq!(replace_preedit("かな", 1, 9, "x"), None);
        assert_eq!(replace_preedit("かな", 2, 0, &"x".repeat(MAX_PREEDIT_CHARS)), None);
    }
}
