//! Checker for condlist functions used in `.ltx` logic that are not defined in `xr_conditions.script` or `xr_effects.script`.
//!
//! When X-Ray encounters an undefined condition or effect function, it aborts execution.

use crate::lexer::{LuaLexer, TokenKind};
use crate::models::{LintFinding, LintSeverity};
use std::collections::HashSet;

/// Extracts top-level function names defined in a Lua script using the native lexer.
#[must_use]
pub fn extract_lua_function_defs(source: &[u8]) -> HashSet<String> {
    let mut lexer = LuaLexer::new(source);
    let mut defs = HashSet::new();
    let tokens = lexer.tokenize_all();

    let mut i = 0;
    while i < tokens.len() {
        if let Some(tok) = tokens.get(i) {
            match &tok.kind {
                TokenKind::Keyword(kw) if kw == "local" => {
                    // Skip local declaration or local function
                    if let Some(next) = tokens.get(i.saturating_add(1)) {
                        if next.kind == TokenKind::Keyword("function".to_string()) {
                            i = i.saturating_add(2);
                        } else {
                            i = i.saturating_add(1);
                        }
                    }
                }
                TokenKind::Keyword(kw) if kw == "function" => {
                    if let Some(next) = tokens.get(i.saturating_add(1)) {
                        if let TokenKind::Identifier(fn_name) = &next.kind {
                            defs.insert(fn_name.clone());
                        }
                    }
                }
                TokenKind::Identifier(id) => {
                    // Check top-level assignment: `name = ...`
                    if let Some(eq_tok) = tokens.get(i.saturating_add(1)) {
                        if eq_tok.kind == TokenKind::Symbol("=".to_string()) {
                            defs.insert(id.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        i = i.saturating_add(1);
    }

    defs
}

/// Parses condition function calls from a `{...}` block.
/// E.g. `{=func_one !func_two(1:2)}` -> `["func_one", "func_two"]`.
#[must_use]
pub fn parse_condition_funcs(block: &str) -> Vec<String> {
    let mut funcs = Vec::new();
    let bytes = block.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            if b == b'=' || b == b'!' {
                let start = i.saturating_add(1);
                let mut end = start;
                while end < bytes.len() {
                    let c = bytes.get(end).copied().unwrap_or(0);
                    if c.is_ascii_alphanumeric() || c == b'_' {
                        end = end.saturating_add(1);
                    } else {
                        break;
                    }
                }
                if end > start {
                    if let Some(sub) = block.get(start..end) {
                        funcs.push(sub.to_string());
                    }
                }
                i = end;
                continue;
            }
        }
        i = i.saturating_add(1);
    }
    funcs
}

/// Parses effect function calls from a `%...%` block.
/// E.g. `%=effect_one =effect_two%` -> `["effect_one", "effect_two"]`.
#[must_use]
pub fn parse_effect_funcs(block: &str) -> Vec<String> {
    let mut funcs = Vec::new();
    let bytes = block.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            if b == b'=' {
                let start = i.saturating_add(1);
                let mut end = start;
                while end < bytes.len() {
                    let c = bytes.get(end).copied().unwrap_or(0);
                    if c.is_ascii_alphanumeric() || c == b'_' {
                        end = end.saturating_add(1);
                    } else {
                        break;
                    }
                }
                if end > start {
                    if let Some(sub) = block.get(start..end) {
                        funcs.push(sub.to_string());
                    }
                }
                i = end;
                continue;
            }
        }
        i = i.saturating_add(1);
    }
    funcs
}

/// Extracts curly-brace `{...}` and percent-brace `%...%` blocks from an LTX line.
fn extract_blocks(line: &str) -> (Vec<String>, Vec<String>) {
    let mut cond_blocks = Vec::new();
    let mut eff_blocks = Vec::new();

    let mut in_brace = false;
    let mut brace_buf = String::new();

    let mut in_pct = false;
    let mut pct_buf = String::new();

    for ch in line.chars() {
        if ch == '{' {
            in_brace = true;
            brace_buf.clear();
        } else if ch == '}' && in_brace {
            in_brace = false;
            cond_blocks.push(brace_buf.clone());
        } else if in_brace {
            brace_buf.push(ch);
        }

        if ch == '%' {
            if in_pct {
                in_pct = false;
                eff_blocks.push(pct_buf.clone());
            } else {
                in_pct = true;
                pct_buf.clear();
            }
        } else if in_pct {
            pct_buf.push(ch);
        }
    }

    (cond_blocks, eff_blocks)
}

/// Checks an LTX line against available conditions and effects function sets.
pub fn check_condfuncs_line(
    path: &str,
    line_num: usize,
    line: &str,
    conditions: &HashSet<String>,
    effects: &HashSet<String>,
    findings: &mut Vec<LintFinding>,
) {
    let body = line.split(';').next().unwrap_or("");
    let trimmed = body.trim();
    if !trimmed.contains('=') || trimmed.starts_with('[') {
        return;
    }

    let Some((_, value)) = body.split_once('=') else {
        return;
    };

    let (cond_blocks, eff_blocks) = extract_blocks(value);

    for blk in cond_blocks {
        for fn_name in parse_condition_funcs(&blk) {
            if !conditions.contains(&fn_name) {
                let snippet = trimmed.chars().take(150).collect::<String>();
                findings.push(LintFinding {
                    checker: "check_condfuncs".to_string(),
                    file: path.to_string(),
                    line: line_num,
                    severity: LintSeverity::Error,
                    message: format!("condition {fn_name}: {snippet}"),
                });
            }
        }
    }

    for blk in eff_blocks {
        for fn_name in parse_effect_funcs(&blk) {
            if !effects.contains(&fn_name) {
                let snippet = trimmed.chars().take(150).collect::<String>();
                findings.push(LintFinding {
                    checker: "check_condfuncs".to_string(),
                    file: path.to_string(),
                    line: line_num,
                    severity: LintSeverity::Error,
                    message: format!("effect {fn_name}: {snippet}"),
                });
            }
        }
    }
}
