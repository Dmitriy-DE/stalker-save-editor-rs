//! Catalogs, official names, installed game catalog builder, and translations.
//!
//! Owner: Gemini (G2).

pub mod builder;
pub mod bundle;
pub mod i18n;
pub mod models;
pub mod naming;
pub mod official_names;
pub mod s2;
pub mod service;
pub mod value;

pub use value::{parse_json, JsonValue};

pub use builder::InstalledGameCatalogBuilder;
pub use bundle::{CatalogBundleReader, CatalogBundleWriter};
pub use i18n::{I18nCompletenessChecker, I18nService, ValidationResult, SUPPORTED_LANGUAGES};
pub use models::{
    CatalogBundle, FactionCatalog, FactionDefinition, FactionRelation, FactionRelationAddress, GameCatalog,
    ItemCatalog, ItemDefinition, UpgradeCatalog, UpgradeDefinition,
};
pub use naming::SaveNaming;
pub use official_names::OfficialNamesCatalog;
pub use s2::{Stalker2ArmorUpgrade, Stalker2ArmorUpgrades, Stalker2ItemCatalog, Stalker2ItemEntry};
pub use service::{GameContent, GameContentService, GameContentStatus};
#[cfg(test)]
mod embedded_json_tests;
