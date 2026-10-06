//! Bounded streaming fetches through the operating system.

use sse_core::{Error, Result};
use std::time::Duration;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod curl;
mod local;
#[cfg(target_os = "windows")]
mod windows;

pub use local::{FileFetch, MemoryFetch};

/// Default maximum body size accepted by the system fetcher.
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum request body accepted by the system POST implementation.
pub const MAX_POST_BODY_BYTES: u64 = 2 * 1024 * 1024;

/// Metadata returned after a completed fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// HTTP-like status code (`200`/`206` for file and memory fetches).
    pub status: u16,
    /// Remaining response body length when known.
    pub content_length: Option<u64>,
    /// Effective URL after redirects.
    pub final_url: String,
}

/// Streaming fetch interface. Returning `false` from `sink` cancels the transfer.
pub trait Fetch {
    /// Fetches `url`, optionally starting at `range_from`, without buffering the whole body.
    fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response>;

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
    /// Total transfer timeout.
    pub total_timeout: Duration,
}

impl Default for SystemFetch {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            connect_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(60),
        }
    }
}

impl Fetch for SystemFetch {
    fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response> {
        if url.starts_with("file://") {
            return FileFetch {
                max_bytes: self.max_bytes,
            }
            .get(url, range_from, sink);
        }
        if !url.starts_with("https://") {
            return Err(Error::Refused("only https:// and file:// URLs are allowed".to_owned()));
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            curl::get(self, url, range_from, sink)
        }
        #[cfg(target_os = "windows")]
        {
            windows::get(self, url, range_from, sink)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = (range_from, sink);
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
