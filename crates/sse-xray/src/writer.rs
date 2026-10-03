//! One-copy X-Ray edit preparation. Unknown fields remain untouched.

use sse_catalog::{CatalogBundleReader, FactionCatalog, UpgradeCatalog};
use sse_core::{Cursor, Error, Result, SaveBuffer};
use std::collections::{HashMap, HashSet};

use crate::Save;

const MAXIMUM_MONEY: u32 = 2_000_000_000;

/// Reference capability maturity from the C# 1.3.1 capability registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// The field is confirmed by the reference fixtures.
    Verified,
    /// The reference permits this field with experimental status.
    Experimental,
    /// The field is read-only or unsupported.
    Unsupported,
}

/// A mutation kind listed by the X-Ray capability registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Clone an item from an indexed template.
    AddItems,
    /// Change actor money.
    EditMoney,
    /// Change ammunition counts.
    EditStacks,
    /// Delete registry items.
    RemoveItems,
    /// Change equipment durability.
    EditDurability,
    /// Change item placement.
    EditPlacement,
    /// Change the actor faction.
    EditPlayerFaction,
    /// Change faction relations.
    EditRelations,
    /// Change upgrades.
    EditUpgrades,
    /// Transfer items into or out of a stash.
    MoveItems,
    /// Add actor quest/story info portions.
    EditInfoPortions,
    /// Move the actor to a save-resident level-changer destination.
    RelocateActor,
}

/// Returns the C# capability maturity for one format and edit kind.
#[must_use]
pub const fn capability(format: crate::Format, kind: ChangeKind) -> Capability {
    use crate::Format::{Cop, CopEe, Cs, CsEe, Soc, SocEe};
    use Capability::{Experimental, Unsupported, Verified};
    use ChangeKind::{
        AddItems, EditDurability, EditInfoPortions, EditMoney, EditPlacement, EditPlayerFaction, EditRelations,
        EditStacks, EditUpgrades, MoveItems, RelocateActor, RemoveItems,
    };

    match (format, kind) {
        (Soc | Cs | Cop, AddItems | EditMoney | EditStacks | RemoveItems) => Verified,
        (Soc | Cs | Cop, EditDurability | EditPlacement | EditPlayerFaction | EditRelations | MoveItems) => {
            Experimental
        }
        (Soc | Cs | Cop, EditInfoPortions | RelocateActor) => Experimental,
        (Cs | Cop, EditUpgrades) => Experimental,
        (Soc, EditUpgrades) => Unsupported,
        (SocEe | CsEe | CopEe, AddItems | EditMoney | EditStacks | RemoveItems) => Experimental,
        (
            SocEe | CsEe | CopEe,
            EditDurability | EditPlacement | EditPlayerFaction | EditRelations | EditUpgrades | EditInfoPortions
            | RelocateActor | MoveItems,
        ) => Unsupported,
    }
}

/// A confirmed change to one indexed save field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Move into the backpack.
    Ruck,
    /// Move onto the artifact belt.
    Belt,
    /// Equip in the item's proven base slot.
    Slot(u8),
}

/// One requested, guarded X-Ray image edit.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Replace the actor's wallet after checking its recorded old value.
    SetMoney {
        /// The actor registry object id.
        target_object: u16,
        /// The wallet value observed when the edit was prepared.
        old_value: u32,
        /// The requested wallet value.
        new_value: u32,
    },
    /// Replace the count of one confirmed actor-owned ammunition stack.
    SetStack {
        /// The inventory object id.
        target_object: u16,
        /// The count observed when the edit was prepared.
        old_value: u16,
        /// The requested count.
        new_value: u16,
    },
    /// Replace one confirmed equipment durability value and its unique serialized mirrors.
    SetDurability {
        /// Actor-owned equipment registry id.
        target_object: u16,
        /// The condition observed when the edit was prepared.
        old_value: f32,
        /// Requested normalized condition in the range 0..=1.
        new_value: f32,
    },
    /// Change one actor-owned item's packed client-data placement.
    SetPlacement {
        /// Actor-owned item registry id.
        target_object: u16,
        /// Requested destination.
        destination: Placement,
    },
    /// Transfer an item between the actor and a verified inventory-box stash.
    MoveItem {
        /// Item registry id.
        target_object: u16,
        /// Parent observed when the transfer was prepared.
        old_parent: u16,
        /// Actor id to take an item, or a verified inventory-box id to store it.
        new_parent: u16,
    },
    /// Remove one directly actor-owned registry object.
    RemoveItem {
        /// Actor-owned registry id to remove.
        target_object: u16,
    },
    /// Clone one explicitly selected registry template into the actor's inventory.
    AddItem {
        /// Existing registry record to clone.
        template_object: u16,
        /// Serialized item section key; must be an ASCII identifier.
        item_key: String,
        /// New unique registry id.
        object_id: u16,
        /// Stack count for ammunition; non-ammunition templates require one.
        quantity: u16,
    },
    /// Change the actor's faction by a key resolved through the matching faction catalog.
    SetPlayerFaction {
        /// The actor registry object id.
        target_object: u16,
        /// Player faction numeric id observed during preparation.
        old_value: i32,
        /// Requested catalog key.
        faction_key: String,
    },
    /// Change one actor-to-community goodwill value.
    SetFactionRelation {
        /// The actor registry object id.
        target_object: u16,
        /// Requested catalog key.
        faction_key: String,
        /// Existing goodwill when supplied by the caller.
        old_value: Option<i32>,
        /// Requested goodwill.
        new_value: i32,
    },
    /// Replace one actor-owned item's upgrade vector.
    SetUpgrades {
        /// Actor-owned item registry id.
        target_object: u16,
        /// Upgrade vector observed during preparation.
        old_value: Vec<String>,
        /// Requested catalog-backed upgrade vector.
        new_value: Vec<String>,
    },
    /// Add actor info portions, which represent story and quest flags.
    AddInfoPortions {
        /// The actor registry object id.
        target_object: u16,
        /// ASCII info-portion identifiers to add.
        info_portions: Vec<String>,
    },
    /// Move the actor to a destination read from one level changer in this save.
    RelocateActor {
        /// Level-changer object id whose parsed destination should become the actor location.
        destination_changer: u16,
    },
}

/// A bounded set of edits applied to one save image in one pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChangeSet {
    changes: Vec<Change>,
}

impl ChangeSet {
    /// Creates a change set from the requested edits.
    #[must_use]
    pub fn new(changes: Vec<Change>) -> Self {
        Self { changes }
    }

    /// Requested edits in caller order.
    #[must_use]
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }
}

/// Applies a supported change set, repacks once, and verifies the unpacked result.
pub fn apply(save: &Save, changes: &ChangeSet) -> Result<SaveBuffer> {
    let bundle = CatalogBundleReader::load_embedded().get(save.format().id());
    apply_with_catalog(
        save,
        changes,
        bundle.and_then(|bundle| bundle.factions.as_ref()),
        bundle.and_then(|bundle| bundle.upgrades.as_ref()),
    )
}

/// Applies changes with explicitly supplied catalogs, or the embedded catalogs when using [`apply`].
pub fn apply_with_catalog(
    save: &Save,
    changes: &ChangeSet,
    faction_catalog: Option<&FactionCatalog>,
    upgrade_catalog: Option<&UpgradeCatalog>,
) -> Result<SaveBuffer> {
    if changes.changes.is_empty() {
        return Err(Error::Refused("X-Ray change set is empty".to_owned()));
    }
    if changes.changes.len() > 100_000 {
        return Err(Error::Refused("X-Ray change set exceeds 100000 entries".to_owned()));
    }

    let inventory = if changes.changes().iter().any(|change| {
        matches!(
            change,
            Change::SetStack { .. }
                | Change::SetDurability { .. }
                | Change::SetPlacement { .. }
                | Change::MoveItem { .. }
                | Change::RemoveItem { .. }
        )
    }) {
        Some(save.inventory()?)
    } else {
        None
    };
    let mut seen_money = false;
    let mut seen_stacks = HashSet::new();
    let mut seen_durability = HashSet::new();
    let mut seen_placement = HashSet::new();
    let mut seen_moves = HashSet::new();
    let mut seen_removals = HashSet::new();
    let mut seen_faction = false;
    let mut seen_relations = HashSet::new();
    let mut seen_upgrades = HashSet::new();
    let mut seen_info_portions = false;
    let mut seen_relocation = false;
    let mut removed_objects = Vec::new();
    let mut added_objects = Vec::new();
    let mut added_records = Vec::new();
    let mut seen_additions = HashSet::new();
    let mut object_replacements = HashMap::new();
    let mut deferred_upgrade_vectors = Vec::new();
    let mut relation_payload: Option<Vec<u8>> = None;
    let mut writes = Vec::with_capacity(changes.changes.len().saturating_mul(2));
    for change in changes.changes() {
        let kind = match change {
            Change::SetMoney { .. } => ChangeKind::EditMoney,
            Change::SetStack { .. } => ChangeKind::EditStacks,
            Change::SetDurability { .. } => ChangeKind::EditDurability,
            Change::SetPlacement { .. } => ChangeKind::EditPlacement,
            Change::MoveItem { .. } => ChangeKind::MoveItems,
            Change::RemoveItem { .. } => ChangeKind::RemoveItems,
            Change::AddItem { .. } => ChangeKind::AddItems,
            Change::SetPlayerFaction { .. } => ChangeKind::EditPlayerFaction,
            Change::SetFactionRelation { .. } => ChangeKind::EditRelations,
            Change::SetUpgrades { .. } => ChangeKind::EditUpgrades,
            Change::AddInfoPortions { .. } => ChangeKind::EditInfoPortions,
            Change::RelocateActor { .. } => ChangeKind::RelocateActor,
        };
        if capability(save.format(), kind) == Capability::Unsupported {
            return Err(Error::Refused(format!(
                "{kind:?} is unsupported for {}",
                save.format().id()
            )));
        }
        match change {
            Change::SetMoney {
                target_object,
                old_value,
                new_value,
            } => {
                if seen_money {
                    return Err(Error::Refused(
                        "X-Ray change set contains duplicate money edits".to_owned(),
                    ));
                }
                seen_money = true;
                if *target_object != save.actor_id() {
                    return Err(Error::Refused("money edit target is not the actor".to_owned()));
                }
                if *old_value != save.money()? {
                    return Err(Error::Refused(
                        "money edit old value does not match the indexed save".to_owned(),
                    ));
                }
                if *new_value > MAXIMUM_MONEY {
                    return Err(Error::Refused(format!("money must be in the range 0..{MAXIMUM_MONEY}")));
                }
                writes.push(PendingWrite::u32(save.money_offset(), *new_value));
            }
            Change::SetStack {
                target_object,
                old_value,
                new_value,
            } => {
                if !seen_stacks.insert(*target_object) {
                    return Err(Error::Refused(format!(
                        "duplicate stack edit for 0x{target_object:04X}"
                    )));
                }
                if *new_value == 0 {
                    return Err(Error::Refused("stack count must be in the range 1..65535".to_owned()));
                }
                let item = inventory
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|item| item.handle == *target_object)
                    .ok_or_else(|| Error::Refused(format!("object 0x{target_object:04X} is not actor-owned")))?;
                if item.count != Some(*old_value) {
                    return Err(Error::Refused(format!(
                        "stack old count does not match object 0x{target_object:04X}"
                    )));
                }
                let state_offset = item.state_count_offset.ok_or_else(|| {
                    Error::Refused(format!("object 0x{target_object:04X} has no proven state count field"))
                })?;
                let update_offset = item.update_count_offset.ok_or_else(|| {
                    Error::Refused(format!("object 0x{target_object:04X} has no proven update count field"))
                })?;
                writes.push(PendingWrite::u16(state_offset, *new_value));
                writes.push(PendingWrite::u16(update_offset, *new_value));
            }
            Change::SetDurability {
                target_object,
                old_value,
                new_value,
            } => {
                if !seen_durability.insert(*target_object) {
                    return Err(Error::Refused(format!(
                        "duplicate durability edit for 0x{target_object:04X}"
                    )));
                }
                if !new_value.is_finite() || !(0.0..=1.0).contains(new_value) {
                    return Err(Error::Refused(
                        "durability must be finite and in the range 0..=1".to_owned(),
                    ));
                }
                let item = inventory
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|item| item.handle == *target_object)
                    .ok_or_else(|| Error::Refused(format!("object 0x{target_object:04X} is not actor-owned")))?;
                if item.condition != Some(*old_value) {
                    return Err(Error::Refused(format!(
                        "durability old value does not match object 0x{target_object:04X}"
                    )));
                }
                let state_offset = item.condition_offset.ok_or_else(|| {
                    Error::Refused(format!("object 0x{target_object:04X} has no proven condition field"))
                })?;
                writes.push(PendingWrite::f32(state_offset, *new_value));
                if let Some(offset) = item.update_condition_offset {
                    #[allow(clippy::cast_possible_truncation)]
                    let encoded = ((*new_value * 255.0) + 0.5).floor().clamp(0.0, 255.0) as u8;
                    writes.push(PendingWrite::u8(offset, encoded));
                }
                if let Some(offset) = item.client_condition_offset {
                    writes.push(PendingWrite::f32(offset, *new_value));
                }
            }
            Change::SetPlacement {
                target_object,
                destination,
            } => {
                if !seen_placement.insert(*target_object) {
                    return Err(Error::Refused(format!(
                        "duplicate placement edit for 0x{target_object:04X}"
                    )));
                }
                let item = inventory
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|item| item.handle == *target_object)
                    .ok_or_else(|| Error::Refused(format!("object 0x{target_object:04X} is not actor-owned")))?;
                let current = item.placement_value.ok_or_else(|| {
                    Error::Refused(format!("object 0x{target_object:04X} has no proven placement field"))
                })?;
                let offset = item.placement_offset.ok_or_else(|| {
                    Error::Refused(format!("object 0x{target_object:04X} has no proven placement offset"))
                })?;
                let replacement = match destination {
                    Placement::Ruck => (current & 0xFFF0) | 3,
                    Placement::Belt if item.section.to_ascii_lowercase().starts_with("af_") => (current & 0xFFF0) | 2,
                    Placement::Belt => {
                        return Err(Error::Refused(
                            "only artifact sections may be moved to the belt".to_owned(),
                        ))
                    }
                    Placement::Slot(slot) => {
                        let maximum_slot = if save.format() == crate::Format::Cop { 12 } else { 10 };
                        if *slot == 0 || usize::from(*slot) > maximum_slot || item.placement_base_slot != Some(*slot) {
                            return Err(Error::Refused(format!("item cannot be equipped in slot {slot}")));
                        }
                        (current & 0xFC00) | (u16::from(*slot) << 4) | 1
                    }
                };
                writes.push(PendingWrite::u16(offset, replacement));
            }
            Change::MoveItem {
                target_object,
                old_parent,
                new_parent,
            } => {
                if !seen_moves.insert(*target_object) {
                    return Err(Error::Refused(format!(
                        "duplicate stash transfer for 0x{target_object:04X}"
                    )));
                }
                let record = save
                    .registry_objects()
                    .iter()
                    .find(|record| record.object_id == *target_object)
                    .ok_or_else(|| {
                        Error::Refused(format!("object 0x{target_object:04X} is missing from the registry"))
                    })?;
                if record.parent_id != *old_parent {
                    return Err(Error::Refused(format!(
                        "stash transfer old parent does not match object 0x{target_object:04X}"
                    )));
                }
                if *old_parent == save.actor_id() {
                    let box_record = save
                        .registry_objects()
                        .iter()
                        .find(|candidate| candidate.object_id == *new_parent && candidate.name == "inventory_box")
                        .ok_or_else(|| {
                            Error::Refused(format!("destination 0x{new_parent:04X} is not an inventory_box stash"))
                        })?;
                    let _ = box_record;
                    let item = inventory
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .find(|item| item.handle == *target_object)
                        .ok_or_else(|| Error::Refused(format!("object 0x{target_object:04X} is not actor-owned")))?;
                    let backpack = item.placement_value.is_some_and(|value| value & 0x0F == 3);
                    if !backpack {
                        return Err(Error::Refused(format!(
                            "object 0x{target_object:04X} must be in the backpack before it can be stored"
                        )));
                    }
                } else {
                    let source_box = save
                        .registry_objects()
                        .iter()
                        .find(|candidate| candidate.object_id == *old_parent && candidate.name == "inventory_box")
                        .ok_or_else(|| {
                            Error::Refused(format!("object 0x{target_object:04X} is not in an inventory_box stash"))
                        })?;
                    let _ = source_box;
                    if *new_parent != save.actor_id() {
                        return Err(Error::Refused("stash take destination must be the actor".to_owned()));
                    }
                    let box_start = record.client_data_offset;
                    if save.format() == crate::Format::Cs {
                        if let Some(start) = box_start.filter(|_| record.client_data_length >= 2) {
                            let kind_offset = start
                                .checked_add(1)
                                .ok_or_else(|| Error::damaged("X-Ray client-data offset overflow"))?;
                            if save.raw_image().get(start) == Some(&2)
                                && save
                                    .raw_image()
                                    .get(kind_offset)
                                    .is_some_and(|kind| (1..=3).contains(kind))
                            {
                                writes.push(PendingWrite::u8(kind_offset, 3));
                            } else if let Some(value_end) = kind_offset.checked_add(2) {
                                if save.raw_image().get(kind_offset..value_end).is_some() {
                                    if let Ok(current) = read_u16(save.raw_image(), kind_offset) {
                                        if (1..=3).contains(&(current & 0x0F)) {
                                            writes.push(PendingWrite::u16(kind_offset, (current & 0xFFF0) | 3));
                                        }
                                    }
                                }
                            }
                        }
                    } else if let Some(start) = box_start {
                        let offset = start
                            .checked_add(1)
                            .ok_or_else(|| Error::damaged("X-Ray client-data offset overflow"))?;
                        let end = offset
                            .checked_add(2)
                            .ok_or_else(|| Error::damaged("X-Ray placement range overflow"))?;
                        if save.raw_image().get(offset..end).is_some() {
                            if let Ok(current) = read_u16(save.raw_image(), offset) {
                                if (1..=3).contains(&(current & 0x0F)) {
                                    writes.push(PendingWrite::u16(offset, (current & 0xFFF0) | 3));
                                }
                            }
                        }
                    }
                }
                writes.push(PendingWrite::u16(record.parent_id_offset, *new_parent));
            }
            Change::RemoveItem { target_object } => {
                if !seen_removals.insert(*target_object) {
                    return Err(Error::Refused(format!("duplicate removal for 0x{target_object:04X}")));
                }
                let record = save
                    .registry_objects()
                    .iter()
                    .find(|record| record.object_id == *target_object)
                    .ok_or_else(|| Error::Refused(format!("object 0x{target_object:04X} is unresolved or missing")))?;
                if record.object_id == save.actor_id() {
                    return Err(Error::Refused("the actor object cannot be removed".to_owned()));
                }
                if record.parent_id != save.actor_id() {
                    return Err(Error::Refused(format!(
                        "object 0x{target_object:04X} is not directly owned by the actor"
                    )));
                }
                if save
                    .registry_objects()
                    .iter()
                    .any(|child| child.object_id != record.object_id && child.parent_id == record.object_id)
                {
                    return Err(Error::Refused(format!(
                        "object 0x{target_object:04X} has dependent registry children"
                    )));
                }
                if let Some(item) = inventory
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|item| item.handle == *target_object)
                {
                    if item.placement_value.is_some_and(|value| value & 0x0F == 1) {
                        return Err(Error::Refused(format!(
                            "equipped object 0x{target_object:04X} cannot be removed"
                        )));
                    }
                    if record.name.to_ascii_lowercase().starts_with("ammo_") && item.count.is_none() {
                        return Err(Error::Refused(format!(
                            "ammo object 0x{target_object:04X} has unresolved count fields"
                        )));
                    }
                }
                removed_objects.push(*target_object);
            }
            Change::AddItem {
                template_object,
                item_key,
                object_id,
                quantity,
            } => {
                if !seen_additions.insert(*object_id) {
                    return Err(Error::Refused(format!("duplicate new object id 0x{object_id:04X}")));
                }
                if save
                    .registry_objects()
                    .iter()
                    .any(|record| record.object_id == *object_id)
                    || *object_id == save.actor_id()
                {
                    return Err(Error::Refused(format!(
                        "new object id 0x{object_id:04X} already exists"
                    )));
                }
                if seen_removals.contains(template_object) {
                    return Err(Error::Refused(
                        "an item scheduled for removal cannot be used as an add template".to_owned(),
                    ));
                }
                let template = save
                    .registry_objects()
                    .iter()
                    .find(|record| record.object_id == *template_object)
                    .ok_or_else(|| Error::Refused(format!("template object 0x{template_object:04X} is missing")))?;
                if item_key.is_empty()
                    || !item_key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
                {
                    return Err(Error::Refused(
                        "item key must be a non-empty ASCII identifier".to_owned(),
                    ));
                }
                let ammunition = item_key.to_ascii_lowercase().starts_with("ammo_");
                if *quantity == 0 || (!ammunition && *quantity != 1) {
                    return Err(Error::Refused(
                        "ammo quantity must be 1..65535; non-ammo clones require quantity 1".to_owned(),
                    ));
                }
                let record = clone_template_record(save, template, item_key, *object_id, *quantity)?;
                added_records.push(record);
                added_objects.push((*object_id, save.actor_id(), ammunition.then_some(*quantity)));
            }
            Change::SetPlayerFaction {
                target_object,
                old_value,
                faction_key,
            } => {
                if seen_faction {
                    return Err(Error::Refused("duplicate X-Ray player-faction edit".to_owned()));
                }
                seen_faction = true;
                if *target_object != save.actor_id() {
                    return Err(Error::Refused("player-faction target is not the actor".to_owned()));
                }
                if save.player_faction() != Some(*old_value) {
                    return Err(Error::Refused(
                        "player-faction old value does not match the indexed save".to_owned(),
                    ));
                }
                let catalog = matching_faction_catalog(faction_catalog, save.format())?;
                let faction = catalog
                    .resolve(faction_key)
                    .map_err(|_| Error::Refused(format!("unknown faction key '{faction_key}'")))?;
                let numeric = faction
                    .numeric_id
                    .ok_or_else(|| Error::Refused(format!("faction '{faction_key}' has no numeric community id")))?;
                let offset = save.player_faction_offset.ok_or_else(|| {
                    Error::Refused("actor community field is not confirmed for this STATE layout".to_owned())
                })?;
                writes.push(PendingWrite::i32(offset, numeric));
            }
            Change::SetFactionRelation {
                target_object,
                faction_key,
                old_value,
                new_value,
            } => {
                if *target_object != save.actor_id() {
                    return Err(Error::Refused("faction relation target is not the actor".to_owned()));
                }
                if save.relation_registry.is_none() {
                    return Err(Error::Refused("relation registry is absent or malformed".to_owned()));
                }
                let catalog = matching_faction_catalog(faction_catalog, save.format())?;
                let goodwill_min = catalog
                    .goodwill_min()
                    .ok_or_else(|| Error::Refused("catalog has no confirmed minimum goodwill".to_owned()))?;
                let goodwill_max = catalog
                    .goodwill_max()
                    .ok_or_else(|| Error::Refused("catalog has no confirmed maximum goodwill".to_owned()))?;
                if !(goodwill_min..=goodwill_max).contains(new_value) {
                    return Err(Error::Refused(format!(
                        "goodwill must be in the range {goodwill_min}..={goodwill_max}"
                    )));
                }
                let faction = catalog
                    .resolve(faction_key)
                    .map_err(|_| Error::Refused(format!("unknown faction key '{faction_key}'")))?;
                let community_id = faction
                    .numeric_id
                    .ok_or_else(|| Error::Refused(format!("faction '{faction_key}' has no numeric community id")))?;
                if !seen_relations.insert(community_id) {
                    return Err(Error::Refused(format!("duplicate relation edit for '{faction_key}'")));
                }
                let payload = relation_payload_for(&mut relation_payload, save)?;
                let parsed = crate::save::parse_relation_registry(payload, save.relation_has_timestamps())?;
                let row = parsed
                    .relation_rows
                    .iter()
                    .find(|row| row.object_id == save.actor_id())
                    .ok_or_else(|| Error::Refused("actor relation row is absent".to_owned()))?;
                let existing = row.communities.iter().find(|item| item.community_id == community_id);
                if old_value.is_some_and(|old| existing.map(|item| item.goodwill) != Some(old)) {
                    return Err(Error::Refused(format!(
                        "old goodwill for '{faction_key}' does not match the save"
                    )));
                }
                patch_relation_payload(
                    payload,
                    save.relation_has_timestamps(),
                    save.actor_id(),
                    community_id,
                    *new_value,
                )?;
            }
            Change::SetUpgrades {
                target_object,
                old_value,
                new_value,
            } => {
                if !seen_upgrades.insert(*target_object) {
                    return Err(Error::Refused(format!(
                        "duplicate upgrade edit for 0x{target_object:04X}"
                    )));
                }
                let catalog = matching_upgrade_catalog(upgrade_catalog, save.format())?;
                let record = save
                    .registry_objects()
                    .iter()
                    .find(|record| record.object_id == *target_object)
                    .ok_or_else(|| Error::Refused(format!("upgrade target 0x{target_object:04X} is missing")))?;
                if record.parent_id != save.actor_id() {
                    return Err(Error::Refused(format!(
                        "upgrade target 0x{target_object:04X} is not actor-owned"
                    )));
                }
                let (current, vector_offset, vector_length) = read_upgrade_vector(save.raw_image(), record)?;
                if &current != old_value {
                    return Err(Error::Refused(format!(
                        "old upgrades do not match object 0x{target_object:04X}"
                    )));
                }
                validate_upgrade_vector(catalog, &record.name, &current, new_value)?;
                if current != *new_value {
                    deferred_upgrade_vectors.push((record.clone(), vector_offset, vector_length, new_value.clone()));
                }
            }
            Change::AddInfoPortions {
                target_object,
                info_portions,
            } => {
                if seen_info_portions {
                    return Err(Error::Refused("duplicate actor info-portion edit".to_owned()));
                }
                seen_info_portions = true;
                if *target_object != save.actor_id() {
                    return Err(Error::Refused("info-portion target is not the actor".to_owned()));
                }
                if save.relation_registry.is_none() {
                    return Err(Error::Refused("relation registry is absent or malformed".to_owned()));
                }
                let payload = relation_payload_for(&mut relation_payload, save)?;
                add_info_portions_to_payload(
                    payload,
                    save.relation_has_timestamps(),
                    save.actor_id(),
                    save.game_time(),
                    info_portions,
                )?;
            }
            Change::RelocateActor { destination_changer } => {
                if seen_relocation {
                    return Err(Error::Refused("duplicate actor relocation".to_owned()));
                }
                seen_relocation = true;
                let destination = save
                    .level_changer_destinations()?
                    .into_iter()
                    .find(|(handle, _)| handle == destination_changer)
                    .map(|(_, destination)| destination)
                    .ok_or_else(|| Error::Refused("destination is not a verified level-changer anchor".to_owned()))?;
                let destination_position = destination
                    .dest_position
                    .ok_or_else(|| Error::Refused("destination has no confirmed position".to_owned()))?;
                let destination_direction = destination
                    .dest_direction
                    .ok_or_else(|| Error::Refused("destination has no confirmed direction".to_owned()))?;
                let game_vertex = destination
                    .dest_game_vertex_id
                    .ok_or_else(|| Error::Refused("destination has no game-graph vertex".to_owned()))?;
                let level_vertex = destination
                    .dest_level_vertex_id
                    .ok_or_else(|| Error::Refused("destination has no level-graph vertex".to_owned()))?;
                if !vector_is_finite(destination_position) || !vector_is_finite(destination_direction) {
                    return Err(Error::Refused("destination contains non-finite coordinates".to_owned()));
                }
                let actor = save
                    .registry_objects()
                    .iter()
                    .find(|record| record.object_id == save.actor_id())
                    .ok_or_else(|| Error::damaged("X-Ray actor record is missing"))?;
                let spawn_position = actor_spawn_position_offset(save.raw_image(), actor)?;
                let update_position = actor
                    .update_offset
                    .checked_add(11)
                    .ok_or_else(|| Error::damaged("actor UPDATE position offset overflow"))?;
                let update_end = update_position
                    .checked_add(12)
                    .ok_or_else(|| Error::damaged("actor UPDATE position range overflow"))?;
                let actor_update_end = actor
                    .update_offset
                    .checked_add(actor.update_length)
                    .ok_or_else(|| Error::damaged("actor UPDATE range overflow"))?;
                if actor.state_length < 14 || update_end > actor_update_end {
                    return Err(Error::Refused(
                        "actor record lacks verified relocation fields".to_owned(),
                    ));
                }
                let current_position = read_vector(save.raw_image(), spawn_position)?;
                let update_current = read_vector(save.raw_image(), update_position)?;
                if !vector_is_finite(current_position) || current_position != update_current {
                    return Err(Error::Refused(
                        "actor spawn and UPDATE positions are not a verified match".to_owned(),
                    ));
                }
                let graph_offset = actor.state_offset;
                for (relative, value) in [
                    (0_usize, destination_position.x),
                    (4, destination_position.y),
                    (8, destination_position.z),
                    (12, destination_direction.x),
                    (16, destination_direction.y),
                    (20, destination_direction.z),
                ] {
                    let offset = spawn_position
                        .checked_add(relative)
                        .ok_or_else(|| Error::damaged("actor spawn position offset overflow"))?;
                    writes.push(PendingWrite::f32(offset, value));
                }
                let graph_level_offset = graph_offset
                    .checked_add(10)
                    .ok_or_else(|| Error::damaged("actor graph vertex offset overflow"))?;
                let update_y = update_position
                    .checked_add(4)
                    .ok_or_else(|| Error::damaged("actor UPDATE position offset overflow"))?;
                let update_z = update_position
                    .checked_add(8)
                    .ok_or_else(|| Error::damaged("actor UPDATE position offset overflow"))?;
                writes.push(PendingWrite::u16(graph_offset, game_vertex));
                writes.push(PendingWrite::u32(graph_level_offset, level_vertex));
                writes.push(PendingWrite::f32(update_position, destination_position.x));
                writes.push(PendingWrite::f32(update_y, destination_position.y));
                writes.push(PendingWrite::f32(update_z, destination_position.z));
            }
        }
    }
    if seen_removals.iter().any(|id| seen_upgrades.contains(id)) {
        return Err(Error::Refused(
            "an item cannot be upgraded and removed in one change set".to_owned(),
        ));
    }
    writes.sort_by_key(|write| write.offset);
    for pair in writes.windows(2) {
        let Some((previous, next)) = pair.first().zip(pair.get(1)) else {
            continue;
        };
        let previous_end = previous
            .offset
            .checked_add(previous.length)
            .ok_or_else(|| Error::damaged("X-Ray write range overflow"))?;
        if previous_end > next.offset {
            return Err(Error::Refused("X-Ray changes overlap the same image bytes".to_owned()));
        }
    }

    let mut working = save.raw_image().to_vec();
    for write in &writes {
        let end = write
            .offset
            .checked_add(write.length)
            .ok_or_else(|| Error::damaged("X-Ray write range overflow"))?;
        let field = working
            .get_mut(write.offset..end)
            .ok_or_else(|| Error::damaged("X-Ray write range is outside the image"))?;
        let value = write
            .bytes
            .get(..write.length)
            .ok_or_else(|| Error::damaged("X-Ray write value has an invalid width"))?;
        field.copy_from_slice(value);
    }
    for (record, vector_offset, vector_length, requested) in deferred_upgrade_vectors {
        let replacement = replace_upgrade_record(&working, &record, vector_offset, vector_length, &requested)?;
        object_replacements.insert(record.object_id, replacement);
    }

    let has_object_changes =
        !removed_objects.is_empty() || !added_records.is_empty() || !object_replacements.is_empty();
    let mut object_payload = None;
    if has_object_changes {
        let object_bytes = save.object_chunk_bytes(&working)?;
        let count_bytes = object_bytes
            .get(..4)
            .ok_or_else(|| Error::damaged("X-Ray OBJECT chunk is shorter than its count"))?;
        let old_count = u32::from_le_bytes(
            <[u8; 4]>::try_from(count_bytes).map_err(|_| Error::damaged("invalid X-Ray object count"))?,
        );
        let removed_count =
            u32::try_from(removed_objects.len()).map_err(|_| Error::Refused("too many X-Ray removals".to_owned()))?;
        let after_removals = old_count
            .checked_sub(removed_count)
            .ok_or_else(|| Error::Refused("removal count exceeds the OBJECT registry".to_owned()))?;
        let new_count = after_removals
            .checked_add(
                u32::try_from(added_records.len())
                    .map_err(|_| Error::Refused("too many X-Ray additions".to_owned()))?,
            )
            .filter(|count| *count > 0 && *count <= 1_000_000)
            .ok_or_else(|| Error::Refused("OBJECT registry count is outside its supported range".to_owned()))?;
        let removed_length = save
            .registry_objects()
            .iter()
            .filter(|record| seen_removals.contains(&record.object_id))
            .try_fold(0_usize, |sum, record| sum.checked_add(record.record_length))
            .ok_or_else(|| Error::Refused("removed X-Ray record length overflow".to_owned()))?;
        let replaced_old_length = save
            .registry_objects()
            .iter()
            .filter(|record| object_replacements.contains_key(&record.object_id))
            .try_fold(0_usize, |sum, record| sum.checked_add(record.record_length))
            .ok_or_else(|| Error::Refused("replaced X-Ray record length overflow".to_owned()))?;
        let replaced_new_length = object_replacements
            .values()
            .try_fold(0_usize, |sum, record| sum.checked_add(record.len()))
            .ok_or_else(|| Error::Refused("replacement X-Ray record length overflow".to_owned()))?;
        let added_length = added_records
            .iter()
            .try_fold(0_usize, |sum, record| sum.checked_add(record.len()))
            .ok_or_else(|| Error::Refused("added X-Ray record length overflow".to_owned()))?;
        let capacity = object_bytes
            .len()
            .checked_sub(removed_length)
            .and_then(|length| length.checked_sub(replaced_old_length))
            .and_then(|length| length.checked_add(replaced_new_length))
            .and_then(|length| length.checked_add(added_length))
            .ok_or_else(|| Error::damaged("rebuilt OBJECT chunk length overflow"))?;
        let mut payload = Vec::with_capacity(capacity);
        payload.extend_from_slice(&new_count.to_le_bytes());
        let mut offset = 4_usize;
        for record in save.registry_objects() {
            let end = offset
                .checked_add(record.record_length)
                .ok_or_else(|| Error::damaged("X-Ray record range overflow"))?;
            let bytes = object_bytes
                .get(offset..end)
                .ok_or_else(|| Error::damaged("X-Ray OBJECT record exceeds its chunk"))?;
            if !seen_removals.contains(&record.object_id) {
                if let Some(replacement) = object_replacements.get(&record.object_id) {
                    payload.extend_from_slice(replacement);
                } else {
                    payload.extend_from_slice(bytes);
                }
            }
            offset = end;
        }
        if offset != object_bytes.len() {
            return Err(Error::damaged("X-Ray OBJECT records do not consume their chunk"));
        }
        for record in &added_records {
            payload.extend_from_slice(record);
        }
        object_payload = Some(payload);
    }
    let mut replacements = Vec::with_capacity(2);
    if let Some(payload) = object_payload.as_deref() {
        replacements.push((2, payload));
    }
    if let Some(payload) = relation_payload.as_deref() {
        replacements.push((9, payload));
    }
    if !replacements.is_empty() {
        save.rebuild_chunks_in_place(&mut working, &replacements)?;
    }
    let packed = save.repack(&working)?;
    let verified = Save::read(packed.as_slice())?;
    if verified.format() != save.format() {
        return Err(Error::Refused("X-Ray edit changed the detected save format".to_owned()));
    }
    let after = verified.raw_image();
    if after != working.as_slice() {
        return Err(Error::Refused(
            "X-Ray edit failed its exact unpacked-image read-back check".to_owned(),
        ));
    }
    if seen_money {
        let expected_money = changes.changes().iter().find_map(|change| match change {
            Change::SetMoney { new_value, .. } => Some(*new_value),
            Change::SetStack { .. }
            | Change::SetDurability { .. }
            | Change::SetPlacement { .. }
            | Change::MoveItem { .. }
            | Change::RemoveItem { .. }
            | Change::AddItem { .. }
            | Change::SetPlayerFaction { .. }
            | Change::SetFactionRelation { .. }
            | Change::SetUpgrades { .. }
            | Change::AddInfoPortions { .. }
            | Change::RelocateActor { .. } => None,
        });
        if verified.money()? != expected_money.unwrap_or_default() {
            return Err(Error::Refused(
                "X-Ray money edit failed its value read-back check".to_owned(),
            ));
        }
    }
    if !seen_stacks.is_empty() {
        let verified_items = verified.inventory()?;
        for change in changes.changes() {
            if let Change::SetStack {
                target_object,
                new_value,
                ..
            } = change
            {
                let item = verified_items
                    .iter()
                    .find(|item| item.handle == *target_object)
                    .ok_or_else(|| Error::Refused(format!("stack object 0x{target_object:04X} disappeared")))?;
                if item.count != Some(*new_value) {
                    return Err(Error::Refused(format!(
                        "stack count read-back failed for 0x{target_object:04X}"
                    )));
                }
            }
        }
    }
    if !seen_durability.is_empty() || !seen_placement.is_empty() {
        let verified_items = verified.inventory()?;
        for change in changes.changes() {
            match change {
                Change::SetDurability {
                    target_object,
                    new_value,
                    ..
                } => {
                    let item = verified_items
                        .iter()
                        .find(|item| item.handle == *target_object)
                        .ok_or_else(|| {
                            Error::Refused(format!("durability object 0x{target_object:04X} disappeared"))
                        })?;
                    if item.condition != Some(*new_value) {
                        return Err(Error::Refused(format!(
                            "durability read-back failed for 0x{target_object:04X}"
                        )));
                    }
                }
                Change::SetPlacement {
                    target_object,
                    destination,
                    ..
                } => {
                    let item = verified_items
                        .iter()
                        .find(|item| item.handle == *target_object)
                        .ok_or_else(|| Error::Refused(format!("placement object 0x{target_object:04X} disappeared")))?;
                    let value = item
                        .placement_value
                        .ok_or_else(|| Error::Refused("placement read-back is unknown".to_owned()))?;
                    let kind = value & 0x0F;
                    let slot = (value >> 4) & 0x3F;
                    let matches = match destination {
                        Placement::Ruck => kind == 3,
                        Placement::Belt => kind == 2,
                        Placement::Slot(expected_slot) => kind == 1 && slot == u16::from(*expected_slot),
                    };
                    if !matches {
                        return Err(Error::Refused(format!(
                            "placement read-back failed for 0x{target_object:04X}"
                        )));
                    }
                }
                Change::SetMoney { .. }
                | Change::SetStack { .. }
                | Change::MoveItem { .. }
                | Change::RemoveItem { .. }
                | Change::AddItem { .. }
                | Change::SetPlayerFaction { .. }
                | Change::SetFactionRelation { .. }
                | Change::SetUpgrades { .. }
                | Change::AddInfoPortions { .. }
                | Change::RelocateActor { .. } => {}
            }
        }
    }
    if !seen_moves.is_empty() {
        for change in changes.changes() {
            if let Change::MoveItem {
                target_object,
                new_parent,
                ..
            } = change
            {
                let record = verified
                    .registry_objects()
                    .iter()
                    .find(|record| record.object_id == *target_object)
                    .ok_or_else(|| Error::Refused(format!("transferred object 0x{target_object:04X} disappeared")))?;
                if record.parent_id != *new_parent {
                    return Err(Error::Refused(format!(
                        "stash transfer parent read-back failed for 0x{target_object:04X}"
                    )));
                }
            }
        }
    }
    if removed_objects
        .iter()
        .any(|id| verified.registry_objects().iter().any(|record| record.object_id == *id))
    {
        return Err(Error::Refused(
            "removed X-Ray object remains in the registry after read-back".to_owned(),
        ));
    }
    for (object_id, parent_id, expected_count) in &added_objects {
        let record = verified
            .registry_objects()
            .iter()
            .find(|record| record.object_id == *object_id)
            .ok_or_else(|| Error::Refused(format!("added object 0x{object_id:04X} is missing after read-back")))?;
        if record.parent_id != *parent_id {
            return Err(Error::Refused(format!(
                "added object 0x{object_id:04X} has the wrong parent after read-back"
            )));
        }
        if let Some(expected_count) = expected_count {
            let item = verified
                .inventory()?
                .into_iter()
                .find(|item| item.handle == *object_id)
                .ok_or_else(|| {
                    Error::Refused(format!(
                        "added ammo 0x{object_id:04X} is not actor-owned after read-back"
                    ))
                })?;
            if item.count != Some(*expected_count) {
                return Err(Error::Refused(format!(
                    "added ammo count read-back failed for 0x{object_id:04X}"
                )));
            }
        }
    }
    verify_extended_changes(save, &verified, changes, faction_catalog)?;
    Ok(packed)
}

fn verify_extended_changes(
    source: &Save,
    verified: &Save,
    changes: &ChangeSet,
    faction_catalog: Option<&FactionCatalog>,
) -> Result<()> {
    for change in changes.changes() {
        match change {
            Change::SetPlayerFaction { faction_key, .. } => {
                let faction = matching_faction_catalog(faction_catalog, source.format())?
                    .resolve(faction_key)
                    .map_err(|_| Error::Refused(format!("unknown faction key '{faction_key}'")))?;
                if verified.player_faction() != faction.numeric_id {
                    return Err(Error::Refused("player-faction read-back did not match".to_owned()));
                }
            }
            Change::SetFactionRelation {
                faction_key, new_value, ..
            } => {
                let faction = matching_faction_catalog(faction_catalog, source.format())?
                    .resolve(faction_key)
                    .map_err(|_| Error::Refused(format!("unknown faction key '{faction_key}'")))?;
                let community_id = faction
                    .numeric_id
                    .ok_or_else(|| Error::Refused(format!("faction '{faction_key}' has no numeric community id")))?;
                let registry = verified
                    .relation_registry
                    .as_ref()
                    .ok_or_else(|| Error::Refused("relation registry read-back is unavailable".to_owned()))?;
                let row = registry
                    .relation_rows
                    .iter()
                    .find(|row| row.object_id == verified.actor_id())
                    .ok_or_else(|| Error::Refused("actor relation row disappeared after write".to_owned()))?;
                if !row
                    .communities
                    .iter()
                    .any(|relation| relation.community_id == community_id && relation.goodwill == *new_value)
                {
                    return Err(Error::Refused(format!("relation read-back failed for '{faction_key}'")));
                }
            }
            Change::SetUpgrades {
                target_object,
                new_value,
                ..
            } => {
                let record = verified
                    .registry_objects()
                    .iter()
                    .find(|record| record.object_id == *target_object)
                    .ok_or_else(|| Error::Refused(format!("upgraded object 0x{target_object:04X} disappeared")))?;
                let (actual, _, _) = read_upgrade_vector(verified.raw_image(), record)?;
                if actual != *new_value {
                    return Err(Error::Refused(format!(
                        "upgrade read-back failed for 0x{target_object:04X}"
                    )));
                }
            }
            Change::AddInfoPortions { info_portions, .. } => {
                let registry = verified
                    .relation_registry
                    .as_ref()
                    .ok_or_else(|| Error::Refused("info-portion registry read-back is unavailable".to_owned()))?;
                let row = registry
                    .info_rows
                    .iter()
                    .find(|row| row.object_id == verified.actor_id())
                    .ok_or_else(|| Error::Refused("actor info-portion row disappeared after write".to_owned()))?;
                if info_portions.iter().any(|name| !row.names.contains(name)) {
                    return Err(Error::Refused(
                        "info-portion read-back did not contain every requested name".to_owned(),
                    ));
                }
            }
            Change::RelocateActor { destination_changer } => {
                let destination = source
                    .level_changer_destinations()?
                    .into_iter()
                    .find(|(handle, _)| handle == destination_changer)
                    .map(|(_, destination)| destination)
                    .ok_or_else(|| Error::Refused("relocation destination disappeared".to_owned()))?;
                let position = destination
                    .dest_position
                    .ok_or_else(|| Error::Refused("destination has no position".to_owned()))?;
                let direction = destination
                    .dest_direction
                    .ok_or_else(|| Error::Refused("destination has no direction".to_owned()))?;
                let game_vertex = destination
                    .dest_game_vertex_id
                    .ok_or_else(|| Error::Refused("destination has no game vertex".to_owned()))?;
                let level_vertex = destination
                    .dest_level_vertex_id
                    .ok_or_else(|| Error::Refused("destination has no level vertex".to_owned()))?;
                let actor = verified
                    .registry_objects()
                    .iter()
                    .find(|record| record.object_id == verified.actor_id())
                    .ok_or_else(|| Error::damaged("X-Ray actor is missing after relocation"))?;
                let position_offset = actor_spawn_position_offset(verified.raw_image(), actor)?;
                let update_position = actor
                    .update_offset
                    .checked_add(11)
                    .ok_or_else(|| Error::damaged("actor UPDATE position offset overflow"))?;
                let direction_offset = position_offset
                    .checked_add(12)
                    .ok_or_else(|| Error::damaged("actor direction offset overflow"))?;
                let level_vertex_offset = actor
                    .state_offset
                    .checked_add(10)
                    .ok_or_else(|| Error::damaged("actor graph vertex offset overflow"))?;
                if read_vector(verified.raw_image(), position_offset)? != position
                    || read_vector(verified.raw_image(), direction_offset)? != direction
                    || read_vector(verified.raw_image(), update_position)? != position
                    || read_u16(verified.raw_image(), actor.state_offset)? != game_vertex
                    || read_u32(verified.raw_image(), level_vertex_offset)? != level_vertex
                {
                    return Err(Error::Refused("actor relocation read-back failed".to_owned()));
                }
            }
            Change::SetMoney { .. }
            | Change::SetStack { .. }
            | Change::SetDurability { .. }
            | Change::SetPlacement { .. }
            | Change::MoveItem { .. }
            | Change::RemoveItem { .. }
            | Change::AddItem { .. } => {}
        }
    }
    Ok(())
}

#[derive(Debug)]
struct PendingWrite {
    offset: usize,
    bytes: [u8; 4],
    length: usize,
}

impl PendingWrite {
    fn u8(offset: usize, value: u8) -> Self {
        Self {
            offset,
            bytes: [value, 0, 0, 0],
            length: 1,
        }
    }

    fn u16(offset: usize, value: u16) -> Self {
        let [first, second] = value.to_le_bytes();
        Self {
            offset,
            bytes: [first, second, 0, 0],
            length: 2,
        }
    }

    fn u32(offset: usize, value: u32) -> Self {
        Self {
            offset,
            bytes: value.to_le_bytes(),
            length: 4,
        }
    }

    fn i32(offset: usize, value: i32) -> Self {
        Self {
            offset,
            bytes: value.to_le_bytes(),
            length: 4,
        }
    }

    fn f32(offset: usize, value: f32) -> Self {
        Self {
            offset,
            bytes: value.to_le_bytes(),
            length: 4,
        }
    }
}

fn matching_faction_catalog(catalog: Option<&FactionCatalog>, format: crate::Format) -> Result<&FactionCatalog> {
    let catalog = catalog.ok_or_else(|| Error::Refused("matching faction catalog is required".to_owned()))?;
    if catalog.release_id() != format.id() {
        return Err(Error::Refused(format!(
            "faction catalog '{}' does not match {}",
            catalog.release_id(),
            format.id()
        )));
    }
    Ok(catalog)
}

fn matching_upgrade_catalog(catalog: Option<&UpgradeCatalog>, format: crate::Format) -> Result<&UpgradeCatalog> {
    let catalog = catalog.ok_or_else(|| Error::Refused("matching upgrade catalog is required".to_owned()))?;
    if catalog.release_id() != format.id() || catalog.upgrades().is_empty() {
        return Err(Error::Refused(format!(
            "upgrade catalog '{}' does not match {} or is empty",
            catalog.release_id(),
            format.id()
        )));
    }
    Ok(catalog)
}

fn relation_payload_for<'a>(payload: &'a mut Option<Vec<u8>>, save: &Save) -> Result<&'a mut Vec<u8>> {
    if payload.is_none() {
        *payload = Some(save.relation_chunk_bytes(save.raw_image())?.to_vec());
    }
    payload
        .as_mut()
        .ok_or_else(|| Error::damaged("X-Ray relation payload disappeared"))
}

fn patch_relation_payload(
    payload: &mut Vec<u8>,
    has_timestamps: bool,
    actor_id: u16,
    community_id: i32,
    goodwill: i32,
) -> Result<()> {
    let registry = crate::save::parse_relation_registry(payload, has_timestamps)?;
    let row = registry
        .relation_rows
        .iter()
        .find(|row| row.object_id == actor_id)
        .ok_or_else(|| Error::Refused("actor relation row is absent".to_owned()))?;
    if let Some(existing) = row
        .communities
        .iter()
        .find(|relation| relation.community_id == community_id)
    {
        write_i32(payload, existing.goodwill_offset, goodwill)?;
        return Ok(());
    }
    let count_end = row
        .community_count_offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("community count range overflow"))?;
    let current_count = read_u32(payload, row.community_count_offset)?;
    let new_count = current_count
        .checked_add(1)
        .filter(|count| *count <= 1_000_000)
        .ok_or_else(|| Error::Refused("community relation count exceeds its limit".to_owned()))?;
    let mut communities: Vec<(i32, i32)> = row
        .communities
        .iter()
        .map(|relation| (relation.community_id, relation.goodwill))
        .collect();
    communities.push((community_id, goodwill));
    communities.sort_unstable_by_key(|relation| relation.0);
    let mut replacement = Vec::with_capacity(
        row.end
            .checked_sub(row.start)
            .and_then(|length| length.checked_add(8))
            .ok_or_else(|| Error::damaged("relation row replacement length overflow"))?,
    );
    replacement.extend_from_slice(
        payload
            .get(row.start..row.community_count_offset)
            .ok_or_else(|| Error::damaged("relation row prefix is outside the chunk"))?,
    );
    replacement.extend_from_slice(&new_count.to_le_bytes());
    for (community, value) in communities {
        replacement.extend_from_slice(&community.to_le_bytes());
        replacement.extend_from_slice(&value.to_le_bytes());
    }
    if count_end > row.end {
        return Err(Error::damaged("community count is outside its relation row"));
    }
    payload.splice(row.start..row.end, replacement);
    Ok(())
}

fn add_info_portions_to_payload(
    payload: &mut Vec<u8>,
    has_timestamps: bool,
    actor_id: u16,
    game_time: u64,
    requested: &[String],
) -> Result<()> {
    if requested.is_empty() {
        return Err(Error::Refused("at least one info portion is required".to_owned()));
    }
    if requested.len() > 1_000_000 {
        return Err(Error::Refused("info-portion count exceeds its limit".to_owned()));
    }
    if requested
        .iter()
        .any(|name| name.is_empty() || name.len() > 1 << 20 || !name.is_ascii() || name.as_bytes().contains(&0))
    {
        return Err(Error::Refused("info portions must be non-empty ASCII names".to_owned()));
    }
    let registry = crate::save::parse_relation_registry(payload, has_timestamps)?;
    let row = registry.info_rows.iter().find(|row| row.object_id == actor_id);
    let existing: HashSet<&str> = row
        .into_iter()
        .flat_map(|row| row.names.iter().map(String::as_str))
        .collect();
    let mut seen = HashSet::new();
    let missing: Vec<&str> = requested
        .iter()
        .map(String::as_str)
        .filter(|name| seen.insert(*name) && !existing.contains(name))
        .collect();
    if missing.is_empty() {
        return Err(Error::Refused(
            "actor already knows every requested info portion".to_owned(),
        ));
    }
    let addition_length = missing
        .iter()
        .try_fold(0_usize, |length, name| {
            let timestamp_length = if has_timestamps { 8_usize } else { 0 };
            length
                .checked_add(name.len())
                .and_then(|size| size.checked_add(1))
                .and_then(|size| size.checked_add(timestamp_length))
        })
        .ok_or_else(|| Error::Refused("info-portion vector length overflow".to_owned()))?;
    if payload
        .len()
        .checked_add(addition_length)
        .is_none_or(|size| size > 512 * 1024 * 1024)
    {
        return Err(Error::Refused(
            "updated relation chunk exceeds the image size limit".to_owned(),
        ));
    }
    let mut additions = Vec::new();
    additions
        .try_reserve_exact(addition_length)
        .map_err(|_| Error::Refused("cannot allocate info-portion payload".to_owned()))?;
    for name in &missing {
        additions.extend_from_slice(name.as_bytes());
        additions.push(0);
        if has_timestamps {
            additions.extend_from_slice(&game_time.to_le_bytes());
        }
    }
    if let Some(row) = row {
        let old_count = read_u32(payload, row.count_offset)?;
        let new_count = old_count
            .checked_add(u32::try_from(missing.len()).map_err(|_| Error::Refused("too many info portions".to_owned()))?)
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(|| Error::Refused("info-portion count exceeds its limit".to_owned()))?;
        payload.splice(row.end_offset..row.end_offset, additions);
        write_u32(payload, row.count_offset, new_count)?;
    } else {
        let old_count = read_u32(payload, 0)?;
        let new_count = old_count
            .checked_add(1)
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(|| Error::Refused("info-portion object count exceeds its limit".to_owned()))?;
        let value_count =
            u32::try_from(missing.len()).map_err(|_| Error::Refused("too many info portions".to_owned()))?;
        let mut entry = Vec::with_capacity(6_usize.saturating_add(additions.len()));
        entry.extend_from_slice(&actor_id.to_le_bytes());
        entry.extend_from_slice(&value_count.to_le_bytes());
        entry.extend_from_slice(&additions);
        payload.splice(registry.info_section_end..registry.info_section_end, entry);
        write_u32(payload, 0, new_count)?;
    }
    Ok(())
}

fn read_upgrade_vector(raw: &[u8], record: &crate::RegistryObject) -> Result<(Vec<String>, usize, usize)> {
    if record.version <= 123 {
        return Err(Error::Refused(
            "upgrade vector is not confirmed for this object version".to_owned(),
        ));
    }
    let state_end = record
        .state_offset
        .checked_add(record.state_length)
        .ok_or_else(|| Error::damaged("upgrade STATE range overflow"))?;
    let state = raw
        .get(record.state_offset..state_end)
        .ok_or_else(|| Error::damaged("upgrade STATE is outside the image"))?;
    let mut reader = Cursor::new(state);
    crate::save::skip_dynamic_visual(&mut reader, record.version)?;
    if record.version > 52 {
        reader.skip(4)?;
    }
    let start = reader.position();
    let count = reader.u32()?;
    if count > 1_000_000
        || usize::try_from(count)
            .ok()
            .is_none_or(|count| count > reader.remaining())
    {
        return Err(Error::damaged("upgrade count exceeds its limit"));
    }
    let mut values = Vec::with_capacity(usize::try_from(count).unwrap_or_default());
    for _ in 0..count {
        let bytes = reader.zero_terminated(1 << 20)?;
        let value = std::str::from_utf8(bytes)
            .map_err(|_| Error::Refused("upgrade vector contains invalid UTF-8".to_owned()))?;
        values.push(value.to_owned());
    }
    let length = reader
        .position()
        .checked_sub(start)
        .ok_or_else(|| Error::damaged("upgrade vector length underflow"))?;
    let offset = record
        .state_offset
        .checked_add(start)
        .ok_or_else(|| Error::damaged("upgrade vector offset overflow"))?;
    Ok((values, offset, length))
}

fn validate_upgrade_vector(
    catalog: &UpgradeCatalog,
    item_key: &str,
    existing: &[String],
    requested: &[String],
) -> Result<()> {
    let mut seen = HashSet::new();
    for key in requested {
        if key.is_empty() || key.len() > 1 << 20 || key.as_bytes().contains(&0) || !seen.insert(key.as_str()) {
            return Err(Error::Refused(
                "upgrade keys must be unique, non-empty UTF-8 strings".to_owned(),
            ));
        }
        if existing.iter().any(|current| current == key) {
            continue;
        }
        let definition = catalog
            .resolve(key)
            .ok_or_else(|| Error::Refused(format!("upgrade '{key}' is not in the catalog")))?;
        if !definition.applies_to(item_key) {
            return Err(Error::Refused(format!(
                "upgrade '{key}' does not apply to '{item_key}'"
            )));
        }
    }
    Ok(())
}

fn replace_upgrade_record(
    raw: &[u8],
    record: &crate::RegistryObject,
    vector_offset: usize,
    vector_length: usize,
    requested: &[String],
) -> Result<Vec<u8>> {
    let start = record.record_offset;
    let end = start
        .checked_add(record.record_length)
        .ok_or_else(|| Error::damaged("upgrade record range overflow"))?;
    let bytes = raw
        .get(start..end)
        .ok_or_else(|| Error::damaged("upgrade record is outside the image"))?;
    let spawn_length = usize::from(read_u16(bytes, 0)?);
    let spawn_start = 2_usize;
    let spawn_end = spawn_start
        .checked_add(spawn_length)
        .ok_or_else(|| Error::damaged("upgrade SPAWN range overflow"))?;
    let update_header = spawn_end;
    let update_length = usize::from(read_u16(bytes, update_header)?);
    let update_start = update_header
        .checked_add(2)
        .ok_or_else(|| Error::damaged("upgrade UPDATE offset overflow"))?;
    if update_start.checked_add(update_length) != Some(bytes.len()) {
        return Err(Error::damaged("upgrade record framing is inconsistent"));
    }
    let spawn_absolute = start
        .checked_add(spawn_start)
        .ok_or_else(|| Error::damaged("upgrade SPAWN absolute offset overflow"))?;
    let vector_local = vector_offset
        .checked_sub(spawn_absolute)
        .ok_or_else(|| Error::damaged("upgrade vector is outside its SPAWN"))?;
    let vector_end = vector_local
        .checked_add(vector_length)
        .ok_or_else(|| Error::damaged("upgrade vector range overflow"))?;
    if vector_end > spawn_length {
        return Err(Error::damaged("upgrade vector exceeds its SPAWN"));
    }
    let encoded_length = requested
        .iter()
        .try_fold(4_usize, |length, key| length.checked_add(key.len())?.checked_add(1))
        .ok_or_else(|| Error::Refused("upgrade vector length overflow".to_owned()))?;
    if requested.len() > 1_000_000 || encoded_length > u16::MAX as usize {
        return Err(Error::Refused("upgrade vector exceeds its STATE size field".to_owned()));
    }
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(encoded_length)
        .map_err(|_| Error::Refused("cannot allocate upgrade vector".to_owned()))?;
    encoded.extend_from_slice(
        &u32::try_from(requested.len())
            .map_err(|_| Error::Refused("too many upgrades".to_owned()))?
            .to_le_bytes(),
    );
    for key in requested {
        encoded.extend_from_slice(key.as_bytes());
        encoded.push(0);
    }
    let delta = isize::try_from(encoded.len())
        .map_err(|_| Error::Refused("upgrade vector is too long".to_owned()))?
        .checked_sub(isize::try_from(vector_length).map_err(|_| Error::damaged("upgrade vector length overflow"))?)
        .ok_or_else(|| Error::Refused("upgrade vector length delta overflow".to_owned()))?;
    let state_size_absolute = record
        .state_offset
        .checked_sub(2)
        .ok_or_else(|| Error::damaged("STATE size field is outside the SPAWN"))?;
    let state_size_offset = state_size_absolute
        .checked_sub(spawn_absolute)
        .ok_or_else(|| Error::damaged("STATE size field is outside the SPAWN"))?;
    let state_size_position = 2_usize
        .checked_add(state_size_offset)
        .ok_or_else(|| Error::damaged("STATE size field offset overflow"))?;
    let state_size = i32::from(read_u16(bytes, state_size_position)?);
    let new_state_size = state_size
        .checked_add(i32::try_from(delta).map_err(|_| Error::Refused("STATE size delta overflow".to_owned()))?)
        .filter(|size| (2..=i32::from(u16::MAX)).contains(size))
        .ok_or_else(|| Error::Refused("new STATE size exceeds its u16 field".to_owned()))?;
    let new_spawn_length = i32::try_from(spawn_length)
        .ok()
        .and_then(|length| length.checked_add(i32::try_from(delta).ok()?))
        .filter(|length| (1..=i32::from(u16::MAX)).contains(length))
        .ok_or_else(|| Error::Refused("new SPAWN size exceeds its u16 field".to_owned()))?;
    let mut spawn = Vec::with_capacity(usize::try_from(new_spawn_length).unwrap_or_default());
    let vector_prefix_end = spawn_start
        .checked_add(vector_local)
        .ok_or_else(|| Error::damaged("upgrade vector prefix range overflow"))?;
    let vector_suffix_start = spawn_start
        .checked_add(vector_end)
        .ok_or_else(|| Error::damaged("upgrade vector suffix offset overflow"))?;
    let vector_prefix = bytes
        .get(spawn_start..vector_prefix_end)
        .ok_or_else(|| Error::damaged("upgrade vector prefix is outside the SPAWN"))?;
    let vector_suffix = bytes
        .get(vector_suffix_start..spawn_end)
        .ok_or_else(|| Error::damaged("upgrade vector suffix is outside the SPAWN"))?;
    let update_bytes = bytes
        .get(update_start..)
        .ok_or_else(|| Error::damaged("UPDATE packet is outside the record"))?;
    spawn.extend_from_slice(vector_prefix);
    spawn.extend_from_slice(&encoded);
    spawn.extend_from_slice(vector_suffix);
    write_u16(
        &mut spawn,
        state_size_offset,
        u16::try_from(new_state_size).unwrap_or_default(),
    )?;
    let mut replacement = Vec::with_capacity(4_usize.saturating_add(spawn.len()).saturating_add(update_length));
    replacement.extend_from_slice(
        &u16::try_from(spawn.len())
            .map_err(|_| Error::Refused("new SPAWN size exceeds u16".to_owned()))?
            .to_le_bytes(),
    );
    replacement.extend_from_slice(&spawn);
    replacement.extend_from_slice(
        &u16::try_from(update_length)
            .map_err(|_| Error::Refused("UPDATE size exceeds u16".to_owned()))?
            .to_le_bytes(),
    );
    replacement.extend_from_slice(update_bytes);
    Ok(replacement)
}

fn actor_spawn_position_offset(raw: &[u8], actor: &crate::RegistryObject) -> Result<usize> {
    let record_end = actor
        .record_offset
        .checked_add(actor.record_length)
        .ok_or_else(|| Error::damaged("actor record range overflow"))?;
    let mut offset = actor
        .record_offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("actor spawn header offset overflow"))?;
    for _ in 0..2 {
        let bytes = raw
            .get(offset..record_end)
            .ok_or_else(|| Error::damaged("actor spawn header is truncated"))?;
        let length = bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| Error::damaged("actor spawn string is unterminated"))?;
        offset = offset
            .checked_add(length)
            .and_then(|offset| offset.checked_add(1))
            .ok_or_else(|| Error::damaged("actor spawn string offset overflow"))?;
    }
    offset = offset
        .checked_add(2)
        .ok_or_else(|| Error::damaged("actor spawn position offset overflow"))?;
    if offset.checked_add(24).is_none_or(|end| end > actor.state_offset) {
        return Err(Error::Refused(
            "actor record lacks the verified spawn position layout".to_owned(),
        ));
    }
    Ok(offset)
}

fn read_vector(raw: &[u8], offset: usize) -> Result<crate::Vector3> {
    let y_offset = offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("X-Ray vector offset overflow"))?;
    let z_offset = offset
        .checked_add(8)
        .ok_or_else(|| Error::damaged("X-Ray vector offset overflow"))?;
    Ok(crate::Vector3 {
        x: read_f32(raw, offset)?,
        y: read_f32(raw, y_offset)?,
        z: read_f32(raw, z_offset)?,
    })
}

fn vector_is_finite(value: crate::Vector3) -> bool {
    value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("X-Ray u32 range overflow"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| Error::damaged("X-Ray u32 field is outside the buffer"))?;
    let value = <[u8; 4]>::try_from(value).map_err(|_| Error::damaged("X-Ray u32 field has the wrong width"))?;
    Ok(u32::from_le_bytes(value))
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) -> Result<()> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("X-Ray u32 range overflow"))?;
    let target = bytes
        .get_mut(offset..end)
        .ok_or_else(|| Error::damaged("X-Ray u32 field is outside the buffer"))?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_i32(bytes: &mut [u8], offset: usize, value: i32) -> Result<()> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("X-Ray i32 range overflow"))?;
    let target = bytes
        .get_mut(offset..end)
        .ok_or_else(|| Error::damaged("X-Ray i32 field is outside the buffer"))?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("X-Ray f32 range overflow"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| Error::damaged("X-Ray f32 field is outside the buffer"))?;
    let value = <[u8; 4]>::try_from(value).map_err(|_| Error::damaged("X-Ray f32 field has the wrong width"))?;
    Ok(f32::from_le_bytes(value))
}

fn clone_template_record(
    save: &Save,
    template: &crate::RegistryObject,
    item_key: &str,
    object_id: u16,
    quantity: u16,
) -> Result<Vec<u8>> {
    let raw = save.raw_image();
    let record_end = template
        .record_offset
        .checked_add(template.record_length)
        .ok_or_else(|| Error::damaged("X-Ray template record range overflow"))?;
    let record = raw
        .get(template.record_offset..record_end)
        .ok_or_else(|| Error::damaged("X-Ray template record is outside the image"))?;
    let spawn_length_bytes = record
        .get(..2)
        .ok_or_else(|| Error::damaged("X-Ray template record is shorter than its SPAWN length"))?;
    let spawn_length_bytes = <[u8; 2]>::try_from(spawn_length_bytes)
        .map_err(|_| Error::damaged("X-Ray template SPAWN length has the wrong width"))?;
    let spawn_length = usize::from(u16::from_le_bytes(spawn_length_bytes));
    let spawn_end = 2_usize
        .checked_add(spawn_length)
        .ok_or_else(|| Error::damaged("X-Ray template SPAWN range overflow"))?;
    let update_length_end = spawn_end
        .checked_add(2)
        .ok_or_else(|| Error::damaged("X-Ray template UPDATE header overflow"))?;
    let update_length_bytes = record
        .get(spawn_end..update_length_end)
        .ok_or_else(|| Error::damaged("X-Ray template record has no UPDATE length"))?;
    let update_length_bytes = <[u8; 2]>::try_from(update_length_bytes)
        .map_err(|_| Error::damaged("X-Ray template UPDATE length has the wrong width"))?;
    let update_length = usize::from(u16::from_le_bytes(update_length_bytes));
    let update_end = update_length_end
        .checked_add(update_length)
        .ok_or_else(|| Error::damaged("X-Ray template UPDATE range overflow"))?;
    if update_end != record.len() {
        return Err(Error::damaged("X-Ray template record framing is inconsistent"));
    }
    let spawn_absolute = template
        .record_offset
        .checked_add(2)
        .ok_or_else(|| Error::damaged("X-Ray template SPAWN offset overflow"))?;
    let mut spawn = record
        .get(2..spawn_end)
        .ok_or_else(|| Error::damaged("X-Ray template SPAWN is truncated"))?
        .to_vec();
    let name_start = 2_usize;
    let name_terminator = spawn
        .get(name_start..)
        .and_then(|remaining| remaining.iter().position(|byte| *byte == 0))
        .and_then(|relative| name_start.checked_add(relative))
        .ok_or_else(|| Error::damaged("X-Ray template section name is unterminated"))?;
    let name_end = name_terminator
        .checked_add(1)
        .ok_or_else(|| Error::damaged("X-Ray template section name range overflow"))?;
    let mut replacement_name = item_key.as_bytes().to_vec();
    replacement_name.push(0);
    let original_name_length = name_end
        .checked_sub(name_start)
        .ok_or_else(|| Error::damaged("template section name range underflow"))?;
    let name_delta = isize::try_from(replacement_name.len())
        .map_err(|_| Error::Refused("item key length overflow".to_owned()))?
        .checked_sub(
            isize::try_from(original_name_length)
                .map_err(|_| Error::damaged("template section name length overflow"))?,
        )
        .ok_or_else(|| Error::Refused("item key length delta overflow".to_owned()))?;
    spawn.splice(name_start..name_end, replacement_name);
    let adjust = |offset: usize| -> Result<usize> {
        usize::try_from(
            isize::try_from(offset)
                .map_err(|_| Error::damaged("X-Ray template field offset overflow"))?
                .checked_add(name_delta)
                .ok_or_else(|| Error::damaged("X-Ray template field offset overflow"))?,
        )
        .map_err(|_| Error::damaged("negative X-Ray template field offset"))
    };
    let object_id_offset = adjust(
        template
            .object_id_offset
            .checked_sub(spawn_absolute)
            .ok_or_else(|| Error::damaged("X-Ray object id is outside its SPAWN"))?,
    )?;
    let parent_id_offset = adjust(
        template
            .parent_id_offset
            .checked_sub(spawn_absolute)
            .ok_or_else(|| Error::damaged("X-Ray parent id is outside its SPAWN"))?,
    )?;
    write_u16(&mut spawn, object_id_offset, object_id)?;
    write_u16(&mut spawn, parent_id_offset, save.actor_id())?;

    let state_offset = adjust(
        template
            .state_offset
            .checked_sub(spawn_absolute)
            .ok_or_else(|| Error::damaged("X-Ray STATE is outside its SPAWN"))?,
    )?;
    let mut state_length = template.state_length;
    if template.version > 123 {
        let (vector_offset, vector_length, vector_count) = upgrade_vector_range(raw, template)?;
        if vector_count > 0 {
            let vector_offset = adjust(
                vector_offset
                    .checked_sub(spawn_absolute)
                    .ok_or_else(|| Error::damaged("X-Ray upgrades are outside their SPAWN"))?,
            )?;
            let vector_end = vector_offset
                .checked_add(vector_length)
                .ok_or_else(|| Error::damaged("X-Ray upgrades range overflow"))?;
            if spawn.get(vector_offset..vector_end).is_none() {
                return Err(Error::damaged("X-Ray upgrades vector is outside the template SPAWN"));
            }
            let size_offset = state_offset
                .checked_sub(2)
                .ok_or_else(|| Error::damaged("X-Ray STATE size field is outside the SPAWN"))?;
            let old_size = read_u16(&spawn, size_offset)?;
            let delta = 4_isize
                .checked_sub(
                    isize::try_from(vector_length)
                        .map_err(|_| Error::damaged("X-Ray upgrades vector length overflow"))?,
                )
                .ok_or_else(|| Error::damaged("X-Ray upgrades size delta overflow"))?;
            let delta_i32 =
                i32::try_from(delta).map_err(|_| Error::damaged("X-Ray upgrades size delta exceeds i32"))?;
            let new_size = i32::from(old_size)
                .checked_add(delta_i32)
                .filter(|size| (2..=i32::from(u16::MAX)).contains(size))
                .ok_or_else(|| Error::Refused("cloned X-Ray STATE size exceeds its u16 framing".to_owned()))?;
            spawn.splice(vector_offset..vector_end, [0_u8; 4]);
            write_u16(
                &mut spawn,
                size_offset,
                u16::try_from(new_size)
                    .map_err(|_| Error::Refused("cloned X-Ray STATE size exceeds u16".to_owned()))?,
            )?;
            state_length = usize::try_from(
                isize::try_from(state_length)
                    .map_err(|_| Error::damaged("X-Ray STATE length overflow"))?
                    .checked_add(delta)
                    .ok_or_else(|| Error::damaged("X-Ray STATE length overflow"))?,
            )
            .map_err(|_| Error::damaged("negative X-Ray STATE length"))?;
        }
    }

    if let Some(client_offset) = template.client_data_offset {
        if template.client_data_length >= 2 {
            let client_offset = adjust(
                client_offset
                    .checked_sub(spawn_absolute)
                    .ok_or_else(|| Error::damaged("X-Ray client data is outside its SPAWN"))?,
            )?;
            let place_offset = client_offset
                .checked_add(1)
                .ok_or_else(|| Error::damaged("X-Ray client placement offset overflow"))?;
            let place_end = place_offset
                .checked_add(2)
                .ok_or_else(|| Error::damaged("X-Ray client placement range overflow"))?;
            if let Some(bytes) = spawn.get(place_offset..place_end) {
                let Ok(bytes) = <[u8; 2]>::try_from(bytes) else {
                    return Err(Error::damaged("X-Ray client placement has the wrong width"));
                };
                let place = u16::from_le_bytes(bytes);
                if valid_packed_placement(place) {
                    write_u16(&mut spawn, place_offset, (place & 0xFFF0) | 3)?;
                } else if spawn.get(client_offset) == Some(&2)
                    && spawn.get(place_offset).is_some_and(|kind| (1..=3).contains(kind))
                {
                    if let Some(kind) = spawn.get_mut(place_offset) {
                        *kind = 3;
                    }
                }
            }
        }
    }

    if item_key.to_ascii_lowercase().starts_with("ammo_") {
        let state_end = state_offset
            .checked_add(state_length)
            .ok_or_else(|| Error::damaged("cloned ammo STATE range overflow"))?;
        let state = spawn
            .get(state_offset..state_end)
            .ok_or_else(|| Error::damaged("cloned ammo STATE is outside its SPAWN"))?;
        let mut reader = Cursor::new(state);
        crate::save::skip_dynamic_visual(&mut reader, template.version)?;
        if template.version > 52 {
            reader.skip(4)?;
        }
        if template.version > 123 {
            skip_string_vector(&mut reader)?;
        }
        let count_offset = state_offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("cloned ammo count offset overflow"))?;
        write_u16(&mut spawn, count_offset, quantity)?;
        if update_length < 5 {
            return Err(Error::Refused(
                "ammo template UPDATE packet does not expose a count".to_owned(),
            ));
        }
    }

    let new_spawn_length = u16::try_from(spawn.len())
        .map_err(|_| Error::Refused("cloned X-Ray SPAWN exceeds its u16 framing".to_owned()))?;
    let mut cloned = Vec::with_capacity(4_usize.saturating_add(spawn.len()).saturating_add(update_length));
    cloned.extend_from_slice(&new_spawn_length.to_le_bytes());
    cloned.extend_from_slice(&spawn);
    cloned.extend_from_slice(
        &u16::try_from(update_length)
            .map_err(|_| Error::Refused("cloned X-Ray UPDATE exceeds u16".to_owned()))?
            .to_le_bytes(),
    );
    let mut update = record
        .get(update_length_end..update_end)
        .ok_or_else(|| Error::damaged("X-Ray template UPDATE is truncated"))?
        .to_vec();
    if item_key.to_ascii_lowercase().starts_with("ammo_") {
        let update_count_offset = update
            .len()
            .checked_sub(2)
            .ok_or_else(|| Error::damaged("ammo UPDATE count offset underflow"))?;
        write_u16(&mut update, update_count_offset, quantity)?;
    }
    cloned.extend_from_slice(&update);
    Ok(cloned)
}

fn upgrade_vector_range(raw: &[u8], record: &crate::RegistryObject) -> Result<(usize, usize, u32)> {
    let state_end = record
        .state_offset
        .checked_add(record.state_length)
        .ok_or_else(|| Error::damaged("X-Ray upgrades STATE range overflow"))?;
    let state = raw
        .get(record.state_offset..state_end)
        .ok_or_else(|| Error::damaged("X-Ray upgrades STATE is outside the image"))?;
    let mut reader = Cursor::new(state);
    crate::save::skip_dynamic_visual(&mut reader, record.version)?;
    if record.version > 52 {
        reader.skip(4)?;
    }
    let start = reader.position();
    let count = reader.u32()?;
    if count > 1_000_000 {
        return Err(Error::damaged("X-Ray upgrades count exceeds its limit"));
    }
    for _ in 0..count {
        let _ = reader.zero_terminated(1 << 20)?;
    }
    let length = reader
        .position()
        .checked_sub(start)
        .ok_or_else(|| Error::damaged("X-Ray upgrades vector length underflow"))?;
    let offset = record
        .state_offset
        .checked_add(start)
        .ok_or_else(|| Error::damaged("X-Ray upgrades vector offset overflow"))?;
    Ok((offset, length, count))
}

fn skip_string_vector(reader: &mut Cursor<'_>) -> Result<()> {
    let count = reader.u32()?;
    if count > 1_000_000 {
        return Err(Error::damaged("X-Ray string vector count exceeds its limit"));
    }
    for _ in 0..count {
        let _ = reader.zero_terminated(1 << 20)?;
    }
    Ok(())
}

fn valid_packed_placement(value: u16) -> bool {
    match value & 0x0F {
        1 => (1..14).contains(&((value >> 4) & 0x3F)) && (1..14).contains(&((value >> 10) & 0x3F)),
        2 | 3 => true,
        _ => false,
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| Error::damaged("X-Ray u16 range overflow"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| Error::damaged("X-Ray u16 field is outside its record"))?;
    let value = <[u8; 2]>::try_from(value).map_err(|_| Error::damaged("X-Ray u16 field has the wrong width"))?;
    Ok(u16::from_le_bytes(value))
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) -> Result<()> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| Error::damaged("X-Ray u16 range overflow"))?;
    let target = bytes
        .get_mut(offset..end)
        .ok_or_else(|| Error::damaged("X-Ray u16 field is outside its record"))?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::type_complexity
)]
mod tests {
    use super::{
        actor_spawn_position_offset, apply, apply_with_catalog, capability, read_u16, read_u32, read_vector,
        Capability, Change, ChangeKind, ChangeSet, Placement,
    };
    use crate::{Format, Save};
    use sse_catalog::{UpgradeCatalog, UpgradeDefinition};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn faction_writes_match_three_reference_fixture_pairs() -> TestResult {
        let cases: [(&[u8], &[u8], &[u8], &[u8], i32); 3] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-factions/soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-factions/soc-player-faction.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-factions/soc-relations.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-factions/soc-expected.sav"),
                12,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-factions/cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-factions/cs-player-faction.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-factions/cs-relations.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-factions/cs-expected.sav"),
                6,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-factions/cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-factions/cop-player-faction.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-factions/cop-relations.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-factions/cop-expected.sav"),
                1,
            ),
        ];
        for (source_bytes, player_expected, relation_expected, combined_expected, _) in cases {
            let source = Save::read(source_bytes)?;
            let player = ChangeSet::new(vec![Change::SetPlayerFaction {
                target_object: source.actor_id(),
                old_value: source.player_faction().ok_or("actor faction should be present")?,
                faction_key: "bandit".to_owned(),
            }]);
            assert_eq!(apply(&source, &player)?.as_slice(), player_expected);

            let relation = ChangeSet::new(vec![Change::SetFactionRelation {
                target_object: source.actor_id(),
                faction_key: "bandit".to_owned(),
                old_value: None,
                new_value: 375,
            }]);
            assert_eq!(apply(&source, &relation)?.as_slice(), relation_expected);
            let combined = ChangeSet::new(vec![player.changes()[0].clone(), relation.changes()[0].clone()]);
            assert_eq!(apply(&source, &combined)?.as_slice(), combined_expected);
        }
        Ok(())
    }

    #[test]
    fn info_portions_preserve_format_timestamps_and_other_chunks() -> TestResult {
        let cases: [(&[u8], bool, i32); 3] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-factions/soc-source.sav"),
                true,
                12,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-factions/cs-source.sav"),
                true,
                6,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-factions/cop-source.sav"),
                false,
                1,
            ),
        ];
        for (source_bytes, has_timestamps, bandit_id) in cases {
            let source = Save::read(source_bytes)?;
            let raw_relation = source.relation_chunk_bytes(source.raw_image())?.to_vec();
            let old_registry = crate::save::parse_relation_registry(&raw_relation, has_timestamps)?;
            let old_info_row = old_registry
                .info_rows
                .iter()
                .find(|row| row.object_id == source.actor_id())
                .cloned();
            let requested = vec!["codex_fixture_flag".to_owned()];
            let changes = ChangeSet::new(vec![
                Change::SetFactionRelation {
                    target_object: source.actor_id(),
                    faction_key: "bandit".to_owned(),
                    old_value: None,
                    new_value: 375,
                },
                Change::AddInfoPortions {
                    target_object: source.actor_id(),
                    info_portions: requested.clone(),
                },
            ]);
            let output = apply(&source, &changes)?;
            let verified = Save::read(output.as_slice())?;
            let registry = verified
                .relation_registry
                .as_ref()
                .ok_or("relation registry should read back")?;
            let info_row = registry
                .info_rows
                .iter()
                .find(|row| row.object_id == verified.actor_id())
                .ok_or("actor info row should read back")?;
            assert!(info_row.names.contains(&requested[0]));
            let actor_relation = registry
                .relation_rows
                .iter()
                .find(|row| row.object_id == verified.actor_id())
                .ok_or("actor relation row should read back")?;
            assert!(actor_relation
                .communities
                .iter()
                .any(|relation| relation.community_id == bandit_id && relation.goodwill == 375));
            assert_eq!(verified.money()?, source.money()?);

            let output_container = crate::container::Container::read(output.as_slice())?;
            let mut output_relation = None;
            for chunk in output_container.chunks() {
                if chunk.kind == 9 {
                    output_relation = Some(output_container.chunk_bytes(*chunk)?);
                }
            }
            let output_relation = output_relation.ok_or("relation chunk should remain")?;
            let insertion_offset = old_info_row
                .as_ref()
                .map_or(old_registry.info_section_end, |row| row.end_offset);
            let entry = if old_info_row.is_some() {
                let mut bytes = b"codex_fixture_flag\0".to_vec();
                if has_timestamps {
                    bytes.extend_from_slice(&source.game_time().to_le_bytes());
                }
                bytes
            } else {
                let mut bytes = source.actor_id().to_le_bytes().to_vec();
                bytes.extend_from_slice(&1_u32.to_le_bytes());
                bytes.extend_from_slice(b"codex_fixture_flag\0");
                if has_timestamps {
                    bytes.extend_from_slice(&source.game_time().to_le_bytes());
                }
                bytes
            };
            assert_eq!(
                output_relation.get(insertion_offset..insertion_offset + entry.len()),
                Some(entry.as_slice())
            );

            let original_container = crate::container::Container::read(source_bytes)?;
            for original_chunk in original_container.chunks() {
                if original_chunk.kind == 9 {
                    continue;
                }
                let old = original_container.chunk_bytes(*original_chunk)?;
                let new = output_container
                    .chunks()
                    .iter()
                    .find(|chunk| chunk.kind == original_chunk.kind)
                    .ok_or("unmodified chunk should remain")?;
                assert_eq!(output_container.chunk_bytes(*new)?, old);
            }
        }
        Ok(())
    }

    #[test]
    fn relocation_uses_a_save_resident_anchor_and_updates_all_confirmed_fields() -> TestResult {
        let source = Save::read(include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat.sav"))?;
        let actor = source
            .registry_objects()
            .iter()
            .find(|record| record.object_id == source.actor_id())
            .ok_or("actor record should exist")?;
        let old_objects = source.object_chunk_bytes(source.raw_image())?;
        let first_record = source
            .registry_objects()
            .first()
            .ok_or("first object record should exist")?;
        let actor_record_offset = 4_usize
            .checked_add(
                actor
                    .record_offset
                    .checked_sub(first_record.record_offset)
                    .ok_or("actor record precedes the first object")?,
            )
            .ok_or("actor record offset overflow")?;
        let spawn_length = usize::from(read_u16(old_objects, actor_record_offset)?);
        let update_length_offset = actor_record_offset
            .checked_add(2)
            .and_then(|offset| offset.checked_add(spawn_length))
            .ok_or("actor UPDATE length offset overflow")?;
        let update_length = usize::from(read_u16(old_objects, update_length_offset)?);
        assert!(
            update_length < 23,
            "fixture is expected to have a short actor UPDATE packet"
        );
        let update_start = update_length_offset.checked_add(2).ok_or("UPDATE start overflow")?;
        let insert_at = update_start.checked_add(update_length).ok_or("UPDATE end overflow")?;
        let extension = 23_usize
            .checked_sub(update_length)
            .ok_or("UPDATE extension underflow")?;
        let mut object_payload = old_objects.to_vec();
        object_payload.splice(insert_at..insert_at, std::iter::repeat_n(0_u8, extension));
        let new_length_end = update_length_offset
            .checked_add(2)
            .ok_or("UPDATE length range overflow")?;
        object_payload
            .get_mut(update_length_offset..new_length_end)
            .ok_or("UPDATE length field is missing")?
            .copy_from_slice(&23_u16.to_le_bytes());
        let actor_position = actor_spawn_position_offset(source.raw_image(), actor)?;
        let position = read_vector(source.raw_image(), actor_position)?;
        let relocation_position = update_start.checked_add(11).ok_or("UPDATE position offset overflow")?;
        for (index, value) in [position.x, position.y, position.z].into_iter().enumerate() {
            let start = relocation_position
                .checked_add(index * 4)
                .ok_or("UPDATE position component offset overflow")?;
            let end = start.checked_add(4).ok_or("UPDATE position component range overflow")?;
            object_payload
                .get_mut(start..end)
                .ok_or("UPDATE position is missing")?
                .copy_from_slice(&value.to_le_bytes());
        }
        let old_count = read_u32(old_objects, 0)?;
        object_payload[..4].copy_from_slice(&old_count.checked_add(1).ok_or("object count overflow")?.to_le_bytes());
        object_payload.extend_from_slice(&synthetic_level_changer_record());
        let extended_raw = source.rebuild_chunks(source.raw_image(), &[(2, &object_payload)])?;
        let extended_packed = source.repack(&extended_raw)?;
        let extended_parse = Save::read(extended_packed.as_slice());
        assert!(
            extended_parse.is_ok(),
            "synthetic level-changer fixture should parse: {extended_parse:?}"
        );
        let extended_save = extended_parse?;
        let (destination_handle, destination) = extended_save
            .level_changer_destinations()?
            .into_iter()
            .find(|(handle, _)| *handle == 0x2222)
            .ok_or("synthetic level changer destination should parse")?;
        assert_eq!(destination_handle, 0x2222);
        let target_position = destination.dest_position.ok_or("destination position should exist")?;
        let target_direction = destination.dest_direction.ok_or("destination direction should exist")?;
        let game_vertex = destination.dest_game_vertex_id.ok_or("game vertex should exist")?;
        let level_vertex = destination.dest_level_vertex_id.ok_or("level vertex should exist")?;
        let changes = ChangeSet::new(vec![Change::RelocateActor {
            destination_changer: destination_handle,
        }]);
        let output = apply(&extended_save, &changes)?;
        let verified = Save::read(output.as_slice())?;
        let verified_actor = verified
            .registry_objects()
            .iter()
            .find(|record| record.object_id == verified.actor_id())
            .ok_or("actor should remain after relocation")?;
        let position_offset = actor_spawn_position_offset(verified.raw_image(), verified_actor)?;
        let update_position = verified_actor.update_offset + 11;
        assert_eq!(read_vector(verified.raw_image(), position_offset)?, target_position);
        assert_eq!(
            read_vector(verified.raw_image(), position_offset + 12)?,
            target_direction
        );
        assert_eq!(read_vector(verified.raw_image(), update_position)?, target_position);
        assert_eq!(
            read_u16(verified.raw_image(), verified_actor.state_offset)?,
            game_vertex
        );
        assert_eq!(
            read_u32(verified.raw_image(), verified_actor.state_offset + 10)?,
            level_vertex
        );
        assert!(apply(
            &extended_save,
            &ChangeSet::new(vec![Change::RelocateActor {
                destination_changer: 0x7777,
            }])
        )
        .is_err());
        Ok(())
    }

    fn synthetic_level_changer_record() -> Vec<u8> {
        let mut spawn = Vec::new();
        spawn.extend_from_slice(&1_u16.to_le_bytes());
        spawn.extend_from_slice(b"level_changer\0level_changer\0");
        spawn.extend_from_slice(&[0; 2 + 24 + 2]);
        spawn.extend_from_slice(&0x2222_u16.to_le_bytes());
        spawn.extend_from_slice(&0_u16.to_le_bytes());
        spawn.extend_from_slice(&[0; 2]);
        spawn.extend_from_slice(&(1_u16 << 5).to_le_bytes());
        spawn.extend_from_slice(&128_u16.to_le_bytes());
        spawn.extend_from_slice(&[0; 2 + 2]);
        spawn.extend_from_slice(&0_u16.to_le_bytes());
        spawn.extend_from_slice(&[0; 2]);

        let mut state = vec![0_u8; 2];
        state.extend_from_slice(&[1, 0]);
        state.extend_from_slice(&[0; 16]);
        state.push(0);
        state.extend_from_slice(&0x1234_u16.to_le_bytes());
        state.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
        for value in [12.5_f32, -3.25, 7.0, 0.0, 1.0, 0.0] {
            state.extend_from_slice(&value.to_le_bytes());
        }
        state.extend_from_slice(b"level_test\0point_test\0");
        state.push(0);
        spawn.extend_from_slice(
            &u16::try_from(state.len() + 2)
                .expect("synthetic state fits its u16 field")
                .to_le_bytes(),
        );
        spawn.extend_from_slice(&state);

        let mut record = Vec::new();
        record.extend_from_slice(
            &u16::try_from(spawn.len())
                .expect("synthetic spawn fits its u16 field")
                .to_le_bytes(),
        );
        record.extend_from_slice(&spawn);
        record.extend_from_slice(&2_u16.to_le_bytes());
        record.extend_from_slice(&0_u16.to_le_bytes());
        record
    }

    #[test]
    fn upgrade_writes_match_two_reference_fixture_pairs() -> TestResult {
        let cases: [(&[u8], &[u8], &[u8]); 2] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-upgrades/xray-upgrades-cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-upgrades/xray-upgrades-cs-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-upgrades/xray-upgrades-cs-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-upgrades/xray-upgrades-cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-upgrades/xray-upgrades-cop-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-upgrades/xray-upgrades-cop-expected.raw"),
            ),
        ];
        for (source_bytes, expected, expected_raw) in cases {
            let source = Save::read(source_bytes)?;
            let record = source
                .registry_objects()
                .iter()
                .find(|record| record.object_id == 13398)
                .ok_or("upgrade target should exist")?;
            let release = source.format().id().to_owned();
            let upgrade = UpgradeDefinition::new(
                "up_c_wpn_test".to_owned(),
                None,
                Some("weapon".to_owned()),
                Some(record.name.clone()),
                "synthetic fixture".to_owned(),
                release.clone(),
                None,
                None,
                None,
                vec![record.name.clone()],
            )?;
            let catalog = UpgradeCatalog::new(release, vec![upgrade])?;
            let changes = ChangeSet::new(vec![Change::SetUpgrades {
                target_object: record.object_id,
                old_value: vec!["up_a_wpn_test".to_owned(), "legacy_unknown".to_owned()],
                new_value: vec!["legacy_unknown".to_owned(), "up_c_wpn_test".to_owned()],
            }]);
            let output = apply_with_catalog(&source, &changes, None, Some(&catalog))?;
            assert_eq!(output.as_slice(), expected);
            assert_eq!(Save::read(output.as_slice())?.raw_image(), expected_raw);
        }
        Ok(())
    }

    #[test]
    fn capability_maturities_match_the_reference_matrix() {
        for format in [Format::Soc, Format::Cs, Format::Cop] {
            for kind in [
                ChangeKind::AddItems,
                ChangeKind::EditMoney,
                ChangeKind::EditStacks,
                ChangeKind::RemoveItems,
            ] {
                assert_eq!(capability(format, kind), Capability::Verified);
            }
            for kind in [
                ChangeKind::EditDurability,
                ChangeKind::EditPlacement,
                ChangeKind::EditPlayerFaction,
                ChangeKind::EditRelations,
                ChangeKind::MoveItems,
            ] {
                assert_eq!(capability(format, kind), Capability::Experimental);
            }
        }
        assert_eq!(
            capability(Format::Soc, ChangeKind::EditUpgrades),
            Capability::Unsupported
        );
        for format in [Format::Cs, Format::Cop] {
            assert_eq!(capability(format, ChangeKind::EditUpgrades), Capability::Experimental);
        }
        for format in [Format::SocEe, Format::CsEe, Format::CopEe] {
            for kind in [
                ChangeKind::AddItems,
                ChangeKind::EditMoney,
                ChangeKind::EditStacks,
                ChangeKind::RemoveItems,
            ] {
                assert_eq!(capability(format, kind), Capability::Experimental);
            }
            for kind in [
                ChangeKind::EditDurability,
                ChangeKind::EditPlacement,
                ChangeKind::EditPlayerFaction,
                ChangeKind::EditRelations,
                ChangeKind::EditUpgrades,
                ChangeKind::MoveItems,
            ] {
                assert_eq!(capability(format, kind), Capability::Unsupported);
            }
        }
    }

    #[test]
    fn money_changes_match_all_six_reference_packed_and_raw_fixtures() -> TestResult {
        let pairs: [(&[u8], &[u8], &[u8]); 6] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cs-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-ee-expected.raw"),
            ),
        ];

        for (source_bytes, expected_packed, expected_raw) in pairs {
            let source = Save::read(source_bytes)?;
            let expected = Save::read(expected_packed)?;
            let actor_id = source
                .registry_objects()
                .iter()
                .find(|object| object.name.eq_ignore_ascii_case("actor"))
                .ok_or_else(|| std::io::Error::other("actor object should be indexed"))?
                .object_id;
            let changes = ChangeSet::new(vec![Change::SetMoney {
                target_object: actor_id,
                old_value: source.money()?,
                new_value: expected.money()?,
            }]);

            let output = apply(&source, &changes)?;
            assert_eq!(output.as_slice(), expected_packed);
            let read_back = Save::read(output.as_slice())?;
            assert_eq!(read_back.raw_image(), expected_raw);

            let inverse = ChangeSet::new(vec![Change::SetMoney {
                target_object: actor_id,
                old_value: expected.money()?,
                new_value: source.money()?,
            }]);
            let restored = apply(&read_back, &inverse)?;
            assert_eq!(restored.as_slice(), source_bytes);
        }
        Ok(())
    }

    #[test]
    fn refuses_a_change_whose_old_value_does_not_match() -> TestResult {
        let source = Save::read(include_bytes!("../../../fixtures/synthetic/xray-soc.sav"))?;
        let actor_id = source
            .registry_objects()
            .iter()
            .find(|object| object.name.eq_ignore_ascii_case("actor"))
            .ok_or_else(|| std::io::Error::other("actor object should be indexed"))?
            .object_id;
        let changes = ChangeSet::new(vec![Change::SetMoney {
            target_object: actor_id,
            old_value: source
                .money()?
                .checked_add(1)
                .ok_or_else(|| std::io::Error::other("money test value overflow"))?,
            new_value: 50,
        }]);

        assert!(apply(&source, &changes).is_err());
        Ok(())
    }

    #[test]
    fn stack_changes_match_all_six_reference_packed_and_raw_fixtures() -> TestResult {
        let pairs: [(&[u8], &[u8], &[u8]); 6] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cs-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-cop-ee-expected.raw"),
            ),
        ];

        for (source_bytes, expected_packed, expected_raw) in pairs {
            let source = Save::read(source_bytes)?;
            let expected = Save::read(expected_packed)?;
            assert_eq!(expected.raw_image(), expected_raw);
            let original_count = source
                .inventory()?
                .into_iter()
                .find(|item| item.handle == 0x1234)
                .and_then(|item| item.count)
                .ok_or_else(|| std::io::Error::other("ammunition count should be indexed"))?;
            let changes = ChangeSet::new(vec![Change::SetStack {
                target_object: 0x1234,
                old_value: original_count,
                new_value: 44,
            }]);

            let output = apply(&source, &changes)?;
            assert_eq!(output.as_slice(), expected_packed);
            let read_back = Save::read(output.as_slice())?;
            assert_eq!(read_back.raw_image(), expected_raw);

            let inverse = ChangeSet::new(vec![Change::SetStack {
                target_object: 0x1234,
                old_value: 44,
                new_value: original_count,
            }]);
            let restored = apply(&read_back, &inverse)?;
            assert_eq!(restored.as_slice(), source_bytes);
        }
        Ok(())
    }

    #[test]
    fn durability_change_matches_the_reference_raw_and_packed_fixture() -> TestResult {
        let pairs: [(&[u8], &[u8], &[u8]); 6] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-soc-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-soc-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cs-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cs-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-soc-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-soc-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-soc-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cs-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cs-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cs-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-ee-expected.raw"),
            ),
        ];
        for (source_bytes, expected_bytes, expected_raw) in pairs {
            let source = Save::read(source_bytes)?;
            if capability(source.format(), ChangeKind::EditDurability) == Capability::Unsupported {
                continue;
            }
            let changes = ChangeSet::new(vec![Change::SetDurability {
                target_object: 13398,
                old_value: 0.25,
                new_value: 0.75,
            }]);
            let output = apply(&source, &changes)?;
            assert_eq!(output.as_slice(), expected_bytes);
            let verified = Save::read(output.as_slice())?;
            assert_eq!(verified.raw_image(), expected_raw);
            let condition = verified
                .inventory()?
                .into_iter()
                .find(|item| item.handle == 13398)
                .and_then(|item| item.condition)
                .ok_or("durability should read back")?;
            let inverse = ChangeSet::new(vec![Change::SetDurability {
                target_object: 13398,
                old_value: condition,
                new_value: 0.25,
            }]);
            assert_eq!(apply(&verified, &inverse)?.as_slice(), source_bytes);
        }
        Ok(())
    }

    #[test]
    fn placement_change_matches_the_reference_raw_and_packed_fixture() -> TestResult {
        let pairs: [(&[u8], &[u8], &[u8], Placement); 9] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-expected.raw"),
                Placement::Slot(3),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cs-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cs-expected.raw"),
                Placement::Slot(3),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-expected.raw"),
                Placement::Slot(3),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-belt-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-belt-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-belt-expected.raw"),
                Placement::Belt,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cs-belt-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cs-belt-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cs-belt-expected.raw"),
                Placement::Belt,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-belt-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-belt-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-belt-expected.raw"),
                Placement::Belt,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-ruck-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-ruck-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-ruck-expected.raw"),
                Placement::Ruck,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cs-ruck-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cs-ruck-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cs-ruck-expected.raw"),
                Placement::Ruck,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-ruck-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-ruck-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-ruck-expected.raw"),
                Placement::Ruck,
            ),
        ];
        for (source_bytes, expected_bytes, expected_raw, destination) in pairs {
            let source = Save::read(source_bytes)?;
            let changes = ChangeSet::new(vec![Change::SetPlacement {
                target_object: 13398,
                destination,
            }]);
            let output = apply(&source, &changes)?;
            assert_eq!(output.as_slice(), expected_bytes);
            let verified = Save::read(output.as_slice())?;
            assert_eq!(verified.raw_image(), expected_raw);
        }
        Ok(())
    }

    #[test]
    fn taking_a_stash_item_matches_the_reference_fixture() -> TestResult {
        let pairs: [(&[u8], &[u8], &[u8]); 3] = [
            (
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-soc-take.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-soc-take.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cs-take.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cs-take.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cop-take.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cop-take.raw"),
            ),
        ];
        for (source_bytes, expected_bytes, expected_raw) in pairs {
            let source = Save::read(source_bytes)?;
            let changes = ChangeSet::new(vec![Change::MoveItem {
                target_object: 9029,
                old_parent: 16,
                new_parent: source.actor_id(),
            }]);
            let output = apply(&source, &changes)?;
            assert_eq!(output.as_slice(), expected_bytes);
            let verified = Save::read(output.as_slice())?;
            assert_eq!(verified.raw_image(), expected_raw);
        }
        Ok(())
    }

    #[test]
    fn putting_a_backpack_item_in_a_stash_changes_only_its_parent() -> TestResult {
        let cases: [(&[u8], &[u8]); 3] = [
            (
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-soc-source.sav"),
                b"stalker-soc",
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cs-source.sav"),
                b"stalker-cs",
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cop-source.sav"),
                b"stalker-cop",
            ),
        ];
        for (source_bytes, format_id) in cases {
            let source = Save::read(source_bytes)?;
            let actor_id = source.actor_id();
            let item = source
                .registry_objects()
                .iter()
                .find(|record| record.object_id == 0x3456)
                .ok_or("backpack item should be in the registry")?;
            if item.parent_id != actor_id {
                return Err("backpack item should be actor-owned".into());
            }
            let mut expected = source.raw_image().to_vec();
            let parent_end = item.parent_id_offset.checked_add(2).ok_or("parent range overflow")?;
            expected
                .get_mut(item.parent_id_offset..parent_end)
                .ok_or("item parent is outside the fixture")?
                .copy_from_slice(&16_u16.to_le_bytes());
            let output = apply(
                &source,
                &ChangeSet::new(vec![Change::MoveItem {
                    target_object: 0x3456,
                    old_parent: actor_id,
                    new_parent: 16,
                }]),
            )?;
            let verified = Save::read(output.as_slice())?;
            let moved = verified
                .registry_objects()
                .iter()
                .find(|record| record.object_id == 0x3456)
                .ok_or("stored item should remain in the registry")?;
            assert_eq!(moved.parent_id, 16);
            assert_eq!(verified.raw_image(), expected);
            assert_eq!(output.as_slice(), source.repack(&expected)?.as_slice(), "{format_id:?}");
            let inverse = ChangeSet::new(vec![Change::MoveItem {
                target_object: 0x3456,
                old_parent: 16,
                new_parent: actor_id,
            }]);
            assert_eq!(apply(&verified, &inverse)?.as_slice(), source_bytes);
        }
        Ok(())
    }

    #[test]
    fn item_removal_matches_all_six_reference_fixtures() -> TestResult {
        let pairs: [(&[u8], &[u8], &[u8]); 6] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-soc-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-soc-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cs-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cs-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cop-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cop-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-soc-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-soc-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-soc-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cs-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cs-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cs-ee-expected.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cop-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cop-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-cop-ee-expected.raw"),
            ),
        ];
        for (source_bytes, expected_bytes, expected_raw) in pairs {
            let source = Save::read(source_bytes)?;
            let changes = ChangeSet::new(vec![Change::RemoveItem { target_object: 4660 }]);
            let output = apply(&source, &changes)?;
            assert_eq!(output.as_slice(), expected_bytes);
            assert_eq!(Save::read(output.as_slice())?.raw_image(), expected_raw);
        }
        Ok(())
    }

    #[test]
    fn item_addition_matches_all_twelve_reference_fixtures() -> TestResult {
        let cases: [(&[u8], &[u8], &[u8], u16, &str, u16, u16); 12] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-expected.raw"),
                9029,
                "exo_outfit",
                9030,
                1,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-expected.raw"),
                9029,
                "exo_outfit",
                9030,
                1,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-expected.raw"),
                9029,
                "exo_outfit",
                9030,
                1,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ee-expected.raw"),
                9029,
                "exo_outfit",
                9030,
                1,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-ee-expected.raw"),
                9029,
                "exo_outfit",
                9030,
                1,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ee-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ee-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ee-expected.raw"),
                9029,
                "exo_outfit",
                9030,
                1,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-expected.raw"),
                4660,
                "ammo_9x39_pab9",
                4661,
                17,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-ammo-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-ammo-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-ammo-expected.raw"),
                4660,
                "ammo_9x39_pab9",
                4661,
                17,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-expected.raw"),
                4660,
                "ammo_9x39_pab9",
                4661,
                17,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ee-ammo-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ee-ammo-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ee-ammo-expected.raw"),
                13398,
                "ammo_9x39_pab9",
                13399,
                17,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-ee-ammo-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-ee-ammo-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cs-ee-ammo-expected.raw"),
                13398,
                "ammo_9x39_pab9",
                13399,
                17,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ee-ammo-source.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ee-ammo-expected.sav"),
                include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ee-ammo-expected.raw"),
                13398,
                "ammo_9x39_pab9",
                13399,
                17,
            ),
        ];
        for (source_bytes, expected_bytes, expected_raw, template_object, item_key, object_id, quantity) in cases {
            let source = Save::read(source_bytes)?;
            let changes = ChangeSet::new(vec![Change::AddItem {
                template_object,
                item_key: item_key.to_owned(),
                object_id,
                quantity,
            }]);
            let output = apply(&source, &changes)?;
            let actual_raw = Save::read(output.as_slice())?;
            assert_eq!(actual_raw.raw_image(), expected_raw);
            assert_eq!(output.as_slice(), expected_bytes);
            let inverse = ChangeSet::new(vec![Change::RemoveItem {
                target_object: object_id,
            }]);
            assert_eq!(apply(&actual_raw, &inverse)?.as_slice(), source_bytes);
        }
        Ok(())
    }
}
