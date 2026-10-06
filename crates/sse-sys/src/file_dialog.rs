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
    use std::process::{Command, Stdio};

    let separator = char::from(0x1f);
    let mut child = Command::new("zenity")
        .arg("--file-selection")
        .arg("--multiple")
        .arg(format!("--separator={separator}"))
        .arg("--title=Открыть сохранение")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| sse_core::Error::System(format!("native file picker unavailable: {error}")))?;
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
        parse_selection_output, parse_selection_status, read_picker_output, MAX_PICKER_OUTPUT_BYTES, MAX_SELECTED_FILES,
    };
    use std::path::PathBuf;

    #[test]
    fn separates_multiple_paths_and_preserves_spaces_and_unicode() {
        let paths = parse_selection_output("/tmp/первый сейв.sav\u{1f}/tmp/second save.sav\n".as_bytes())
            .expect("valid picker output");
        assert_eq!(
            paths,
            [
                PathBuf::from("/tmp/первый сейв.sav"),
                PathBuf::from("/tmp/second save.sav")
            ]
        );
    }

    #[test]
    fn ignores_empty_output_fields_and_rejects_relative_paths() {
        let paths = parse_selection_output(b"/tmp/one.sav\x1f\x1f\n").expect("valid picker output");
        assert_eq!(paths, [PathBuf::from("/tmp/one.sav")]);
        assert!(parse_selection_output(b"relative.sav").is_err());
    }

    #[test]
    fn native_picker_status_distinguishes_cancel_from_failure() {
        assert!(parse_selection_status(0).expect("success status"));
        assert!(!parse_selection_status(1).expect("cancel status"));
        assert!(parse_selection_status(5).is_err());
    }

    #[test]
    fn picker_output_is_bounded_before_it_is_parsed() {
        let bytes = vec![b'x'; MAX_PICKER_OUTPUT_BYTES + 1];
        assert!(read_picker_output(bytes.as_slice()).is_err());
        assert_eq!(
            read_picker_output(b"/tmp/save.sav".as_slice()).expect("small output"),
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
