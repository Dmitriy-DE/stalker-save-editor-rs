//! Checker for malformed X-Ray condlists in `.ltx` logic.
//!
//! Validates bracket nesting and syntax in condlists:
//! - nested `{`
//! - stray `}`
//! - unclosed `{`
//! - `%` inside `{}`
//! - unbalanced `%`

use crate::models::{LintFinding, LintSeverity};

/// Runs the condlist syntax check on an LTX line content.
#[must_use]
pub fn check_condlist_line(path: &str, line_num: usize, line: &str) -> Option<LintFinding> {
    let body = line.split(';').next().unwrap_or("");
    let trimmed = body.trim();
    if !trimmed.contains('=') || trimmed.starts_with('[') {
        return None;
    }

    let (_, value) = body.split_once('=')?;

    if !value.contains('{') && !value.contains('%') && !value.contains('}') {
        return None;
    }

    let mut depth: usize = 0;
    let mut pct: usize = 0;
    let mut bad_reason = None;

    for ch in value.chars() {
        if ch == '{' {
            if depth > 0 || pct % 2 != 0 {
                bad_reason = Some("nested {");
                break;
            }
            depth = depth.saturating_add(1);
        } else if ch == '}' {
            if depth == 0 {
                bad_reason = Some("stray }");
                break;
            }
            depth = depth.saturating_sub(1);
        } else if ch == '%' {
            if depth > 0 {
                bad_reason = Some("% inside {}");
                break;
            }
            pct = pct.saturating_add(1);
        }
    }

    if bad_reason.is_none() && depth > 0 {
        bad_reason = Some("unclosed {");
    }
    if bad_reason.is_none() && pct % 2 != 0 {
        bad_reason = Some("unbalanced %");
    }

    bad_reason.map(|reason| {
        let snippet = trimmed.chars().take(160).collect::<String>();
        LintFinding {
            checker: "check_condlists".to_string(),
            file: path.to_string(),
            line: line_num,
            severity: LintSeverity::Error,
            message: format!("{reason}: {snippet}"),
        }
    })
}

/// Runs condlists checker across raw text of an `.ltx` file.
pub fn check_condlists_text(path: &str, text: &str, findings: &mut Vec<LintFinding>) {
    for (line_idx, line) in text.lines().enumerate() {
        let line_num = line_idx.saturating_add(1);
        if let Some(finding) = check_condlist_line(path, line_num, line) {
            findings.push(finding);
        }
    }
}
