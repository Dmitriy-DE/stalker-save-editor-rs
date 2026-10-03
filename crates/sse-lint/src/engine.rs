//! Static analysis engine orchestrator.
//!
//! Executes checkers across game files from loose directories or [`GameFileTree`].

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::condfuncs::{check_condfuncs_line, extract_lua_function_defs};
use crate::condlists::check_condlists_text;
use crate::dialogs::check_all_dialogs;
use crate::globals::LuaGlobalsAnalyzer;
use crate::infos::InfoPortionIndex;
use crate::logic_refs::check_logic_refs_text;
use crate::models::LintReport;
use crate::module_calls::{analyze_script_module, check_script_module_calls, check_xml_module_refs, ScriptModuleInfo};
use crate::trade_items::check_trade_file;
use sse_content::GameFileTree;

/// Options configuring static analysis execution.
#[derive(Debug, Clone, Default)]
pub struct LintOptions {
    /// Specific checker to run (or `None` for all checkers)
    pub single_checker: Option<String>,
    /// Config subfolder name (default `"configs"` or `"config"`)
    pub config_subdir: Option<String>,
    /// Max file occurrence threshold for single-file unassigned globals (default 1)
    pub max_files_globals: Option<usize>,
}

/// Orchestrates static checks over a game file tree.
pub struct LintEngine {
    options: LintOptions,
}

impl LintEngine {
    /// Creates a new lint engine with the given options.
    #[must_use]
    pub fn new(options: LintOptions) -> Self {
        Self { options }
    }

    /// Runs all enabled checkers across a [`GameFileTree`].
    #[must_use]
    pub fn lint_tree(&self, tree: &GameFileTree) -> LintReport {
        let start = Instant::now();
        let mut findings = Vec::new();

        let checker_filter = self.options.single_checker.as_deref();

        // 1. Gather all file contents
        let mut ltx_files: HashMap<String, String> = HashMap::new();
        let mut xml_files: HashMap<String, String> = HashMap::new();
        let mut script_files: HashMap<String, Vec<u8>> = HashMap::new();
        let mut config_sections: HashSet<String> = HashSet::new();

        let mut files_checked: usize = 0;

        for (rel_path, file) in &tree.files {
            let lower = rel_path.to_ascii_lowercase();
            if lower.ends_with(".ltx") {
                if let Ok(bytes) = file.read() {
                    files_checked = files_checked.saturating_add(1);
                    let text = sse_content::decode_windows_1251(&bytes);
                    // Collect section headers for trade checker
                    for line in text.lines() {
                        let trimmed = line.trim();
                        if trimmed.starts_with('[') {
                            if let Some(end) = trimmed.find(']') {
                                if let Some(sec) = trimmed.get(1..end) {
                                    config_sections.insert(sec.trim().to_ascii_lowercase());
                                }
                            }
                        }
                    }
                    ltx_files.insert(rel_path.clone(), text);
                }
            } else if lower.ends_with(".xml") {
                if let Ok(bytes) = file.read() {
                    files_checked = files_checked.saturating_add(1);
                    let text = sse_content::decode_windows_1251(&bytes);
                    xml_files.insert(rel_path.clone(), text);
                }
            } else if lower.ends_with(".script") {
                if let Ok(bytes) = file.read() {
                    files_checked = files_checked.saturating_add(1);
                    script_files.insert(rel_path.clone(), bytes);
                }
            }
        }

        // --- CHECKER 1: check_condlists ---
        if checker_filter.is_none() || checker_filter == Some("check_condlists") {
            for (path, text) in &ltx_files {
                check_condlists_text(path, text, &mut findings);
            }
        }

        // --- CHECKER 2: check_logic_refs ---
        if checker_filter.is_none() || checker_filter == Some("check_logic_refs") {
            for (path, text) in &ltx_files {
                check_logic_refs_text(path, text, &mut findings);
            }
        }

        // --- CHECKER 3: check_dialogs ---
        if checker_filter.is_none() || checker_filter == Some("check_dialogs") {
            let files: Vec<(&str, &str)> = xml_files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
            check_all_dialogs(&files, &mut findings);
        }

        // --- CHECKER 4: check_condfuncs ---
        if checker_filter.is_none() || checker_filter == Some("check_condfuncs") {
            let xr_cond_bytes = script_files.get("scripts/xr_conditions.script");
            let xr_eff_bytes = script_files.get("scripts/xr_effects.script");

            let conditions = xr_cond_bytes.map(|b| extract_lua_function_defs(b)).unwrap_or_default();
            let effects = xr_eff_bytes.map(|b| extract_lua_function_defs(b)).unwrap_or_default();

            for (path, text) in &ltx_files {
                for (idx, line) in text.lines().enumerate() {
                    let line_num = idx.saturating_add(1);
                    check_condfuncs_line(path, line_num, line, &conditions, &effects, &mut findings);
                }
            }
        }

        // --- CHECKER 5: check_infos ---
        if checker_filter.is_none() || checker_filter == Some("check_infos") {
            let mut info_index = InfoPortionIndex::new();
            for (path, text) in &ltx_files {
                info_index.feed_ltx(path, text);
            }
            for text in xml_files.values() {
                info_index.feed_xml(text);
            }
            for bytes in script_files.values() {
                let text = sse_content::decode_windows_1251(bytes);
                info_index.feed_script(&text);
            }
            findings.extend(info_index.evaluate());
        }

        // --- CHECKER 6: check_module_calls ---
        if checker_filter.is_none() || checker_filter == Some("check_module_calls") {
            let mut modules: HashMap<String, ScriptModuleInfo> = HashMap::new();
            for (path, bytes) in &script_files {
                let mod_name = path
                    .strip_prefix("scripts/")
                    .unwrap_or(path)
                    .strip_suffix(".script")
                    .unwrap_or(path);
                let info = analyze_script_module(mod_name, bytes);
                modules.insert(mod_name.to_string(), info);
            }

            check_script_module_calls(&modules, &mut findings);

            for (path, text) in &xml_files {
                check_xml_module_refs(path, text, &modules, &mut findings);
            }
        }

        // --- CHECKER 7: check_trade_items ---
        if checker_filter.is_none() || checker_filter == Some("check_trade_items") {
            for (path, text) in &ltx_files {
                let lower = path.to_ascii_lowercase();
                if lower.contains("trade") && lower.contains("misc") {
                    check_trade_file(path, text, &config_sections, &mut findings);
                }
            }
        }

        // --- CHECKER 8: lua_globals ---
        if checker_filter.is_none() || checker_filter == Some("lua_globals") {
            let mut globals_analyzer = LuaGlobalsAnalyzer::new();
            for path in script_files.keys() {
                let mod_name = path
                    .strip_prefix("scripts/")
                    .unwrap_or(path)
                    .strip_suffix(".script")
                    .unwrap_or(path);
                globals_analyzer.add_module(mod_name);
            }
            for (path, bytes) in &script_files {
                if path.ends_with("lua_help.script") {
                    continue;
                }
                globals_analyzer.scan_script(path, bytes);
            }
            let max_files = self.options.max_files_globals.unwrap_or(1);
            findings.extend(globals_analyzer.evaluate(max_files));
        }

        let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        LintReport {
            files_checked,
            elapsed_ms,
            findings,
        }
    }
}
