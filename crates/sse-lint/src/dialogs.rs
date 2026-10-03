//! Checker for dialog tree errors that crash X-Ray.
//!
//! Validates:
//! - `<next>` pointing at a missing phrase
//! - duplicate phrase IDs inside the same dialog
//! - missing start phrase `0`

use crate::models::{LintFinding, LintSeverity};
use std::collections::{HashMap, HashSet};

/// Intermediate representation of phrases parsed for a dialog.
#[derive(Default)]
struct DialogEntry {
    path: String,
    line: usize,
    phrase_counts: HashMap<String, usize>,
    all_phrase_nexts: Vec<(String, Vec<String>)>,
}

/// Scans XML dialog content and finds dialog consistency issues across all files.
pub fn check_all_dialogs(files: &[(&str, &str)], findings: &mut Vec<LintFinding>) {
    let mut dialogs: HashMap<String, DialogEntry> = HashMap::new();

    for &(path, text) in files {
        let mut dialog_start_pos = 0;
        while let Some(start_idx) = text.get(dialog_start_pos..).and_then(|t| t.find("<dialog")) {
            let abs_start = dialog_start_pos.saturating_add(start_idx);

            // Ensure `<dialog` is followed by whitespace or `>`
            let after_dialog = abs_start.saturating_add("<dialog".len());
            let is_dialog_tag = match text.as_bytes().get(after_dialog) {
                Some(&b) => b.is_ascii_whitespace() || b == b'>',
                None => false,
            };
            if !is_dialog_tag {
                dialog_start_pos = after_dialog;
                continue;
            }

            let Some(tag_close) = text.get(abs_start..).and_then(|t| t.find('>')) else {
                break;
            };
            let tag_close_abs = abs_start.saturating_add(tag_close);

            let Some(dialog_end) = text.get(tag_close_abs..).and_then(|t| t.find("</dialog>")) else {
                break;
            };
            let dialog_end_abs = tag_close_abs.saturating_add(dialog_end);

            // Extract dialog id attribute: `id="value"`
            let open_tag = text.get(abs_start..tag_close_abs).unwrap_or("");
            let dialog_id = extract_attribute(open_tag, "id").unwrap_or_else(|| "unknown".to_string());
            let body = text.get(tag_close_abs.saturating_add(1)..dialog_end_abs).unwrap_or("");
            let line_num = count_lines_up_to(text, abs_start);

            let entry = dialogs.entry(dialog_id).or_insert_with(|| DialogEntry {
                path: path.to_string(),
                line: line_num,
                phrase_counts: HashMap::new(),
                all_phrase_nexts: Vec::new(),
            });

            parse_phrases_into(body, entry);

            dialog_start_pos = dialog_end_abs.saturating_add("</dialog>".len());
        }
    }

    // Now evaluate consistency for each merged dialog
    for (dialog_id, entry) in dialogs {
        let defined_ids: HashSet<&str> = entry.phrase_counts.keys().map(String::as_str).collect();

        // 1. Duplicate phrase IDs
        for (id, &count) in &entry.phrase_counts {
            if count > 1 {
                findings.push(LintFinding {
                    checker: "check_dialogs".to_string(),
                    file: entry.path.clone(),
                    line: entry.line,
                    severity: LintSeverity::Error,
                    message: format!("{dialog_id}: duplicate phrase id {id}"),
                });
            }
        }

        // 2. Missing start phrase 0
        if !entry.phrase_counts.is_empty() && !defined_ids.contains("0") {
            findings.push(LintFinding {
                checker: "check_dialogs".to_string(),
                file: entry.path.clone(),
                line: entry.line,
                severity: LintSeverity::Error,
                message: format!("{dialog_id}: no start phrase 0"),
            });
        }

        // 3. Next pointing at missing phrase
        for (ph_id, nexts) in entry.all_phrase_nexts {
            for next_id in nexts {
                if !defined_ids.contains(next_id.as_str()) {
                    findings.push(LintFinding {
                        checker: "check_dialogs".to_string(),
                        file: entry.path.clone(),
                        line: entry.line,
                        severity: LintSeverity::Error,
                        message: format!("{dialog_id}: phrase {ph_id} -> missing {next_id}"),
                    });
                }
            }
        }
    }
}

/// Scans XML dialog content and finds dialog consistency issues for a single file.
pub fn check_dialogs_xml(path: &str, text: &str, findings: &mut Vec<LintFinding>) {
    check_all_dialogs(&[(path, text)], findings);
}

/// Helper to parse phrases from a dialog body and accumulate into a DialogEntry.
fn parse_phrases_into(body: &str, entry: &mut DialogEntry) {
    let mut pos = 0;
    while let Some(start_idx) = body.get(pos..).and_then(|t| t.find("<phrase")) {
        let abs_start = pos.saturating_add(start_idx);

        // Ensure `<phrase` is followed by whitespace or `>`
        let after_phrase = abs_start.saturating_add("<phrase".len());
        let is_phrase_tag = match body.as_bytes().get(after_phrase) {
            Some(&b) => b.is_ascii_whitespace() || b == b'>',
            None => false,
        };
        if !is_phrase_tag {
            pos = after_phrase;
            continue;
        }

        let Some(tag_close) = body.get(abs_start..).and_then(|t| t.find('>')) else {
            break;
        };
        let tag_close_abs = abs_start.saturating_add(tag_close);

        let open_tag = body.get(abs_start..tag_close_abs).unwrap_or("");
        let phrase_id = extract_attribute(open_tag, "id").unwrap_or_default();

        if !phrase_id.is_empty() {
            let count = entry.phrase_counts.entry(phrase_id.clone()).or_insert(0);
            *count = count.saturating_add(1);
        }

        // Find closing tag `</phrase>`
        let end_idx = body.get(tag_close_abs..).and_then(|t| t.find("</phrase>"));
        if let Some(end) = end_idx {
            let abs_end = tag_close_abs.saturating_add(end);
            let phrase_body = body.get(tag_close_abs.saturating_add(1)..abs_end).unwrap_or("");

            // Parse `<next>...</next>`
            let mut next_pos = 0;
            let mut next_list = Vec::new();
            while let Some(n_start) = phrase_body.get(next_pos..).and_then(|t| t.find("<next>")) {
                let n_abs = next_pos.saturating_add(n_start).saturating_add("<next>".len());
                if let Some(n_close) = phrase_body.get(n_abs..).and_then(|t| t.find("</next>")) {
                    let n_val = phrase_body
                        .get(n_abs..n_abs.saturating_add(n_close))
                        .unwrap_or("")
                        .trim();
                    if !n_val.is_empty() {
                        next_list.push(n_val.to_string());
                    }
                    next_pos = n_abs.saturating_add(n_close).saturating_add("</next>".len());
                } else {
                    break;
                }
            }
            entry.all_phrase_nexts.push((phrase_id, next_list));
            pos = abs_end.saturating_add("</phrase>".len());
        } else {
            pos = tag_close_abs.saturating_add(1);
        }
    }
}

/// Helper to extract attribute value: `attr="value"`
fn extract_attribute(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = tag.find(&pattern)?.saturating_add(pattern.len());
    let end = tag.get(start..)?.find('"')?;
    Some(tag.get(start..start.saturating_add(end))?.to_string())
}

/// Counts 1-based line number up to byte offset.
fn count_lines_up_to(text: &str, offset: usize) -> usize {
    let mut lines: usize = 1;
    for &b in text.as_bytes().iter().take(offset) {
        if b == b'\n' {
            lines = lines.saturating_add(1);
        }
    }
    lines
}
