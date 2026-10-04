//! Checker for references to logic sections that do not exist in the same `.ltx`.
//!
//! Missing section in logic causes X-Ray engine crashes: `"section ... not found"`.

use crate::models::{LintFinding, LintSeverity};
use std::collections::HashSet;

/// Known X-Ray logic schemes.
pub const LOGIC_SCHEMES: &[&str] = &[
    "sr_idle",
    "sr_timer",
    "sr_cutscene",
    "sr_teleport",
    "sr_light",
    "sr_particle",
    "sr_sound",
    "sr_postprocess",
    "sr_psy_antenna",
    "sr_no_weapon",
    "sr_deimos",
    "walker",
    "remark",
    "camper",
    "sleeper",
    "animpoint",
    "mob_home",
    "mob_walker",
    "mob_combat",
    "mob_remark",
    "mob_jump",
    "mob_death",
    "mob_trader",
    "combat",
    "combat_ignore",
    "hit",
    "death",
    "meet",
    "wounded",
    "ph_idle",
    "ph_door",
    "ph_button",
    "ph_hit",
    "ph_on_hit",
    "ph_code",
    "ph_sound",
    "ph_force",
    "ph_minigun",
    "ph_car",
    "heli_move",
    "kamp",
    "patrol",
    "companion",
    "smartcover",
    "cover",
    "sr_monster",
    "sr_robbery",
    "sr_bloodsucker",
    "sr_crow_spawner",
    "sr_squad_guider",
    "dialog",
    "sr_silence",
    "invulnerable",
];

/// Checks if a key name is a logic transition or state handler.
fn is_logic_key(key: &str) -> bool {
    key.starts_with("on_")
        || matches!(
            key,
            "active"
                | "combat_ignore"
                | "on_hit"
                | "on_death"
                | "meet"
                | "wounded"
                | "dialog"
                | "on_info"
                | "on_signal"
                | "on_timer"
                | "on_game_timer"
                | "on_actor_inside"
                | "on_actor_outside"
        )
}

/// Collects all sections defined in the LTX text.
fn extract_sections(text: &str) -> HashSet<String> {
    let mut sections = HashSet::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if let Some(end) = trimmed.find(']') {
                if let Some(sec) = trimmed.get(1..end) {
                    sections.insert(sec.trim().to_string());
                }
            }
        }
    }
    sections
}

/// Finds scheme references with `@section` in a value string.
fn extract_scheme_refs(val: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let bytes = val.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Find word boundary or start
        let prev_ok = if i == 0 {
            true
        } else {
            let prev = bytes.get(i.saturating_sub(1)).copied().unwrap_or(0);
            !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'@'
        };

        if prev_ok {
            for &scheme in LOGIC_SCHEMES {
                let sb = scheme.as_bytes();
                let end = i.saturating_add(sb.len());
                if bytes.get(i..end) == Some(sb) {
                    // Check next character: could be `@` followed by section identifier
                    if let Some(&b'@') = bytes.get(end) {
                        let mut ref_end = end.saturating_add(1);
                        while ref_end < bytes.len() {
                            let c = bytes.get(ref_end).copied().unwrap_or(0);
                            if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' {
                                ref_end = ref_end.saturating_add(1);
                            } else {
                                break;
                            }
                        }
                        // Check boundary after reference
                        let after_ok = if ref_end >= bytes.len() {
                            true
                        } else {
                            let c = bytes.get(ref_end).copied().unwrap_or(0);
                            c.is_ascii_whitespace() || c == b',' || c == b'%' || c == b'}' || c == b';'
                        };
                        if after_ok && ref_end > end.saturating_add(1) {
                            if let Some(sub) = val.get(i..ref_end) {
                                refs.push(sub.to_string());
                            }
                        }
                    }
                }
            }
        }
        i = i.saturating_add(1);
    }

    refs
}

/// Runs logic references check on an `.ltx` file with optional external/global sections.
pub fn check_logic_refs_with_known(
    path: &str,
    text: &str,
    known_sections: Option<&HashSet<String>>,
    findings: &mut Vec<LintFinding>,
) {
    let sections = extract_sections(text);

    for (line_idx, line) in text.lines().enumerate() {
        let line_num = line_idx.saturating_add(1);
        let body = line.split(';').next().unwrap_or("");
        let trimmed = body.trim();
        if !trimmed.contains('=') || trimmed.starts_with('[') {
            continue;
        }

        let Some((key_part, val_part)) = body.split_once('=') else {
            continue;
        };
        let key = key_part.trim();
        if !is_logic_key(key) {
            continue;
        }

        for r in extract_scheme_refs(val_part) {
            if r.contains('@') {
                let found = sections.contains(&r) || known_sections.is_some_and(|k| k.contains(&r.to_ascii_lowercase()));
                if !found {
                    let snippet = trimmed.chars().take(140).collect::<String>();
                    findings.push(LintFinding {
                        checker: "check_logic_refs".to_string(),
                        file: path.to_string(),
                        line: line_num,
                        severity: LintSeverity::Error,
                        message: format!("missing section [{r}]  <- {snippet}"),
                    });
                }
            }
        }
    }
}

/// Runs logic references check on an `.ltx` file.
pub fn check_logic_refs_text(path: &str, text: &str, findings: &mut Vec<LintFinding>) {
    check_logic_refs_with_known(path, text, None, findings);
}
