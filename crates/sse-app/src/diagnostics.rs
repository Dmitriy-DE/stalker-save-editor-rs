//! Local support diagnostics and report export.
//!
//! K10 deliberately has no network transport: reports are generated locally for the user to attach manually.

use crate::{paths, AppSettings};
use sse_core::{Error, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Result severity for one local environment check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckStatus {
    /// Check passed.
    Ok,
    /// Check found a non-fatal condition.
    Warn,
    /// Check failed.
    Fail,
}

/// One local environment check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentCheck {
    /// Stable group name.
    pub group: &'static str,
    /// Human-readable check name.
    pub name: &'static str,
    /// Severity.
    pub status: CheckStatus,
    /// Redacted detail suitable for a support report.
    pub detail: String,
    /// Optional user-facing hint.
    pub hint: String,
}

/// Runs dependency-free local diagnostics. Nothing is sent over the network.
#[must_use]
pub fn run() -> Vec<EnvironmentCheck> {
    let mut checks = Vec::with_capacity(5);
    checks.push(EnvironmentCheck {
        group: "Приложение",
        name: "Платформа",
        status: CheckStatus::Ok,
        detail: format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
        hint: String::new(),
    });

    let settings_path = paths::default_settings_path();
    let settings = match fs::read(&settings_path) {
        Ok(bytes) => match AppSettings::from_json_slice(&bytes) {
            Ok(value) => {
                checks.push(EnvironmentCheck {
                    group: "Приложение",
                    name: "settings.json",
                    status: CheckStatus::Ok,
                    detail: "Настройки читаются.".to_owned(),
                    hint: String::new(),
                });
                value
            }
            Err(_) => {
                checks.push(EnvironmentCheck {
                    group: "Приложение",
                    name: "settings.json",
                    status: CheckStatus::Fail,
                    detail: "Файл настроек повреждён.".to_owned(),
                    hint: "Исправьте или переименуйте settings.json; файл не перезаписывается этой проверкой.".to_owned(),
                });
                AppSettings::default()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            checks.push(EnvironmentCheck {
                group: "Приложение",
                name: "settings.json",
                status: CheckStatus::Ok,
                detail: "Файл ещё не создан; используются значения по умолчанию.".to_owned(),
                hint: String::new(),
            });
            AppSettings::default()
        }
        Err(_) => {
            checks.push(EnvironmentCheck {
                group: "Приложение",
                name: "settings.json",
                status: CheckStatus::Warn,
                detail: "Файл настроек сейчас недоступен для чтения.".to_owned(),
                hint: "Проверьте права на каталог данных приложения.".to_owned(),
            });
            AppSettings::default()
        }
    };

    let backup = paths::backup_directory(&settings);
    checks.push(directory_check(
        "Хранилище",
        "Папка резервных копий",
        &backup,
        false,
    ));
    checks.push(directory_check(
        "Хранилище",
        "Каталог данных",
        &paths::default_data_directory(),
        true,
    ));
    checks.push(temp_check());
    checks
}

fn directory_check(group: &'static str, name: &'static str, path: &Path, create: bool) -> EnvironmentCheck {
    let result = if create { fs::create_dir_all(path) } else { Ok(()) };
    if result.is_err() {
        return EnvironmentCheck {
            group,
            name,
            status: CheckStatus::Fail,
            detail: "Каталог недоступен.".to_owned(),
            hint: "Проверьте путь и права доступа.".to_owned(),
        };
    }
    if path.exists() && !path.is_dir() {
        return EnvironmentCheck {
            group,
            name,
            status: CheckStatus::Fail,
            detail: "Вместо каталога найден файл.".to_owned(),
            hint: "Выберите другой каталог.".to_owned(),
        };
    }
    EnvironmentCheck {
        group,
        name,
        status: if path.exists() {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        },
        detail: if path.exists() {
            "Каталог доступен.".to_owned()
        } else {
            "Каталог будет создан при первой записи.".to_owned()
        },
        hint: String::new(),
    }
}

fn temp_check() -> EnvironmentCheck {
    let path = std::env::temp_dir().join(format!("sse-diagnostic-{}.tmp", std::process::id()));
    let result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut file| file.write_all(b"ok").and_then(|()| file.sync_all()));
    let _ = fs::remove_file(&path);
    match result {
        Ok(()) => EnvironmentCheck {
            group: "Система",
            name: "Временные файлы",
            status: CheckStatus::Ok,
            detail: "Запись во временный каталог работает.".to_owned(),
            hint: String::new(),
        },
        Err(_) => EnvironmentCheck {
            group: "Система",
            name: "Временные файлы",
            status: CheckStatus::Fail,
            detail: "Не удалось записать временный файл.".to_owned(),
            hint: "Проверьте свободное место и права временного каталога.".to_owned(),
        },
    }
}

/// Formats local checks for the Settings screen.
#[must_use]
pub fn summary(checks: &[EnvironmentCheck]) -> String {
    let failed = checks.iter().filter(|check| check.status == CheckStatus::Fail).count();
    let warned = checks.iter().filter(|check| check.status == CheckStatus::Warn).count();
    if failed == 0 && warned == 0 {
        "Всё в порядке.".to_owned()
    } else {
        format!("Ошибок: {failed}, предупреждений: {warned}.")
    }
}

/// Builds a redacted plain-text support report.
#[must_use]
pub fn report_text(checks: &[EnvironmentCheck]) -> String {
    let mut text = format!(
        "S.T.A.L.K.E.R. Save Editor {}\nplatform: {} / {}\nnetwork upload: disabled\n\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    for check in checks {
        let status = match check.status {
            CheckStatus::Ok => "OK",
            CheckStatus::Warn => "!",
            CheckStatus::Fail => "X",
        };
        text.push_str(&format!(
            "[{status}] {} · {} — {}\n",
            check.group, check.name, check.detail
        ));
        if !check.hint.is_empty() {
            text.push_str("  hint: ");
            text.push_str(&check.hint);
            text.push('\n');
        }
    }
    redact(&text)
}

/// Redacts user-specific path/name fragments from arbitrary diagnostic text.
#[must_use]
pub fn redact(text: &str) -> String {
    let mut out = text.to_owned();
    for key in ["HOME", "USERPROFILE", "USERNAME", "USER"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                out = out.replace(value, "<redacted>");
            }
        }
    }
    out
}

/// Encodes report text as a deterministic gzip member.
pub fn gzip_report(text: &str) -> Result<Vec<u8>> {
    let input = text.as_bytes();
    let deflated = sse_codecs::deflate::compress_raw(input, sse_codecs::deflate::Level::Fast)?;
    let mut out = Vec::with_capacity(deflated.len().saturating_add(18));
    out.extend_from_slice(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255]);
    out.extend_from_slice(&deflated);
    out.extend_from_slice(&sse_codecs::crc32::crc32(input).to_le_bytes());
    let size = u32::try_from(input.len()).map_err(|_| Error::Refused("diagnostic report exceeds gzip size".to_owned()))?;
    out.extend_from_slice(&size.to_le_bytes());
    Ok(out)
}

/// Writes a local support report under the application data directory and returns its path.
pub fn save_default_report(checks: &[EnvironmentCheck]) -> Result<PathBuf> {
    let directory = paths::default_data_directory();
    fs::create_dir_all(&directory)?;
    let path = directory.join("save-editor-report.txt.gz");
    let bytes = gzip_report(&report_text(checks))?;
    let temp = directory.join(format!(".save-editor-report.{}.tmp", std::process::id()));
    let write = (|| {
        let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, &path)?;
        Ok(())
    })();
    if write.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write.map(|()| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_failures_and_warnings() {
        let checks = [
            EnvironmentCheck {
                group: "g",
                name: "a",
                status: CheckStatus::Fail,
                detail: String::new(),
                hint: String::new(),
            },
            EnvironmentCheck {
                group: "g",
                name: "b",
                status: CheckStatus::Warn,
                detail: String::new(),
                hint: String::new(),
            },
        ];
        assert_eq!(summary(&checks), "Ошибок: 1, предупреждений: 1.");
    }

    #[test]
    fn gzip_has_header_crc_and_input_size() {
        let input = "diagnostic";
        let gzip = gzip_report(input).unwrap_or_default();
        assert_eq!(gzip.get(..3), Some(&[0x1f, 0x8b, 8][..]));
        let tail = gzip.get(gzip.len().saturating_sub(8)..).unwrap_or_default();
        assert_eq!(tail.get(..4), Some(&sse_codecs::crc32::crc32(input.as_bytes()).to_le_bytes()));
        assert_eq!(tail.get(4..), Some(&u32::try_from(input.len()).unwrap_or_default().to_le_bytes()));
    }
}
