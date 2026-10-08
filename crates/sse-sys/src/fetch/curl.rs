use super::{Response, SystemFetch};
use sse_core::{Error, Result};
use std::{
    ffi::{c_char, c_int, c_long, c_void, CStr, CString},
    ptr,
    sync::OnceLock,
};

const RTLD_NOW: c_int = 2;
const CURL_GLOBAL_DEFAULT: c_long = 3;
const CURLE_OK: c_int = 0;
const CURLOPT_WRITEDATA: c_int = 10_001;
const CURLOPT_URL: c_int = 10_002;
const CURLOPT_RANGE: c_int = 10_007;
const CURLOPT_POSTFIELDS: c_int = 10_015;
const CURLOPT_WRITEFUNCTION: c_int = 20_011;
const CURLOPT_POSTFIELDSIZE: c_int = 60;
const CURLOPT_FOLLOWLOCATION: c_int = 52;
const CURLOPT_MAXREDIRS: c_int = 68;
const CURLOPT_NOSIGNAL: c_int = 99;
const CURLOPT_CONNECTTIMEOUT_MS: c_int = 156;
const CURLOPT_TIMEOUT_MS: c_int = 155;
const CURLOPT_PROTOCOLS: c_int = 181;
const CURLOPT_REDIR_PROTOCOLS: c_int = 182;
const CURLOPT_HTTPHEADER: c_int = 10_023;
const CURLPROTO_HTTPS: c_long = 1 << 1;
const CURLINFO_EFFECTIVE_URL: c_int = 0x10_0001;
const CURLINFO_RESPONSE_CODE: c_int = 0x20_0002;
const CURLINFO_CONTENT_LENGTH_DOWNLOAD_T: c_int = 0x60_000f;

type EasyInit = unsafe extern "C" fn() -> *mut c_void;
type EasyCleanup = unsafe extern "C" fn(*mut c_void);
type EasyPerform = unsafe extern "C" fn(*mut c_void) -> c_int;
type EasySetopt = unsafe extern "C" fn(*mut c_void, c_int, ...) -> c_int;
type EasyGetinfo = unsafe extern "C" fn(*mut c_void, c_int, ...) -> c_int;
type EasyStrerror = unsafe extern "C" fn(c_int) -> *const c_char;
type GlobalInit = unsafe extern "C" fn(c_long) -> c_int;
type WriteCallback = unsafe extern "C" fn(*mut c_char, usize, usize, *mut c_void) -> usize;
type SlistAppend = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type SlistFreeAll = unsafe extern "C" fn(*mut c_void);

#[derive(Clone, Copy)]
struct Api {
    easy_init: EasyInit,
    easy_cleanup: EasyCleanup,
    easy_perform: EasyPerform,
    easy_setopt: EasySetopt,
    easy_getinfo: EasyGetinfo,
    easy_strerror: EasyStrerror,
    slist_append: SlistAppend,
    slist_free_all: SlistFreeAll,
}

static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();

#[cfg(target_os = "linux")]
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

fn api() -> Result<&'static Api> {
    match API.get_or_init(load) {
        Ok(api) => Ok(api),
        Err(message) => Err(Error::System(message.clone())),
    }
}

fn load() -> std::result::Result<Api, String> {
    #[cfg(target_os = "linux")]
    let library = c"libcurl.so.4";
    #[cfg(target_os = "macos")]
    let library = c"libcurl.4.dylib";
    // SAFETY: library is a static NUL-terminated name and RTLD_NOW is a valid dlopen flag.
    let handle = unsafe { dlopen(library.as_ptr(), RTLD_NOW) };
    if handle.is_null() {
        return Err(format!("{} is not available", library.to_string_lossy()));
    }
    let symbol = |name: &'static CStr| -> std::result::Result<*mut c_void, String> {
        // SAFETY: handle remains intentionally loaded for process lifetime; name is NUL-terminated.
        let pointer = unsafe { dlsym(handle, name.as_ptr()) };
        if pointer.is_null() {
            Err(format!("libcurl symbol {} is missing", name.to_string_lossy()))
        } else {
            Ok(pointer)
        }
    };
    // SAFETY: each dlsym result is checked non-null and the symbol names have the libcurl ABI signatures below.
    let global_init: GlobalInit = unsafe { std::mem::transmute(symbol(c"curl_global_init")?) };
    // SAFETY: same argument as above for the exact named libcurl function.
    let easy_init: EasyInit = unsafe { std::mem::transmute(symbol(c"curl_easy_init")?) };
    // SAFETY: same argument as above for the exact named libcurl function.
    let easy_cleanup: EasyCleanup = unsafe { std::mem::transmute(symbol(c"curl_easy_cleanup")?) };
    // SAFETY: same argument as above for the exact named libcurl function.
    let easy_perform: EasyPerform = unsafe { std::mem::transmute(symbol(c"curl_easy_perform")?) };
    // SAFETY: same argument as above for the exact named libcurl function.
    let easy_setopt: EasySetopt = unsafe { std::mem::transmute(symbol(c"curl_easy_setopt")?) };
    // SAFETY: same argument as above for the exact named libcurl function.
    let easy_getinfo: EasyGetinfo = unsafe { std::mem::transmute(symbol(c"curl_easy_getinfo")?) };
    // SAFETY: same argument as above for the exact named libcurl function.
    let easy_strerror: EasyStrerror = unsafe { std::mem::transmute(symbol(c"curl_easy_strerror")?) };
    // SAFETY: same argument as above for the exact named libcurl function.
    let slist_append: SlistAppend = unsafe { std::mem::transmute(symbol(c"curl_slist_append")?) };
    // SAFETY: same argument as above for the exact named libcurl function.
    let slist_free_all: SlistFreeAll = unsafe { std::mem::transmute(symbol(c"curl_slist_free_all")?) };
    // SAFETY: global initialization is called once by OnceLock before any easy handle is created.
    let result = unsafe { global_init(CURL_GLOBAL_DEFAULT) };
    if result != CURLE_OK {
        return Err(format!("curl_global_init failed with code {result}"));
    }
    Ok(Api {
        easy_init,
        easy_cleanup,
        easy_perform,
        easy_setopt,
        easy_getinfo,
        easy_strerror,
        slist_append,
        slist_free_all,
    })
}

struct CallbackState<'a> {
    sink: &'a mut dyn FnMut(&[u8]) -> bool,
    received: u64,
    limit: u64,
    cancelled: bool,
    too_large: bool,
}

unsafe extern "C" fn write_callback(data: *mut c_char, size: usize, count: usize, user: *mut c_void) -> usize {
    let Some(length) = size.checked_mul(count) else {
        return 0;
    };
    if user.is_null() || (data.is_null() && length != 0) {
        return 0;
    }
    // SAFETY: WRITEDATA points to CallbackState for the synchronous duration of curl_easy_perform.
    let state = unsafe { &mut *user.cast::<CallbackState<'static>>() };
    if isize::try_from(length).is_err() {
        state.too_large = true;
        return 0;
    }
    let Ok(length64) = u64::try_from(length) else {
        state.too_large = true;
        return 0;
    };
    let Some(next) = state.received.checked_add(length64) else {
        state.too_large = true;
        return 0;
    };
    if next > state.limit {
        state.too_large = true;
        return 0;
    }
    let bytes = if length == 0 {
        &[]
    } else {
        // SAFETY: the null check above and libcurl's callback contract provide `length` readable bytes.
        unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) }
    };
    if !(state.sink)(bytes) {
        state.cancelled = true;
        return 0;
    }
    state.received = next;
    length
}

fn milliseconds(value: std::time::Duration) -> Result<c_long> {
    c_long::try_from(value.as_millis()).map_err(|_| Error::Refused("timeout is too large".to_owned()))
}

fn curl_error(api: &Api, code: c_int) -> Error {
    // SAFETY: curl_easy_strerror returns a static NUL-terminated string for a CURLcode.
    let pointer = unsafe { (api.easy_strerror)(code) };
    if pointer.is_null() {
        return Error::System(format!("libcurl error {code}"));
    }
    // SAFETY: pointer is non-null and documented as a NUL-terminated libcurl-owned string.
    let text = unsafe { CStr::from_ptr(pointer) }.to_string_lossy();
    Error::System(format!("libcurl: {text}"))
}

pub(super) fn get(
    config: &SystemFetch,
    url: &str,
    range_from: u64,
    sink: &mut dyn FnMut(&[u8]) -> bool,
) -> Result<Response> {
    let api = api()?;
    let url_c = CString::new(url).map_err(|_| Error::Refused("URL contains NUL".to_owned()))?;
    let range = if range_from == 0 {
        None
    } else {
        Some(CString::new(format!("{range_from}-")).map_err(|_| Error::Refused("range contains NUL".to_owned()))?)
    };
    // SAFETY: API was resolved from libcurl and global initialization succeeded.
    let handle = unsafe { (api.easy_init)() };
    if handle.is_null() {
        return Err(Error::System("curl_easy_init returned null".to_owned()));
    }
    let mut state = CallbackState {
        sink,
        received: 0,
        limit: config.max_bytes,
        cancelled: false,
        too_large: false,
    };
    let result = (|| -> Result<Response> {
        let set_long = |option: c_int, value: c_long| -> Result<()> {
            // SAFETY: option is a CURLOPT_LONG option and value has C long ABI.
            let code = unsafe { (api.easy_setopt)(handle, option, value) };
            if code == CURLE_OK {
                Ok(())
            } else {
                Err(curl_error(api, code))
            }
        };
        // SAFETY: URL and callback pointers stay alive through synchronous curl_easy_perform.
        let code = unsafe { (api.easy_setopt)(handle, CURLOPT_URL, url_c.as_ptr()) };
        if code != CURLE_OK {
            return Err(curl_error(api, code));
        }
        set_long(CURLOPT_PROTOCOLS, CURLPROTO_HTTPS)?;
        set_long(CURLOPT_REDIR_PROTOCOLS, CURLPROTO_HTTPS)?;
        set_long(CURLOPT_FOLLOWLOCATION, 1)?;
        set_long(CURLOPT_MAXREDIRS, 8)?;
        set_long(CURLOPT_NOSIGNAL, 1)?;
        set_long(CURLOPT_CONNECTTIMEOUT_MS, milliseconds(config.connect_timeout)?)?;
        set_long(CURLOPT_TIMEOUT_MS, milliseconds(config.total_timeout)?)?;
        if let Some(range) = &range {
            // SAFETY: range CString lives through perform and CURLOPT_RANGE expects a char pointer.
            let code = unsafe { (api.easy_setopt)(handle, CURLOPT_RANGE, range.as_ptr()) };
            if code != CURLE_OK {
                return Err(curl_error(api, code));
            }
        }
        // SAFETY: function pointer matches curl_write_callback and state remains live through perform.
        let code = unsafe { (api.easy_setopt)(handle, CURLOPT_WRITEFUNCTION, write_callback as WriteCallback) };
        if code != CURLE_OK {
            return Err(curl_error(api, code));
        }
        // SAFETY: state address is stable until perform returns.
        let code = unsafe {
            (api.easy_setopt)(
                handle,
                CURLOPT_WRITEDATA,
                (&mut state as *mut CallbackState<'_>).cast::<c_void>(),
            )
        };
        if code != CURLE_OK {
            return Err(curl_error(api, code));
        }
        // SAFETY: all configured pointers remain live for this synchronous call.
        let code = unsafe { (api.easy_perform)(handle) };
        if code != CURLE_OK {
            if state.cancelled {
                return Err(Error::Refused("fetch cancelled by sink".to_owned()));
            }
            if state.too_large {
                return Err(Error::Refused("HTTPS response exceeds size limit".to_owned()));
            }
            return Err(curl_error(api, code));
        }
        let mut status: c_long = 0;
        let mut content_length: i64 = -1;
        let mut effective: *mut c_char = ptr::null_mut();
        // SAFETY: output pointers match the documented CURLINFO result types and handle is live.
        if unsafe { (api.easy_getinfo)(handle, CURLINFO_RESPONSE_CODE, &mut status) } != CURLE_OK {
            return Err(Error::System("libcurl could not report HTTP status".to_owned()));
        }
        // SAFETY: curl_off_t is a signed 64-bit integer on the supported 64-bit targets.
        let _ = unsafe { (api.easy_getinfo)(handle, CURLINFO_CONTENT_LENGTH_DOWNLOAD_T, &mut content_length) };
        // SAFETY: EFFECTIVE_URL returns a libcurl-owned char pointer.
        let _ = unsafe { (api.easy_getinfo)(handle, CURLINFO_EFFECTIVE_URL, &mut effective) };
        let status = u16::try_from(status).map_err(|_| Error::damaged("HTTP status out of range"))?;
        let content_length = u64::try_from(content_length).ok();
        if content_length.is_some_and(|length| length > config.max_bytes) {
            return Err(Error::Refused("HTTPS response exceeds size limit".to_owned()));
        }
        let final_url = if effective.is_null() {
            url.to_owned()
        } else {
            // SAFETY: non-null EFFECTIVE_URL is a NUL-terminated string owned until cleanup.
            unsafe { CStr::from_ptr(effective) }.to_string_lossy().into_owned()
        };
        if !final_url.starts_with("https://") {
            return Err(Error::Refused("redirect left HTTPS".to_owned()));
        }
        Ok(Response {
            status,
            content_length,
            final_url,
        })
    })();
    // SAFETY: handle came from curl_easy_init and is cleaned exactly once after all getinfo calls.
    unsafe { (api.easy_cleanup)(handle) };
    result
}

pub(super) fn post(
    config: &SystemFetch,
    url: &str,
    content_type: &str,
    body: &[u8],
    sink: &mut dyn FnMut(&[u8]) -> bool,
) -> Result<Response> {
    let api = api()?;
    let url_c = CString::new(url).map_err(|_| Error::Refused("URL contains NUL".to_owned()))?;
    let header = CString::new(format!("Content-Type: {content_type}"))
        .map_err(|_| Error::Refused("POST header contains NUL".to_owned()))?;
    // SAFETY: API was resolved from libcurl and global initialization succeeded.
    let handle = unsafe { (api.easy_init)() };
    if handle.is_null() {
        return Err(Error::System("curl_easy_init returned null".to_owned()));
    }
    // SAFETY: libcurl copies the header string into the list; `header` lives through the call.
    let headers = unsafe { (api.slist_append)(ptr::null_mut(), header.as_ptr()) };
    if headers.is_null() {
        // SAFETY: handle came from curl_easy_init and is cleaned exactly once.
        unsafe { (api.easy_cleanup)(handle) };
        return Err(Error::System("curl_slist_append returned null".to_owned()));
    }
    let mut state = CallbackState {
        sink,
        received: 0,
        limit: config.max_bytes,
        cancelled: false,
        too_large: false,
    };
    let result = (|| -> Result<Response> {
        let set_long = |option: c_int, value: c_long| -> Result<()> {
            // SAFETY: option is a CURLOPT_LONG option and value has C long ABI.
            let code = unsafe { (api.easy_setopt)(handle, option, value) };
            if code == CURLE_OK {
                Ok(())
            } else {
                Err(curl_error(api, code))
            }
        };
        // SAFETY: URL and request body pointers stay alive through synchronous curl_easy_perform.
        let code = unsafe { (api.easy_setopt)(handle, CURLOPT_URL, url_c.as_ptr()) };
        if code != CURLE_OK {
            return Err(curl_error(api, code));
        }
        set_long(CURLOPT_PROTOCOLS, CURLPROTO_HTTPS)?;
        set_long(CURLOPT_REDIR_PROTOCOLS, CURLPROTO_HTTPS)?;
        // Reports are never redirected: this prevents a POST body being replayed at another URL.
        set_long(CURLOPT_FOLLOWLOCATION, 0)?;
        set_long(CURLOPT_NOSIGNAL, 1)?;
        set_long(CURLOPT_CONNECTTIMEOUT_MS, milliseconds(config.connect_timeout)?)?;
        set_long(CURLOPT_TIMEOUT_MS, milliseconds(config.total_timeout)?)?;
        // SAFETY: the header list remains live until after handle cleanup.
        let code = unsafe { (api.easy_setopt)(handle, CURLOPT_HTTPHEADER, headers) };
        if code != CURLE_OK {
            return Err(curl_error(api, code));
        }
        // SAFETY: body storage remains borrowed through the synchronous transfer; POSTFIELDS is read-only.
        let code = unsafe { (api.easy_setopt)(handle, CURLOPT_POSTFIELDS, body.as_ptr().cast::<c_char>()) };
        if code != CURLE_OK {
            return Err(curl_error(api, code));
        }
        set_long(
            CURLOPT_POSTFIELDSIZE,
            c_long::try_from(body.len()).map_err(|_| Error::Refused("POST body is too large".to_owned()))?,
        )?;
        // SAFETY: function pointer matches curl_write_callback and state remains live through perform.
        let code = unsafe { (api.easy_setopt)(handle, CURLOPT_WRITEFUNCTION, write_callback as WriteCallback) };
        if code != CURLE_OK {
            return Err(curl_error(api, code));
        }
        // SAFETY: state address is stable until perform returns.
        let code = unsafe {
            (api.easy_setopt)(
                handle,
                CURLOPT_WRITEDATA,
                (&mut state as *mut CallbackState<'_>).cast::<c_void>(),
            )
        };
        if code != CURLE_OK {
            return Err(curl_error(api, code));
        }
        // SAFETY: all configured pointers remain live for this synchronous call.
        let code = unsafe { (api.easy_perform)(handle) };
        if code != CURLE_OK {
            if state.cancelled {
                return Err(Error::Refused("fetch cancelled by sink".to_owned()));
            }
            if state.too_large {
                return Err(Error::Refused("HTTPS response exceeds size limit".to_owned()));
            }
            return Err(curl_error(api, code));
        }
        let mut status: c_long = 0;
        let mut content_length: i64 = -1;
        let mut effective: *mut c_char = ptr::null_mut();
        // SAFETY: output pointers match the documented CURLINFO result types and handle is live.
        if unsafe { (api.easy_getinfo)(handle, CURLINFO_RESPONSE_CODE, &mut status) } != CURLE_OK {
            return Err(Error::System("libcurl could not report HTTP status".to_owned()));
        }
        // SAFETY: curl_off_t is a signed 64-bit integer on the supported 64-bit targets.
        let _ = unsafe { (api.easy_getinfo)(handle, CURLINFO_CONTENT_LENGTH_DOWNLOAD_T, &mut content_length) };
        // SAFETY: EFFECTIVE_URL returns a libcurl-owned char pointer.
        let _ = unsafe { (api.easy_getinfo)(handle, CURLINFO_EFFECTIVE_URL, &mut effective) };
        let status = u16::try_from(status).map_err(|_| Error::damaged("HTTP status out of range"))?;
        let content_length = u64::try_from(content_length).ok();
        if content_length.is_some_and(|length| length > config.max_bytes) {
            return Err(Error::Refused("HTTPS response exceeds size limit".to_owned()));
        }
        let final_url = if effective.is_null() {
            url.to_owned()
        } else {
            // SAFETY: non-null EFFECTIVE_URL is a NUL-terminated string owned until cleanup.
            unsafe { CStr::from_ptr(effective) }.to_string_lossy().into_owned()
        };
        if !final_url.starts_with("https://") {
            return Err(Error::Refused("redirect left HTTPS".to_owned()));
        }
        Ok(Response {
            status,
            content_length,
            final_url,
        })
    })();
    // SAFETY: handle came from curl_easy_init and is cleaned exactly once after all getinfo calls.
    unsafe { (api.easy_cleanup)(handle) };
    // SAFETY: headers came from curl_slist_append and are no longer referenced after handle cleanup.
    unsafe { (api.slist_free_all)(headers) };
    result
}

#[cfg(test)]
mod tests {
    use super::{write_callback, CallbackState};
    use std::{
        ffi::c_void,
        io,
        process::Command,
        ptr,
        sync::atomic::{AtomicBool, Ordering},
    };

    const CHILD_ENV: &str = "SSE_CURL_EMPTY_CALLBACK_CHILD";

    fn run_child(test_name: &str, case: &str) -> io::Result<()> {
        let output = Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg(test_name)
            .arg("--nocapture")
            .env(CHILD_ENV, case)
            .output()?;
        assert!(
            output.status.success(),
            "callback child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    #[test]
    fn empty_curl_chunk_accepts_null_data_without_undefined_behavior() -> io::Result<()> {
        run_child("fetch::curl::tests::empty_curl_chunk_child", "empty")
    }

    #[test]
    fn empty_curl_chunk_child() {
        if std::env::var(CHILD_ENV).ok().as_deref() != Some("empty") {
            return;
        }

        let saw_valid_empty = AtomicBool::new(false);
        let mut sink = |bytes: &[u8]| {
            saw_valid_empty.store(bytes.is_empty(), Ordering::Relaxed);
            true
        };
        let mut state = CallbackState {
            sink: &mut sink,
            received: 0,
            limit: 1,
            cancelled: false,
            too_large: false,
        };
        let user = (&mut state as *mut CallbackState<'_>).cast::<c_void>();
        // SAFETY: the state is live for this direct callback invocation; (null, 0) is the case under test.
        let accepted = unsafe { write_callback(ptr::null_mut(), 0, 1, user) };
        assert_eq!(accepted, 0);
        assert!(saw_valid_empty.load(Ordering::Relaxed));
        assert_eq!(state.received, 0);
    }

    #[test]
    fn oversized_curl_chunk_is_rejected_before_slice_creation() -> io::Result<()> {
        run_child("fetch::curl::tests::oversized_curl_chunk_child", "oversized")
    }

    #[test]
    fn oversized_curl_chunk_child() -> io::Result<()> {
        if std::env::var(CHILD_ENV).ok().as_deref() != Some("oversized") {
            return Ok(());
        }

        let length = usize::try_from(isize::MAX)
            .ok()
            .and_then(|maximum| maximum.checked_add(1))
            .ok_or_else(|| io::Error::other("could not construct an oversized slice length"))?;
        let mut sink = |_: &[u8]| true;
        let mut state = CallbackState {
            sink: &mut sink,
            received: 0,
            limit: u64::MAX,
            cancelled: false,
            too_large: false,
        };
        let user = (&mut state as *mut CallbackState<'_>).cast::<c_void>();
        // SAFETY: callback state is live; this verifies rejection before the callback reads its synthetic buffer.
        let accepted =
            unsafe { write_callback(ptr::NonNull::<std::ffi::c_char>::dangling().as_ptr(), length, 1, user) };
        assert_eq!(accepted, 0);
        assert!(state.too_large);
        assert_eq!(state.received, 0);
        Ok(())
    }
}
