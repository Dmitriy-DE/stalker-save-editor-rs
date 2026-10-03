//! Checker for info portions tested in logic with `{+name}` that are never given by any source.
//!
//! If an info portion is tested but never given in LTX, XML, or Lua, the condition can never become true.

use crate::models::{LintFinding, LintSeverity};
use std::collections::{HashMap, HashSet};

/// State aggregator for whole-game info portion validation.
#[derive(Default)]
pub struct InfoPortionIndex {
    /// Set of info portions given via XML (`<give_info>`) or LTX (`%+name%`)
    pub given_infos: HashSet<String>,
    /// Map of tested info portion name -> list of occurrences `(file, line, line_snippet)`
    pub tested_infos: HashMap<String, Vec<(String, usize, String)>>,
    /// Combined script text or script word set
    pub script_words: HashSet<String>,
}

impl InfoPortionIndex {
    /// Creates a new index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Indexes an XML file's `<give_info>...</give_info>` tags.
    pub fn feed_xml(&mut self, text: &str) {
        let mut pos = 0;
        while let Some(start) = text.get(pos..).and_then(|t| t.find("<give_info>")) {
            let abs_start = pos.saturating_add(start).saturating_add("<give_info>".len());
            if let Some(end) = text.get(abs_start..).and_then(|t| t.find("</give_info>")) {
                let name = text.get(abs_start..abs_start.saturating_add(end)).unwrap_or("").trim();
                if !name.is_empty() {
                    self.given_infos.insert(name.to_string());
                }
                pos = abs_start.saturating_add(end).saturating_add("</give_info>".len());
            } else {
                break;
            }
        }
    }

    /// Indexes an LTX file's `%+give%` and `{+test}` expressions.
    pub fn feed_ltx(&mut self, path: &str, text: &str) {
        for (line_idx, line) in text.lines().enumerate() {
            let line_num = line_idx.saturating_add(1);
            let body = line.split(';').next().unwrap_or("");
            if !body.contains('+') {
                continue;
            }

            // Extract %+name%
            let mut pos = 0;
            while let Some(pct_start) = body.get(pos..).and_then(|t| t.find('%')) {
                let abs_pct = pos.saturating_add(pct_start).saturating_add(1);
                if let Some(pct_end) = body.get(abs_pct..).and_then(|t| t.find('%')) {
                    let block = body.get(abs_pct..abs_pct.saturating_add(pct_end)).unwrap_or("");
                    extract_plus_names(block, &mut self.given_infos);
                    pos = abs_pct.saturating_add(pct_end).saturating_add(1);
                } else {
                    break;
                }
            }

            // Extract {+name}
            pos = 0;
            while let Some(brace_start) = body.get(pos..).and_then(|t| t.find('{')) {
                let abs_brace = pos.saturating_add(brace_start).saturating_add(1);
                if let Some(brace_end) = body.get(abs_brace..).and_then(|t| t.find('}')) {
                    let block = body.get(abs_brace..abs_brace.saturating_add(brace_end)).unwrap_or("");
                    let mut tested_here = HashSet::new();
                    extract_plus_names(block, &mut tested_here);
                    for name in tested_here {
                        self.tested_infos.entry(name).or_default().push((
                            path.to_string(),
                            line_num,
                            line.trim().to_string(),
                        ));
                    }
                    pos = abs_brace.saturating_add(brace_end).saturating_add(1);
                } else {
                    break;
                }
            }
        }
    }

    /// Feeds words from Lua script contents.
    pub fn feed_script(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes.get(i).copied().unwrap_or(0);
            if b.is_ascii_alphanumeric() || b == b'_' {
                let start = i;
                while i < bytes.len() {
                    let c = bytes.get(i).copied().unwrap_or(0);
                    if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' {
                        i = i.saturating_add(1);
                    } else {
                        break;
                    }
                }
                if let Some(sub) = text.get(start..i) {
                    self.script_words.insert(sub.to_string());
                }
                continue;
            }
            i = i.saturating_add(1);
        }
    }

    /// Evaluates the index and outputs findings for orphaned tested info portions.
    #[must_use]
    pub fn evaluate(self) -> Vec<LintFinding> {
        let mut findings = Vec::new();
        let mut sorted_tested: Vec<_> = self.tested_infos.into_iter().collect();
        sorted_tested.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, occurrences) in sorted_tested {
            if self.given_infos.contains(&name) || self.script_words.contains(&name) {
                continue;
            }
            if let Some((path, line, line_snippet)) = occurrences.first() {
                let count = occurrences.len();
                let snippet = line_snippet.chars().take(140).collect::<String>();
                findings.push(LintFinding {
                    checker: "check_infos".to_string(),
                    file: path.clone(),
                    line: *line,
                    severity: LintSeverity::Warning,
                    message: format!("{name} ({count}x): {snippet}"),
                });
            }
        }
        findings
    }
}

/// Extracts words prefixed with `+` inside a condlist block.
fn extract_plus_names(block: &str, sink: &mut HashSet<String>) {
    let bytes = block.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes.get(i) == Some(&b'+') {
            let start = i.saturating_add(1);
            let mut end = start;
            while end < bytes.len() {
                let c = bytes.get(end).copied().unwrap_or(0);
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' {
                    end = end.saturating_add(1);
                } else {
                    break;
                }
            }
            if end > start {
                if let Some(sub) = block.get(start..end) {
                    sink.insert(sub.to_string());
                }
            }
            i = end;
            continue;
        }
        i = i.saturating_add(1);
    }
}
