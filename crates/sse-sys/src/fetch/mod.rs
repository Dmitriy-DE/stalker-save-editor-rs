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
    fn get(
        &mut self,
        url: &str,
        range_from: u64,
        sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<Response>;
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
    fn get(
        &mut self,
        url: &str,
        range_from: u64,
        sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<Response> {
        if url.starts_with("file://") {
            return FileFetch {
                max_bytes: self.max_bytes,
            }
            .get(url, range_from, sink);
        }
        if !url.starts_with("https://") {
            return Err(Error::Refused(
                "only https:// and file:// URLs are allowed".to_owned(),
            ));
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
}
