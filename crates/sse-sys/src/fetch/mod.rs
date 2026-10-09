//! Bounded streaming fetches through the operating system.

use sse_core::{Error, Result};
use std::time::Duration;
#[cfg(any(target_os = "windows", test))]
use std::time::Instant;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod curl;
mod local;
use local::FileFetch;
#[cfg(target_os = "windows")]
mod windows;

/// Default maximum body size accepted by the system fetcher.
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum request body accepted by the system POST implementation.
pub const MAX_POST_BODY_BYTES: u64 = 2 * 1024 * 1024;

/// Byte interval advertised by an HTTP Content-Range response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentRange {
    /// First byte included in the response.
    pub start: u64,
    /// Last byte included in the response.
    pub end: u64,
    /// Total size of the complete representation.
    pub total: u64,
}

/// Parses a complete byte Content-Range value such as bytes 100-199/500.
#[must_use]
pub fn parse_content_range(value: &str) -> Option<ContentRange> {
    let value = value.trim();
    let (unit, range_and_total) = value.split_once(' ')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (range, total) = range_and_total.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let parsed = ContentRange {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
        total: total.parse().ok()?,
    };
    (parsed.start <= parsed.end && parsed.end < parsed.total).then_some(parsed)
}

/// Metadata returned after a completed fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// HTTP-like status code (`200`/`206` for file and memory fetches).
    pub status: u16,
    /// Remaining response body length when known.
    pub content_length: Option<u64>,
    /// Byte interval for a partial response, if present.
    pub content_range: Option<ContentRange>,
    /// Effective URL after redirects.
    pub final_url: String,
}

/// Streaming fetch interface. Returning `false` from `sink` cancels the transfer.
pub trait Fetch {
    /// Fetches `url`, optionally starting at `range_from`, without buffering the whole body.
    fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response>;

    /// Fetches a response while exposing its final headers before any body bytes reach the sink.
    ///
    /// Implementations that cannot guarantee this ordering fail closed.
    fn get_with_response(
        &mut self,
        _url: &str,
        _range_from: u64,
        _on_response: &mut dyn FnMut(&Response) -> bool,
        _sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<Response> {
        Err(Error::Refused(
            "fetcher does not support pre-body response inspection".to_owned(),
        ))
    }

    /// Sends a bounded HTTPS POST and streams its response into `sink`.
    ///
    /// Implementations that do not support network POST return a refusal. The system
    /// implementation accepts only HTTPS, a printable content type, and request bodies
    /// no larger than [`MAX_POST_BODY_BYTES`].
    fn post(
        &mut self,
        _url: &str,
        _content_type: &str,
        _body: &[u8],
        _sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<Response> {
        Err(Error::Refused("POST is unsupported by this fetcher".to_owned()))
    }
}

/// Native HTTPS implementation with bounded body size and timeouts.
#[derive(Clone, Debug)]
pub struct SystemFetch {
    /// Maximum accepted response body bytes.
    pub max_bytes: u64,
    /// Connection timeout.
    pub connect_timeout: Duration,
    /// Limit on the whole transfer, or `None` for no whole-transfer limit. Connect and idle
    /// timeouts still apply when this is `None`.
    pub total_timeout: Option<Duration>,
    /// Maximum time without receiving response data.
    pub idle_timeout: Duration,
}

#[cfg(any(target_os = "windows", test))]
fn read_with_total_timeout<T>(
    started: Instant,
    total_timeout: Option<Duration>,
    idle_timeout: Duration,
    read: impl FnOnce(Duration) -> Result<T>,
) -> Result<T> {
    let Some(total_timeout) = total_timeout else {
        if idle_timeout.is_zero() {
            return Err(Error::System("HTTPS response idle timeout".to_owned()));
        }
        return read(idle_timeout);
    };
    let elapsed = started.elapsed();
    let remaining = total_timeout
        .checked_sub(elapsed)
        .ok_or_else(|| Error::System("HTTPS transfer timed out".to_owned()))?;
    let read_timeout = remaining.min(idle_timeout);
    if read_timeout.is_zero() {
        return Err(Error::System("HTTPS response idle timeout".to_owned()));
    }

    let result = read(read_timeout);
    if started.elapsed() >= total_timeout {
        return Err(Error::System("HTTPS transfer timed out".to_owned()));
    }
    result
}

impl Default for SystemFetch {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            connect_timeout: Duration::from_secs(10),
            total_timeout: Some(Duration::from_secs(60)),
            idle_timeout: Duration::from_secs(15),
        }
    }
}

impl Fetch for SystemFetch {
    fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response> {
        self.get_with_response(url, range_from, &mut |_| true, sink)
    }

    fn get_with_response(
        &mut self,
        url: &str,
        range_from: u64,
        on_response: &mut dyn FnMut(&Response) -> bool,
        sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<Response> {
        if url.starts_with("file://") {
            return FileFetch {
                max_bytes: self.max_bytes,
            }
            .get_with_response(url, range_from, on_response, sink);
        }
        if !url.starts_with("https://") {
            return Err(Error::Refused("only https:// and file:// URLs are allowed".to_owned()));
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            curl::get_with_response(self, url, range_from, on_response, sink)
        }
        #[cfg(target_os = "windows")]
        {
            windows::get_with_response(self, url, range_from, on_response, sink)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = (range_from, on_response, sink);
            Err(Error::System(
                "HTTPS is unsupported on this operating system".to_owned(),
            ))
        }
    }

    fn post(
        &mut self,
        url: &str,
        content_type: &str,
        body: &[u8],
        sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<Response> {
        if !url.starts_with("https://") {
            return Err(Error::Refused("only HTTPS POST URLs are allowed".to_owned()));
        }
        if content_type.is_empty()
            || !content_type.is_ascii()
            || content_type
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b'\r' || byte == b'\n')
        {
            return Err(Error::Refused("invalid POST content type".to_owned()));
        }
        if u64::try_from(body.len()).unwrap_or(u64::MAX) > MAX_POST_BODY_BYTES {
            return Err(Error::Refused("HTTPS POST body exceeds size limit".to_owned()));
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            curl::post(self, url, content_type, body, sink)
        }
        #[cfg(target_os = "windows")]
        {
            windows::post(self, url, content_type, body, sink)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = sink;
            Err(Error::System(
                "HTTPS is unsupported on this operating system".to_owned(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn content_range_parser_accepts_only_well_formed_byte_ranges() {
        assert_eq!(
            parse_content_range("bytes 12-19/40"),
            Some(ContentRange {
                start: 12,
                end: 19,
                total: 40,
            })
        );
        for invalid in [
            "items 12-19/40",
            "bytes 19-12/40",
            "bytes 12-40/40",
            "bytes */40",
            "bytes 12-x/40",
            "bytes 12-19/*",
            "bytes 12-19/40 trailing",
        ] {
            assert_eq!(parse_content_range(invalid), None, "accepted {invalid:?}");
        }
    }

    #[test]
    fn windows_body_read_rejects_eof_returned_after_the_total_deadline() {
        let calls = Cell::new(0usize);
        let result = read_with_total_timeout(
            std::time::Instant::now(),
            Some(Duration::from_millis(25)),
            Duration::from_secs(5),
            |read_timeout| {
                calls.set(calls.get().saturating_add(1));
                assert!(read_timeout <= Duration::from_millis(25));
                std::thread::sleep(Duration::from_millis(40));
                Ok(0usize)
            },
        );

        assert!(matches!(result, Err(Error::System(_))));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn body_read_without_total_limit_is_bounded_only_by_idle_timeout() {
        let calls = Cell::new(0usize);
        let result = read_with_total_timeout(Instant::now(), None, Duration::from_secs(5), |read_timeout| {
            calls.set(calls.get().saturating_add(1));
            assert_eq!(read_timeout, Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(40));
            Ok(7usize)
        });

        assert_eq!(result.ok(), Some(7));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn non_https_system_url_is_refused_before_network_io() {
        let mut fetch = SystemFetch::default();
        assert!(matches!(
            fetch.get("http://example.invalid", 0, &mut |_| true),
            Err(Error::Refused(_))
        ));
    }

    #[test]
    fn post_rejects_http_and_oversized_bodies_before_network_io() {
        let mut fetch = SystemFetch::default();
        assert!(matches!(
            fetch.post("http://example.invalid", "application/gzip", b"body", &mut |_| true),
            Err(Error::Refused(_))
        ));
        let body = vec![0; usize::try_from(MAX_POST_BODY_BYTES).unwrap_or(0).saturating_add(1)];
        assert!(matches!(
            fetch.post("https://example.invalid", "application/gzip", &body, &mut |_| true),
            Err(Error::Refused(_))
        ));
    }
}
