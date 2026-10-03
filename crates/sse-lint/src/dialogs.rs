//! Checker for dialog tree errors that crash X-Ray.
//!
//! Validates:
//! - `<next>` pointing at a missing phrase
//! - duplicate phrase IDs inside the same dialog
//! - missing start phrase `0`

use crate::models::{LintFinding, LintSeverity};
use std::collections::{HashMap, HashSet};

/// Scans XML dialog content and finds dialog consistency issues.
pub fn check_dialogs_xml(path: &str, text: &str, findings: &mut Vec<LintFinding>) {
    // Parse dialog elements: `<dialog id="id">...</dialog>`
    let mut dialog_start_pos = 0;
    while let Some(start_idx) = text.get(dialog_start_pos..).and_then(|t| t.find("<dialog")) {
        let abs_start = dialog_start_pos.saturating_add(start_idx);
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

        // Calculate approximate line number of the dialog
        let line_num = count_lines_up_to(text, abs_start);

        check_single_dialog(path, line_num, &dialog_id, body, findings);

        dialog_start_pos = dialog_end_abs.saturating_add("</dialog>".len());
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

/// Analyzes phrases within a single `<dialog>...</dialog>` block.
fn check_single_dialog(path: &str, dialog_line: usize, dialog_id: &str, body: &str, findings: &mut Vec<LintFinding>) {
    let mut phrase_counts: HashMap<String, usize> = HashMap::new();
    let mut all_phrase_nexts: Vec<(String, Vec<String>)> = Vec::new();

    let mut pos = 0;
    while let Some(start_idx) = body.get(pos..).and_then(|t| t.find("<phrase")) {
        let abs_start = pos.saturating_add(start_idx);
        let Some(tag_close) = body.get(abs_start..).and_then(|t| t.find('>')) else {
            break;
        };
        let tag_close_abs = abs_start.saturating_add(tag_close);

        let open_tag = body.get(abs_start..tag_close_abs).unwrap_or("");
        let phrase_id = extract_attribute(open_tag, "id").unwrap_or_default();

        if !phrase_id.is_empty() {
            let count = phrase_counts.entry(phrase_id.clone()).or_insert(0);
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
            all_phrase_nexts.push((phrase_id, next_list));
            pos = abs_end.saturating_add("</phrase>".len());
        } else {
            pos = tag_close_abs.saturating_add(1);
        }
    }

    let defined_ids: HashSet<&str> = phrase_counts.keys().map(String::as_str).collect();

    // 1. Duplicate phrase IDs
    for (id, &count) in &phrase_counts {
        if count > 1 {
            findings.push(LintFinding {
                checker: "check_dialogs".to_string(),
                file: path.to_string(),
                line: dialog_line,
                severity: LintSeverity::Error,
                message: format!("{dialog_id}: duplicate phrase id {id}"),
            });
        }
    }

    // 2. Missing start phrase 0
    if !phrase_counts.is_empty() && !defined_ids.contains("0") {
        findings.push(LintFinding {
            checker: "check_dialogs".to_string(),
            file: path.to_string(),
            line: dialog_line,
            severity: LintSeverity::Error,
            message: format!("{dialog_id}: no start phrase 0"),
        });
    }

    // 3. Next pointing at missing phrase
    for (ph_id, nexts) in all_phrase_nexts {
        for next_id in nexts {
            if !defined_ids.contains(next_id.as_str()) {
                findings.push(LintFinding {
                    checker: "check_dialogs".to_string(),
                    file: path.to_string(),
                    line: dialog_line,
                    severity: LintSeverity::Error,
                    message: format!("{dialog_id}: phrase {ph_id} -> missing {next_id}"),
                });
            }
        }
    }
}
