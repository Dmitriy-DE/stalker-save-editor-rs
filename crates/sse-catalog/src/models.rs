//! Catalog models for items, factions, upgrades, and game catalogs.

use sse_core::{Error, Result};
use std::collections::HashMap;

/// An item definition from a shipped or installed catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemDefinition {
    /// Section / item key.
    pub key: String,
    /// Human-readable display name.
    pub display_name: Option<String>,
    /// High-level category (e.g. "weapon", "outfit", "consumable", "ammo").
    pub category: Option<String>,
    /// Unit weight in kilograms.
    pub unit_weight: Option<f64>,
    /// Grid width in inventory slots.
    pub width: Option<u32>,
    /// Grid height in inventory slots.
    pub height: Option<u32>,
    /// Maximum stack size (or box size).
    pub max_stack: Option<u32>,
    /// Valid equipment slots.
    pub slots: Vec<String>,
    /// Provenance source string (e.g. "configs/misc/items.ltx#medkit").
    pub source: String,
    /// Serialization family (e.g. "weapon_magazined", "weapon_wgl", "ammo").
    pub serialization_family: Option<String>,
    /// Atlas icon X coordinate in cells.
    pub icon_x: Option<u32>,
    /// Atlas icon Y coordinate in cells.
    pub icon_y: Option<u32>,
    /// Atlas texture name (e.g. "ui_icon_equipment").
    pub icon_texture: Option<String>,
    /// Engine class name (e.g. "WP_AK74").
    pub class_name: Option<String>,
    /// String table key for the display name.
    pub display_name_key: Option<String>,
    /// Raw prototype bytes if available.
    pub prototype: Option<Vec<u8>>,
    /// Base cost in rubles.
    pub cost: Option<u32>,
}

impl ItemDefinition {
    /// Creates a new item definition with validation.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: String,
        display_name: Option<String>,
        category: Option<String>,
        unit_weight: Option<f64>,
        width: Option<u32>,
        height: Option<u32>,
        max_stack: Option<u32>,
        slots: Vec<String>,
        source: String,
        serialization_family: Option<String>,
        icon_x: Option<u32>,
        icon_y: Option<u32>,
        icon_texture: Option<String>,
        class_name: Option<String>,
        display_name_key: Option<String>,
        prototype: Option<Vec<u8>>,
        cost: Option<u32>,
    ) -> Result<Self> {
        if key.trim().is_empty() {
            return Err(Error::damaged("Item key must not be empty."));
        }
        if source.trim().is_empty() {
            return Err(Error::damaged("Item source must not be empty."));
        }
        if let Some(w) = unit_weight {
            // NaN and infinity fail `w < 0.0` silently, so finiteness must be checked explicitly.
            if !w.is_finite() || w < 0.0 {
                return Err(Error::damaged("Item weight must be a finite, non-negative number."));
            }
        }

        let serialization_family = serialization_family
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        let icon_texture = icon_texture
            .map(|s| s.replace('\\', "/").trim().to_string())
            .filter(|s| !s.is_empty());
        let class_name = class_name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let display_name_key = display_name_key.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

        Ok(Self {
            key,
            display_name,
            category,
            unit_weight,
            width,
            height,
            max_stack,
            slots,
            source,
            serialization_family,
            icon_x,
            icon_y,
            icon_texture,
            class_name,
            display_name_key,
            prototype,
            cost,
        })
    }

    /// Clones this definition with an updated display name.
    #[must_use]
    pub fn with_display_name(&self, display_name: Option<String>) -> Self {
        let mut cloned = self.clone();
        cloned.display_name = display_name;
        cloned
    }
}

/// Catalog of items for a specific game release.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemCatalog {
    release_id: String,
    items: Vec<ItemDefinition>,
    by_key: HashMap<String, usize>,
}

impl ItemCatalog {
    /// Constructs a new item catalog, verifying uniqueness of item keys.
    pub fn new(release_id: String, items: Vec<ItemDefinition>) -> Result<Self> {
        if release_id.trim().is_empty() {
            return Err(Error::damaged("Release id must not be empty."));
        }
        let mut by_key = HashMap::new();
        for (index, item) in items.iter().enumerate() {
            if by_key.insert(item.key.clone(), index).is_some() {
                return Err(Error::damaged(format!(
                    "Duplicate item key '{}' in catalog '{}'.",
                    item.key, release_id
                )));
            }
        }
        Ok(Self {
            release_id,
            items,
            by_key,
        })
    }

    /// The release identifier (e.g. "stalker-cop").
    #[must_use]
    pub fn release_id(&self) -> &str {
        &self.release_id
    }

    /// All items in catalog order.
    #[must_use]
    pub fn items(&self) -> &[ItemDefinition] {
        &self.items
    }

    /// All items in catalog order as mutable slice.
    pub fn items_mut(&mut self) -> &mut [ItemDefinition] {
        &mut self.items
    }

    /// Resolves an item by its exact key.
    #[must_use]
    pub fn resolve(&self, key: &str) -> Option<&ItemDefinition> {
        self.by_key.get(key).and_then(|&idx| self.items.get(idx))
    }

    /// Resolves an item by its human-readable display name (case-insensitive).
    ///
    /// Returns `None` if multiple items share the same display name (ambiguous).
    #[must_use]
    pub fn resolve_display_name(&self, display_name: &str) -> Option<&ItemDefinition> {
        let normalized = display_name.trim();
        if normalized.is_empty() {
            return None;
        }

        // Unicode lowercasing allocates, so the query is lowered once and the per-item lowering is
        // skipped when both strings are ASCII (there, ASCII case-insensitive equality is the same test).
        let normalized_lower = normalized.to_lowercase();
        let normalized_ascii = normalized.is_ascii();
        let mut match_item: Option<&ItemDefinition> = None;
        for item in &self.items {
            if let Some(dn) = &item.display_name {
                let trimmed = dn.trim();
                let matches = trimmed.eq_ignore_ascii_case(normalized)
                    || (!(normalized_ascii && trimmed.is_ascii()) && trimmed.to_lowercase() == normalized_lower);
                if matches {
                    if match_item.is_some() {
                        return None;
                    }
                    match_item = Some(item);
                }
            }
        }
        match_item
    }

    /// Resolves an item by exact key, display_name_key, or unique display name.
    #[must_use]
    pub fn resolve_key_or_display_name(&self, value: &str) -> Option<&ItemDefinition> {
        let normalized = value.trim();
        if normalized.is_empty() {
            return None;
        }
        if let Some(item) = self.resolve(normalized) {
            return Some(item);
        }

        let mut name_key_matches = Vec::new();
        for item in &self.items {
            if let Some(dnk) = &item.display_name_key {
                if dnk.trim().eq_ignore_ascii_case(normalized) {
                    name_key_matches.push(item);
                    if name_key_matches.len() > 1 {
                        break;
                    }
                }
            }
        }
        if name_key_matches.len() == 1 {
            return name_key_matches.first().copied();
        }

        self.resolve_display_name(normalized)
    }
}

/// A faction definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactionDefinition {
    /// Faction key (e.g. "actor", "dolg", "freedom").
    pub key: String,
    /// Display name.
    pub display_name: Option<String>,
    /// Provenance source.
    pub source: String,
    /// Release ID.
    pub release_id: String,
    /// Numeric community ID if defined in `game_relations.ltx`.
    pub numeric_id: Option<i32>,
}

impl FactionDefinition {
    /// Creates a new faction definition with validation.
    pub fn new(
        key: String,
        display_name: Option<String>,
        source: String,
        release_id: String,
        numeric_id: Option<i32>,
    ) -> Result<Self> {
        if key.trim().is_empty() {
            return Err(Error::damaged("Faction key must not be empty."));
        }
        if source.trim().is_empty() {
            return Err(Error::damaged("Faction source must not be empty."));
        }
        if release_id.trim().is_empty() {
            return Err(Error::damaged("Release id must not be empty."));
        }
        if let Some(id) = numeric_id {
            if id < 0 {
                return Err(Error::damaged("Faction numeric ID must not be negative."));
            }
        }
        let display_name = display_name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        Ok(Self {
            key,
            display_name,
            source,
            release_id,
            numeric_id,
        })
    }
}

/// A relation between two factions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactionRelation {
    /// Source faction key.
    pub source: String,
    /// Target faction key.
    pub target: String,
    /// Numerical relation attitude.
    pub value: i32,
}

/// A relation address with matrix row and column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactionRelationAddress {
    /// Source faction key.
    pub source: String,
    /// Target faction key.
    pub target: String,
    /// Matrix row.
    pub row: i32,
    /// Matrix column.
    pub column: i32,
    /// Relation value.
    pub value: i32,
}

/// Catalog of factions and relations for a game release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactionCatalog {
    release_id: String,
    factions: Vec<FactionDefinition>,
    relations: Vec<FactionRelation>,
    goodwill_min: Option<i32>,
    goodwill_max: Option<i32>,
    attitude_neutral_threshold: Option<i32>,
    attitude_friend_threshold: Option<i32>,
    by_key: HashMap<String, usize>,
}

impl FactionCatalog {
    /// Constructs a new faction catalog with validation.
    pub fn new(
        release_id: String,
        factions: Vec<FactionDefinition>,
        relations: Vec<FactionRelation>,
        goodwill_min: Option<i32>,
        goodwill_max: Option<i32>,
        attitude_neutral_threshold: Option<i32>,
        attitude_friend_threshold: Option<i32>,
    ) -> Result<Self> {
        if release_id.trim().is_empty() {
            return Err(Error::damaged("Release id must not be empty."));
        }
        let mut by_key = HashMap::new();
        let mut seen_numeric = HashMap::new();

        for (index, faction) in factions.iter().enumerate() {
            if faction.release_id != release_id {
                return Err(Error::damaged(format!(
                    "Faction release id does not match catalog '{}'.",
                    release_id
                )));
            }
            if by_key.insert(faction.key.clone(), index).is_some() {
                return Err(Error::damaged(format!(
                    "Duplicate faction key '{}' in catalog '{}'.",
                    faction.key, release_id
                )));
            }
            if let Some(num_id) = faction.numeric_id {
                if seen_numeric.insert(num_id, faction.key.clone()).is_some() {
                    return Err(Error::damaged(format!(
                        "Duplicate faction numeric id {} in catalog '{}'.",
                        num_id, release_id
                    )));
                }
            }
        }

        let mut seen_relations = HashMap::new();
        for rel in &relations {
            if !by_key.contains_key(&rel.source) || !by_key.contains_key(&rel.target) {
                return Err(Error::damaged("Faction relation references an unknown community."));
            }
            if seen_relations
                .insert((rel.source.clone(), rel.target.clone()), ())
                .is_some()
            {
                return Err(Error::damaged("Duplicate faction relation in catalog."));
            }
        }

        if let (Some(min), Some(max)) = (goodwill_min, goodwill_max) {
            if min > max {
                return Err(Error::damaged("goodwill_min must not exceed goodwill_max."));
            }
        }

        Ok(Self {
            release_id,
            factions,
            relations,
            goodwill_min,
            goodwill_max,
            attitude_neutral_threshold,
            attitude_friend_threshold,
            by_key,
        })
    }

    /// The release identifier.
    #[must_use]
    pub fn release_id(&self) -> &str {
        &self.release_id
    }

    /// All factions in catalog order.
    #[must_use]
    pub fn factions(&self) -> &[FactionDefinition] {
        &self.factions
    }

    /// All factions in catalog order as mutable slice.
    pub fn factions_mut(&mut self) -> &mut [FactionDefinition] {
        &mut self.factions
    }

    /// All faction relations.
    #[must_use]
    pub fn relations(&self) -> &[FactionRelation] {
        &self.relations
    }

    /// Minimum goodwill.
    #[must_use]
    pub fn goodwill_min(&self) -> Option<i32> {
        self.goodwill_min
    }

    /// Maximum goodwill.
    #[must_use]
    pub fn goodwill_max(&self) -> Option<i32> {
        self.goodwill_max
    }

    /// Neutral attitude threshold.
    #[must_use]
    pub fn attitude_neutral_threshold(&self) -> Option<i32> {
        self.attitude_neutral_threshold
    }

    /// Friend attitude threshold.
    #[must_use]
    pub fn attitude_friend_threshold(&self) -> Option<i32> {
        self.attitude_friend_threshold
    }

    /// Resolves a faction by key, or returns an error if not found.
    pub fn resolve(&self, key: &str) -> Result<&FactionDefinition> {
        self.by_key
            .get(key)
            .and_then(|&idx| self.factions.get(idx))
            .ok_or_else(|| {
                Error::damaged(format!(
                    "Faction key '{}' is absent from catalog '{}'.",
                    key, self.release_id
                ))
            })
    }

    /// Resolves a faction by its numeric ID.
    #[must_use]
    pub fn resolve_numeric(&self, numeric_id: i32) -> Option<&FactionDefinition> {
        self.factions.iter().find(|f| f.numeric_id == Some(numeric_id))
    }

    /// Computes the relation address matrix coordinates `(row, column)`.
    pub fn relation_address(&self, source: &str, target: &str) -> Result<(i32, i32)> {
        let row = self.resolve(source)?.numeric_id;
        let column = self.resolve(target)?.numeric_id;
        match (row, column) {
            (Some(r), Some(c)) => Ok((r, c)),
            _ => Err(Error::damaged(format!(
                "Catalog '{}' has no numeric relation address.",
                self.release_id
            ))),
        }
    }

    /// Returns the default relation value between source and target factions.
    pub fn default_relation(&self, source: &str, target: &str) -> Result<Option<i32>> {
        self.resolve(source)?;
        self.resolve(target)?;
        Ok(self
            .relations
            .iter()
            .find(|rel| rel.source == source && rel.target == target)
            .map(|rel| rel.value))
    }

    /// Returns all relation addresses.
    pub fn relation_addresses(&self) -> Result<Vec<FactionRelationAddress>> {
        let mut addresses = Vec::with_capacity(self.relations.len());
        for rel in &self.relations {
            let (row, column) = self.relation_address(&rel.source, &rel.target)?;
            addresses.push(FactionRelationAddress {
                source: rel.source.clone(),
                target: rel.target.clone(),
                row,
                column,
                value: rel.value,
            });
        }
        Ok(addresses)
    }
}

/// An upgrade definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeDefinition {
    /// Upgrade key.
    pub key: String,
    /// Display name.
    pub display_name: Option<String>,
    /// Category ("weapon", "outfit", etc.).
    pub category: Option<String>,
    /// Base item key this upgrade belongs to.
    pub item_key: Option<String>,
    /// Provenance source.
    pub source: String,
    /// Release ID.
    pub release_id: String,
    /// Section name in upgrade configs.
    pub section: Option<String>,
    /// Property name modified.
    pub property_name: Option<String>,
    /// Upgrade icon name.
    pub icon: Option<String>,
    /// Applicable item keys.
    pub applicable_item_keys: Vec<String>,
}

impl UpgradeDefinition {
    /// Creates a new upgrade definition with validation.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: String,
        display_name: Option<String>,
        category: Option<String>,
        item_key: Option<String>,
        source: String,
        release_id: String,
        section: Option<String>,
        property_name: Option<String>,
        icon: Option<String>,
        applicable_item_keys: Vec<String>,
    ) -> Result<Self> {
        if key.trim().is_empty() {
            return Err(Error::damaged("Upgrade key must not be empty."));
        }
        if source.trim().is_empty() {
            return Err(Error::damaged("Upgrade source must not be empty."));
        }
        if release_id.trim().is_empty() {
            return Err(Error::damaged("Release id must not be empty."));
        }

        let display_name = display_name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let category = category.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let item_key = item_key.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let section = section.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let property_name = property_name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let icon = icon.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

        let mut keys = Vec::new();
        for k in applicable_item_keys {
            let trimmed = k.trim().to_string();
            if !trimmed.is_empty() && !keys.contains(&trimmed) {
                keys.push(trimmed);
            }
        }
        if let Some(ref ik) = item_key {
            if !keys.contains(ik) {
                keys.insert(0, ik.clone());
            }
        }

        Ok(Self {
            key,
            display_name,
            category,
            item_key,
            source,
            release_id,
            section,
            property_name,
            icon,
            applicable_item_keys: keys,
        })
    }

    /// Returns true if this upgrade applies to `item_key`.
    #[must_use]
    pub fn applies_to(&self, item_key: &str) -> bool {
        self.applicable_item_keys.iter().any(|k| k == item_key)
    }
}

/// Catalog of upgrades for a game release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeCatalog {
    release_id: String,
    upgrades: Vec<UpgradeDefinition>,
    by_key: HashMap<String, usize>,
}

impl UpgradeCatalog {
    /// Constructs a new upgrade catalog with validation.
    pub fn new(release_id: String, upgrades: Vec<UpgradeDefinition>) -> Result<Self> {
        if release_id.trim().is_empty() {
            return Err(Error::damaged("Release id must not be empty."));
        }
        let mut by_key = HashMap::new();
        for (index, upgrade) in upgrades.iter().enumerate() {
            if upgrade.release_id != release_id {
                return Err(Error::damaged(format!(
                    "Upgrade release id does not match catalog '{}'.",
                    release_id
                )));
            }
            if by_key.insert(upgrade.key.clone(), index).is_some() {
                return Err(Error::damaged(format!(
                    "Duplicate upgrade key '{}' in catalog '{}'.",
                    upgrade.key, release_id
                )));
            }
        }
        Ok(Self {
            release_id,
            upgrades,
            by_key,
        })
    }

    /// The release identifier.
    #[must_use]
    pub fn release_id(&self) -> &str {
        &self.release_id
    }

    /// All upgrades in catalog order.
    #[must_use]
    pub fn upgrades(&self) -> &[UpgradeDefinition] {
        &self.upgrades
    }

    /// Resolves an upgrade by exact key.
    #[must_use]
    pub fn resolve(&self, key: &str) -> Option<&UpgradeDefinition> {
        self.by_key.get(key).and_then(|&idx| self.upgrades.get(idx))
    }

    /// Returns all upgrades applicable to `item_key`.
    #[must_use]
    pub fn for_item(&self, item_key: &str) -> Vec<&UpgradeDefinition> {
        self.upgrades.iter().filter(|u| u.applies_to(item_key)).collect()
    }
}

/// A complete game catalog combining items, factions, and upgrades.
#[derive(Debug, Clone, PartialEq)]
pub struct GameCatalog {
    /// Release ID.
    pub release_id: String,
    /// Items catalog.
    pub items: ItemCatalog,
    /// Factions catalog.
    pub factions: FactionCatalog,
    /// Upgrades catalog if present.
    pub upgrades: Option<UpgradeCatalog>,
}

/// A catalog bundle for a release (factions or upgrades may be absent).
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogBundle {
    /// Release identifier.
    pub release_id: String,
    /// Items catalog.
    pub items: ItemCatalog,
    /// Factions catalog if present.
    pub factions: Option<FactionCatalog>,
    /// Upgrades catalog if present.
    pub upgrades: Option<UpgradeCatalog>,
}

impl CatalogBundle {
    /// Constructs a new catalog bundle, verifying matching release IDs.
    pub fn new(
        release_id: String,
        items: ItemCatalog,
        factions: Option<FactionCatalog>,
        upgrades: Option<UpgradeCatalog>,
    ) -> Result<Self> {
        if items.release_id != release_id
            || factions.as_ref().is_some_and(|f| f.release_id != release_id)
            || upgrades.as_ref().is_some_and(|u| u.release_id != release_id)
        {
            return Err(Error::damaged("Catalog bundle release IDs do not match."));
        }
        Ok(Self {
            release_id,
            items,
            factions,
            upgrades,
        })
    }

    /// Returns a new bundle with updated items or factions.
    pub fn with(&self, items: Option<ItemCatalog>, factions: Option<FactionCatalog>) -> Result<Self> {
        Self::new(
            self.release_id.clone(),
            items.unwrap_or_else(|| self.items.clone()),
            factions.or_else(|| self.factions.clone()),
            self.upgrades.clone(),
        )
    }
}
