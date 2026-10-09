//! Native system folder picker adapters.

use sse_core::Result;
use std::path::PathBuf;

/// Opens a native picker for one existing folder.
///
/// # Errors
/// Returns an error when the native picker is unavailable, fails, or returns an invalid path.
pub fn choose_directory() -> Result<Option<PathBuf>> {
    #[cfg(windows)]
    {
        crate::window_win32::choose_directory()
    }
    #[cfg(target_os = "macos")]
    {
        crate::window_macos::choose_directory()
    }
    #[cfg(target_os = "linux")]
    {
        choose_directory_linux()
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err(sse_core::Error::Refused(
            "native folder picker is not available on this platform".to_owned(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn choose_directory_linux() -> Result<Option<PathBuf>> {
    use std::process::Command;

    let mut zenity = Command::new("zenity");
    zenity
        .arg("--file-selection")
        .arg("--directory")
        .arg("--title=Выберите папку с сохранениями");
    let mut kdialog = Command::new("kdialog");
    kdialog
        .arg("--title")
        .arg("Выберите папку с сохранениями")
        .arg("--getexistingdirectory")
        .arg(".");
    open_linux_picker(zenity, kdialog)
}

#[cfg(target_os = "linux")]
fn open_linux_picker(mut zenity: std::process::Command, mut kdialog: std::process::Command) -> Result<Option<PathBuf>> {
    use std::io::ErrorKind;
    use std::process::Stdio;

    zenity.stdout(Stdio::piped()).stderr(Stdio::null());
    kdialog.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = match zenity.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => return open_kdialog_picker(&mut kdialog),
        Err(error) => {
            return Err(sse_core::Error::System(format!(
                "native folder picker unavailable: {error}"
            )));
        }
    };
    finish_linux_picker(&mut child)
}

#[cfg(target_os = "linux")]
fn open_kdialog_picker(kdialog: &mut std::process::Command) -> Result<Option<PathBuf>> {
    use std::io::ErrorKind;

    let mut child = match kdialog.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(sse_core::Error::System(
                "native folder picker unavailable: install zenity or kdialog".to_owned(),
            ));
        }
        Err(error) => {
            return Err(sse_core::Error::System(format!(
                "native folder picker unavailable: {error}"
            )));
        }
    };
    finish_linux_picker(&mut child)
}

#[cfg(target_os = "linux")]
fn finish_linux_picker(child: &mut std::process::Child) -> Result<Option<PathBuf>> {
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(sse_core::Error::System(
            "native folder picker has no output pipe".to_owned(),
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
        .map_err(|error| sse_core::Error::System(format!("native folder picker failed: {error}")))?;
    match status.code() {
        Some(0) => parse_directory_output(&output).map(Some),
        Some(1) => Ok(None),
        code => Err(sse_core::Error::System(format!(
            "native folder picker exited with status {}",
            code.map_or_else(|| "without an exit code".to_owned(), |value| value.to_string())
        ))),
    }
}

#[cfg(target_os = "linux")]
const MAX_PICKER_OUTPUT_BYTES: usize = 1024 * 1024;

#[cfg(target_os = "linux")]
fn read_picker_output(reader: impl std::io::Read) -> Result<Vec<u8>> {
    use std::io::Read;

    let limit = u64::try_from(MAX_PICKER_OUTPUT_BYTES.saturating_add(1)).unwrap_or(u64::MAX);
    let mut output = Vec::new();
    reader
        .take(limit)
        .read_to_end(&mut output)
        .map_err(|error| sse_core::Error::System(format!("native folder picker output failed: {error}")))?;
    if output.len() > MAX_PICKER_OUTPUT_BYTES {
        return Err(sse_core::Error::Refused(
            "native folder picker returned too much data".to_owned(),
        ));
    }
    Ok(output)
}

#[cfg(target_os = "linux")]
fn parse_directory_output(output: &[u8]) -> Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let output = output.strip_suffix(b"\n").unwrap_or(output);
    if output.is_empty() {
        return Err(sse_core::Error::Refused(
            "native folder picker returned an empty path".to_owned(),
        ));
    }
    let path_bytes = if output.starts_with(b"file://") {
        decode_local_file_url(output)?
    } else {
        output.to_vec()
    };
    if path_bytes.contains(&0) {
        return Err(sse_core::Error::Refused(
            "native folder picker returned a path containing a null byte".to_owned(),
        ));
    }
    let path = PathBuf::from(OsString::from_vec(path_bytes));
    if !path.is_absolute() {
        return Err(sse_core::Error::Refused(
            "native folder picker returned a relative path".to_owned(),
        ));
    }
    Ok(path)
}

#[cfg(target_os = "linux")]
fn decode_local_file_url(output: &[u8]) -> Result<Vec<u8>> {
    let encoded_path = output
        .strip_prefix(b"file://")
        .filter(|path| path.starts_with(b"/"))
        .ok_or_else(|| sse_core::Error::Refused("native folder picker returned a non-local URL".to_owned()))?;
    let mut decoded = Vec::with_capacity(encoded_path.len());
    let mut index = 0_usize;
    while index < encoded_path.len() {
        let byte = encoded_path
            .get(index)
            .copied()
            .ok_or_else(|| sse_core::Error::damaged("native folder picker URL offset is out of range"))?;
        if byte == b'%' {
            let high = encoded_path
                .get(index.saturating_add(1))
                .and_then(|value| hex_nibble(*value))
                .ok_or_else(|| sse_core::Error::Refused("native folder picker returned an invalid URL".to_owned()))?;
            let low = encoded_path
                .get(index.saturating_add(2))
                .and_then(|value| hex_nibble(*value))
                .ok_or_else(|| sse_core::Error::Refused("native folder picker returned an invalid URL".to_owned()))?;
            decoded.push((high << 4) | low);
            index = index.saturating_add(3);
        } else {
            decoded.push(byte);
            index = index.saturating_add(1);
        }
    }
    Ok(decoded)
}

#[cfg(target_os = "linux")]
fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => byte.checked_sub(b'0'),
        b'a'..=b'f' => byte.checked_sub(b'a').and_then(|value| value.checked_add(10)),
        b'A'..=b'F' => byte.checked_sub(b'A').and_then(|value| value.checked_add(10)),
        _ => None,
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::path::PathBuf;

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};

            static NEXT: AtomicU64 = AtomicU64::new(1);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("sse-directory-picker-{}-{id}", std::process::id()));
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
    fn directory_picker_falls_back_to_kdialog_when_zenity_is_missing() {
        let temp = TempDirectory::new();
        let kdialog = temp.executable(
            "kdialog",
            "#!/bin/sh\nprintf '%s\\n' 'file:///tmp/Сохранения%20S.T.A.L.K.E.R.'\n",
        );
        let selection = super::open_linux_picker(
            std::process::Command::new(temp.0.join("zenity-not-installed")),
            std::process::Command::new(kdialog),
        )
        .unwrap_or_else(|error| panic!("kdialog fallback failed: {error}"));

        assert_eq!(selection, Some(PathBuf::from("/tmp/Сохранения S.T.A.L.K.E.R.")));
    }

    #[test]
    fn parses_plain_and_file_url_directory_results() {
        assert_eq!(
            super::parse_directory_output("/tmp/Сохранения\n".as_bytes()).unwrap_or_else(|error| panic!("{error:?}")),
            PathBuf::from("/tmp/Сохранения")
        );
        assert_eq!(
            super::parse_directory_output(b"file:///tmp/%D0%A1%D0%B5%D0%B9%D0%B2%D1%8B%20A")
                .unwrap_or_else(|error| panic!("{error:?}")),
            PathBuf::from("/tmp/Сейвы A")
        );
    }

    #[test]
    fn rejects_remote_relative_and_malformed_directory_results() {
        for output in [
            &b"file://host/share"[..],
            &b"relative/folder"[..],
            &b"file:///tmp/%GG"[..],
            &b"file:///tmp/%00"[..],
        ] {
            assert!(super::parse_directory_output(output).is_err());
        }
    }
}
