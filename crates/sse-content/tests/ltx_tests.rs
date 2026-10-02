//! LTX document parser and resolver tests.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::collections::HashMap;

use sse_content::ltx::LtxDocument;

#[test]
fn parses_sections_inheritance_comments_and_bare_entries() {
    let sections = LtxDocument::parse(
        "[base]\ninv_weight = 1.5 ; comment\nclass = II_ANTIR\n[medkit]:base\ninv_name = st_medkit\ninv_weight=0,5\n[list]\nfoo\nbar ; x\n",
        "configs/misc/items.ltx",
    );

    let resolved_list = LtxDocument::resolve(&sections);
    let resolved: HashMap<String, HashMap<String, String>> =
        resolved_list.into_iter().map(|(sec, vals)| (sec.name, vals)).collect();

    assert_eq!(resolved["medkit"]["inv_weight"], "0,5");
    assert_eq!(resolved["medkit"]["class"], "II_ANTIR");
    assert_eq!(sections["list"].entries, vec!["foo", "bar"]);
}

#[test]
fn include_graph_ignores_ltx_files_the_engine_does_not_include() {
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    files.insert(
        "configs/system.ltx".to_string(),
        b"#include \"misc\\items.ltx\"\n#include \"weapons\\*.ltx\"\n".to_vec(),
    );
    files.insert(
        "configs/misc/items.ltx".to_string(),
        b"[medkit]\ninv_name = st_medkit\n".to_vec(),
    );
    files.insert(
        "configs/weapons/w_pm.ltx".to_string(),
        b"[wpn_pm]\nclass = WP_PM\ninv_name = st_pm\n".to_vec(),
    );
    files.insert(
        "configs/misc/notepad.ltx".to_string(),
        b"[medkit]\nnote = not an item\n".to_vec(),
    );

    let sections = LtxDocument::parse_include_graph("configs/system.ltx", &files, |v| Some(v.clone())).unwrap();

    assert_eq!(sections["medkit"].values["inv_name"], "st_medkit");
    assert!(sections.contains_key("wpn_pm"));
    assert!(!sections["medkit"].values.contains_key("note"));
}

#[test]
fn an_ltx_inheritance_cycle_resolves_the_same_way_in_any_section_order() {
    let case1 = "[a]:b\nx = 1\n[b]:a\ny = 2\n";
    let sections1 = LtxDocument::parse(case1, "test.ltx");
    let resolved1: HashMap<String, HashMap<String, String>> = LtxDocument::resolve(&sections1)
        .into_iter()
        .map(|(sec, vals)| (sec.name, vals))
        .collect();

    assert_eq!(resolved1["a"]["x"], "1");
    assert_eq!(resolved1["a"]["y"], "2");
    assert_eq!(resolved1["b"]["x"], "1");
    assert_eq!(resolved1["b"]["y"], "2");

    let case2 = "[b]:a\ny = 2\n[a]:b\nx = 1\n";
    let sections2 = LtxDocument::parse(case2, "test.ltx");
    let resolved2: HashMap<String, HashMap<String, String>> = LtxDocument::resolve(&sections2)
        .into_iter()
        .map(|(sec, vals)| (sec.name, vals))
        .collect();

    assert_eq!(resolved2["a"]["x"], "1");
    assert_eq!(resolved2["a"]["y"], "2");
    assert_eq!(resolved2["b"]["x"], "1");
    assert_eq!(resolved2["b"]["y"], "2");
}

#[test]
fn include_masks_match_like_a_simple_glob() {
    let cases = [
        ("weapons.ltx", "*.ltx", true),
        ("WEAPONS.LTX", "*.ltx", true),
        ("w_ak74.ltx", "w_*.ltx", true),
        ("w_ak74_up.ltx", "w_*_up.ltx", true),
        ("w_ak74.ltx", "w_*_up.ltx", false),
        ("weapons.ltx", "weapons.ltx", true),
        ("weapons.ltx.bak", "*.ltx", false),
        ("", "*", true),
        ("a", "", false),
    ];

    for (name, mask, expected) in cases {
        assert_eq!(
            LtxDocument::matches_mask(name, mask),
            expected,
            "Failed mask match for name='{name}' mask='{mask}'"
        );
    }
}

#[test]
fn wildcard_includes_take_only_the_files_of_their_own_directory() {
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    files.insert(
        "configs/system.ltx".to_string(),
        b"#include \"weapons\\w_*.ltx\"\n#include \"MISC\\Items.ltx\"\n".to_vec(),
    );
    files.insert("configs/weapons/w_b.ltx".to_string(), b"[b]\nx = 1\n".to_vec());
    files.insert("configs/weapons/w_a.ltx".to_string(), b"[a]\nx = 1\n".to_vec());
    files.insert("configs/weapons/other.ltx".to_string(), b"[other]\nx = 1\n".to_vec());
    files.insert("configs/weapons/deep/w_c.ltx".to_string(), b"[deep]\nx = 1\n".to_vec());
    files.insert("configs/misc/items.ltx".to_string(), b"[item]\nx = 1\n".to_vec());

    let sections = LtxDocument::parse_include_graph("configs/system.ltx", &files, |v| Some(v.clone())).unwrap();

    let mut keys: Vec<String> = sections.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["a", "b", "item"]);
}
