//! Checker for undefined or misspelled global variables in Lua scripts.
//!
//! Flags global variable reads that are never defined anywhere and are not
//! standard Lua globals, engine globals, or engine class names.

use crate::lexer::{LuaLexer, TokenKind};
use crate::models::{LintFinding, LintSeverity};
use std::collections::{HashMap, HashSet};

/// Standard Lua 5.1 globals always available in every interpreter.
const LUA_GLOBALS: &[&str] = &[
    "assert",
    "collectgarbage",
    "dofile",
    "error",
    "gcinfo",
    "getfenv",
    "getmetatable",
    "ipairs",
    "load",
    "loadfile",
    "loadstring",
    "module",
    "newproxy",
    "next",
    "pairs",
    "pcall",
    "print",
    "rawequal",
    "rawget",
    "rawset",
    "require",
    "select",
    "setfenv",
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
    "package",
    "_G",
    "_VERSION",
    "self",
];

/// X-Ray engine globals exposed to every game script.
///
/// These are C++ singletons and free functions registered in the engine's
/// global Lua environment.  They appear across all three games (SoC, CS, CoP).
///
/// Source: X-Ray engine bindings and official script headers.
const ENGINE_GLOBALS_ALL: &[&str] = &[
    // Singletons / managers
    "device",
    "level",
    "relation_registry",
    "system_ini",
    "game",
    "alife",
    // Free functions exported by the engine
    "abort",
    "bit_and",
    "bit_not",
    "bit_or",
    "bit_xor",
    "dik_to_bind",
    "get_console",
    "get_hud",
    "IsMonster",
    "IsStalker",
    "IsItem",
    "IsArtefact",
    "IsWeapon",
    "game_graph",
    "game_object",
    "ini_file",
    "object_binder",
    "patrol",
    "printf",
    // Math / geometry types
    "hit",
    "vector",
    "Frect",
    "Irect",
    "GetFontLetterica16Russian",
    "GetFontLetterica18Russian",
    "GetFontLetterica25",
    "GetFontSmall",
    "GetFontMedium",
    "GetFontBig",
    "GetFontGraffiti19Russian",
    "GetFontGraffiti22Russian",
    // Callbacks / infrastructure
    "callback",
    "level_changer",
    "stalker_ids",
    "xr_class_alias",
    "class",
];

/// Engine globals specific to Shadow of Chernobyl.
const ENGINE_GLOBALS_SOC: &[&str] = &[
    "actor_stats",
    "app_ready",
    "bind_restrictor",
    "death_manager",
    "game_stats",
    "has_alife_info",
    "packer",
    "result",
    "sim_board",
    "sim_squad_scripted",
    "sr_light",
    "sr_psy_antenna",
    "surge_manager",
    "task",
    "task_manager",
    "test",
    "this",
    "ui_load_dialog",
    "ui_main_menu",
    "ui_mm_opt_main",
    "ui_mp_main",
    "ui_save_dialog",
    "ui_spawn_dialog",
    "xr_detector",
    "xr_logic",
    "xr_motivator",
    "xr_sound",
    "xr_surge_hide",
    "get_object_squad",
    "get_object_story_id",
    "get_story_object",
    "level_tasks",
    "actor",
    "simulation_objects",
    "sim_squad_generic",
];

/// Engine globals specific to Clear Sky.
const ENGINE_GLOBALS_CS: &[&str] = &[];

/// Engine globals specific to Call of Pripyat.
const ENGINE_GLOBALS_COP: &[&str] = &[
    "actor",
    "simulation_objects",
    "sim_squad_generic",
    "ui_game_sp",
    "actor_stats",
    "game_stats",
    "sim_board",
    "surge_manager",
    "task_manager",
    "xr_logic",
    "xr_sound",
    "xr_detector",
    "bind_restrictor",
    "death_manager",
    "sr_light",
    "sr_psy_antenna",
    "get_object_squad",
    "get_object_story_id",
    "get_story_object",
];

/// Returns `true` for identifiers that match the X-Ray engine class naming
/// conventions and should never be reported as undefined globals.
///
/// Matches the reference `lua_globals.py` filter:
/// `re.match(r'^(C[A-Z]|DIK_|cse_|[A-Z][A-Z_0-9]+$)', name)`
fn is_engine_name_pattern(name: &str) -> bool {
    // DIK_ keycodes (e.g. DIK_SPACE)
    if name.starts_with("DIK_") {
        return true;
    }
    // cse_ prefixed C++ script-engine classes (cse_alife_object, etc.)
    if name.starts_with("cse_") {
        return true;
    }
    // C++ engine class names start with C then an uppercase letter
    // (CUIStatic, CUIWindow, CScriptXmlInit, CGameObject, ...)
    let mut chars = name.chars();
    if let (Some('C'), Some(second)) = (chars.next(), chars.next()) {
        if second.is_ascii_uppercase() {
            return true;
        }
    }
    // ALL_CAPS_CONSTANTS (e.g. LOST_SMALL_BANDIT_DIALOG, MEDKIT, ...)
    if name.len() > 1
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return true;
    }
    false
}

/// Collects globals analysis across a set of Lua scripts.
#[derive(Default)]
pub struct LuaGlobalsAnalyzer {
    reads: HashMap<String, HashSet<String>>,
    sets: HashSet<String>,
    positions: HashMap<(String, String), Vec<usize>>,
    module_names: HashSet<String>,
}

impl LuaGlobalsAnalyzer {
    /// Creates a new analyzer pre-seeded with standard Lua and engine globals.
    #[must_use]
    pub fn new() -> Self {
        let mut sets = HashSet::new();
        for &g in LUA_GLOBALS {
            sets.insert(g.to_string());
        }
        for &g in ENGINE_GLOBALS_ALL {
            sets.insert(g.to_string());
        }
        // Add game-specific lists; the full set is always loaded here so the
        // analyzer works without knowing which game the tree belongs to.
        // False negatives (suppressing a real typo that happens to match a
        // CoP-only name in a SoC install) are preferable to false positives.
        for &g in ENGINE_GLOBALS_SOC {
            sets.insert(g.to_string());
        }
        for &g in ENGINE_GLOBALS_CS {
            sets.insert(g.to_string());
        }
        for &g in ENGINE_GLOBALS_COP {
            sets.insert(g.to_string());
        }
        Self {
            reads: HashMap::new(),
            sets,
            positions: HashMap::new(),
            module_names: HashSet::new(),
        }
    }

    /// Registers a module name (the stem of a `.script` file).
    ///
    /// A global that equals a module name is never undefined — scripts access
    /// each other by their module name.
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
                                        // local a, b, c = ...\
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
                        // Collect class "ClassName" declarations
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

            // Skip engine naming conventions: CUI*, DIK_*, cse_*, ALL_CAPS
            // (matches the reference lua_globals.py pattern filter)
            if is_engine_name_pattern(&name) {
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
