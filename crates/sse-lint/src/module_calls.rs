//! Checker for calls to script functions that the script does not define.
//!
//! X-Ray crashes with `"attempt to call field ... (a nil value)"` when an undefined function is called.

use crate::lexer::{LuaLexer, TokenKind};
use crate::models::{LintFinding, LintSeverity};
use std::collections::{HashMap, HashSet};

/// Information collected from a script module.
pub struct ScriptModuleInfo {
    /// Name of module (script file name without `.script`)
    pub name: String,
    /// Functions and symbols defined in module
    pub defs: HashSet<String>,
    /// Calls made by module: `(target_module, func_name, line_num, code_line)`
    pub calls: Vec<(String, String, usize, String)>,
    /// Whether `local <module_name>` is declared inside this script
    pub local_modules: HashSet<String>,
}

/// Analyzes a single Lua script for definitions and calls.
#[must_use]
pub fn analyze_script_module(name: &str, source: &[u8]) -> ScriptModuleInfo {
    let mut lexer = LuaLexer::new(source);
    let mut defs = HashSet::new();
    let mut calls = Vec::new();
    let mut local_modules = HashSet::new();

    let tokens = lexer.tokenize_all();

    // Pass 1: Extract definitions and local declarations
    let mut i = 0;
    while i < tokens.len() {
        if let Some(tok) = tokens.get(i) {
            match &tok.kind {
                TokenKind::Keyword(kw) if kw == "local" => {
                    // Check `local function <name>`
                    if let Some(next) = tokens.get(i.saturating_add(1)) {
                        if next.kind == TokenKind::Keyword("function".to_string()) {
                            if let Some(id_tok) = tokens.get(i.saturating_add(2)) {
                                if let TokenKind::Identifier(fn_name) = &id_tok.kind {
                                    defs.insert(fn_name.clone());
                                }
                            }
                        } else if let TokenKind::Identifier(var_name) = &next.kind {
                            local_modules.insert(var_name.clone());
                        }
                    }
                }
                TokenKind::Keyword(kw) if kw == "function" => {
                    // Check `function <name>(` or `function <mod>.<name>(`
                    if let Some(id_tok) = tokens.get(i.saturating_add(1)) {
                        if let TokenKind::Identifier(fn_name) = &id_tok.kind {
                            if let Some(dot_tok) = tokens.get(i.saturating_add(2)) {
                                if dot_tok.kind == TokenKind::Symbol(".".to_string())
                                    || dot_tok.kind == TokenKind::Symbol(":".to_string())
                                {
                                    if let Some(method_tok) = tokens.get(i.saturating_add(3)) {
                                        if let TokenKind::Identifier(m_name) = &method_tok.kind {
                                            defs.insert(m_name.clone());
                                        }
                                    }
                                } else {
                                    defs.insert(fn_name.clone());
                                }
                            } else {
                                defs.insert(fn_name.clone());
                            }
                        }
                    }
                }
                TokenKind::Identifier(id) => {
                    // Check `class "<name>"` or `name = ...`
                    if id == "class" {
                        if let Some(arg_tok) = tokens.get(i.saturating_add(1)) {
                            if let TokenKind::StringLiteral(c_name) = &arg_tok.kind {
                                defs.insert(c_name.clone());
                            }
                        }
                    } else if let Some(eq_tok) = tokens.get(i.saturating_add(1)) {
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

    // Pass 2: Extract calls `mod.func(`
    i = 0;
    while i < tokens.len() {
        if let Some(tok) = tokens.get(i) {
            if let TokenKind::Identifier(mod_name) = &tok.kind {
                if let Some(dot_tok) = tokens.get(i.saturating_add(1)) {
                    if dot_tok.kind == TokenKind::Symbol(".".to_string()) {
                        if let Some(fn_tok) = tokens.get(i.saturating_add(2)) {
                            if let TokenKind::Identifier(fn_name) = &fn_tok.kind {
                                if let Some(paren_tok) = tokens.get(i.saturating_add(3)) {
                                    if paren_tok.kind == TokenKind::Symbol("(".to_string()) {
                                        let line = tok.line;
                                        calls.push((
                                            mod_name.clone(),
                                            fn_name.clone(),
                                            line,
                                            format!("{mod_name}.{fn_name}()"),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        i = i.saturating_add(1);
    }

    ScriptModuleInfo {
        name: name.to_string(),
        defs,
        calls,
        local_modules,
    }
}

/// Evaluates script-to-script module calls.
pub fn check_script_module_calls(modules: &HashMap<String, ScriptModuleInfo>, findings: &mut Vec<LintFinding>) {
    let mut sorted_keys: Vec<_> = modules.keys().collect();
    sorted_keys.sort();

    for m in sorted_keys {
        if let Some(mod_info) = modules.get(m) {
            for (target_mod, target_fn, line, code_snippet) in &mod_info.calls {
                if target_mod == m || mod_info.local_modules.contains(target_mod) {
                    continue;
                }

                if let Some(target_info) = modules.get(target_mod) {
                    if !target_info.defs.contains(target_fn) {
                        findings.push(LintFinding {
                            checker: "check_module_calls".to_string(),
                            file: format!("scripts/{m}.script"),
                            line: *line,
                            severity: LintSeverity::Error,
                            message: format!("{target_mod}.{target_fn}: {code_snippet}"),
                        });
                    }
                }
            }
        }
    }
}

/// Checks XML `<action>mod.func</action>` and `<precondition>mod.func</precondition>` references.
pub fn check_xml_module_refs(
    path: &str,
    text: &str,
    modules: &HashMap<String, ScriptModuleInfo>,
    findings: &mut Vec<LintFinding>,
) {
    for (line_idx, line) in text.lines().enumerate() {
        let line_num = line_idx.saturating_add(1);
        let trimmed = line.trim();
        for tag in &["<action>", "<precondition>"] {
            let close_tag = match *tag {
                "<action>" => "</action>",
                "<precondition>" => "</precondition>",
                _ => "",
            };

            let mut pos = 0;
            while let Some(start) = trimmed.get(pos..).and_then(|t| t.find(tag)) {
                let abs_start = pos.saturating_add(start).saturating_add(tag.len());
                if let Some(end) = trimmed.get(abs_start..).and_then(|t| t.find(close_tag)) {
                    let content = trimmed
                        .get(abs_start..abs_start.saturating_add(end))
                        .unwrap_or("")
                        .trim();
                    if let Some((mod_name, fn_name)) = content.split_once('.') {
                        let m = mod_name.trim();
                        let f = fn_name.trim();
                        if let Some(mod_info) = modules.get(m) {
                            if !mod_info.defs.contains(f) {
                                findings.push(LintFinding {
                                    checker: "check_module_calls".to_string(),
                                    file: path.to_string(),
                                    line: line_num,
                                    severity: LintSeverity::Error,
                                    message: format!("{m}.{f} not defined: {trimmed}"),
                                });
                            }
                        } else {
                            findings.push(LintFinding {
                                checker: "check_module_calls".to_string(),
                                file: path.to_string(),
                                line: line_num,
                                severity: LintSeverity::Error,
                                message: format!("no script {m}: {trimmed}"),
                            });
                        }
                    }
                    pos = abs_start.saturating_add(end).saturating_add(close_tag.len());
                } else {
                    break;
                }
            }
        }
    }
}
