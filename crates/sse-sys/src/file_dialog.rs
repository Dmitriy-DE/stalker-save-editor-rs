//! Native file selection backed by the operating system's desktop picker.

use sse_core::Result;
use std::path::PathBuf;

/// Opens a native multi-file picker.
///
/// A cancelled picker returns `Ok(None)`. Returned paths are absolute and capped at 512 entries.
///
/// # Errors
/// Returns an error when the platform picker is unavailable or fails.
pub fn open_files() -> Result<Option<Vec<PathBuf>>> {
    #[cfg(windows)]
    {
        crate::window_win32::open_files()
    }
    #[cfg(target_os = "macos")]
    {
        crate::window_macos::open_files()
    }
    #[cfg(target_os = "linux")]
    {
        open_files_linux()
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err(sse_core::Error::Refused(
            "native file picker is not available on this platform".to_owned(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn open_files_linux() -> Result<Option<Vec<PathBuf>>> {
    use std::process::Command;

    let separator = char::from(0x1f);
    let mut zenity = Command::new("zenity");
    zenity
        .arg("--file-selection")
        .arg("--multiple")
        .arg(format!("--separator={separator}"))
        .arg("--title=Открыть сохранение");
    let mut kdialog = Command::new("kdialog");
    kdialog
        .arg("--title")
        .arg("Открыть сохранение")
        .arg("--multiple")
        .arg("--separate-output")
        .arg("--getopenurl")
        .arg(".");
    open_linux_picker(zenity, kdialog)
}

#[cfg(target_os = "linux")]
fn open_linux_picker(
    mut zenity: std::process::Command,
    mut kdialog: std::process::Command,
) -> Result<Option<Vec<PathBuf>>> {
    use std::io::ErrorKind;
    use std::process::Stdio;

    zenity.stdout(Stdio::piped()).stderr(Stdio::null());
    kdialog.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = match zenity.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return open_kdialog_picker(&mut kdialog);
        }
        Err(error) => {
            return Err(sse_core::Error::System(format!(
                "native file picker unavailable: {error}"
            )));
        }
    };
    finish_linux_picker(&mut child)
}

#[cfg(target_os = "linux")]
fn open_kdialog_picker(kdialog: &mut std::process::Command) -> Result<Option<Vec<PathBuf>>> {
    use std::io::ErrorKind;

    let mut child = match kdialog.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(sse_core::Error::System(
                "native file picker unavailable: install zenity or kdialog".to_owned(),
            ));
        }
        Err(error) => {
            return Err(sse_core::Error::System(format!(
                "native file picker unavailable: {error}"
            )))
        }
    };
    finish_linux_picker(&mut child)
}

#[cfg(target_os = "linux")]
fn finish_linux_picker(child: &mut std::process::Child) -> Result<Option<Vec<PathBuf>>> {
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(sse_core::Error::System(
            "native file picker has no output pipe".to_owned(),
        ));
    };
    let output = match read_picker_output(stdout) {
        Ok(output) => output,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let status = child
        .wait()
        .map_err(|error| sse_core::Error::System(format!("native file picker failed: {error}")))?;
    if !parse_selection_status(status.code().unwrap_or(-1))? {
        return Ok(None);
    }
    parse_selection_output(&output).map(Some)
}

#[cfg(target_os = "linux")]
const MAX_PICKER_OUTPUT_BYTES: usize = 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_SELECTED_FILES: usize = 512;

#[cfg(target_os = "linux")]
fn read_picker_output(reader: impl std::io::Read) -> Result<Vec<u8>> {
    use std::io::Read;

    let limit = u64::try_from(MAX_PICKER_OUTPUT_BYTES.saturating_add(1)).unwrap_or(u64::MAX);
    let mut output = Vec::new();
    reader
        .take(limit)
        .read_to_end(&mut output)
        .map_err(|error| sse_core::Error::System(format!("native file picker output failed: {error}")))?;
    if output.len() > MAX_PICKER_OUTPUT_BYTES {
        return Err(sse_core::Error::Refused(
            "native file picker returned too much data".to_owned(),
        ));
    }
    Ok(output)
}

#[cfg(target_os = "linux")]
fn parse_selection_output(output: &[u8]) -> Result<Vec<PathBuf>> {
    if output.starts_with(b"file://") {
        return parse_file_url_selection_output(output);
    }

    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let output = output.strip_suffix(b"\n").unwrap_or(output);
    let mut paths = Vec::new();
    let mut start = 0_usize;
    for index in 0..=output.len() {
        if output.get(index).copied() == Some(0x1f) || index == output.len() {
            let Some(bytes) = output.get(start..index) else {
                return Err(sse_core::Error::damaged(
                    "native file picker returned an invalid path list",
                ));
            };
            if !bytes.is_empty() {
                if paths.len() >= MAX_SELECTED_FILES {
                    return Err(sse_core::Error::Refused(
                        "native file picker selected more than 512 files".to_owned(),
                    ));
                }
                let path = PathBuf::from(OsString::from_vec(bytes.to_vec()));
                if !path.is_absolute() {
                    return Err(sse_core::Error::Refused(
                        "native file picker returned a relative path".to_owned(),
                    ));
                }
                paths.push(path);
            }
            start = index.saturating_add(1);
        }
    }
    Ok(paths)
}

#[cfg(target_os = "linux")]
fn parse_file_url_selection_output(output: &[u8]) -> Result<Vec<PathBuf>> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let output = output.strip_suffix(b"\n").unwrap_or(output);
    let mut paths = Vec::new();
    for line in output.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
        if paths.len() >= MAX_SELECTED_FILES {
            return Err(sse_core::Error::Refused(
                "native file picker selected more than 512 files".to_owned(),
            ));
        }
        let encoded_path = line
            .strip_prefix(b"file://")
            .filter(|path| path.starts_with(b"/"))
            .ok_or_else(|| sse_core::Error::Refused("native file picker returned a non-local file URL".to_owned()))?;
        let mut path_bytes = Vec::with_capacity(encoded_path.len());
        let mut index = 0_usize;
        while index < encoded_path.len() {
            let byte = encoded_path
                .get(index)
                .copied()
                .ok_or_else(|| sse_core::Error::damaged("native file picker URL offset is out of range"))?;
            if byte == b'%' {
                let high = encoded_path
                    .get(index.saturating_add(1))
                    .and_then(|value| crate::hex_util::hex_nibble(*value))
                    .ok_or_else(|| {
                        sse_core::Error::Refused("native file picker returned an invalid file URL".to_owned())
                    })?;
                let low = encoded_path
                    .get(index.saturating_add(2))
                    .and_then(|value| crate::hex_util::hex_nibble(*value))
                    .ok_or_else(|| {
                        sse_core::Error::Refused("native file picker returned an invalid file URL".to_owned())
                    })?;
                path_bytes.push((high << 4) | low);
                index = index.saturating_add(3);
            } else {
                path_bytes.push(byte);
                index = index.saturating_add(1);
            }
        }
        if path_bytes.contains(&0) {
            return Err(sse_core::Error::Refused(
                "native file picker returned a path containing a null byte".to_owned(),
            ));
        }
        let path = PathBuf::from(OsString::from_vec(path_bytes));
        if !path.is_absolute() {
            return Err(sse_core::Error::Refused(
                "native file picker returned a relative path".to_owned(),
            ));
        }
        paths.push(path);
    }
    Ok(paths)
}

#[cfg(target_os = "linux")]
fn parse_selection_status(code: i32) -> Result<bool> {
    match code {
        0 => Ok(true),
        1 => Ok(false),
        other => Err(sse_core::Error::System(format!(
            "native file picker exited with status {other}"
        ))),
    }
}

#[cfg(target_os = "linux")]
#[cfg(test)]
mod tests {
    use super::{
        open_linux_picker, parse_selection_output, parse_selection_status, read_picker_output, MAX_PICKER_OUTPUT_BYTES,
        MAX_SELECTED_FILES,
    };
    use std::path::PathBuf;

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};

            static NEXT: AtomicU64 = AtomicU64::new(1);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("sse-file-picker-{}-{id}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("create picker test directory: {error}"));
            Self(path)
        }

        fn executable(&self, name: &str, body: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;

            let path = self.0.join(name);
            std::fs::write(&path, body).unwrap_or_else(|error| panic!("write picker test command: {error}"));
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .unwrap_or_else(|error| panic!("make picker test command executable: {error}"));
            path
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn native_picker_falls_back_to_kdialog_when_zenity_is_missing() {
        let temp = TempDirectory::new();
        let kdialog = temp.executable(
            "kdialog",
            "#!/bin/sh\nprintf '%s\\n' 'file:///tmp/selected%20save.sav'\n",
        );
        let missing_zenity = temp.0.join("zenity-not-installed");

        let selection = open_linux_picker(
            std::process::Command::new(missing_zenity),
            std::process::Command::new(kdialog),
        )
        .unwrap_or_else(|error| panic!("kdialog fallback failed: {error}"));

        assert_eq!(selection, Some(vec![PathBuf::from("/tmp/selected save.sav")]));
    }

    #[test]
    fn native_picker_reports_both_missing_linux_dialogs() {
        let temp = TempDirectory::new();
        assert!(open_linux_picker(
            std::process::Command::new(temp.0.join("zenity-not-installed")),
            std::process::Command::new(temp.0.join("kdialog-not-installed")),
        )
        .is_err_and(|error| error.to_string().contains("install zenity or kdialog")));
    }

    #[test]
    fn separates_multiple_paths_and_preserves_spaces_and_unicode() {
        let paths = parse_selection_output("/tmp/первый сейв.sav\u{1f}/tmp/second save.sav\n".as_bytes())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            paths,
            [
                PathBuf::from("/tmp/первый сейв.sav"),
                PathBuf::from("/tmp/second save.sav")
            ]
        );
    }

    #[test]
    fn parses_multiple_kdialog_file_urls_with_spaces_and_unicode() {
        let output = b"file:///tmp/%D0%BF%D0%B5%D1%80%D0%B2%D1%8B%D0%B9%20save.sav\nfile:///tmp/second%20save.sav\n";
        let paths = parse_selection_output(output).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            paths,
            [
                PathBuf::from("/tmp/первый save.sav"),
                PathBuf::from("/tmp/second save.sav")
            ]
        );
    }

    #[test]
    fn rejects_remote_malformed_and_nul_kdialog_file_urls() {
        for output in [
            &b"file://remote/share/save.sav"[..],
            &b"file:///tmp/%GG.sav"[..],
            &b"file:///tmp/%00.sav"[..],
        ] {
            assert!(parse_selection_output(output).is_err());
        }
    }

    #[test]
    fn ignores_empty_output_fields_and_rejects_relative_paths() {
        let paths = parse_selection_output(b"/tmp/one.sav\x1f\x1f\n").unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(paths, [PathBuf::from("/tmp/one.sav")]);
        assert!(parse_selection_output(b"relative.sav").is_err());
    }

    #[test]
    fn native_picker_status_distinguishes_cancel_from_failure() {
        assert!(matches!(parse_selection_status(0), Ok(true)));
        assert!(matches!(parse_selection_status(1), Ok(false)));
        assert!(parse_selection_status(5).is_err());
    }

    #[test]
    fn picker_output_is_bounded_before_it_is_parsed() {
        let bytes = vec![b'x'; MAX_PICKER_OUTPUT_BYTES + 1];
        assert!(read_picker_output(bytes.as_slice()).is_err());
        assert_eq!(
            read_picker_output(b"/tmp/save.sav".as_slice()).unwrap_or_else(|error| panic!("{error:?}")),
            b"/tmp/save.sav"
        );
    }

    #[test]
    fn picker_refuses_more_than_512_paths() {
        let paths = (0..MAX_SELECTED_FILES + 1)
            .map(|index| format!("/tmp/save-{index}.sav"))
            .collect::<Vec<_>>()
            .join("\u{1f}");
        assert!(parse_selection_output(paths.as_bytes()).is_err());
    }
}
