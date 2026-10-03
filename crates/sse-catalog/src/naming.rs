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
        let lang = Self::names_language();
        let official_catalog = OfficialNamesCatalog::load_embedded();

        if let Some(official) = official_catalog.resolve(Some(release_id), "items", Some(key), Some(&lang)) {
            return official;
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
