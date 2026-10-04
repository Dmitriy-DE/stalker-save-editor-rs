//! Streaming HTTP fetch trait and constant-memory verified downloader.

use crate::manifest::UpdateArtifact;
use sse_core::{Error, Result};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// HTTP response status and headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// HTTP status code (200, 206, 301, etc.).
    pub status_code: u16,
    /// Content length in bytes, if reported.
    pub content_length: Option<u64>,
    /// Location header for HTTP redirects.
    pub location: Option<String>,
}

/// Abstract fetch interface for OS-level or mock streaming HTTP downloads.
pub trait Fetch {
    /// Performs an HTTP GET request, streaming response payload chunks into `sink`.
    ///
    /// If `sink` returns `false`, reading is cancelled early.
    ///
    /// # Errors
    /// Returns an error on transport failure or invalid URL.
    fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response>;
}

/// In-memory mock fetcher for tests and unit verification.
#[derive(Clone, Debug, Default)]
pub struct MemoryFetch {
    routes: HashMap<String, Vec<u8>>,
    redirects: HashMap<String, String>,
}

impl MemoryFetch {
    /// Creates an empty in-memory fetcher.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a response payload for a specific URL.
    pub fn register(&mut self, url: impl Into<String>, payload: Vec<u8>) {
        self.routes.insert(url.into(), payload);
    }

    /// Registers a redirect from one URL to another.
    pub fn register_redirect(&mut self, from_url: impl Into<String>, to_url: impl Into<String>) {
        self.redirects.insert(from_url.into(), to_url.into());
    }
}

impl Fetch for MemoryFetch {
    fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response> {
        if let Some(target) = self.redirects.get(url) {
            return Ok(Response {
                status_code: 302,
                content_length: None,
                location: Some(target.clone()),
            });
        }

        let body = self
            .routes
            .get(url)
            .ok_or_else(|| Error::Refused(format!("404 Not Found: {url}")))?;

        let start = usize::try_from(range_from).unwrap_or(body.len());
        let slice = body.get(start..).unwrap_or(&[]);

        // Stream in 64 KiB chunks
        let chunk_size = 64 * 1024;
        let mut offset = 0_usize;
        while offset < slice.len() {
            let end = offset.saturating_add(chunk_size).min(slice.len());
            if let Some(chunk) = slice.get(offset..end) {
                if !sink(chunk) {
                    break;
                }
            }
            offset = end;
        }

        Ok(Response {
            status_code: 200,
            content_length: Some(slice.len() as u64),
            location: None,
        })
    }
}

/// Local file-backed fetcher streaming `file://` URLs.
#[derive(Clone, Debug, Default)]
pub struct FileFetch;

impl Fetch for FileFetch {
    fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response> {
        let path_str = url.strip_prefix("file://").unwrap_or(url);
        let path = Path::new(path_str);
        let mut file = File::open(path).map_err(Error::from)?;

        if range_from > 0 {
            use std::io::Seek;
            file.seek(std::io::SeekFrom::Start(range_from)).map_err(Error::from)?;
        }

        let metadata = file.metadata().map_err(Error::from)?;
        let file_len = metadata.len();
        let remaining = file_len.saturating_sub(range_from);

        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(Error::from)?;
            if read == 0 {
                break;
            }
            if let Some(chunk) = buffer.get(..read) {
                if !sink(chunk) {
                    break;
                }
            }
        }

        Ok(Response {
            status_code: 200,
            content_length: Some(remaining),
            location: None,
        })
    }
}

/// Live update fetcher restricted to the official HTTPS update origin.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultFetch;

const UPDATE_HOST: &str = "save-editor-downloads.save-editor.workers.dev";

fn official_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let Some(authority) = rest.get(..authority_end) else {
        return false;
    };
    if authority.contains('@') {
        return false;
    }
    authority.eq_ignore_ascii_case(UPDATE_HOST) || authority.eq_ignore_ascii_case(&format!("{UPDATE_HOST}:443"))
}

impl Fetch for DefaultFetch {
    fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response> {
        if !official_https_url(url) {
            return Err(Error::Refused(
                "Update fetch is restricted to the official HTTPS host".to_owned(),
            ));
        }
        let mut fetch = sse_sys::fetch::SystemFetch {
            max_bytes: crate::manifest::MAXIMUM_ARTIFACT_BYTES,
            ..sse_sys::fetch::SystemFetch::default()
        };
        let response = sse_sys::fetch::Fetch::get(&mut fetch, url, range_from, sink)?;
        if !official_https_url(&response.final_url) {
            return Err(Error::Refused(
                "Update redirect left the official HTTPS host".to_owned(),
            ));
        }
        Ok(Response {
            status_code: response.status,
            content_length: response.content_length,
            location: None,
        })
    }
}

/// Constant-memory streaming SHA-256 state machine.
pub struct Sha256Hasher {
    state: [u32; 8],
    buffer: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Default for Sha256Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256Hasher {
    /// Creates an initialized SHA-256 hasher.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buffer: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    /// Feeds incoming payload bytes to the running digest.
    pub fn update(&mut self, data: &[u8]) {
        self.total_len = self.total_len.saturating_add(data.len() as u64);
        let mut offset = 0_usize;
        while offset < data.len() {
            let space = 64_usize.saturating_sub(self.buf_len);
            let available = data.len().saturating_sub(offset);
            let to_copy = space.min(available);

            if let (Some(dest), Some(src)) = (
                self.buffer.get_mut(self.buf_len..self.buf_len.saturating_add(to_copy)),
                data.get(offset..offset.saturating_add(to_copy)),
            ) {
                dest.copy_from_slice(src);
                self.buf_len = self.buf_len.saturating_add(to_copy);
                offset = offset.saturating_add(to_copy);
            } else {
                break;
            }

            if self.buf_len == 64 {
                process_block(&self.buffer, &mut self.state);
                self.buf_len = 0;
            }
        }
    }

    /// Finalizes the digest calculation and produces the 32-byte hash.
    #[must_use]
    pub fn finish(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);
        if let Some(slot) = self.buffer.get_mut(self.buf_len) {
            *slot = 0x80;
        }
        self.buf_len = self.buf_len.wrapping_add(1);

        if self.buf_len > 56 {
            while self.buf_len < 64 {
                if let Some(slot) = self.buffer.get_mut(self.buf_len) {
                    *slot = 0;
                }
                self.buf_len = self.buf_len.wrapping_add(1);
            }
            process_block(&self.buffer, &mut self.state);
            self.buf_len = 0;
        }

        while self.buf_len < 56 {
            if let Some(slot) = self.buffer.get_mut(self.buf_len) {
                *slot = 0;
            }
            self.buf_len = self.buf_len.wrapping_add(1);
        }

        let len_bytes = bit_len.to_be_bytes();
        for (i, &b) in len_bytes.iter().enumerate() {
            if let Some(slot) = self.buffer.get_mut(56_usize.wrapping_add(i)) {
                *slot = b;
            }
        }
        process_block(&self.buffer, &mut self.state);

        let mut out = [0u8; 32];
        for (i, &word) in self.state.iter().enumerate() {
            let bytes = word.to_be_bytes();
            let base = i.wrapping_mul(4);
            for (j, &b) in bytes.iter().enumerate() {
                if let Some(slot) = out.get_mut(base.wrapping_add(j)) {
                    *slot = b;
                }
            }
        }
        out
    }
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
    0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
    0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
    0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

fn process_block(block: &[u8; 64], state: &mut [u32; 8]) {
    let mut w = [0u32; 64];
    for t in 0..16usize {
        let base = t.wrapping_mul(4);
        let b0 = block.get(base).copied().unwrap_or(0);
        let b1 = block.get(base.wrapping_add(1)).copied().unwrap_or(0);
        let b2 = block.get(base.wrapping_add(2)).copied().unwrap_or(0);
        let b3 = block.get(base.wrapping_add(3)).copied().unwrap_or(0);
        if let Some(slot) = w.get_mut(t) {
            *slot = u32::from_be_bytes([b0, b1, b2, b3]);
        }
    }
    for t in 16..64usize {
        let w15 = w.get(t.wrapping_sub(15)).copied().unwrap_or(0);
        let s0 = w15.rotate_right(7) ^ w15.rotate_right(18) ^ (w15 >> 3);
        let w2 = w.get(t.wrapping_sub(2)).copied().unwrap_or(0);
        let s1 = w2.rotate_right(17) ^ w2.rotate_right(19) ^ (w2 >> 10);
        let w16 = w.get(t.wrapping_sub(16)).copied().unwrap_or(0);
        let w7 = w.get(t.wrapping_sub(7)).copied().unwrap_or(0);
        if let Some(slot) = w.get_mut(t) {
            *slot = w16.wrapping_add(s0).wrapping_add(w7).wrapping_add(s1);
        }
    }

    let mut a = state.first().copied().unwrap_or(0);
    let mut b = state.get(1).copied().unwrap_or(0);
    let mut c = state.get(2).copied().unwrap_or(0);
    let mut d = state.get(3).copied().unwrap_or(0);
    let mut e = state.get(4).copied().unwrap_or(0);
    let mut f = state.get(5).copied().unwrap_or(0);
    let mut g = state.get(6).copied().unwrap_or(0);
    let mut h = state.get(7).copied().unwrap_or(0);

    for (t, &kt) in K.iter().enumerate() {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let wt = w.get(t).copied().unwrap_or(0);
        let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(kt).wrapping_add(wt);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    if let Some(s) = state.get_mut(0) {
        *s = s.wrapping_add(a);
    }
    if let Some(s) = state.get_mut(1) {
        *s = s.wrapping_add(b);
    }
    if let Some(s) = state.get_mut(2) {
        *s = s.wrapping_add(c);
    }
    if let Some(s) = state.get_mut(3) {
        *s = s.wrapping_add(d);
    }
    if let Some(s) = state.get_mut(4) {
        *s = s.wrapping_add(e);
    }
    if let Some(s) = state.get_mut(5) {
        *s = s.wrapping_add(f);
    }
    if let Some(s) = state.get_mut(6) {
        *s = s.wrapping_add(g);
    }
    if let Some(s) = state.get_mut(7) {
        *s = s.wrapping_add(h);
    }
}

/// Downloads and verifies an update artifact in a single streaming pass using constant memory.
///
/// # Errors
/// Returns an error on network failure, size mismatch, or SHA-256 mismatch.
pub fn download_artifact(
    fetch: &mut dyn Fetch,
    artifact: &UpdateArtifact,
    destination: &Path,
    mut progress: Option<&mut dyn FnMut(u64, u64)>,
) -> Result<PathBuf> {
    artifact.validate()?;

    // 1. Check if destination already has a fully valid file (re-use verified download)
    if destination.is_file() {
        if verify_existing_file(destination, artifact).is_ok() {
            if let Some(ref mut report) = progress {
                report(artifact.size, artifact.size);
            }
            return Ok(destination.to_path_buf());
        }
        let _ = std::fs::remove_file(destination);
    }

    let parent = destination
        .parent()
        .ok_or_else(|| Error::Refused("Destination has no parent directory".to_string()))?;
    std::fs::create_dir_all(parent).map_err(Error::from)?;

    let file_name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::damaged("Invalid destination filename"))?;

    let part_path = parent.join(format!(".{file_name}.part"));
    if part_path.exists() {
        let _ = std::fs::remove_file(&part_path);
    }

    let mut out_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part_path)
        .map_err(Error::from)?;

    let mut hasher = Sha256Hasher::new();
    let mut total_downloaded: u64 = 0;
    let mut size_exceeded = false;
    let mut write_err: Option<std::io::Error> = None;

    let response = fetch.get(&artifact.url, 0, &mut |chunk| {
        total_downloaded = total_downloaded.saturating_add(chunk.len() as u64);
        if total_downloaded > artifact.size {
            size_exceeded = true;
            return false;
        }

        hasher.update(chunk);

        if let Err(e) = out_file.write_all(chunk) {
            write_err = Some(e);
            return false;
        }

        if let Some(ref mut report) = progress {
            report(total_downloaded, artifact.size);
        }

        true
    });

    // Ensure part file is cleaned up on any failure
    let cleanup = |path: &Path| {
        let _ = std::fs::remove_file(path);
    };

    if let Some(err) = write_err {
        cleanup(&part_path);
        return Err(Error::from(err));
    }

    let resp = match response {
        Ok(r) => r,
        Err(e) => {
            cleanup(&part_path);
            return Err(e);
        }
    };

    if resp.status_code != 200 && resp.status_code != 206 {
        cleanup(&part_path);
        return Err(Error::Refused(format!("Server returned HTTP {}", resp.status_code)));
    }

    if size_exceeded || total_downloaded != artifact.size {
        cleanup(&part_path);
        return Err(Error::damaged(format!(
            "Download size mismatch: expected {} bytes, got {} bytes",
            artifact.size, total_downloaded
        )));
    }

    let digest_bytes = hasher.finish();
    let actual_sha256 = hex_encode(&digest_bytes);
    if !actual_sha256.eq_ignore_ascii_case(&artifact.sha256) {
        cleanup(&part_path);
        return Err(Error::damaged(format!(
            "Download SHA-256 mismatch: expected {}, got {}",
            artifact.sha256, actual_sha256
        )));
    }

    out_file.flush().map_err(Error::from)?;
    drop(out_file);

    std::fs::rename(&part_path, destination).map_err(Error::from)?;

    Ok(destination.to_path_buf())
}

/// Verifies an existing file on disk against an artifact size and SHA-256 digest.
pub fn verify_existing_file(path: &Path, artifact: &UpdateArtifact) -> Result<()> {
    let mut file = File::open(path).map_err(Error::from)?;
    let metadata = file.metadata().map_err(Error::from)?;
    if metadata.len() != artifact.size {
        return Err(Error::damaged("File size mismatch"));
    }

    let mut hasher = Sha256Hasher::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf).map_err(Error::from)?;
        if read == 0 {
            break;
        }
        if let Some(chunk) = buf.get(..read) {
            hasher.update(chunk);
        }
    }

    let digest = hasher.finish();
    let actual_hex = hex_encode(&digest);
    if actual_hex.eq_ignore_ascii_case(&artifact.sha256) {
        Ok(())
    } else {
        Err(Error::damaged("File SHA-256 mismatch"))
    }
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len().saturating_mul(2));
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fetch_origin_filter_rejects_non_https_and_foreign_hosts() {
        assert!(official_https_url(
            "https://save-editor-downloads.save-editor.workers.dev/latest.json"
        ));
        assert!(official_https_url(
            "https://save-editor-downloads.save-editor.workers.dev:443/latest.json"
        ));
        assert!(!official_https_url(
            "http://save-editor-downloads.save-editor.workers.dev/latest.json"
        ));
        assert!(!official_https_url("file:///tmp/update"));
        assert!(!official_https_url(
            "https://save-editor-downloads.save-editor.workers.dev.evil.test/update"
        ));
        assert!(!official_https_url(
            "https://save-editor-downloads.save-editor.workers.dev@evil.test/update"
        ));
    }

    #[test]
    fn sha256_hasher_matches_sha256_function() {
        let test_cases: &[&[u8]] = &[
            b"",
            b"a",
            b"abc",
            b"message digest",
            b"abcdefghijklmnopqrstuvwxyz0123456789",
            &[42u8; 63],
            &[42u8; 64],
            &[42u8; 65],
            &[42u8; 1000],
            &[42u8; 65536],
        ];

        for &case in test_cases {
            let expected = sse_codecs::sha256::sha256(case);
            let mut hasher = Sha256Hasher::new();
            // Feed in varying chunk sizes (1 byte, 17 bytes, 64 bytes)
            let mut offset = 0usize;
            let chunk_sizes = [1, 7, 16, 64, 128];
            let mut chunk_idx = 0usize;
            while offset < case.len() {
                let sz = chunk_sizes
                    .get(chunk_idx.checked_rem(chunk_sizes.len()).unwrap_or_default())
                    .copied()
                    .unwrap_or(64);
                let end = offset.saturating_add(sz).min(case.len());
                if let Some(slice) = case.get(offset..end) {
                    hasher.update(slice);
                }
                offset = end;
                chunk_idx = chunk_idx.saturating_add(1);
            }
            let actual = hasher.finish();
            assert_eq!(actual, expected, "Mismatch on case of length {}", case.len());
        }
    }
}
