//! Resolution order for user-facing entity names.

use crate::bundle::CatalogBundleReader;
use crate::i18n::I18nService;
use crate::official_names::OfficialNamesCatalog;

/// Name resolution utilities following the C# editor's priority chain.
pub struct SaveNaming;

impl SaveNaming {
    /// Resolves an item's display name following the priority:
    /// 1. Official string tables in the interface language.
    /// 2. Installed game's own metadata (`installed_name`).
    /// 3. Embedded shipped catalog.
    /// 4. Raw item section key.
    #[must_use]
    pub fn item_name(release_id: &str, key: &str, installed_name: Option<&str>) -> String {
        Self::item_name_for(&Self::names_language(), release_id, key, installed_name)
    }

    /// Same chain as [`SaveNaming::item_name`] for an explicit language code (e.g. "en", "zh_CN").
    #[must_use]
    pub fn item_name_for(lang: &str, release_id: &str, key: &str, installed_name: Option<&str>) -> String {
        let official_catalog = OfficialNamesCatalog::load_embedded();

        if let Some(official) = official_catalog.resolve(Some(release_id), "items", Some(key), Some(lang)) {
            return official;
        }

        // Installed and shipped names come from the game's own (Russian) text. Only a Russian interface may show them;
        // other languages fall back to English and then to the raw key.
        if lang != "ru" {
            return key.to_string();
        }

        if let Some(installed) = installed_name {
            if !installed.trim().is_empty() {
                return installed.to_string();
            }
        }

        if let Some(bundle) = CatalogBundleReader::load_embedded().get(release_id) {
            if let Some(item) = bundle.items.resolve(key) {
                if let Some(ref dn) = item.display_name {
                    if !dn.trim().is_empty() {
                        return dn.clone();
                    }
                }
            }
        }

        key.to_string()
    }

    /// Resolves an upgrade's display name:
    /// 1. Official string tables in the interface language.
    /// 2. Provided fallback name.
    /// 3. Raw upgrade key.
    #[must_use]
    pub fn upgrade_name(release_id: Option<&str>, key: &str, fallback: Option<&str>) -> String {
        let lang = Self::names_language();
        let official_catalog = OfficialNamesCatalog::load_embedded();

        if let Some(official) = official_catalog.resolve(release_id, "upgrades", Some(key), Some(&lang)) {
            return official;
        }

        if let Some(fb) = fallback {
            if !fb.trim().is_empty() {
                return fb.to_string();
            }
        }

        key.to_string()
    }

    /// Current interface language formatted for catalog keys (e.g. "zh_CN", "pt_BR").
    #[must_use]
    pub fn names_language() -> String {
        I18nService::instance().current_language().replace('-', "_")
    }
}

#[cfg(test)]
mod tests {
    use super::SaveNaming;

    const LANGUAGES: [&str; 15] = [
        "ru", "uk", "en", "de", "fr", "it", "es", "pl", "cs", "pt_BR", "tr", "ja", "ko", "zh_CN", "zh_TW",
    ];
    const GAMES: [&str; 6] = [
        "stalker-soc",
        "stalker-cs",
        "stalker-cop",
        "stalker-soc-ee",
        "stalker-cs-ee",
        "stalker-cop-ee",
    ];

    fn has_cyrillic(text: &str) -> bool {
        text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
    }

    #[test]
    fn english_interface_names_a_pda_in_english_not_russian() {
        assert_eq!(SaveNaming::item_name_for("en", "stalker-cs", "device_pda", None), "PDA");
    }

    #[test]
    fn non_russian_interfaces_never_show_russian_game_names() {
        for game in GAMES {
            for lang in LANGUAGES.iter().filter(|l| **l != "ru" && **l != "uk") {
                // The official table has every trilogy item in English, so the name is never the Russian one.
                let pda = SaveNaming::item_name_for(lang, game, "device_pda", Some("КПК"));
                assert!(!has_cyrillic(&pda), "{game}/{lang}: {pda}");
                // A key absent from every table falls back to the raw key, not to the game's Russian name.
                let unknown = SaveNaming::item_name_for(lang, game, "matras", Some("Матрас"));
                assert_eq!(unknown, "matras", "{game}/{lang}");
            }
        }
    }

    #[test]
    fn russian_interface_keeps_the_game_name_for_keys_without_a_table_entry() {
        assert_eq!(
            SaveNaming::item_name_for("ru", "stalker-cs", "matras", Some("Матрас")),
            "Матрас"
        );
    }
}
