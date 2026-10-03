//! Checker for item names in trade lists that are not defined in any game config section.
//!
//! If a trader references a non-existent item section, the game fails when opening trade.

use crate::models::{LintFinding, LintSeverity};
use std::collections::HashSet;

/// Checks trade `.ltx` content against known config section names.
pub fn check_trade_file(path: &str, text: &str, known_sections: &HashSet<String>, findings: &mut Vec<LintFinding>) {
    let mut current_sec: Option<String> = None;

    for (line_idx, line) in text.lines().enumerate() {
        let line_num = line_idx.saturating_add(1);
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            if let Some(end) = trimmed.find(']') {
                if let Some(sec) = trimmed.get(1..end) {
                    current_sec = Some(sec.trim().to_string());
                }
            }
            continue;
        }

        let body = line.split(';').next().unwrap_or("").trim();
        if body.is_empty() || body.starts_with('#') {
            continue;
        }

        let Some(sec) = &current_sec else {
            continue;
        };

        if sec.eq_ignore_ascii_case("trader") {
            continue;
        }

        let key = body.split('=').next().unwrap_or("").trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }

        if key.starts_with("buy_") || key.starts_with("sell_") || key.starts_with("discounts") {
            continue;
        }

        let is_valid_ident = key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-');
        if is_valid_ident && !known_sections.contains(&key) {
            findings.push(LintFinding {
                checker: "check_trade_items".to_string(),
                file: path.to_string(),
                line: line_num,
                severity: LintSeverity::Warning,
                message: format!("[{sec}] {key}"),
            });
        }
    }
}
