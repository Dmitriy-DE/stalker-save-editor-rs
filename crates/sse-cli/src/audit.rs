//! Read-only release audit of save paths supplied in a local path-list file.

use sse_codecs::json::Writer as JsonWriter;
use sse_core::{Error, Result};
use sse_s2::{S2Change, S2Save};
use sse_xray::writer::{self, Change, ChangeSet};
use sse_xray::Save;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Default)]
struct FileAudit {
    path: String,
    format: Option<String>,
    info_ok: bool,
    inventory_count: Option<usize>,
    money_edit: String,
    stack_edit: String,
    elapsed: Duration,
    errors: Vec<String>,
}

struct AuditReport {
    files: Vec<FileAudit>,
    elapsed: Duration,
    peak_rss_bytes: Option<u64>,
}

/// Audits the list file named on the CLI and prints a JSON report to stdout.
///
/// The file contains one save path per line. Saves are read-only: edits are encoded, re-read, and checked
/// in memory, with no output or backup files created.
pub fn run(path_list: Option<&str>) -> Result<()> {
    let path_list = path_list.ok_or_else(|| Error::damaged("missing audit path-list file"))?;
    let content = fs::read_to_string(path_list)?;
    let paths = parse_path_list(&content);
    let started = Instant::now();
    let files = paths.iter().map(|path| audit_path(Path::new(path))).collect::<Vec<_>>();
    let report = AuditReport {
        files,
        elapsed: started.elapsed(),
        peak_rss_bytes: peak_rss_bytes(),
    };
    let json = encode_report(&report)?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&json)?;
    stdout.write_all(b"\n")?;
    let error_count = report_error_count(&report);
    if error_count != 0 {
        return Err(Error::damaged(format!("audit found {error_count} error(s)")));
    }
    Ok(())
}

fn parse_path_list(content: &str) -> Vec<String> {
    content
        .lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn audit_path(path: &Path) -> FileAudit {
    let started = Instant::now();
    let mut audit = FileAudit {
        path: path.to_string_lossy().into_owned(),
        ..FileAudit::default()
    };
    match fs::read(path) {
        Ok(bytes) => audit_bytes(&mut audit, &bytes),
        Err(error) => audit.errors.push(format!("read failed: {error}")),
    }
    audit.elapsed = started.elapsed();
    audit
}

fn audit_bytes(audit: &mut FileAudit, bytes: &[u8]) {
    if let Ok(save) = S2Save::from_bytes(bytes) {
        audit_s2(audit, &save);
        return;
    }
    let save = match Save::read(bytes) {
        Ok(save) => save,
        Err(error) => {
            audit.errors.push(format!("unsupported or damaged save: {error}"));
            return;
        }
    };
    audit_xray(audit, &save);
}

fn audit_xray(audit: &mut FileAudit, save: &Save) {
    audit.format = Some(save.format().id().to_owned());
    let original_money = match save.money() {
        Ok(value) => value,
        Err(error) => {
            audit.errors.push(format!("info money read failed: {error}"));
            return;
        }
    };
    let inventory = match save.inventory() {
        Ok(items) => items,
        Err(error) => {
            audit.errors.push(format!("inventory read failed: {error}"));
            return;
        }
    };
    audit.info_ok = true;
    audit.inventory_count = Some(inventory.len());

    let new_money = increment_or_decrement(original_money);
    let mut changes = vec![Change::SetMoney {
        target_object: save.actor_id(),
        old_value: original_money,
        new_value: new_money,
    }];
    let target_stack = inventory
        .iter()
        .find_map(|item| item.count.map(|count| (item.handle, count)));
    if let Some((handle, old_count)) = target_stack {
        changes.push(Change::SetStack {
            target_object: handle,
            old_value: old_count,
            new_value: increment_or_decrement(old_count),
        });
        audit.stack_edit = "pending".to_owned();
    } else {
        audit.stack_edit = "not_applicable".to_owned();
    }

    let output = match writer::apply(save, &ChangeSet::new(changes)) {
        Ok(output) => output,
        Err(error) => {
            audit.money_edit = "error".to_owned();
            if target_stack.is_some() {
                audit.stack_edit = "error".to_owned();
            }
            audit.errors.push(format!("in-memory edit failed: {error}"));
            return;
        }
    };
    let verified = match Save::read(output.as_slice()) {
        Ok(save) => save,
        Err(error) => {
            audit.money_edit = "error".to_owned();
            if target_stack.is_some() {
                audit.stack_edit = "error".to_owned();
            }
            audit.errors.push(format!("write/read-back parse failed: {error}"));
            return;
        }
    };
    match verified.money() {
        Ok(value) if value == new_money => audit.money_edit = "verified".to_owned(),
        Ok(value) => {
            audit.money_edit = "error".to_owned();
            audit
                .errors
                .push(format!("money read-back mismatch: expected {new_money}, found {value}"));
        }
        Err(error) => {
            audit.money_edit = "error".to_owned();
            audit.errors.push(format!("money read-back failed: {error}"));
        }
    }
    if let Some((handle, old_count)) = target_stack {
        let stack_after = verified
            .inventory()
            .ok()
            .and_then(|items| items.into_iter().find(|item| item.handle == handle))
            .and_then(|item| item.count);
        if stack_after == Some(increment_or_decrement(old_count)) {
            audit.stack_edit = "verified".to_owned();
        } else {
            audit.stack_edit = "error".to_owned();
            audit
                .errors
                .push(format!("stack read-back mismatch for handle 0x{handle:04X}"));
        }
    }
}

fn audit_s2(audit: &mut FileAudit, save: &S2Save) {
    audit.format = Some("s2".to_owned());
    let inventory = save.items();
    audit.info_ok = true;
    audit.inventory_count = Some(inventory.len());
    let original_money = save.money();
    let new_money = increment_or_decrement(original_money);
    let mut changes = vec![S2Change::SetMoney(new_money)];
    let target_stack = inventory
        .iter()
        .find(|item| item.editable_count)
        .map(|item| (item.handle, item.count));
    if let Some((handle, count)) = target_stack {
        changes.push(S2Change::SetStackCount {
            handle,
            count: increment_or_decrement(count),
        });
    }
    match save.write_changes(&changes) {
        Ok(packed) => match S2Save::from_bytes(&packed) {
            Ok(verified) => {
                audit.money_edit = if verified.money() == new_money {
                    "verified".to_owned()
                } else {
                    "error".to_owned()
                };
                if let Some((handle, count)) = target_stack {
                    let expected = increment_or_decrement(count);
                    let actual = verified
                        .items()
                        .into_iter()
                        .find(|item| item.handle == handle)
                        .map(|item| item.count);
                    audit.stack_edit = if actual == Some(expected) {
                        "verified".to_owned()
                    } else {
                        "error".to_owned()
                    };
                } else {
                    audit.stack_edit = "not_applicable".to_owned();
                }
                if audit.money_edit == "error" || audit.stack_edit == "error" {
                    audit
                        .errors
                        .push("S2 in-memory write/read-back value mismatch".to_owned());
                }
            }
            Err(error) => {
                audit.money_edit = "error".to_owned();
                audit.stack_edit = "error".to_owned();
                audit.errors.push(format!("S2 write/read-back parse failed: {error}"));
            }
        },
        Err(error @ Error::Refused(_)) => {
            audit.money_edit = "blocked".to_owned();
            audit.stack_edit = if target_stack.is_some() {
                "blocked".to_owned()
            } else {
                "not_applicable".to_owned()
            };
            audit.errors.push(format!("S2 writer refused the audit edit: {error}"));
        }
        Err(error) => {
            audit.money_edit = "error".to_owned();
            audit.stack_edit = "error".to_owned();
            audit.errors.push(format!("S2 in-memory edit failed: {error}"));
        }
    }
}

fn increment_or_decrement<T>(value: T) -> T
where
    T: Copy + TryInto<u64> + TryFrom<u64>,
{
    let numeric = value.try_into().unwrap_or(u64::MAX);
    // At the top of the type the edit must still change the value, so step down instead of returning it unchanged.
    let up = numeric.checked_add(1).and_then(|next| T::try_from(next).ok());
    up.or_else(|| T::try_from(numeric.saturating_sub(1)).ok())
        .unwrap_or(value)
}

fn encode_report(report: &AuditReport) -> Result<Vec<u8>> {
    let mut writer = JsonWriter::compact();
    writer.object_start()?;
    writer.key("schema_version")?;
    writer.u64(1)?;
    writer.key("file_count")?;
    writer.u64(u64::try_from(report.files.len()).unwrap_or(u64::MAX))?;
    writer.key("error_count")?;
    writer.u64(u64::try_from(report_error_count(report)).unwrap_or(u64::MAX))?;
    writer.key("elapsed_us")?;
    writer.u64(u64::try_from(report.elapsed.as_micros()).unwrap_or(u64::MAX))?;
    writer.key("peak_rss_bytes")?;
    write_optional_u64(&mut writer, report.peak_rss_bytes)?;
    writer.key("files")?;
    writer.array_start()?;
    for file in &report.files {
        writer.object_start()?;
        writer.key("path")?;
        writer.string(&file.path)?;
        writer.key("format")?;
        if let Some(format) = &file.format {
            writer.string(format)?;
        } else {
            writer.null()?;
        }
        writer.key("info_ok")?;
        writer.bool(file.info_ok)?;
        writer.key("inventory_count")?;
        write_optional_u64(
            &mut writer,
            file.inventory_count
                .map(|count| u64::try_from(count).unwrap_or(u64::MAX)),
        )?;
        writer.key("money_edit")?;
        writer.string(&file.money_edit)?;
        writer.key("stack_edit")?;
        writer.string(&file.stack_edit)?;
        writer.key("elapsed_us")?;
        writer.u64(u64::try_from(file.elapsed.as_micros()).unwrap_or(u64::MAX))?;
        writer.key("errors")?;
        writer.array_start()?;
        for error in &file.errors {
            writer.string(error)?;
        }
        writer.array_end()?;
        writer.object_end()?;
    }
    writer.array_end()?;
    writer.object_end()?;
    writer.finish()
}

fn report_error_count(report: &AuditReport) -> usize {
    report
        .files
        .iter()
        .fold(0_usize, |total, file| total.saturating_add(file.errors.len()))
}

fn write_optional_u64(writer: &mut JsonWriter, value: Option<u64>) -> Result<()> {
    match value {
        Some(value) => writer.u64(value),
        None => writer.null(),
    }
}

#[cfg(target_os = "linux")]
fn peak_rss_bytes() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let kilobytes = status.lines().find_map(|line| {
        line.strip_prefix("VmHWM:")
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
    })?;
    Some(kilobytes.saturating_mul(1024))
}

#[cfg(not(target_os = "linux"))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::{audit_bytes, encode_report, increment_or_decrement, parse_path_list, AuditReport, FileAudit};
    use std::time::Duration;

    const XRAY: &[u8] = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
    const S2: &[u8] = include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-source.sav");

    #[test]
    fn xray_money_and_stack_edits_round_trip_without_changing_the_source_buffer() {
        let original_sha = sse_codecs::sha256::sha256_hex(XRAY);
        let mut audit = FileAudit {
            path: "fixture with spaces.sav".to_owned(),
            ..FileAudit::default()
        };
        audit_bytes(&mut audit, XRAY);
        assert!(audit.errors.is_empty(), "{:?}", audit.errors);
        assert!(audit.info_ok);
        assert_eq!(audit.money_edit, "verified");
        assert_eq!(audit.stack_edit, "verified");
        assert_eq!(sse_codecs::sha256::sha256_hex(XRAY), original_sha);
    }

    #[test]
    fn s2_round_trip_is_reported_without_modifying_the_source_buffer() {
        let original_sha = sse_codecs::sha256::sha256_hex(S2);
        let mut audit = FileAudit::default();
        audit_bytes(&mut audit, S2);
        assert!(audit.info_ok);
        assert_eq!(audit.format.as_deref(), Some("s2"));
        assert_eq!(audit.money_edit, "verified");
        assert!(audit.errors.is_empty(), "{:?}", audit.errors);
        assert_eq!(sse_codecs::sha256::sha256_hex(S2), original_sha);
    }

    #[test]
    fn audit_edit_changes_the_value_even_at_the_top_of_its_type() {
        assert_eq!(increment_or_decrement(u32::MAX), u32::MAX - 1);
        assert_eq!(increment_or_decrement(u16::MAX), u16::MAX - 1);
        assert_eq!(increment_or_decrement(41_u32), 42);
    }

    #[test]
    fn path_list_is_one_path_per_nonempty_line_and_keeps_spaces() {
        assert_eq!(
            parse_path_list("/one/save.sav\r\n/tmp/a folder/save.sav\n\n"),
            vec!["/one/save.sav".to_owned(), "/tmp/a folder/save.sav".to_owned()]
        );
    }

    #[test]
    fn audit_returns_failure_when_a_listed_path_cannot_be_read() {
        let root = std::env::temp_dir();
        let list_path = root.join(format!("sse-audit-path-list-{}.txt", std::process::id()));
        let missing_save = root.join(format!("sse-audit-missing-{}.sav", std::process::id()));
        let text = missing_save.to_string_lossy().into_owned();
        assert!(std::fs::write(&list_path, text).is_ok());
        let result = super::run(list_path.to_str());
        let _ = std::fs::remove_file(list_path);
        assert!(result.is_err());
    }

    #[test]
    fn json_report_escapes_paths_and_contains_peak_memory_and_errors() {
        let file = FileAudit {
            path: "save \"one\".sav".to_owned(),
            elapsed: Duration::from_millis(3),
            errors: vec!["a checked error".to_owned()],
            ..FileAudit::default()
        };
        let report = AuditReport {
            files: vec![file],
            elapsed: Duration::from_millis(5),
            peak_rss_bytes: Some(4096),
        };
        let json = encode_report(&report).unwrap_or_default();
        let text = String::from_utf8(json).unwrap_or_default();
        assert!(text.contains("save \\\"one\\\".sav"));
        assert!(text.contains("\"peak_rss_bytes\":4096"));
        assert_eq!(text.matches("\"peak_rss_bytes\"").count(), 1);
        assert!(text.contains("\"error_count\":1"));
    }
}
