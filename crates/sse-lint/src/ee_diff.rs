//! Enhanced Edition diff tool.
//!
//! Compares Retail game files with Enhanced Edition files, ignoring comments and whitespace differences.

/// Normalized line representation for comparison.
fn normalize_content(path: &str, raw_bytes: &[u8]) -> Vec<String> {
    let text = match std::str::from_utf8(raw_bytes) {
        Ok(s) => s.to_string(),
        Err(_) => sse_content::decode_windows_1251(raw_bytes),
    };

    let is_script = path.ends_with(".script");
    let mut normalized_lines = Vec::new();

    for raw_line in text.replace('\r', "").lines() {
        let code = if is_script {
            raw_line.split("--").next().unwrap_or("").trim()
        } else {
            raw_line.trim()
        };

        // Collapse multiple whitespace
        let mut collapsed = String::new();
        let mut prev_is_space = false;
        for ch in code.chars() {
            if ch.is_whitespace() {
                if !prev_is_space {
                    collapsed.push(' ');
                    prev_is_space = true;
                }
            } else {
                collapsed.push(ch);
                prev_is_space = false;
            }
        }

        let trimmed = collapsed.trim();
        if !trimmed.is_empty() {
            normalized_lines.push(trimmed.to_string());
        }
    }

    normalized_lines
}

/// A line change between retail and EE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffHunk {
    /// Line added in EE
    Added(String),
    /// Line removed from Retail
    Removed(String),
}

/// Computes unified differences between retail and EE normalized lines.
#[must_use]
pub fn diff_retail_and_ee(path: &str, retail_bytes: &[u8], ee_bytes: &[u8]) -> Vec<DiffHunk> {
    let retail_lines = normalize_content(path, retail_bytes);
    let ee_lines = normalize_content(path, ee_bytes);

    if retail_lines == ee_lines {
        return Vec::new();
    }

    // Fast difference calculation
    let mut hunks = Vec::new();
    let max_len = std::cmp::max(retail_lines.len(), ee_lines.len());
    let mut r_idx = 0;
    let mut e_idx = 0;

    while r_idx < retail_lines.len() || e_idx < ee_lines.len() {
        let r_line = retail_lines.get(r_idx);
        let e_line = ee_lines.get(e_idx);

        if r_line == e_line {
            r_idx = r_idx.saturating_add(1);
            e_idx = e_idx.saturating_add(1);
            continue;
        }

        // Check if retail line appears ahead in EE (added in EE)
        let found_r_in_e = r_line.and_then(|r| {
            ee_lines
                .get(e_idx..)
                .and_then(|slice| slice.iter().position(|e| e == r))
        });
        let found_e_in_r = e_line.and_then(|e| {
            retail_lines
                .get(r_idx..)
                .and_then(|slice| slice.iter().position(|r| r == e))
        });

        match (found_r_in_e, found_e_in_r) {
            (Some(e_offset), _) if e_offset > 0 => {
                // Lines in EE before match were added
                for k in 0..e_offset {
                    if let Some(add) = ee_lines.get(e_idx.saturating_add(k)) {
                        hunks.push(DiffHunk::Added(add.clone()));
                    }
                }
                e_idx = e_idx.saturating_add(e_offset);
            }
            (_, Some(r_offset)) if r_offset > 0 => {
                // Lines in Retail before match were removed
                for k in 0..r_offset {
                    if let Some(rem) = retail_lines.get(r_idx.saturating_add(k)) {
                        hunks.push(DiffHunk::Removed(rem.clone()));
                    }
                }
                r_idx = r_idx.saturating_add(r_offset);
            }
            _ => {
                if let Some(rem) = r_line {
                    hunks.push(DiffHunk::Removed(rem.clone()));
                    r_idx = r_idx.saturating_add(1);
                }
                if let Some(add) = e_line {
                    hunks.push(DiffHunk::Added(add.clone()));
                    e_idx = e_idx.saturating_add(1);
                }
            }
        }

        if hunks.len() > max_len.saturating_mul(2) {
            break;
        }
    }

    hunks
}
