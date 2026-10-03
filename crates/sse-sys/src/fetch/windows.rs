use super::{Response, SystemFetch};
use sse_core::{Error, Result};
use std::{ffi::c_void, ptr, time::Instant};

type Hinternet = *mut c_void;
const ACCESS_TYPE_AUTOMATIC_PROXY: u32 = 4;
const FLAG_SECURE: u32 = 0x0080_0000;
const QUERY_CONTENT_LENGTH: u32 = 5;
const QUERY_STATUS_CODE: u32 = 19;
const QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
const QUERY_FLAG_NUMBER64: u32 = 0x0800_0000;
const OPTION_URL: u32 = 34;
const OPTION_REDIRECT_POLICY: u32 = 88;
const REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP: u32 = 1;

#[link(name = "winhttp")]
unsafe extern "system" {
    fn WinHttpOpen(
        agent: *const u16,
        access: u32,
        proxy: *const u16,
        bypass: *const u16,
        flags: u32,
    ) -> Hinternet;
    fn WinHttpConnect(
        session: Hinternet,
        server: *const u16,
        port: u16,
        reserved: u32,
    ) -> Hinternet;
    fn WinHttpOpenRequest(
        connect: Hinternet,
        verb: *const u16,
        object: *const u16,
        version: *const u16,
        referer: *const u16,
        accept_types: *const *const u16,
        flags: u32,
    ) -> Hinternet;
    fn WinHttpSetOption(
        handle: Hinternet,
        option: u32,
        buffer: *const c_void,
        length: u32,
    ) -> i32;
    fn WinHttpSetTimeouts(
        handle: Hinternet,
        resolve: i32,
        connect: i32,
        send: i32,
        receive: i32,
    ) -> i32;
    fn WinHttpSendRequest(
        request: Hinternet,
        headers: *const u16,
        headers_length: u32,
        optional: *mut c_void,
        optional_length: u32,
        total_length: u32,
        context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(request: Hinternet, reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(
        request: Hinternet,
        info_level: u32,
        name: *const u16,
        buffer: *mut c_void,
        buffer_length: *mut u32,
        index: *mut u32,
    ) -> i32;
    fn WinHttpQueryOption(
        handle: Hinternet,
        option: u32,
        buffer: *mut c_void,
        buffer_length: *mut u32,
    ) -> i32;
    fn WinHttpReadData(
        request: Hinternet,
        buffer: *mut c_void,
        bytes_to_read: u32,
        bytes_read: *mut u32,
    ) -> i32;
    fn WinHttpCloseHandle(handle: Hinternet) -> i32;
}

struct Handle(Hinternet);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: handle is owned by this RAII wrapper and is closed once.
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn milliseconds(value: std::time::Duration) -> Result<i32> {
    i32::try_from(value.as_millis())
        .map_err(|_| Error::Refused("timeout is too large".to_owned()))
}

fn parse_https(url: &str) -> Result<(String, u16, String)> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| Error::Refused("only HTTPS is allowed".to_owned()))?;
    let (authority, path) = rest.split_once('/').map_or(
        (rest, "/".to_owned()),
        |(authority, tail)| (authority, format!("/{tail}")),
    );
    if authority.is_empty() || authority.contains('@') {
        return Err(Error::Refused("invalid HTTPS authority".to_owned()));
    }
    if let Some(host) = authority.strip_prefix('[') {
        let end = host
            .find(']')
            .ok_or_else(|| Error::Refused("invalid IPv6 authority".to_owned()))?;
        let name = host.get(..end).unwrap_or_default().to_owned();
        let suffix = host.get(end.saturating_add(1)..).unwrap_or_default();
        let port = if suffix.is_empty() {
            443
        } else {
            suffix
                .strip_prefix(':')
                .ok_or_else(|| Error::Refused("invalid IPv6 port".to_owned()))?
                .parse::<u16>()
                .map_err(|_| Error::Refused("invalid HTTPS port".to_owned()))?
        };
        return Ok((name, port, path));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (
            host.to_owned(),
            port.parse::<u16>()
                .map_err(|_| Error::Refused("invalid HTTPS port".to_owned()))?,
        ),
        _ => (authority.to_owned(), 443),
    };
    if host.is_empty() {
        return Err(Error::Refused("empty HTTPS host".to_owned()));
    }
    Ok((host, port, path))
}

fn status(request: Hinternet) -> Result<u16> {
    let mut value = 0u32;
    let mut length = u32::try_from(std::mem::size_of::<u32>()).unwrap_or_default();
    // SAFETY: value and length are writable and QUERY_FLAG_NUMBER requests a DWORD result.
    if unsafe {
        WinHttpQueryHeaders(
            request,
            QUERY_STATUS_CODE | QUERY_FLAG_NUMBER,
            ptr::null(),
            (&mut value as *mut u32).cast(),
            &mut length,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(Error::System("WinHTTP could not read status".to_owned()));
    }
    u16::try_from(value).map_err(|_| Error::damaged("HTTP status out of range"))
}

fn content_length(request: Hinternet) -> Option<u64> {
    let mut value = 0u64;
    let mut length = u32::try_from(std::mem::size_of::<u64>()).unwrap_or_default();
    // SAFETY: value and length are writable and NUMBER64 requests an unsigned 64-bit header value.
    if unsafe {
        WinHttpQueryHeaders(
            request,
            QUERY_CONTENT_LENGTH | QUERY_FLAG_NUMBER64,
            ptr::null(),
            (&mut value as *mut u64).cast(),
            &mut length,
            ptr::null_mut(),
        )
    } == 0
    {
        None
    } else {
        Some(value)
    }
}

fn effective_url(request: Hinternet, fallback: &str) -> String {
    let mut length = 0u32;
    // SAFETY: null buffer asks WinHTTP for required byte count.
    let _ = unsafe { WinHttpQueryOption(request, OPTION_URL, ptr::null_mut(), &mut length) };
    if length < 2 {
        return fallback.to_owned();
    }
    let units = usize::try_from(length / 2).unwrap_or_default();
    let mut buffer = vec![0u16; units];
    // SAFETY: buffer has exactly the byte capacity reported by WinHTTP.
    if unsafe {
        WinHttpQueryOption(
            request,
            OPTION_URL,
            buffer.as_mut_ptr().cast(),
            &mut length,
        )
    } == 0
    {
        return fallback.to_owned();
    }
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(buffer.get(..end).unwrap_or_default())
}

pub(super) fn get(
    config: &SystemFetch,
    url: &str,
    range_from: u64,
    sink: &mut dyn FnMut(&[u8]) -> bool,
) -> Result<Response> {
    let (host, port, path) = parse_https(url)?;
    let agent = wide("S.T.A.L.K.E.R. Save Editor/2");
    // SAFETY: NUL-terminated agent is valid and automatic proxy asks WinHTTP to use OS proxy configuration.
    let session = Handle(unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err(Error::System("WinHttpOpen failed".to_owned()));
    }
    let connect_ms = milliseconds(config.connect_timeout)?;
    let total_ms = milliseconds(config.total_timeout)?;
    // SAFETY: session is live; timeout integers are milliseconds.
    let _ = unsafe {
        WinHttpSetTimeouts(session.0, connect_ms, connect_ms, total_ms, total_ms)
    };
    let host_w = wide(&host);
    // SAFETY: session and NUL-terminated host are live for the call.
    let connect = Handle(unsafe { WinHttpConnect(session.0, host_w.as_ptr(), port, 0) });
    if connect.0.is_null() {
        return Err(Error::System("WinHttpConnect failed".to_owned()));
    }
    let verb = wide("GET");
    let path_w = wide(&path);
    // SAFETY: connect is live; verb/path are NUL-terminated; null optional arguments select defaults.
    let request = Handle(unsafe {
        WinHttpOpenRequest(
            connect.0,
            verb.as_ptr(),
            path_w.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            FLAG_SECURE,
        )
    });
    if request.0.is_null() {
        return Err(Error::System("WinHttpOpenRequest failed".to_owned()));
    }
    let policy = REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP;
    // SAFETY: request is live and option buffer points to a DWORD policy.
    if unsafe {
        WinHttpSetOption(
            request.0,
            OPTION_REDIRECT_POLICY,
            (&policy as *const u32).cast(),
            u32::try_from(std::mem::size_of::<u32>()).unwrap_or_default(),
        )
    } == 0
    {
        return Err(Error::System(
            "WinHTTP redirect policy failed".to_owned(),
        ));
    }
    let range = if range_from == 0 {
        None
    } else {
        Some(wide(&format!("Range: bytes={range_from}-\r\n")))
    };
    let (headers, headers_len) = range.as_ref().map_or((ptr::null(), 0), |value| {
        (
            value.as_ptr(),
            u32::try_from(value.len().saturating_sub(1)).unwrap_or_default(),
        )
    });
    // SAFETY: request and optional header storage remain live through SendRequest.
    if unsafe {
        WinHttpSendRequest(
            request.0,
            headers,
            headers_len,
            ptr::null_mut(),
            0,
            0,
            0,
        )
    } == 0
    {
        return Err(Error::System("WinHttpSendRequest failed".to_owned()));
    }
    // SAFETY: request is live and no reserved argument is supplied.
    if unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) } == 0 {
        return Err(Error::System(
            "WinHttpReceiveResponse failed".to_owned(),
        ));
    }
    let status = status(request.0)?;
    let content_length = content_length(request.0);
    if content_length.is_some_and(|length| length > config.max_bytes) {
        return Err(Error::Refused(
            "HTTPS response exceeds size limit".to_owned(),
        ));
    }
    let final_url = effective_url(request.0, url);
    if !final_url.starts_with("https://") {
        return Err(Error::Refused("redirect left HTTPS".to_owned()));
    }
    let started = Instant::now();
    let mut delivered = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if started.elapsed() > config.total_timeout {
            return Err(Error::System("HTTPS transfer timed out".to_owned()));
        }
        let mut read = 0u32;
        // SAFETY: request is live and buffer is writable for its full declared length.
        if unsafe {
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                u32::try_from(buffer.len()).unwrap_or_default(),
                &mut read,
            )
        } == 0
        {
            return Err(Error::System("WinHttpReadData failed".to_owned()));
        }
        if read == 0 {
            break;
        }
        delivered = delivered
            .checked_add(u64::from(read))
            .ok_or_else(|| Error::Refused("HTTPS response size overflow".to_owned()))?;
        if delivered > config.max_bytes {
            return Err(Error::Refused(
                "HTTPS response exceeds size limit".to_owned(),
            ));
        }
        let read = usize::try_from(read).map_err(|_| Error::damaged("WinHTTP read size"))?;
        if !sink(buffer.get(..read).unwrap_or_default()) {
            return Err(Error::Refused("fetch cancelled by sink".to_owned()));
        }
    }
    Ok(Response {
        status,
        content_length,
        final_url,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_https;

    #[test]
    fn https_parser_keeps_path_query_and_port() {
        assert_eq!(
            parse_https("https://example.test:8443/a?b=c"),
            Ok(("example.test".to_owned(), 8443, "/a?b=c".to_owned()))
        );
        assert!(parse_https("https://user@example.test/").is_err());
        assert!(parse_https("http://example.test/").is_err());
    }
}
