//! Checker for undefined or misspelled global variables in Lua scripts.
//!
//! Flags global variable reads that are never defined anywhere and not standard engine globals.

use crate::lexer::{LuaLexer, TokenKind};
use crate::models::{LintFinding, LintSeverity};
use std::collections::{HashMap, HashSet};

/// Known base Lua and engine global symbols that are always present.
const BASE_GLOBALS: &[&str] = &[
    "assert",
    "error",
    "getmetatable",
    "ipairs",
    "next",
    "pairs",
    "pcall",
    "rawequal",
    "rawget",
    "rawset",
    "select",
    "setmetatable",
    "tonumber",
    "tostring",
    "type",
    "unpack",
    "xpcall",
    "string",
    "table",
    "math",
    "io",
    "os",
    "debug",
    "bit",
    "bit32",
    "coroutine",
    "_G",
    "_VERSION",
    "self",
    "device",
    "level",
    "relation_registry",
    "system_ini",
    "game",
    "alife",
];

/// Collects globals analysis across a set of Lua scripts.
#[derive(Default)]
pub struct LuaGlobalsAnalyzer {
    reads: HashMap<String, HashSet<String>>,
    sets: HashSet<String>,
    positions: HashMap<(String, String), Vec<usize>>,
    module_names: HashSet<String>,
}

impl LuaGlobalsAnalyzer {
    /// Creates a new analyzer.
    #[must_use]
    pub fn new() -> Self {
        let mut sets = HashSet::new();
        for &base in BASE_GLOBALS {
            sets.insert(base.to_string());
        }
        Self {
            reads: HashMap::new(),
            sets,
            positions: HashMap::new(),
            module_names: HashSet::new(),
        }
    }

    /// Registers a module name.
    pub fn add_module(&mut self, module_name: &str) {
        self.module_names.insert(module_name.to_string());
    }

    /// Scans a script using the native Lua lexer.
    pub fn scan_script(&mut self, file_name: &str, source: &[u8]) {
        let mut lexer = LuaLexer::new(source);
        let tokens = lexer.tokenize_all();

        let mut local_scopes: Vec<HashSet<String>> = vec![HashSet::new()];

        let mut i = 0;
        while i < tokens.len() {
            if let Some(tok) = tokens.get(i) {
                match &tok.kind {
                    TokenKind::Keyword(kw) => {
                        match kw.as_str() {
                            "local" => {
                                // Check local variables / function declarations
                                if let Some(next) = tokens.get(i.saturating_add(1)) {
                                    if next.kind == TokenKind::Keyword("function".to_string()) {
                                        if let Some(fn_tok) = tokens.get(i.saturating_add(2)) {
                                            if let TokenKind::Identifier(name) = &fn_tok.kind {
                                                if let Some(current) = local_scopes.last_mut() {
                                                    current.insert(name.clone());
                                                }
                                            }
                                        }
                                    } else {
                                        // local a, b, c = ...
                                        let mut j = i.saturating_add(1);
                                        while j < tokens.len() {
                                            if let Some(var_tok) = tokens.get(j) {
                                                if let TokenKind::Identifier(name) = &var_tok.kind {
                                                    if let Some(current) = local_scopes.last_mut() {
                                                        current.insert(name.clone());
                                                    }
                                                } else if var_tok.kind == TokenKind::Symbol(",".to_string()) {
                                                    j = j.saturating_add(1);
                                                    continue;
                                                } else {
                                                    break;
                                                }
                                            }
                                            j = j.saturating_add(1);
                                        }
                                    }
                                }
                            }
                            "function" => {
                                // Add new local scope for the function body
                                let mut fn_scope = HashSet::new();
                                // Check if global function definition: `function name(`
                                if let Some(fn_id_tok) = tokens.get(i.saturating_add(1)) {
                                    if let TokenKind::Identifier(name) = &fn_id_tok.kind {
                                        let is_method = matches!(
                                            tokens.get(i.saturating_add(2)).map(|t| &t.kind),
                                            Some(TokenKind::Symbol(s)) if s == "." || s == ":"
                                        );
                                        if !is_method {
                                            self.sets.insert(name.clone());
                                        }
                                    }
                                }
                                // Collect parameters between `(` and `)` into fn_scope
                                let mut p_idx = i.saturating_add(1);
                                while p_idx < tokens.len() {
                                    if let Some(TokenKind::Symbol(s)) = tokens.get(p_idx).map(|t| &t.kind) {
                                        if s == "(" {
                                            p_idx = p_idx.saturating_add(1);
                                            while p_idx < tokens.len() {
                                                match tokens.get(p_idx).map(|t| &t.kind) {
                                                    Some(TokenKind::Identifier(param)) => {
                                                        fn_scope.insert(param.clone());
                                                    }
                                                    Some(TokenKind::Symbol(s)) if s == ")" => {
                                                        break;
                                                    }
                                                    _ => {}
                                                }
                                                p_idx = p_idx.saturating_add(1);
                                            }
                                            break;
                                        }
                                    }
                                    p_idx = p_idx.saturating_add(1);
                                }
                                local_scopes.push(fn_scope);
                            }
                            "for" => {
                                let mut for_scope = HashSet::new();
                                let mut f_idx = i.saturating_add(1);
                                while f_idx < tokens.len() {
                                    match tokens.get(f_idx).map(|t| &t.kind) {
                                        Some(TokenKind::Identifier(var)) => {
                                            for_scope.insert(var.clone());
                                        }
                                        Some(TokenKind::Symbol(s)) if s == "=" || s == "in" => {
                                            break;
                                        }
                                        _ => {}
                                    }
                                    f_idx = f_idx.saturating_add(1);
                                }
                                local_scopes.push(for_scope);
                            }
                            "do" | "then" | "repeat" => {
                                local_scopes.push(HashSet::new());
                            }
                            "end" | "until" if local_scopes.len() > 1 => {
                                local_scopes.pop();
                            }
                            _ => {}
                        }
                    }
                    TokenKind::Identifier(ident) => {
                        // Check class "ClassName"
                        if ident == "class" {
                            if let Some(arg_tok) = tokens.get(i.saturating_add(1)) {
                                if let TokenKind::StringLiteral(c_name) = &arg_tok.kind {
                                    self.sets.insert(c_name.clone());
                                }
                            }
                        }

                        // Skip if preceded by '.' or ':' (field/method access)
                        let is_field = i > 0
                            && matches!(
                                tokens.get(i.saturating_sub(1)).map(|t| &t.kind),
                                Some(TokenKind::Symbol(s)) if s == "." || s == ":"
                            );
                        // Skip if function name directly following `function`
                        let is_fn_name = i > 0
                            && matches!(
                                tokens.get(i.saturating_sub(1)).map(|t| &t.kind),
                                Some(TokenKind::Keyword(kw)) if kw == "function"
                            );

                        if !is_field && !is_fn_name {
                            let is_local = local_scopes.iter().any(|scope| scope.contains(ident));
                            if !is_local {
                                // Check if assigned: ident = ...
                                let is_assigned = matches!(
                                    tokens.get(i.saturating_add(1)).map(|t| &t.kind),
                                    Some(TokenKind::Symbol(s)) if s == "="
                                );

                                if is_assigned {
                                    self.sets.insert(ident.clone());
                                } else {
                                    self.reads
                                        .entry(ident.clone())
                                        .or_default()
                                        .insert(file_name.to_string());
                                    self.positions
                                        .entry((ident.clone(), file_name.to_string()))
                                        .or_default()
                                        .push(tok.line);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            i = i.saturating_add(1);
        }
    }

    /// Evaluates collected variables and reports unassigned single-file globals.
    #[must_use]
    pub fn evaluate(self, max_files: usize) -> Vec<LintFinding> {
        let mut findings = Vec::new();
        let mut sorted_names: Vec<_> = self.reads.keys().cloned().collect();
        sorted_names.sort();

        for name in sorted_names {
            if self.sets.contains(&name) || self.module_names.contains(&name) {
                continue;
            }

            // Skip engine prefix conventions: DIK_, cse_, all uppercase constants
            if name.starts_with("DIK_")
                || name.starts_with("cse_")
                || (name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                    && name.len() > 1)
            {
                continue;
            }

            if let Some(files) = self.reads.get(&name) {
                if files.len() <= max_files {
                    for f in files {
                        let lines = self
                            .positions
                            .get(&(name.clone(), f.clone()))
                            .cloned()
                            .unwrap_or_default();
                        let first_line = lines.first().copied().unwrap_or(1);
                        findings.push(LintFinding {
                            checker: "lua_globals".to_string(),
                            file: f.clone(),
                            line: first_line,
                            severity: LintSeverity::Warning,
                            message: format!("undefined global '{name}'"),
                        });
                    }
                }
            }
        }

        findings
    }
}
