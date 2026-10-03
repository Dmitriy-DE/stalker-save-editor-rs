//! Level, stash, and object naming according to game conventions.

use crate::i18n::I18nService;
use crate::official_names::OfficialNamesCatalog;

const FAMILIES: &[&str] = &["stalker-cop", "stalker-cs", "stalker-soc"];

const SAME_LEVEL: &[(&str, &str)] = &[("agroprom_underground", "l03u_agr_underground")];

const PERSONAL_BOXES: &[(&str, &str)] = &[
    ("zat_a2_actor_treasure", "Личный ящик на «Скадовске»"),
    ("jup_b202_actor_treasure", "Личный ящик на «Янове»"),
    ("pri_a16_actor_treasure", "Личный ящик в прачечной"),
];

const PREFIX_LEVELS: &[(&str, &[&str])] = &[
    ("esc", &["escape", "l01_escape"]),
    ("gar", &["garbage", "l02_garbage"]),
    ("agr", &["agroprom", "l03_agroprom"]),
    ("agru", &["l03u_agr_underground"]),
    ("val", &["darkvalley", "l04_darkvalley"]),
    ("x18", &["l04u_labx18"]),
    ("bar", &["l05_bar"]),
    ("ros", &["l06_rostok"]),
    ("mil", &["military", "l07_military"]),
    ("yan", &["yantar", "l08_yantar"]),
    ("rad", &["l10_radar"]),
    ("mar", &["marsh"]),
    ("red", &["red_forest"]),
    ("lim", &["limansk"]),
    ("hos", &["hospital"]),
    ("katacomb", &["katacomb"]),
    ("aes", &["stancia_2", "l12_stancia"]),
    ("zat", &["zaton"]),
    ("zaton", &["zaton"]),
    ("jup", &["jupiter"]),
    ("pas", &["jupiter_underground"]),
    ("labx8", &["labx8"]),
    ("pri", &["pripyat", "l11_pripyat"]),
    ("pripyat", &["pripyat", "l11_pripyat"]),
];

/// Formats level and stash names as the games show them.
pub struct PlaceNames;

impl PlaceNames {
    /// Formats a level's name in the active language, falling back to raw ID.
    #[must_use]
    pub fn level(release_id: Option<&str>, level_id: Option<&str>) -> String {
        let raw = match level_id {
            Some(s) if !s.trim().is_empty() => s.trim(),
            _ => return I18nService::instance().tr("Неизвестно", &[]),
        };

        let id = raw.to_ascii_lowercase();
        if let Some(official_name) = official(release_id, "levels", &id) {
            return official_name;
        }

        for &(from, to) in SAME_LEVEL {
            if from == id {
                if let Some(other_name) = official(release_id, "levels", to) {
                    return other_name;
                }
            }
        }

        raw.to_string()
    }

    /// Determines the level an object belongs to by its name prefix.
    #[must_use]
    pub fn level_of_object(release_id: Option<&str>, object_name: Option<&str>) -> Option<String> {
        let name = object_name?.trim();
        if name.is_empty() {
            return None;
        }
        let prefix = name.split('_').next()?.to_ascii_lowercase();

        for &(pfx, ids) in PREFIX_LEVELS {
            if pfx == prefix {
                for &id in ids {
                    if let Some(official_name) = official(release_id, "levels", id) {
                        return Some(official_name);
                    }
                }
            }
        }

        None
    }

    /// Formats a stash box name by its official name, base container kind, or handle.
    #[must_use]
    pub fn stash(release_id: Option<&str>, object_name: Option<&str>, handle: u16) -> String {
        let name_str = match object_name {
            Some(s) if !s.trim().is_empty() => s.trim(),
            _ => {
                let hex_handle = format!("{handle:04X}");
                return I18nService::instance().tr("Тайник 0x{0}", &[&hex_handle]);
            }
        };

        let lower = name_str.to_ascii_lowercase();
        if let Some(official_name) = official(release_id, "stashes", &lower) {
            return official_name;
        }

        for &(box_id, title) in PERSONAL_BOXES {
            if box_id == lower {
                return I18nService::instance().tr(title, &[]);
            }
        }

        if lower.ends_with("_actor_treasure") {
            return I18nService::instance().tr("Личный ящик", &[]);
        }

        let num_opt = trailing_number(&lower);

        if lower.contains("smart_terrain") {
            let camp = camp_number(&lower);
            return I18nService::instance().tr("Ящик лагеря {0}", &[&camp]);
        }

        if lower.contains("_treasure") || lower.contains("_secret") {
            return match num_opt {
                Some(num) => I18nService::instance().tr("Тайник № {0}", &[&num]),
                None => I18nService::instance().tr("Тайник", &[]),
            };
        }

        if lower.contains("inventory_box") || lower.contains("inv_box") {
            return match num_opt {
                Some(num) => I18nService::instance().tr("Ящик № {0}", &[&num]),
                None => I18nService::instance().tr("Ящик", &[]),
            };
        }

        I18nService::instance().tr("Контейнер «{0}»", &[&name_str])
    }
}

fn official(release_id: Option<&str>, kind: &str, key: &str) -> Option<String> {
    let language = I18nService::instance().current_language().replace('-', "_");
    let official_catalog = OfficialNamesCatalog::load_embedded();

    if let Some(name) = official_catalog.resolve(release_id, kind, Some(key), Some(&language)) {
        return Some(name);
    }

    for &family in FAMILIES {
        if let Some(other) = official_catalog.resolve(Some(family), kind, Some(key), Some(&language)) {
            return Some(other);
        }
    }

    None
}

fn trailing_number(name: &str) -> Option<String> {
    let bytes = name.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes.get(end.saturating_sub(1)).is_some_and(u8::is_ascii_digit) {
        end = end.saturating_sub(1);
    }
    if end == bytes.len() {
        return None;
    }

    let digits_part = name.get(end..)?;
    let prev_char = if end > 0 {
        bytes.get(end.saturating_sub(1)).copied()
    } else {
        None
    };

    if prev_char == Some(b'_') && (bytes.len().saturating_sub(end)) == 4 {
        if let Ok(num) = digits_part.parse::<i32>() {
            return Some((num.saturating_add(1)).to_string());
        }
    }

    let stripped = digits_part.trim_start_matches('0');
    if stripped.is_empty() {
        Some("0".to_string())
    } else {
        Some(stripped.to_string())
    }
}

fn camp_number(name: &str) -> String {
    let marker = "smart_terrain_";
    let start = match name.find(marker) {
        Some(idx) => idx.saturating_add(marker.len()),
        None => 0,
    };
    let rest = name.get(start..).unwrap_or(name);
    let trimmed = rest.strip_suffix("_box").unwrap_or(rest);
    trimmed.replace('_', "-")
}
