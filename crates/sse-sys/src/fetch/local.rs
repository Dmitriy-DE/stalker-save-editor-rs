use super::{Fetch, Response, DEFAULT_MAX_BYTES};
use sse_core::{Error, Result};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

/// Local `file://` streaming fetcher, useful for offline sources and tests.
#[derive(Clone, Debug)]
pub struct FileFetch {
    /// Maximum accepted body bytes.
    pub max_bytes: u64,
}

impl Default for FileFetch {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

impl Fetch for FileFetch {
    fn get(
        &mut self,
        url: &str,
        range_from: u64,
        sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<Response> {
        let path = file_path(url)?;
        let mut file = File::open(&path)?;
        let size = file.metadata()?.len();
        if range_from > size {
            return Ok(Response {
                status: 416,
                content_length: Some(0),
                final_url: url.to_owned(),
            });
        }
        let remaining = size.saturating_sub(range_from);
        if remaining > self.max_bytes {
            return Err(Error::Refused(
                "file response exceeds size limit".to_owned(),
            ));
        }
        file.seek(SeekFrom::Start(range_from))?;
        let mut buffer = [0u8; 64 * 1024];
        let mut delivered = 0u64;
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            delivered = delivered
                .checked_add(
                    u64::try_from(read)
                        .map_err(|_| Error::Refused("file chunk too large".to_owned()))?,
                )
                .ok_or_else(|| Error::Refused("file response size overflow".to_owned()))?;
            if delivered > self.max_bytes {
                return Err(Error::Refused(
                    "file response exceeds size limit".to_owned(),
                ));
            }
            if !sink(buffer.get(..read).unwrap_or_default()) {
                return Err(Error::Refused("fetch cancelled by sink".to_owned()));
            }
        }
        Ok(Response {
            status: if range_from == 0 { 200 } else { 206 },
            content_length: Some(remaining),
            final_url: url.to_owned(),
        })
    }
}

/// Deterministic in-memory fetcher for unit tests.
#[derive(Clone, Debug)]
pub struct MemoryFetch {
    entries: BTreeMap<String, Vec<u8>>,
    /// Maximum accepted body bytes.
    pub max_bytes: u64,
}

impl Default for MemoryFetch {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

impl MemoryFetch {
    /// Inserts or replaces a URL body.
    pub fn insert(&mut self, url: impl Into<String>, body: Vec<u8>) {
        self.entries.insert(url.into(), body);
    }
}

impl Fetch for MemoryFetch {
    fn get(
        &mut self,
        url: &str,
        range_from: u64,
        sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<Response> {
        let body = self
            .entries
            .get(url)
            .ok_or_else(|| Error::System("memory URL not found".to_owned()))?;
        let start = usize::try_from(range_from)
            .map_err(|_| Error::Refused("range is too large".to_owned()))?;
        if start > body.len() {
            return Ok(Response {
                status: 416,
                content_length: Some(0),
                final_url: url.to_owned(),
            });
        }
        let data = body.get(start..).unwrap_or_default();
        let length = u64::try_from(data.len())
            .map_err(|_| Error::Refused("memory response too large".to_owned()))?;
        if length > self.max_bytes {
            return Err(Error::Refused(
                "memory response exceeds size limit".to_owned(),
            ));
        }
        for chunk in data.chunks(64 * 1024) {
            if !sink(chunk) {
                return Err(Error::Refused("fetch cancelled by sink".to_owned()));
            }
        }
        Ok(Response {
            status: if range_from == 0 { 200 } else { 206 },
            content_length: Some(length),
            final_url: url.to_owned(),
        })
    }
}

fn file_path(url: &str) -> Result<PathBuf> {
    let raw = url
        .strip_prefix("file://")
        .ok_or_else(|| Error::Refused("not a file URL".to_owned()))?;
    if !raw.starts_with('/') {
        return Err(Error::Refused("file URL must be absolute".to_owned()));
    }
    let mut bytes = Vec::with_capacity(raw.len());
    let source = raw.as_bytes();
    let mut position = 0usize;
    while position < source.len() {
        if source.get(position) == Some(&b'%') {
            let hi = *source
                .get(position.saturating_add(1))
                .ok_or_else(|| Error::Refused("short percent escape".to_owned()))?;
            let lo = *source
                .get(position.saturating_add(2))
                .ok_or_else(|| Error::Refused("short percent escape".to_owned()))?;
            bytes.push(hex(hi)?.wrapping_shl(4) | hex(lo)?);
            position = position.saturating_add(3);
        } else {
            bytes.push(
                *source
                    .get(position)
                    .ok_or_else(|| Error::Refused("bad file URL".to_owned()))?,
            );
            position = position.saturating_add(1);
        }
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::Refused("file URL is not UTF-8".to_owned()))?;
    #[cfg(target_os = "windows")]
    let text = if text.starts_with('/') && text.as_bytes().get(2) == Some(&b':') {
        text.get(1..).unwrap_or_default().to_owned()
    } else {
        text
    };
    Ok(PathBuf::from(text))
}

fn hex(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value.saturating_sub(b'0')),
        b'a'..=b'f' => Ok(value.saturating_sub(b'a').saturating_add(10)),
        b'A'..=b'F' => Ok(value.saturating_sub(b'A').saturating_add(10)),
        _ => Err(Error::Refused("invalid percent escape".to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn memory_fetch_streams_ranges_and_cancels() {
        let mut fetch = MemoryFetch::default();
        fetch.insert("mem://x", b"abcdef".to_vec());
        let mut output = Vec::new();
        let response = fetch
            .get("mem://x", 2, &mut |chunk| {
                output.extend_from_slice(chunk);
                true
            })
            .unwrap_or_else(|error| panic!("memory fetch: {error}"));
        assert_eq!(response.status, 206);
        assert_eq!(response.content_length, Some(4));
        assert_eq!(output, b"cdef");
        assert!(matches!(
            fetch.get("mem://x", 0, &mut |_| false),
            Err(Error::Refused(_))
        ));
    }

    #[test]
    fn file_fetch_is_bounded_and_supports_percent_escapes() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sse fetch {stamp}.bin"));
        fs::write(&path, b"0123456789")
            .unwrap_or_else(|error| panic!("write fixture: {error}"));
        #[cfg(target_os = "windows")]
        let url = format!(
            "file:///{}",
            path.to_string_lossy()
                .replace('\\', "/")
                .replace(' ', "%20")
        );
        #[cfg(not(target_os = "windows"))]
        let url = format!("file://{}", path.to_string_lossy().replace(' ', "%20"));
        let mut fetch = FileFetch { max_bytes: 16 };
        let mut output = Vec::new();
        let response = fetch
            .get(&url, 4, &mut |chunk| {
                output.extend_from_slice(chunk);
                true
            })
            .unwrap_or_else(|error| panic!("file fetch: {error}"));
        let _ = fs::remove_file(path);
        assert_eq!(response.status, 206);
        assert_eq!(response.content_length, Some(6));
        assert_eq!(output, b"456789");
    }
}
