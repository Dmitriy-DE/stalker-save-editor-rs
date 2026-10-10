//! Streaming HTTP fetch trait and constant-memory verified downloader.

use crate::manifest::UpdateArtifact;
use sse_core::{Error, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub use sse_codecs::sha256::Sha256 as Sha256Hasher;
pub use sse_sys::fetch::{ContentRange, Fetch, Response};

struct DownloadStream {
    file: File,
    hasher: Sha256Hasher,
    total_downloaded: u64,
    response_body_start: u64,
    expected_response_length: Option<u64>,
    response_headers: Option<Response>,
    response_problem: Option<String>,
    size_exceeded: bool,
    write_error: Option<std::io::Error>,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const LINUX_O_NOFOLLOW: Option<i32> = Some(0o400_000);
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const LINUX_O_NOFOLLOW: Option<i32> = Some(0o100_000);
#[cfg(all(target_os = "linux", not(any(target_arch = "x86_64", target_arch = "aarch64"))))]
const LINUX_O_NOFOLLOW: Option<i32> = None;

fn open_existing_regular_file(path: &Path, writable: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(writable);

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;

        let Some(no_follow_flag) = LINUX_O_NOFOLLOW else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Secure file opening is unavailable on this Linux architecture",
            ));
        };
        options.custom_flags(no_follow_flag);
    }

    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x0000_0100);
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = options;
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Secure file opening is unavailable on this platform",
        ));
    }

    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

fn create_new_part(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).write(true).create_new(true).open(path)
}

fn verify_open_file(file: &mut File, artifact: &UpdateArtifact) -> Result<()> {
    let metadata = file.metadata().map_err(Error::from)?;
    if metadata.len() != artifact.size {
        return Err(Error::damaged("File size mismatch"));
    }

    file.seek(SeekFrom::Start(0)).map_err(Error::from)?;
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

    let actual_hex = hasher.finalize_hex();
    if actual_hex.eq_ignore_ascii_case(&artifact.sha256) {
        Ok(())
    } else {
        Err(Error::damaged("File SHA-256 mismatch"))
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

/// Configuration for the small signed metadata requests (manifest and its signature): the whole
/// transfer is bounded.
fn manifest_fetch_config() -> sse_sys::fetch::SystemFetch {
    sse_sys::fetch::SystemFetch {
        max_bytes: crate::manifest::MAXIMUM_ARTIFACT_BYTES,
        ..sse_sys::fetch::SystemFetch::default()
    }
}

/// Configuration for the artifact download: a large package may take longer than any fixed whole-
/// transfer limit, so only connect and idle timeouts apply.
fn artifact_fetch_config() -> sse_sys::fetch::SystemFetch {
    sse_sys::fetch::SystemFetch {
        max_bytes: crate::manifest::MAXIMUM_ARTIFACT_BYTES,
        total_timeout: None,
        ..sse_sys::fetch::SystemFetch::default()
    }
}

impl Fetch for DefaultFetch {
    fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response> {
        if !official_https_url(url) {
            return Err(Error::Refused(
                "Update fetch is restricted to the official HTTPS host".to_owned(),
            ));
        }
        let mut fetch = manifest_fetch_config();
        let response = sse_sys::fetch::Fetch::get_with_response(&mut fetch, url, range_from, &mut |_| true, sink)?;
        if !official_https_url(&response.final_url) {
            return Err(Error::Refused(
                "Update redirect left the official HTTPS host".to_owned(),
            ));
        }
        Ok(response)
    }

    fn get_with_response(
        &mut self,
        url: &str,
        range_from: u64,
        on_response: &mut dyn FnMut(&Response) -> bool,
        sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<Response> {
        if !official_https_url(url) {
            return Err(Error::Refused(
                "Update fetch is restricted to the official HTTPS host".to_owned(),
            ));
        }
        let mut fetch = artifact_fetch_config();
        let mut checked_response = |response: &Response| {
            if !official_https_url(&response.final_url) {
                return false;
            }
            on_response(response)
        };
        let response =
            sse_sys::fetch::Fetch::get_with_response(&mut fetch, url, range_from, &mut checked_response, sink)?;
        if !official_https_url(&response.final_url) {
            return Err(Error::Refused(
                "Update redirect left the official HTTPS host".to_owned(),
            ));
        }
        Ok(response)
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

    // Reuse only a fully verified destination. A failed replacement must leave it intact.
    if destination.is_file() && verify_existing_file(destination, artifact).is_ok() {
        if let Some(ref mut report) = progress {
            report(artifact.size, artifact.size);
        }
        return Ok(destination.to_path_buf());
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
    let mut hasher = Sha256Hasher::new();
    let mut total_downloaded: u64 = 0;
    let mut existing_part = None;
    match std::fs::symlink_metadata(&part_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Error::Refused("partial download path is a symbolic link".to_owned()));
        }
        Ok(metadata) if metadata.file_type().is_file() && metadata.len() <= artifact.size => {
            let mut partial = open_existing_regular_file(&part_path, true).map_err(Error::from)?;
            if partial.metadata().map_err(Error::from)?.len() > artifact.size {
                return Err(Error::Refused("partial download changed while opening".to_owned()));
            }
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = partial.read(&mut buffer).map_err(Error::from)?;
                if read == 0 {
                    break;
                }
                let bytes = buffer.get(..read).unwrap_or_default();
                total_downloaded = total_downloaded
                    .checked_add(
                        u64::try_from(read).map_err(|_| Error::Refused("partial file is too large".to_owned()))?,
                    )
                    .ok_or_else(|| Error::Refused("partial file size overflow".to_owned()))?;
                if total_downloaded > artifact.size {
                    total_downloaded = 0;
                    hasher = Sha256Hasher::new();
                    break;
                }
                hasher.update(bytes);
            }
            if total_downloaded > artifact.size || partial.metadata().map_err(Error::from)?.len() != total_downloaded {
                return Err(Error::Refused("partial download changed while reading".to_owned()));
            }
            if total_downloaded == artifact.size {
                if hasher.finalize_hex().eq_ignore_ascii_case(&artifact.sha256) {
                    verify_open_file(&mut partial, artifact)?;
                    partial.sync_all().map_err(Error::from)?;
                    drop(partial);
                    verify_existing_file(&part_path, artifact)?;
                    std::fs::rename(&part_path, destination).map_err(Error::from)?;
                    if let Some(ref mut report) = progress {
                        report(artifact.size, artifact.size);
                    }
                    return Ok(destination.to_path_buf());
                }
                total_downloaded = 0;
                hasher = Sha256Hasher::new();
            }
            existing_part = Some(partial);
        }
        Ok(metadata) if metadata.file_type().is_file() => {
            // A partial larger than this artifact cannot be resumed. It is our own temporary file, so drop it
            // and download again instead of failing on every attempt until someone deletes it by hand.
            std::fs::remove_file(&part_path).map_err(Error::from)?;
        }
        Ok(_) => return Err(Error::Refused("partial download path is not a regular file".to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::from(error)),
    }

    let mut out_file = match existing_part {
        Some(file) => file,
        None => create_new_part(&part_path).map_err(Error::from)?,
    };
    if out_file.metadata().map_err(Error::from)?.len() != total_downloaded {
        out_file.set_len(total_downloaded).map_err(Error::from)?;
    }
    out_file.seek(SeekFrom::Start(total_downloaded)).map_err(Error::from)?;

    if let Some(ref mut report) = progress {
        report(total_downloaded, artifact.size);
    }

    let requested_from = total_downloaded;
    let stream = std::cell::RefCell::new(DownloadStream {
        file: out_file,
        hasher,
        total_downloaded,
        response_body_start: requested_from,
        expected_response_length: None,
        response_headers: None,
        response_problem: None,
        size_exceeded: false,
        write_error: None,
    });

    let response = fetch.get_with_response(
        &artifact.url,
        requested_from,
        &mut |headers| {
            let mut stream = stream.borrow_mut();
            let body_length = match headers.status {
                200 => {
                    if headers.content_length.is_some_and(|length| length != artifact.size) {
                        stream.response_problem = Some("HTTP 200 Content-Length mismatch with the artifact".to_owned());
                        return false;
                    }
                    if headers.content_range.is_some() {
                        stream.response_problem = Some("HTTP 200 unexpectedly included Content-Range".to_owned());
                        return false;
                    }
                    artifact.size
                }
                206 => {
                    let Some(range) = headers.content_range else {
                        stream.response_problem = Some("HTTP 206 omitted a valid Content-Range".to_owned());
                        return false;
                    };
                    if range.start != requested_from || range.total != artifact.size {
                        stream.response_problem =
                            Some("HTTP 206 Content-Range does not match the requested artifact".to_owned());
                        return false;
                    }
                    let Some(length) = range.end.checked_sub(range.start).and_then(|n| n.checked_add(1)) else {
                        stream.response_problem = Some("HTTP 206 Content-Range length overflow".to_owned());
                        return false;
                    };
                    if headers.content_length.is_some_and(|reported| reported != length) {
                        stream.response_problem =
                            Some("HTTP 206 Content-Length does not match Content-Range".to_owned());
                        return false;
                    }
                    length
                }
                status => {
                    stream.response_problem = Some(format!("Server returned HTTP {status}"));
                    return false;
                }
            };

            if headers.status == 200 && requested_from != 0 {
                if let Err(error) = stream
                    .file
                    .set_len(0)
                    .and_then(|()| stream.file.seek(SeekFrom::Start(0)).map(|_| ()))
                {
                    stream.write_error = Some(error);
                    return false;
                }
                stream.total_downloaded = 0;
                stream.hasher = Sha256Hasher::new();
            }
            stream.response_body_start = stream.total_downloaded;
            stream.expected_response_length = Some(body_length);
            stream.response_headers = Some(headers.clone());
            true
        },
        &mut |chunk| {
            let mut stream = stream.borrow_mut();
            let chunk_len = match u64::try_from(chunk.len()) {
                Ok(length) => length,
                Err(_) => {
                    stream.size_exceeded = true;
                    return false;
                }
            };
            let Some(next_total) = stream.total_downloaded.checked_add(chunk_len) else {
                stream.size_exceeded = true;
                return false;
            };
            if next_total > artifact.size {
                stream.size_exceeded = true;
                return false;
            }
            if let Err(error) = stream.file.write_all(chunk) {
                let _ = stream.file.set_len(stream.total_downloaded);
                stream.write_error = Some(error);
                return false;
            }
            stream.hasher.update(chunk);
            stream.total_downloaded = next_total;

            if let Some(ref mut report) = progress {
                report(stream.total_downloaded, artifact.size);
            }

            true
        },
    );

    let mut stream = stream.into_inner();
    if let Some(err) = stream.write_error.take() {
        let _ = stream.file.sync_all();
        return Err(Error::from(err));
    }

    if let Some(problem) = stream.response_problem.take() {
        return Err(Error::Refused(problem));
    }

    let resp = match response {
        Ok(response) => response,
        Err(error) => {
            if stream.size_exceeded {
                stream.file.set_len(stream.response_body_start).map_err(Error::from)?;
            }
            stream.file.sync_all().map_err(Error::from)?;
            return Err(error);
        }
    };

    let Some(headers) = stream.response_headers.take() else {
        return Err(Error::damaged("fetcher returned without exposing response headers"));
    };
    if resp.status != headers.status || resp.content_range != headers.content_range {
        return Err(Error::damaged("fetcher response metadata changed after body delivery"));
    }
    if stream.size_exceeded {
        stream.file.set_len(stream.response_body_start).map_err(Error::from)?;
        stream.file.sync_all().map_err(Error::from)?;
        return Err(Error::damaged("Download exceeded the manifest size"));
    }

    let received_this_response = stream.total_downloaded.saturating_sub(stream.response_body_start);
    if stream
        .expected_response_length
        .is_some_and(|expected| expected != received_this_response)
    {
        stream.file.sync_all().map_err(Error::from)?;
        return Err(Error::damaged(format!(
            "Response size mismatch: expected {} bytes, got {} bytes",
            stream.expected_response_length.unwrap_or_default(),
            received_this_response
        )));
    }

    if stream.total_downloaded != artifact.size {
        stream.file.sync_all().map_err(Error::from)?;
        return Err(Error::damaged(format!(
            "Download size mismatch: expected {} bytes, got {} bytes",
            artifact.size, stream.total_downloaded
        )));
    }

    let DownloadStream {
        file: mut out_file,
        hasher,
        ..
    } = stream;
    let actual_sha256 = hasher.finalize_hex();
    if !actual_sha256.eq_ignore_ascii_case(&artifact.sha256) {
        out_file.sync_all().map_err(Error::from)?;
        return Err(Error::damaged(format!(
            "Download SHA-256 mismatch: expected {}, got {}",
            artifact.sha256, actual_sha256
        )));
    }

    out_file.sync_all().map_err(Error::from)?;
    verify_open_file(&mut out_file, artifact)?;
    drop(out_file);
    verify_existing_file(&part_path, artifact)?;
    std::fs::rename(&part_path, destination).map_err(Error::from)?;

    Ok(destination.to_path_buf())
}

/// Verifies an existing file on disk against an artifact size and SHA-256 digest.
pub fn verify_existing_file(path: &Path, artifact: &UpdateArtifact) -> Result<()> {
    let mut file = open_existing_regular_file(path, false).map_err(Error::from)?;
    verify_open_file(&mut file, artifact)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_fetch_has_no_whole_transfer_limit_but_keeps_connect_and_idle_limits() {
        let artifact = artifact_fetch_config();
        let defaults = sse_sys::fetch::SystemFetch::default();

        assert_eq!(artifact.total_timeout, None);
        assert_eq!(artifact.connect_timeout, defaults.connect_timeout);
        assert_eq!(artifact.idle_timeout, defaults.idle_timeout);
    }

    #[test]
    fn manifest_fetch_keeps_the_whole_transfer_limit() {
        let manifest = manifest_fetch_config();

        assert_eq!(manifest.total_timeout, Some(std::time::Duration::from_secs(60)));
        assert_eq!(manifest.max_bytes, crate::manifest::MAXIMUM_ARTIFACT_BYTES);
    }

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
