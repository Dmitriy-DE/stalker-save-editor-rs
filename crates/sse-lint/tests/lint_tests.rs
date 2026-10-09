//! Integration tests for sse-lint crate.

#![allow(missing_docs, clippy::indexing_slicing, clippy::expect_used, clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use sse_content::{GameFile, GameFileTree};
use sse_core::Error;
use sse_lint::condfuncs::{check_condfuncs_line, extract_lua_function_defs};
use sse_lint::condlists::{check_condlist_line, check_condlists_text};
use sse_lint::dialogs::check_dialogs_xml;
use sse_lint::ee_diff::{diff_retail_and_ee, DiffHunk};
use sse_lint::globals::LuaGlobalsAnalyzer;
use sse_lint::infos::InfoPortionIndex;
use sse_lint::lexer::{is_keyword, LuaLexer, TokenKind};
use sse_lint::logic_refs::check_logic_refs_text;
use sse_lint::models::LintSeverity;
use sse_lint::module_calls::{analyze_script_module, check_script_module_calls, check_xml_module_refs};
use sse_lint::spawn_diff::diff_spawns;
use sse_lint::trade_items::check_trade_file;
use sse_lint::{LintEngine, LintOptions};

#[test]
fn test_lua_lexer_tokens_and_positions() {
    let code = b"local function calculate(a, b)\n  -- comment\n  local msg = [[hello world]]\n  return a + b\nend";
    let mut lexer = LuaLexer::new(code);
    let tokens = lexer.tokenize_all();

    assert_eq!(tokens[0].kind, TokenKind::Keyword("local".to_string()));
    assert_eq!(tokens[0].line, 1);
    assert_eq!(tokens[0].column, 1);

    assert_eq!(tokens[1].kind, TokenKind::Keyword("function".to_string()));
    assert_eq!(tokens[2].kind, TokenKind::Identifier("calculate".to_string()));

    let has_msg = tokens.iter().any(|t| match &t.kind {
        TokenKind::StringLiteral(s) => s == "hello world",
        _ => false,
    });
    assert!(has_msg, "Long bracket string must be scanned");
    assert!(is_keyword("function"));
    assert!(!is_keyword("my_custom_func"));
}

#[test]
fn test_check_condlists_valid_and_malformed() {
    // Valid lines
    assert!(check_condlist_line("test.ltx", 1, "on_info = {+actor_alive} walker@1").is_none());
    assert!(check_condlist_line(
        "test.ltx",
        2,
        "on_info = {=check_dist -actor_dead} walker@1 %run_effect%"
    )
    .is_none());
    assert!(check_condlist_line("test.ltx", 3, "; this is comment = {unclosed").is_none());

    // Nested {
    let f1 = check_condlist_line("test.ltx", 4, "on_info = {{bad}}").expect("Nested { must fail");
    assert!(f1.message.contains("nested {"));

    // Stray }
    let f2 = check_condlist_line("test.ltx", 5, "on_info = bad}").expect("Stray } must fail");
    assert!(f2.message.contains("stray }"));

    // Unclosed {
    let f3 = check_condlist_line("test.ltx", 6, "on_info = {unclosed walker").expect("Unclosed { must fail");
    assert!(f3.message.contains("unclosed {"));

    // % inside {}
    let f4 = check_condlist_line("test.ltx", 7, "on_info = {%bad%}").expect("% inside {} must fail");
    assert!(f4.message.contains("% inside {}"));

    // Unbalanced %
    let f5 = check_condlist_line("test.ltx", 8, "on_info = {good} %unbalanced").expect("Unbalanced % must fail");
    assert!(f5.message.contains("unbalanced %"));

    // Multi-line scan
    let multi = "on_info1 = {good}\non_info2 = {bad} }\non_info3 = %odd";
    let mut findings = Vec::new();
    check_condlists_text("logic.ltx", multi, &mut findings);
    assert_eq!(findings.len(), 2);
}

#[test]
fn test_check_condfuncs() {
    let xr_cond = b"function is_alive() end\nfunction has_weapon() end";
    let xr_eff = b"function give_reward() end\nfunction kill_actor() end";

    let conds = extract_lua_function_defs(xr_cond);
    let effs = extract_lua_function_defs(xr_eff);

    assert!(conds.contains("is_alive"));
    assert!(conds.contains("has_weapon"));
    assert!(effs.contains("give_reward"));

    let mut findings = Vec::new();
    check_condfuncs_line(
        "logic.ltx",
        10,
        "on_info = {=is_alive !unknown_cond} nil %=give_reward =unknown_eff%",
        &conds,
        &effs,
        &mut findings,
    );

    assert_eq!(findings.len(), 2);
    assert!(findings[0].message.contains("condition unknown_cond"));
    assert!(findings[1].message.contains("effect unknown_eff"));
}

#[test]
fn test_check_dialogs() {
    let valid_xml = r#"
        <game_dialogs>
            <dialog id="test_dialog">
                <phrase id="0">
                    <next>1</next>
                </phrase>
                <phrase id="1">
                </phrase>
            </dialog>
        </game_dialogs>
    "#;
    let mut findings = Vec::new();
    check_dialogs_xml("dialogs.xml", valid_xml, &mut findings);
    assert!(findings.is_empty(), "Valid dialog must produce zero findings");

    let invalid_xml = r#"
        <game_dialogs>
            <dialog id="broken_dialog">
                <phrase id="1">
                    <next>99</next>
                </phrase>
                <phrase id="1">
                </phrase>
            </dialog>
        </game_dialogs>
    "#;
    findings.clear();
    check_dialogs_xml("dialogs.xml", invalid_xml, &mut findings);
    // Expect: duplicate phrase id 1, no start phrase 0, next -> missing 99
    assert_eq!(findings.len(), 3);
    assert!(findings.iter().any(|f| f.message.contains("duplicate phrase id 1")));
    assert!(findings.iter().any(|f| f.message.contains("no start phrase 0")));
    assert!(findings.iter().any(|f| f.message.contains("missing 99")));
}

#[test]
fn test_check_logic_refs() {
    let ltx = r#"
[logic]
active = walker@my_walker

[walker@my_walker]
on_info = {+info} camper@missing_section

[remark@alone]
"#;
    let mut findings = Vec::new();
    check_logic_refs_text("logic.ltx", ltx, &mut findings);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("missing section [camper@missing_section]"));
}

#[test]
fn test_check_infos() {
    let mut index = InfoPortionIndex::new();
    index.feed_xml("<dialog><phrase><give_info>given_in_xml</give_info></phrase></dialog>");
    index.feed_ltx(
        "logic.ltx",
        "on_info = %+given_in_ltx%\non_info2 = {+given_in_xml +given_in_ltx +tested_but_missing}",
    );
    index.feed_script("if has_info('script_info') then end");

    let findings = index.evaluate();
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("tested_but_missing"));
}

#[test]
fn test_check_module_calls() {
    let bar_script = b"function bar_func() end";
    let foo_script = b"function test() bar.bar_func() bar.non_existent() end";

    let mut modules = HashMap::new();
    modules.insert("bar".to_string(), analyze_script_module("bar", bar_script));
    modules.insert("foo".to_string(), analyze_script_module("foo", foo_script));

    let mut findings = Vec::new();
    check_script_module_calls(&modules, &mut findings);

    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("bar.non_existent"));

    // XML reference
    let xml = "<action>bar.non_existent</action><action>bar.bar_func</action>";
    findings.clear();
    check_xml_module_refs("dialogs.xml", xml, &modules, &mut findings);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("bar.non_existent not defined"));
}

#[test]
fn test_check_trade_items() {
    let trade_ltx = r#"
[trader]
buy_condition = trade_generic_buy

[supplies]
medkit = 5, 0.8
unknown_super_item = 1, 1.0
"#;
    let mut known_sections = HashSet::new();
    known_sections.insert("medkit".to_string());

    let mut findings = Vec::new();
    check_trade_file("misc/trade_trader.ltx", trade_ltx, &known_sections, &mut findings);

    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("[supplies] unknown_super_item"));
}

#[test]
fn test_lua_globals_analyzer() {
    let mut analyzer = LuaGlobalsAnalyzer::new();
    analyzer.add_module("my_module");
    analyzer.scan_script("test1.script", b"function run() typo_global = 123 end");
    analyzer.scan_script("test2.script", b"function do_something() local x = typo_global end");

    // typo_global is set in test1, read in test2 -> assigned!
    let findings = analyzer.evaluate(1);
    assert!(findings.is_empty());

    let mut analyzer2 = LuaGlobalsAnalyzer::new();
    analyzer2.add_module("another_module");
    analyzer2.scan_script("test_unassigned.script", b"function foo() return unassigned_st end");
    let findings2 = analyzer2.evaluate(1);
    assert_eq!(findings2.len(), 1);
    assert!(findings2[0].message.contains("undefined global 'unassigned_st'"));
}

#[test]
fn test_ee_diff_ignores_whitespace_and_comments() {
    let retail = b"local x = 1\n-- comment\nlocal y = 2\n";
    let ee = b"local x = 1\nlocal y = 2 -- updated\n";

    let diff = diff_retail_and_ee("test.script", retail, ee);
    assert!(diff.is_empty(), "Comment and trailing changes should be ignored");

    let ee_changed = b"local x = 1\nlocal y = 3\n";
    let diff2 = diff_retail_and_ee("test.script", retail, ee_changed);
    assert_eq!(diff2.len(), 2);
    assert_eq!(diff2[0], DiffHunk::Removed("local y = 2".to_string()));
    assert_eq!(diff2[1], DiffHunk::Added("local y = 3".to_string()));
}

#[test]
fn test_spawn_diff() {
    // Test with two minimal dummy chunk buffers
    let buf1 = vec![0u8; 16];
    let buf2 = vec![0u8; 16];
    let res = diff_spawns(&buf1, &buf2);
    assert!(res.added_objects.is_empty());
    assert!(res.removed_objects.is_empty());
}

#[test]
fn test_lint_engine_speed_under_two_seconds() {
    // Build a synthetic game tree with 500 files
    let mut files = HashMap::new();
    for i in 0..250 {
        let path = format!("configs/logic_{i}.ltx");
        let content = format!("[sec_{i}]\non_info = {{+info_{i}}} walker@1\n").into_bytes();
        files.insert(path.clone(), GameFile::from_bytes(path, "test", content));
    }
    for i in 0..250 {
        let path = format!("scripts/mod_{i}.script");
        let content = format!("function fn_{i}() return {i} end\n").into_bytes();
        files.insert(path.clone(), GameFile::from_bytes(path, "test", content));
    }

    let tree = GameFileTree {
        files,
        fingerprint: "synth".to_string(),
        has_loose_overlay: true,
        config_prefix: "configs/".to_string(),
        data_directory: None,
        issues: Vec::new(),
    };

    let start = Instant::now();
    let engine = LintEngine::new(LintOptions::default());
    let report = engine.lint_tree(&tree);
    let elapsed = start.elapsed();

    assert_eq!(report.files_checked, 500);
    // Must be well under 2 seconds (usually ~10-30 ms in Rust)
    assert!(
        elapsed.as_secs() < 2,
        "Whole game must be checked in under 2 seconds, took {:?}",
        elapsed
    );
    println!("Checked 500 files in {:?}", elapsed);
}

#[test]
fn lint_engine_reports_read_failures_as_incomplete_analysis() {
    let mut files = HashMap::new();
    files.insert(
        "configs/oversized.ltx".to_string(),
        GameFile::new("configs/oversized.ltx", "test", || {
            Err(Error::Refused("File exceeds the 67108864-byte read limit".to_string()))
        }),
    );
    let tree = GameFileTree {
        files,
        fingerprint: "read-failure".to_string(),
        has_loose_overlay: true,
        config_prefix: "configs/".to_string(),
        data_directory: None,
        issues: Vec::new(),
    };

    let report = LintEngine::new(LintOptions::default()).lint_tree(&tree);

    assert_eq!(report.files_checked, 0);
    assert!(report.has_errors(), "an incomplete lint scan must be non-success");
    let finding = report.findings.first().expect("read failure finding");
    assert_eq!(finding.file, "configs/oversized.ltx");
    assert!(finding.message.contains("incomplete"));
}

#[test]
fn lint_engine_reports_file_tree_issues_as_incomplete_analysis() {
    let tree = GameFileTree {
        files: HashMap::new(),
        fingerprint: "tree-issue".to_string(),
        has_loose_overlay: false,
        config_prefix: "configs/".to_string(),
        data_directory: None,
        issues: vec!["Could not read fsgame.ltx: File exceeds the read limit".to_string()],
    };

    let report = LintEngine::new(LintOptions::default()).lint_tree(&tree);

    assert!(report.has_errors(), "tree discovery issues must be non-success");
    let finding = report.findings.first().expect("tree issue finding");
    assert_eq!(finding.file, "fsgame.ltx");
    assert!(finding.message.contains("incomplete"));
}

#[test]
fn test_fix_regress_simulation() {
    // Simulates fix_regress: verifies that patching fixes issues rather than adding new ones
    let orig_ltx = b"[logic]\non_info = {{broken_nested}}\n";
    let fixed_ltx = b"[logic]\non_info = {broken_fixed}\n";

    let mut orig_files = HashMap::new();
    orig_files.insert(
        "configs/logic.ltx".to_string(),
        GameFile::from_bytes("configs/logic.ltx", "test", orig_ltx.to_vec()),
    );
    let orig_tree = GameFileTree {
        files: orig_files,
        fingerprint: "orig".to_string(),
        has_loose_overlay: true,
        config_prefix: "configs/".to_string(),
        data_directory: None,
        issues: Vec::new(),
    };

    let mut fixed_files = HashMap::new();
    fixed_files.insert(
        "configs/logic.ltx".to_string(),
        GameFile::from_bytes("configs/logic.ltx", "test", fixed_ltx.to_vec()),
    );
    let fixed_tree = GameFileTree {
        files: fixed_files,
        fingerprint: "fixed".to_string(),
        has_loose_overlay: true,
        config_prefix: "configs/".to_string(),
        data_directory: None,
        issues: Vec::new(),
    };

    let engine = LintEngine::new(LintOptions::default());
    let report_orig = engine.lint_tree(&orig_tree);
    let report_fixed = engine.lint_tree(&fixed_tree);

    assert_eq!(report_orig.count_by_severity(LintSeverity::Error), 1);
    assert_eq!(report_fixed.count_by_severity(LintSeverity::Error), 0);
}

#[test]
fn test_lua_globals_engine_exports() {
    let script = br#"
function test_engine_calls()
    local wnd = CUIWindow()
    local st = CUIStatic()
    local box = CUIMessageBoxEx()
    local item = CUIListBoxItem()
    local rect = Frect()
    printf("hello %s", "world")
    abort("fatal error")
    if IsMonster(obj) then
        return true
    end
    local ini = ini_file("system.ltx")
    -- An actual undefined global
    bogus_unknown_variable_xyz = undefined_var_123
end
"#;

    let mut files = HashMap::new();
    files.insert(
        "scripts/test.script".to_string(),
        GameFile::from_bytes("scripts/test.script", "test", script.to_vec()),
    );
    let tree = GameFileTree {
        files,
        fingerprint: "test".to_string(),
        has_loose_overlay: true,
        config_prefix: "configs/".to_string(),
        data_directory: None,
        issues: Vec::new(),
    };

    let engine = LintEngine::new(LintOptions {
        single_checker: Some("lua_globals".to_string()),
        ..LintOptions::default()
    });
    let report = engine.lint_tree(&tree);

    // Only undefined_var_123 should be flagged; CUIWindow, CUIStatic, CUIMessageBoxEx,
    // CUIListBoxItem, Frect, printf, abort, IsMonster, ini_file must NOT be flagged.
    let undefined_names: Vec<&str> = report
        .findings
        .iter()
        .map(|f| {
            f.message
                .strip_prefix("undefined global '")
                .and_then(|s| s.strip_suffix('\''))
                .unwrap_or(&f.message)
        })
        .collect();

    assert!(
        !undefined_names.contains(&"CUIWindow"),
        "CUIWindow should not be reported"
    );
    assert!(
        !undefined_names.contains(&"CUIStatic"),
        "CUIStatic should not be reported"
    );
    assert!(
        !undefined_names.contains(&"CUIMessageBoxEx"),
        "CUIMessageBoxEx should not be reported"
    );
    assert!(
        !undefined_names.contains(&"CUIListBoxItem"),
        "CUIListBoxItem should not be reported"
    );
    assert!(!undefined_names.contains(&"Frect"), "Frect should not be reported");
    assert!(!undefined_names.contains(&"printf"), "printf should not be reported");
    assert!(!undefined_names.contains(&"abort"), "abort should not be reported");
    assert!(
        !undefined_names.contains(&"IsMonster"),
        "IsMonster should not be reported"
    );
    assert!(
        !undefined_names.contains(&"ini_file"),
        "ini_file should not be reported"
    );
    assert!(
        undefined_names.contains(&"undefined_var_123"),
        "undefined_var_123 must be reported"
    );
}
