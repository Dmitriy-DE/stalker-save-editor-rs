//! One-copy X-Ray edit preparation. Unknown fields remain untouched.

use sse_catalog::{CatalogBundleReader, FactionCatalog, UpgradeCatalog};
use sse_core::fields::{read_u16, read_u32};
use sse_core::ranges::{verify_unchanged_outside_ranges, ChangedRange};
use sse_core::{Cursor, Error, Result, SaveBuffer};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

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

struct AddedObjectExpectation {
    object_id: u16,
    template_object_id: u16,
    parent_id: u16,
    expected_count: Option<u16>,
    serialized_record: Vec<u8>,
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

/// Reads the validated upgrade vector for one actor-owned registry object.
pub fn current_upgrades(save: &Save, target_object: u16) -> Result<Vec<String>> {
    let record = save
        .registry_objects()
        .iter()
        .find(|record| record.object_id == target_object)
        .ok_or_else(|| Error::Refused(format!("upgrade target 0x{target_object:04X} is missing")))?;
    if record.parent_id != save.actor_id() {
        return Err(Error::Refused(format!(
            "upgrade target 0x{target_object:04X} is not actor-owned"
        )));
    }
    read_upgrade_vector(save.raw_image(), record).map(|(values, _, _)| values)
}

/// Applies changes with explicitly supplied catalogs, or the embedded catalogs when using [`apply`].
pub fn apply_with_catalog(
    save: &Save,
    changes: &ChangeSet,
    faction_catalog: Option<&FactionCatalog>,
    upgrade_catalog: Option<&UpgradeCatalog>,
) -> Result<SaveBuffer> {
    apply_with_catalog_internal(save, changes, faction_catalog, upgrade_catalog)
}

fn apply_with_catalog_internal(
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
    if let Some(inventory) = inventory.as_deref() {
        validate_slot_occupancy(inventory, changes)?;
    }
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
                let update_offset = item.update_condition_offset.ok_or_else(|| {
                    Error::Refused(format!(
                        "object 0x{target_object:04X} has no proven UPDATE condition field"
                    ))
                })?;
                writes.push(PendingWrite::f32(state_offset, *new_value));
                writes.push(PendingWrite::u8(update_offset, encode_condition_q8(*new_value)));
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
                let width = item.placement_width.ok_or_else(|| {
                    Error::Refused(format!("object 0x{target_object:04X} has no proven placement width"))
                })?;
                let replacement = match destination {
                    Placement::Ruck => (current & 0xFFF0) | 3,
                    Placement::Belt if item.section.to_ascii_lowercase().starts_with("af_") && current & 0x0F == 2 => {
                        current
                    }
                    Placement::Belt if item.section.to_ascii_lowercase().starts_with("af_") => {
                        return Err(Error::Refused(
                            "belt capacity cannot be proven without the active armor's game configuration".to_owned(),
                        ));
                    }
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
                match width {
                    1 => writes.push(PendingWrite::u8(
                        offset,
                        u8::try_from(replacement).map_err(|_| {
                            Error::Refused("one-byte X-Ray placement exceeds its supported range".to_owned())
                        })?,
                    )),
                    2 => writes.push(PendingWrite::u16(offset, replacement)),
                    _ => {
                        return Err(Error::Refused("X-Ray placement field width is unsupported".to_owned()));
                    }
                }
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
                    if save.format() == crate::Format::Cs {
                        let placement = crate::save::read_placement_fields(save.raw_image(), record, save.format())?
                            .ok_or_else(|| {
                                Error::Refused("Clear Sky stash item has no proven placement field".to_owned())
                            })?;
                        let placement_write = match placement.width {
                            1 => PendingWrite::u8(placement.offset, 3),
                            2 => PendingWrite::u16(placement.offset, (placement.packed & 0xFFF0) | 3),
                            _ => {
                                return Err(Error::Refused(
                                    "Clear Sky stash placement width is unsupported".to_owned(),
                                ));
                            }
                        };
                        writes.push(placement_write);
                    } else if let Some(client_start) =
                        record.client_data_offset.filter(|_| record.client_data_length >= 3)
                    {
                        let placement_offset = client_start
                            .checked_add(1)
                            .ok_or_else(|| Error::damaged("X-Ray client-data offset overflow"))?;
                        let client_end = client_start
                            .checked_add(record.client_data_length)
                            .ok_or_else(|| Error::damaged("X-Ray client-data range overflow"))?;
                        let placement_end = placement_offset
                            .checked_add(2)
                            .ok_or_else(|| Error::damaged("X-Ray placement range overflow"))?;
                        if placement_end <= client_end {
                            if let Ok(current) = read_u16(save.raw_image(), placement_offset) {
                                if valid_packed_placement(current) {
                                    writes.push(PendingWrite::u16(placement_offset, (current & 0xFFF0) | 3));
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
                if record.story_id != Some(u32::MAX) {
                    return Err(Error::Refused(format!(
                        "story-linked object 0x{target_object:04X} cannot be removed"
                    )));
                }
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
                if *object_id == u16::MAX || *template_object == u16::MAX {
                    return Err(Error::Refused(
                        "0xFFFF is reserved as the ALife no-object sentinel".to_owned(),
                    ));
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
                if !template.name.eq_ignore_ascii_case(item_key) {
                    return Err(Error::Refused(format!(
                        "template section does not match requested item section '{item_key}' (template '{}', replacement '{}')",
                        template.name, template.name_replace
                    )));
                }
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
                added_objects.push(AddedObjectExpectation {
                    object_id: *object_id,
                    template_object_id: *template_object,
                    parent_id: save.actor_id(),
                    expected_count: ammunition.then_some(*quantity),
                    serialized_record: record,
                });
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
        !removed_objects.is_empty() || !added_objects.is_empty() || !object_replacements.is_empty();
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
                u32::try_from(added_objects.len())
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
        let added_length = added_objects
            .iter()
            .try_fold(0_usize, |sum, added| sum.checked_add(added.serialized_record.len()))
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
        for added in &added_objects {
            payload.extend_from_slice(&added.serialized_record);
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
    let replaced_chunks = replacements.iter().map(|(kind, _)| *kind).collect::<Vec<_>>();
    verify_changed_image_ranges(save, &verified, after, &writes, &replaced_chunks, changes)?;
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
                    let update_offset = item.update_condition_offset.ok_or_else(|| {
                        Error::Refused(format!(
                            "durability UPDATE read-back is unavailable for 0x{target_object:04X}"
                        ))
                    })?;
                    if verified.raw_image().get(update_offset).copied() != Some(encode_condition_q8(*new_value)) {
                        return Err(Error::Refused(format!(
                            "durability UPDATE read-back failed for 0x{target_object:04X}"
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
                Change::MoveItem {
                    target_object,
                    new_parent,
                    ..
                } if *new_parent == verified.actor_id() && save.format() == crate::Format::Cs => {
                    let item = verified_items
                        .iter()
                        .find(|item| item.handle == *target_object)
                        .ok_or_else(|| Error::Refused(format!("stash item 0x{target_object:04X} disappeared")))?;
                    if !item.placement_value.is_some_and(|value| value & 0x0F == 3) {
                        return Err(Error::Refused(format!(
                            "stash item 0x{target_object:04X} placement read-back failed"
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
        let source_items = save.inventory()?;
        let verified_items = verified.inventory()?;
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
                if *new_parent == verified.actor_id() {
                    // A placement the transfer did not write keeps its value; a written one must read back as backpack.
                    let before = source_items
                        .iter()
                        .find(|item| item.handle == *target_object)
                        .and_then(|item| item.placement_value);
                    let after = verified_items
                        .iter()
                        .find(|item| item.handle == *target_object)
                        .and_then(|item| item.placement_value);
                    if after != before && after.is_some_and(|value| value & 0x0F != 3) {
                        return Err(Error::Refused(format!(
                            "stash item placement read-back failed for 0x{target_object:04X}"
                        )));
                    }
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
    verify_no_new_story_id_occurrences(
        save.registry_objects().iter().map(|record| record.story_id),
        verified.registry_objects().iter().map(|record| record.story_id),
    )?;
    for added in &added_objects {
        let record = verified
            .registry_objects()
            .iter()
            .find(|record| record.object_id == added.object_id)
            .ok_or_else(|| {
                Error::Refused(format!(
                    "added object 0x{:04X} is missing after read-back",
                    added.object_id
                ))
            })?;
        if record.parent_id != added.parent_id {
            return Err(Error::Refused(format!(
                "added object 0x{:04X} has the wrong parent after read-back",
                added.object_id
            )));
        }
        if added.expected_count.is_none() {
            let added_item = verified
                .inventory()?
                .into_iter()
                .find(|item| item.handle == added.object_id)
                .ok_or_else(|| {
                    Error::Refused(format!(
                        "added object 0x{:04X} is absent from inventory",
                        added.object_id
                    ))
                })?;
            if !added_item.placement_value.is_some_and(|value| value & 0x0F == 3) {
                return Err(Error::Refused(format!(
                    "added object 0x{:04X} has no verified backpack placement",
                    added.object_id
                )));
            }
        }
        if registry_record_bytes(&verified, record)? != added.serialized_record.as_slice() {
            return Err(Error::Refused(format!(
                "added object 0x{:04X} differs from its prepared template clone",
                added.object_id
            )));
        }
        let template_before = save
            .registry_objects()
            .iter()
            .find(|record| record.object_id == added.template_object_id)
            .ok_or_else(|| {
                Error::Refused(format!(
                    "template object 0x{:04X} disappeared from the source registry",
                    added.template_object_id
                ))
            })?;
        let template_after = verified
            .registry_objects()
            .iter()
            .find(|record| record.object_id == added.template_object_id)
            .ok_or_else(|| {
                Error::Refused(format!(
                    "template object 0x{:04X} disappeared after read-back",
                    added.template_object_id
                ))
            })?;
        if registry_record_bytes(save, template_before)? != registry_record_bytes(&verified, template_after)? {
            return Err(Error::Refused(format!(
                "template object 0x{:04X} changed during item addition",
                added.template_object_id
            )));
        }
        if !record.name_replace.is_empty()
            || record.story_id != Some(u32::MAX)
            || record.spawn_story_id != Some(u32::MAX)
            || record.spawn_id != Some(u16::MAX)
            || verified.custom_data(record) != Some(&[][..])
        {
            return Err(Error::Refused(format!(
                "added object 0x{:04X} retained template-bound SPAWN/STATE metadata",
                added.object_id
            )));
        }
        if let Some(expected_count) = added.expected_count {
            let item = verified
                .inventory()?
                .into_iter()
                .find(|item| item.handle == added.object_id)
                .ok_or_else(|| {
                    Error::Refused(format!(
                        "added ammo 0x{:04X} is not actor-owned after read-back",
                        added.object_id
                    ))
                })?;
            if item.count != Some(expected_count) {
                return Err(Error::Refused(format!(
                    "added ammo count read-back failed for 0x{:04X}",
                    added.object_id
                )));
            }
        }
    }
    verify_extended_changes(save, &verified, changes, faction_catalog)?;
    Ok(packed)
}

fn encode_condition_q8(value: f32) -> u8 {
    #[allow(clippy::cast_possible_truncation)]
    let encoded = ((value * 255.0) + 0.5).floor().clamp(0.0, 255.0) as u8;
    encoded
}

fn validate_slot_occupancy(inventory: &[crate::InventoryItem], changes: &ChangeSet) -> Result<()> {
    let mut requested_slots = HashSet::new();
    let mut placements = HashMap::new();
    let mut removed = HashSet::new();
    for change in changes.changes() {
        match change {
            Change::SetPlacement {
                target_object,
                destination: Placement::Slot(slot),
            } => {
                requested_slots.insert(*slot);
                placements.insert(*target_object, Placement::Slot(*slot));
            }
            Change::SetPlacement {
                target_object,
                destination,
            } => {
                placements.insert(*target_object, *destination);
            }
            Change::RemoveItem { target_object } => {
                removed.insert(*target_object);
            }
            Change::SetMoney { .. }
            | Change::SetStack { .. }
            | Change::SetDurability { .. }
            | Change::MoveItem { .. }
            | Change::AddItem { .. }
            | Change::SetPlayerFaction { .. }
            | Change::SetFactionRelation { .. }
            | Change::SetUpgrades { .. }
            | Change::AddInfoPortions { .. }
            | Change::RelocateActor { .. } => {}
        }
    }
    if requested_slots.is_empty() {
        return Ok(());
    }

    let mut occupied = HashMap::new();
    for item in inventory {
        if removed.contains(&item.handle) {
            continue;
        }
        let destination = placements.get(&item.handle).copied();
        let slot = match destination {
            Some(Placement::Slot(slot)) => Some(slot),
            Some(Placement::Ruck | Placement::Belt) => None,
            None => match item.placement_value {
                Some(value) if value & 0x0F == 1 => {
                    let raw_slot = (value >> 4) & 0x3F;
                    Some(u8::try_from(raw_slot).map_err(|_| Error::damaged("X-Ray slot id exceeds 8 bits"))?)
                }
                Some(_) => None,
                None => {
                    return Err(Error::Refused(format!(
                        "slot occupancy cannot be proven because object 0x{:04X} has unknown placement",
                        item.handle
                    )));
                }
            },
        };
        let Some(slot) = slot.filter(|slot| requested_slots.contains(slot)) else {
            continue;
        };
        if let Some(previous) = occupied.insert(slot, item.handle) {
            return Err(Error::Refused(format!(
                "slot {slot} is already occupied by objects 0x{previous:04X} and 0x{:04X}",
                item.handle
            )));
        }
    }
    Ok(())
}

fn registry_record_bytes<'a>(save: &'a Save, record: &crate::RegistryObject) -> Result<&'a [u8]> {
    let end = record
        .record_offset
        .checked_add(record.record_length)
        .ok_or_else(|| Error::damaged("X-Ray registry record range overflow"))?;
    save.raw_image()
        .get(record.record_offset..end)
        .ok_or_else(|| Error::damaged("X-Ray registry record is outside the unpacked image"))
}

fn verify_no_new_story_id_occurrences(
    before: impl IntoIterator<Item = Option<u32>>,
    after: impl IntoIterator<Item = Option<u32>>,
) -> Result<()> {
    fn counts(story_ids: impl IntoIterator<Item = Option<u32>>) -> Result<HashMap<u32, usize>> {
        let mut counts = HashMap::new();
        for story_id in story_ids.into_iter().flatten().filter(|id| *id != u32::MAX) {
            let count = counts.entry(story_id).or_insert(0_usize);
            *count = count
                .checked_add(1)
                .ok_or_else(|| Error::Refused("X-Ray story-id occurrence count overflow".to_owned()))?;
        }
        Ok(counts)
    }

    let before = counts(before)?;
    let after = counts(after)?;
    if let Some((story_id, _)) = after
        .iter()
        .find(|(story_id, count)| **count > 1 && **count > before.get(story_id).copied().unwrap_or_default())
    {
        return Err(Error::Refused(format!(
            "X-Ray read-back introduced a duplicate story_id 0x{story_id:08X}"
        )));
    }
    Ok(())
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

#[derive(Debug)]
struct SpawnSplice {
    range: Range<usize>,
    replacement: Vec<u8>,
}

fn verify_changed_image_ranges(
    source: &Save,
    replacement_layout: &Save,
    replacement_image: &[u8],
    writes: &[PendingWrite],
    replaced_chunks: &[u32],
    changes: &ChangeSet,
) -> Result<()> {
    let source_chunks = source.chunks();
    let replacement_chunks = replacement_layout.chunks();
    if source_chunks.len() != replacement_chunks.len()
        || source_chunks
            .iter()
            .zip(replacement_chunks)
            .any(|(before, after)| before.kind != after.kind)
    {
        return Err(Error::damaged("X-Ray edit changed chunk order or count"));
    }

    let declared = DeclaredChunkChanges::from_changes(source, changes);
    if replaced_chunks.contains(&2) {
        let record_writes = declared_record_writes(source, writes)?;
        verify_object_chunk_records(source, replacement_layout, replacement_image, &declared, &record_writes)?;
    }
    if replaced_chunks.contains(&9) {
        verify_relation_chunk_records(source, replacement_layout, replacement_image, &declared)?;
    }

    let mut ranges = Vec::with_capacity(writes.len().saturating_add(replaced_chunks.len()));
    for (before, after) in source_chunks.iter().zip(replacement_chunks) {
        if replaced_chunks.contains(&before.kind) {
            verify_replaced_chunk_header(replacement_image, *after)?;
            ranges.push(ChangedRange {
                before: chunk_record_range(*before)?,
                after: chunk_record_range(*after)?,
            });
        }
    }

    for write in writes {
        let write_end = write
            .offset
            .checked_add(write.length)
            .ok_or_else(|| Error::damaged("X-Ray changed range overflows"))?;
        let mut owner = None;
        for (index, chunk) in source_chunks.iter().enumerate() {
            let chunk_end = chunk
                .offset
                .checked_add(chunk.length)
                .ok_or_else(|| Error::damaged("X-Ray chunk range overflows"))?;
            if write.offset >= chunk.offset && write_end <= chunk_end && owner.replace(index).is_some() {
                return Err(Error::damaged("X-Ray write belongs to duplicate chunks"));
            }
        }
        let index = owner.ok_or_else(|| Error::damaged("X-Ray write is outside every chunk payload"))?;
        let before = source_chunks
            .get(index)
            .ok_or_else(|| Error::damaged("X-Ray source chunk index disappeared"))?;
        if replaced_chunks.contains(&before.kind) {
            continue;
        }
        let after = replacement_chunks
            .get(index)
            .ok_or_else(|| Error::damaged("X-Ray replacement chunk index disappeared"))?;
        if before.length != after.length {
            return Err(Error::damaged(
                "X-Ray chunk changed length without a declared replacement",
            ));
        }
        let relative_offset = write
            .offset
            .checked_sub(before.offset)
            .ok_or_else(|| Error::damaged("X-Ray changed range offset underflows"))?;
        let after_start = after
            .offset
            .checked_add(relative_offset)
            .ok_or_else(|| Error::damaged("X-Ray replacement changed range overflows"))?;
        let after_end = after_start
            .checked_add(write.length)
            .ok_or_else(|| Error::damaged("X-Ray replacement changed range overflows"))?;
        ranges.push(ChangedRange {
            before: write.offset..write_end,
            after: after_start..after_end,
        });
    }
    ranges.sort_unstable_by_key(|range| (range.before.start, range.after.start));
    verify_unchanged_outside_ranges(source.raw_image(), replacement_image, &ranges)
}

#[derive(Default)]
struct DeclaredChunkChanges {
    object_records: HashSet<u16>,
    upgrade_records: HashSet<u16>,
    added_objects: HashSet<u16>,
    removed_objects: HashSet<u16>,
    info_rows: HashSet<u16>,
    relation_rows: HashSet<u16>,
}

impl DeclaredChunkChanges {
    fn from_changes(source: &Save, changes: &ChangeSet) -> Self {
        let mut declared = Self::default();
        for change in changes.changes() {
            match change {
                Change::SetMoney { target_object, .. }
                | Change::SetStack { target_object, .. }
                | Change::SetDurability { target_object, .. }
                | Change::SetPlacement { target_object, .. }
                | Change::MoveItem { target_object, .. }
                | Change::SetPlayerFaction { target_object, .. } => {
                    declared.object_records.insert(*target_object);
                }
                Change::SetUpgrades { target_object, .. } => {
                    declared.object_records.insert(*target_object);
                    declared.upgrade_records.insert(*target_object);
                }
                Change::RemoveItem { target_object } => {
                    declared.removed_objects.insert(*target_object);
                }
                Change::AddItem { object_id, .. } => {
                    declared.added_objects.insert(*object_id);
                }
                Change::SetFactionRelation { target_object, .. } => {
                    declared.relation_rows.insert(*target_object);
                }
                Change::AddInfoPortions { target_object, .. } => {
                    declared.info_rows.insert(*target_object);
                }
                Change::RelocateActor { .. } => {
                    declared.object_records.insert(source.actor_id());
                }
            }
        }
        declared
    }
}

/// Groups the source-image byte ranges that a change set writes, by the OBJECT record that contains them.
fn declared_record_writes(source: &Save, writes: &[PendingWrite]) -> Result<HashMap<u16, Vec<Range<usize>>>> {
    let object_chunk = source
        .chunks()
        .iter()
        .find(|chunk| chunk.kind == 2)
        .ok_or_else(|| Error::damaged("missing X-Ray OBJECT chunk"))?;
    let object_end = object_chunk
        .offset
        .checked_add(object_chunk.length)
        .ok_or_else(|| Error::damaged("X-Ray OBJECT chunk range overflows"))?;
    let mut by_record: HashMap<u16, Vec<Range<usize>>> = HashMap::new();
    for write in writes {
        let end = write
            .offset
            .checked_add(write.length)
            .ok_or_else(|| Error::damaged("X-Ray write range overflows"))?;
        // Writes outside the OBJECT payload are checked by the chunk loop of the caller, not here.
        if write.offset < object_chunk.offset || end > object_end {
            continue;
        }
        let record = source
            .registry_objects()
            .iter()
            .find(|record| {
                let record_end = record.record_offset.saturating_add(record.record_length);
                record.record_offset <= write.offset && end <= record_end
            })
            .ok_or_else(|| Error::damaged("X-Ray write is outside every OBJECT record"))?;
        by_record.entry(record.object_id).or_default().push(write.offset..end);
    }
    Ok(by_record)
}

/// Checks a record the change set declared as edited: every byte outside its declared writes must be unchanged.
///
/// A record that an upgrade replacement resized keeps its bytes before the upgrade vector and after it, shifted
/// by the size change; the SPAWN length and the STATE size field are rewritten by the replacement and are excluded.
fn verify_declared_record_bytes(
    before: &[u8],
    after: &[u8],
    writes: &[Range<usize>],
    upgrade_vector: Option<Range<usize>>,
    framing: &[Range<usize>],
) -> Result<()> {
    let excluded = |index: usize| writes.iter().chain(framing).any(|range| range.contains(&index));
    let collateral = || Error::damaged("X-Ray OBJECT record changed outside its declared writes");
    let Some(vector) = upgrade_vector else {
        if before.len() != after.len() {
            return Err(Error::damaged(
                "X-Ray OBJECT record changed length without a declared upgrade",
            ));
        }
        for (index, (old, new)) in before.iter().zip(after).enumerate() {
            if old != new && !excluded(index) {
                return Err(collateral());
            }
        }
        return Ok(());
    };
    if vector.start > vector.end
        || vector.end > before.len()
        || writes
            .iter()
            .any(|write| write.start < vector.end && vector.start < write.end)
    {
        return Err(Error::damaged("X-Ray upgrade vector overlaps a declared write"));
    }
    let delta = isize::try_from(after.len())
        .ok()
        .and_then(|after_length| after_length.checked_sub(isize::try_from(before.len()).ok()?))
        .ok_or_else(|| Error::damaged("X-Ray OBJECT record size delta overflow"))?;
    for index in 0..vector.start {
        match (before.get(index), after.get(index)) {
            (Some(old), Some(new)) if old == new || excluded(index) => {}
            _ => return Err(collateral()),
        }
    }
    for index in vector.end..before.len() {
        let shifted = index
            .checked_add_signed(delta)
            .ok_or_else(|| Error::damaged("X-Ray OBJECT record shifted offset overflow"))?;
        match (before.get(index), after.get(shifted)) {
            (Some(old), Some(new)) if old == new || excluded(index) => {}
            _ => return Err(collateral()),
        }
    }
    Ok(())
}

fn verify_object_chunk_records(
    source: &Save,
    replacement_layout: &Save,
    replacement_image: &[u8],
    declared: &DeclaredChunkChanges,
    record_writes: &HashMap<u16, Vec<Range<usize>>>,
) -> Result<()> {
    let source_payload = source.object_chunk_bytes(source.raw_image())?;
    let replacement_payload = replacement_layout.object_chunk_bytes(replacement_image)?;
    let source_records = source.registry_objects();
    let replacement_records = replacement_layout.registry_objects();
    verify_object_record_ranges(source, source_payload, source_records)?;
    verify_object_record_ranges(replacement_layout, replacement_payload, replacement_records)?;
    let replacement_count = read_u32(replacement_payload, 0)?;
    if usize::try_from(replacement_count).ok() != Some(replacement_records.len()) {
        return Err(Error::damaged(
            "X-Ray OBJECT chunk count does not match its indexed records",
        ));
    }
    let removed_count = declared
        .removed_objects
        .iter()
        .filter(|id| source_records.iter().any(|record| record.object_id == **id))
        .count();
    if removed_count != declared.removed_objects.len() {
        return Err(Error::damaged("X-Ray OBJECT chunk removed an undeclared record"));
    }
    let expected_count = source_records
        .len()
        .checked_sub(removed_count)
        .and_then(|count| count.checked_add(declared.added_objects.len()))
        .ok_or_else(|| Error::damaged("X-Ray OBJECT record count overflow"))?;
    if expected_count != replacement_records.len() {
        return Err(Error::damaged(
            "X-Ray OBJECT record count changed outside declared additions or removals",
        ));
    }

    let mut source_by_id = HashMap::with_capacity(source_records.len());
    for record in source_records {
        if source_by_id.insert(record.object_id, record).is_some() {
            return Err(Error::damaged("X-Ray source OBJECT chunk repeats an object id"));
        }
    }
    let mut replacement_by_id = HashMap::with_capacity(replacement_records.len());
    for record in replacement_records {
        if replacement_by_id.insert(record.object_id, record).is_some() {
            return Err(Error::damaged("X-Ray replacement OBJECT chunk repeats an object id"));
        }
        if read_u16(replacement_image, record.object_id_offset)? != record.object_id {
            return Err(Error::damaged(format!(
                "X-Ray OBJECT record 0x{:04X} changed its object id",
                record.object_id
            )));
        }
    }
    for object_id in &declared.added_objects {
        if source_by_id.contains_key(object_id) || !replacement_by_id.contains_key(object_id) {
            return Err(Error::damaged(format!(
                "X-Ray OBJECT chunk did not preserve declared addition 0x{object_id:04X}"
            )));
        }
    }

    for (object_id, before_record) in &source_by_id {
        match replacement_by_id.get(object_id) {
            Some(after_record) => {
                let before_bytes = registry_record_bytes(source, before_record)?;
                let after_bytes = registry_record_bytes_from_image(replacement_image, after_record)?;
                if read_u16(replacement_image, after_record.object_id_offset)? != *object_id {
                    return Err(Error::damaged(format!(
                        "X-Ray OBJECT record 0x{object_id:04X} changed its object id"
                    )));
                }
                if declared.object_records.contains(object_id) {
                    let relative = |range: &Range<usize>| -> Result<Range<usize>> {
                        let start = range
                            .start
                            .checked_sub(before_record.record_offset)
                            .ok_or_else(|| Error::damaged("X-Ray declared write precedes its record"))?;
                        let end = range
                            .end
                            .checked_sub(before_record.record_offset)
                            .ok_or_else(|| Error::damaged("X-Ray declared write precedes its record"))?;
                        Ok(start..end)
                    };
                    let writes = record_writes
                        .get(object_id)
                        .map_or(&[][..], Vec::as_slice)
                        .iter()
                        .map(relative)
                        .collect::<Result<Vec<_>>>()?;
                    let (vector, framing) = if declared.upgrade_records.contains(object_id) {
                        let (offset, length, _) = upgrade_vector_range(source.raw_image(), before_record)?;
                        let start = offset
                            .checked_sub(before_record.record_offset)
                            .ok_or_else(|| Error::damaged("X-Ray upgrade vector precedes its record"))?;
                        let state_start = before_record
                            .state_offset
                            .checked_sub(before_record.record_offset)
                            .ok_or_else(|| Error::damaged("X-Ray STATE precedes its record"))?;
                        let state_size = state_start
                            .checked_sub(2)
                            .ok_or_else(|| Error::damaged("X-Ray STATE size field precedes its record"))?;
                        (
                            Some(start..start.saturating_add(length)),
                            vec![0..2, state_size..state_start],
                        )
                    } else {
                        (None, Vec::new())
                    };
                    verify_declared_record_bytes(before_bytes, after_bytes, &writes, vector, &framing)?;
                } else if before_bytes != after_bytes {
                    return Err(Error::damaged(format!(
                        "X-Ray OBJECT record 0x{object_id:04X} changed without a declared edit"
                    )));
                }
            }
            None if !declared.removed_objects.contains(object_id) => {
                return Err(Error::damaged(format!(
                    "X-Ray OBJECT record 0x{object_id:04X} disappeared without a declared removal"
                )));
            }
            None => {}
        }
    }

    for object_id in replacement_by_id.keys() {
        if !source_by_id.contains_key(object_id) && !declared.added_objects.contains(object_id) {
            return Err(Error::damaged(format!(
                "X-Ray OBJECT chunk added undeclared record 0x{object_id:04X}"
            )));
        }
    }
    let source_count = read_u32(source_payload, 0)?;
    if usize::try_from(source_count).ok() != Some(source_records.len()) {
        return Err(Error::damaged(
            "X-Ray source OBJECT count does not match its indexed records",
        ));
    }
    Ok(())
}

fn verify_object_record_ranges(save: &Save, payload: &[u8], records: &[crate::RegistryObject]) -> Result<()> {
    let chunk = save
        .chunks()
        .iter()
        .find(|chunk| chunk.kind == 2)
        .ok_or_else(|| Error::damaged("missing X-Ray OBJECT chunk"))?;
    let mut cursor = 4_usize;
    for record in records {
        let record_start = record
            .record_offset
            .checked_sub(chunk.offset)
            .ok_or_else(|| Error::damaged("X-Ray OBJECT record offset underflows its chunk"))?;
        let record_end = cursor
            .checked_add(record.record_length)
            .ok_or_else(|| Error::damaged("X-Ray OBJECT record range overflows"))?;
        if record_start != cursor || record_end > payload.len() {
            return Err(Error::damaged("X-Ray OBJECT records do not cover their chunk"));
        }
        cursor = record_end;
    }
    if cursor != payload.len() {
        return Err(Error::damaged("X-Ray OBJECT chunk has unindexed trailing bytes"));
    }
    Ok(())
}

fn verify_relation_chunk_records(
    source: &Save,
    replacement_layout: &Save,
    replacement_image: &[u8],
    declared: &DeclaredChunkChanges,
) -> Result<()> {
    let source_payload = source.relation_chunk_bytes(source.raw_image())?;
    let replacement_payload = replacement_layout.relation_chunk_bytes(replacement_image)?;
    let source_registry = crate::save::parse_relation_registry(source_payload, source.relation_has_timestamps())?;
    let replacement_registry =
        crate::save::parse_relation_registry(replacement_payload, replacement_layout.relation_has_timestamps())?;
    let source_info = source_registry
        .info_rows
        .iter()
        .map(|row| {
            let start = row
                .count_offset
                .checked_sub(2)
                .ok_or_else(|| Error::damaged("X-Ray info-portion row offset underflows"))?;
            Ok((row.object_id, start..row.end_offset))
        })
        .collect::<Result<Vec<_>>>()?;
    let replacement_info = replacement_registry
        .info_rows
        .iter()
        .map(|row| {
            let start = row
                .count_offset
                .checked_sub(2)
                .ok_or_else(|| Error::damaged("X-Ray info-portion row offset underflows"))?;
            Ok((row.object_id, start..row.end_offset))
        })
        .collect::<Result<Vec<_>>>()?;
    verify_indexed_rows(
        source_payload,
        replacement_payload,
        &source_info,
        &replacement_info,
        &declared.info_rows,
        "info-portion",
    )?;

    let source_relations = source_registry
        .relation_rows
        .iter()
        .map(|row| (row.object_id, row.start..row.end))
        .collect::<Vec<_>>();
    let replacement_relations = replacement_registry
        .relation_rows
        .iter()
        .map(|row| (row.object_id, row.start..row.end))
        .collect::<Vec<_>>();
    verify_indexed_rows(
        source_payload,
        replacement_payload,
        &source_relations,
        &replacement_relations,
        &declared.relation_rows,
        "relation",
    )?;

    let source_tail_start = source_registry
        .relation_rows
        .last()
        .map_or_else(|| source_registry.info_section_end.checked_add(4), |row| Some(row.end))
        .ok_or_else(|| Error::damaged("X-Ray relation tail offset overflows"))?;
    let replacement_tail_start = replacement_registry
        .relation_rows
        .last()
        .map_or_else(
            || replacement_registry.info_section_end.checked_add(4),
            |row| Some(row.end),
        )
        .ok_or_else(|| Error::damaged("X-Ray relation tail offset overflows"))?;
    let source_tail = source_payload
        .get(source_tail_start..)
        .ok_or_else(|| Error::damaged("X-Ray source relation tail is outside its chunk"))?;
    let replacement_tail = replacement_payload
        .get(replacement_tail_start..)
        .ok_or_else(|| Error::damaged("X-Ray replacement relation tail is outside its chunk"))?;
    if source_tail != replacement_tail {
        return Err(Error::damaged("X-Ray relation chunk trailing bytes changed"));
    }
    Ok(())
}

fn verify_indexed_rows(
    before_image: &[u8],
    after_image: &[u8],
    before_rows: &[(u16, Range<usize>)],
    after_rows: &[(u16, Range<usize>)],
    allowed_changes: &HashSet<u16>,
    label: &str,
) -> Result<()> {
    let before_by_id = before_rows.iter().cloned().collect::<HashMap<_, _>>();
    let after_by_id = after_rows.iter().cloned().collect::<HashMap<_, _>>();
    if before_by_id.len() != before_rows.len() || after_by_id.len() != after_rows.len() {
        return Err(Error::damaged(format!("X-Ray {label} chunk repeats a row id")));
    }
    if before_by_id.keys().any(|id| !after_by_id.contains_key(id)) {
        return Err(Error::damaged(format!("X-Ray {label} chunk removed a row")));
    }
    if after_by_id
        .keys()
        .any(|id| !before_by_id.contains_key(id) && !allowed_changes.contains(id))
    {
        return Err(Error::damaged(format!("X-Ray {label} chunk added an undeclared row")));
    }
    for (object_id, before_range) in before_by_id {
        let after_range = after_by_id
            .get(&object_id)
            .ok_or_else(|| Error::damaged(format!("X-Ray {label} row disappeared")))?;
        let before = before_image
            .get(before_range)
            .ok_or_else(|| Error::damaged(format!("X-Ray source {label} row is outside its chunk")))?;
        let after = after_image
            .get(after_range.clone())
            .ok_or_else(|| Error::damaged(format!("X-Ray replacement {label} row is outside its chunk")))?;
        if before != after && !allowed_changes.contains(&object_id) {
            return Err(Error::damaged(format!(
                "X-Ray {label} row for object 0x{object_id:04X} changed without a declared edit"
            )));
        }
    }
    Ok(())
}

fn registry_record_bytes_from_image<'a>(image: &'a [u8], record: &crate::RegistryObject) -> Result<&'a [u8]> {
    let end = record
        .record_offset
        .checked_add(record.record_length)
        .ok_or_else(|| Error::damaged("X-Ray registry record range overflow"))?;
    image
        .get(record.record_offset..end)
        .ok_or_else(|| Error::damaged("X-Ray registry record is outside the replacement image"))
}

fn verify_replaced_chunk_header(image: &[u8], chunk: crate::container::Chunk) -> Result<()> {
    let header = chunk
        .offset
        .checked_sub(8)
        .ok_or_else(|| Error::damaged("X-Ray replacement chunk header offset underflows"))?;
    let length_offset = header
        .checked_add(4)
        .ok_or_else(|| Error::damaged("X-Ray replacement chunk length offset overflows"))?;
    let expected_length = u32::try_from(chunk.length)
        .map_err(|_| Error::damaged("X-Ray replacement chunk length does not fit its header"))?;
    if read_u32(image, header)? != chunk.kind || read_u32(image, length_offset)? != expected_length {
        return Err(Error::damaged(format!(
            "X-Ray replacement chunk {} header changed unexpectedly",
            chunk.kind
        )));
    }
    Ok(())
}

fn chunk_record_range(chunk: crate::container::Chunk) -> Result<std::ops::Range<usize>> {
    let start = chunk
        .offset
        .checked_sub(8)
        .ok_or_else(|| Error::damaged("X-Ray chunk header offset underflows"))?;
    let end = chunk
        .offset
        .checked_add(chunk.length)
        .ok_or_else(|| Error::damaged("X-Ray chunk record range overflows"))?;
    Ok(start..end)
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
    if item_key.contains('\0') {
        return Err(Error::Refused("item key contains a zero byte".to_owned()));
    }
    if template.story_id_offset.is_none()
        || template.spawn_story_id_offset.is_none()
        || template.spawn_id_offset.is_none()
    {
        return Err(Error::Refused(
            "template has no proven SPAWN/STATE identity fields".to_owned(),
        ));
    }
    let custom_data_range = template
        .custom_data_range
        .clone()
        .ok_or_else(|| Error::Refused("template STATE has no proven custom-data field".to_owned()))?;
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
    let relative_range = |range: &Range<usize>, label: &str| -> Result<Range<usize>> {
        let start = range
            .start
            .checked_sub(spawn_absolute)
            .ok_or_else(|| Error::damaged(format!("X-Ray {label} starts outside its SPAWN")))?;
        let end = range
            .end
            .checked_sub(spawn_absolute)
            .ok_or_else(|| Error::damaged(format!("X-Ray {label} ends outside its SPAWN")))?;
        if start > end || end > spawn.len() {
            return Err(Error::damaged(format!("X-Ray {label} range exceeds its SPAWN")));
        }
        Ok(start..end)
    };
    let name_replace_range = relative_range(&template.name_replace_range, "name-replacement")?;
    let custom_data_range = relative_range(&custom_data_range, "custom-data")?;
    let mut splices = vec![
        SpawnSplice {
            range: name_start..name_end,
            replacement: replacement_name,
        },
        SpawnSplice {
            range: name_replace_range,
            replacement: vec![0],
        },
        SpawnSplice {
            range: custom_data_range.clone(),
            replacement: vec![0],
        },
    ];
    splices.sort_unstable_by_key(|splice| splice.range.start);
    for pair in splices.windows(2) {
        let first = pair
            .first()
            .ok_or_else(|| Error::damaged("X-Ray SPAWN splice list is incomplete"))?;
        let second = pair
            .get(1)
            .ok_or_else(|| Error::damaged("X-Ray SPAWN splice list is incomplete"))?;
        if first.range.end > second.range.start {
            return Err(Error::damaged("X-Ray SPAWN metadata ranges overlap"));
        }
    }
    for splice in splices.iter().rev() {
        if splice.range.end > spawn.len() {
            return Err(Error::damaged("X-Ray SPAWN metadata range is outside its packet"));
        }
        spawn.splice(splice.range.clone(), splice.replacement.iter().copied());
    }

    let template_state_start = template
        .state_offset
        .checked_sub(spawn_absolute)
        .ok_or_else(|| Error::damaged("X-Ray STATE is outside its SPAWN"))?;
    let mut state_offset = adjust_spawn_offset(template_state_start, &splices)?;
    let custom_data_delta = splice_delta(&splices, &custom_data_range)?;
    let mut state_length = adjust_length(template.state_length, custom_data_delta)?;
    let mut upgrade_splice = None;
    if template.version > 123 {
        let (vector_offset, vector_length, vector_count) = upgrade_vector_range(raw, template)?;
        if vector_count > 0 {
            let original_vector_offset = vector_offset
                .checked_sub(spawn_absolute)
                .ok_or_else(|| Error::damaged("X-Ray upgrades are outside their SPAWN"))?;
            let vector_offset = adjust_spawn_offset(original_vector_offset, &splices)?;
            let vector_end = vector_offset
                .checked_add(vector_length)
                .ok_or_else(|| Error::damaged("X-Ray upgrades range overflow"))?;
            if spawn.get(vector_offset..vector_end).is_none() {
                return Err(Error::damaged("X-Ray upgrades vector is outside the template SPAWN"));
            }
            let replacement = [0_u8; 4];
            let delta = isize::try_from(replacement.len())
                .map_err(|_| Error::damaged("X-Ray empty-upgrades length overflow"))?
                .checked_sub(
                    isize::try_from(vector_length)
                        .map_err(|_| Error::damaged("X-Ray upgrades vector length overflow"))?,
                )
                .ok_or_else(|| Error::damaged("X-Ray upgrades size delta overflow"))?;
            state_length = adjust_length(state_length, delta)?;
            upgrade_splice = Some((vector_offset..vector_end, replacement.to_vec()));
        }
    }

    if let Some((range, replacement)) = &upgrade_splice {
        spawn.splice(range.clone(), replacement.iter().copied());
    }
    let state_size_offset = state_offset
        .checked_sub(2)
        .ok_or_else(|| Error::damaged("X-Ray STATE size field is outside the SPAWN"))?;
    let state_size = state_length
        .checked_add(2)
        .ok_or_else(|| Error::damaged("X-Ray STATE size overflow"))?;
    write_u16(
        &mut spawn,
        state_size_offset,
        u16::try_from(state_size).map_err(|_| Error::Refused("cloned X-Ray STATE exceeds u16 framing".to_owned()))?,
    )?;

    let parsed_spawn = crate::save::parse_spawn(&spawn, 0)?;
    write_u16(&mut spawn, parsed_spawn.object_id_offset, object_id)?;
    write_u16(&mut spawn, parsed_spawn.parent_id_offset, save.actor_id())?;
    write_u16(
        &mut spawn,
        parsed_spawn
            .spawn_id_offset
            .ok_or_else(|| Error::Refused("cloned SPAWN has no proven spawn-id field".to_owned()))?,
        u16::MAX,
    )?;
    write_u32(
        &mut spawn,
        parsed_spawn
            .story_id_offset
            .ok_or_else(|| Error::Refused("cloned STATE has no proven story-id field".to_owned()))?,
        u32::MAX,
    )?;
    write_u32(
        &mut spawn,
        parsed_spawn
            .spawn_story_id_offset
            .ok_or_else(|| Error::Refused("cloned STATE has no proven spawn-story-id field".to_owned()))?,
        u32::MAX,
    )?;

    let ammunition = item_key.to_ascii_lowercase().starts_with("ammo_");
    if !ammunition {
        let placement = crate::save::read_placement_fields(&spawn, &parsed_spawn, save.format())?
            .ok_or_else(|| Error::Refused("item template has no proven placement field".to_owned()))?;
        match placement.width {
            1 => {
                *spawn
                    .get_mut(placement.offset)
                    .ok_or_else(|| Error::damaged("one-byte template placement is outside its SPAWN"))? = 3;
            }
            2 => write_u16(&mut spawn, placement.offset, (placement.packed & 0xFFF0) | 3)?,
            _ => {
                return Err(Error::Refused(
                    "item template placement width is unsupported".to_owned(),
                ))
            }
        }
    }

    if ammunition {
        state_offset = parsed_spawn.state_offset;
        state_length = parsed_spawn.state_length;
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

fn adjust_spawn_offset(offset: usize, splices: &[SpawnSplice]) -> Result<usize> {
    let mut delta = 0_isize;
    for splice in splices {
        if offset >= splice.range.end {
            delta = delta
                .checked_add(splice_delta_for(splice)?)
                .ok_or_else(|| Error::damaged("X-Ray SPAWN offset delta overflow"))?;
        } else if offset > splice.range.start {
            return Err(Error::damaged("X-Ray field overlaps a replaced SPAWN string"));
        }
    }
    let adjusted = isize::try_from(offset)
        .map_err(|_| Error::damaged("X-Ray SPAWN offset exceeds isize"))?
        .checked_add(delta)
        .ok_or_else(|| Error::damaged("X-Ray adjusted SPAWN offset overflow"))?;
    usize::try_from(adjusted).map_err(|_| Error::damaged("negative X-Ray adjusted SPAWN offset"))
}

fn splice_delta(splices: &[SpawnSplice], range: &Range<usize>) -> Result<isize> {
    let splice = splices
        .iter()
        .find(|splice| &splice.range == range)
        .ok_or_else(|| Error::damaged("X-Ray custom-data splice is missing"))?;
    splice_delta_for(splice)
}

fn splice_delta_for(splice: &SpawnSplice) -> Result<isize> {
    let old_length = splice
        .range
        .end
        .checked_sub(splice.range.start)
        .ok_or_else(|| Error::damaged("X-Ray SPAWN splice length underflow"))?;
    isize::try_from(splice.replacement.len())
        .map_err(|_| Error::damaged("X-Ray SPAWN replacement length exceeds isize"))?
        .checked_sub(isize::try_from(old_length).map_err(|_| Error::damaged("X-Ray SPAWN range exceeds isize"))?)
        .ok_or_else(|| Error::damaged("X-Ray SPAWN splice delta overflow"))
}

fn adjust_length(length: usize, delta: isize) -> Result<usize> {
    let adjusted = isize::try_from(length)
        .map_err(|_| Error::damaged("X-Ray STATE length exceeds isize"))?
        .checked_add(delta)
        .ok_or_else(|| Error::damaged("X-Ray STATE length delta overflow"))?;
    usize::try_from(adjusted).map_err(|_| Error::damaged("negative X-Ray STATE length"))
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
        actor_spawn_position_offset, apply, apply_with_catalog, capability, encode_condition_q8, read_u16, read_u32,
        read_vector, verify_changed_image_ranges, verify_no_new_story_id_occurrences, write_u16, write_u32, Capability,
        Change, ChangeKind, ChangeSet, PendingWrite, Placement,
    };
    use crate::{Format, Save};
    use sse_catalog::{CatalogBundleReader, UpgradeCatalog, UpgradeDefinition};
    use sse_core::Cursor;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn next_fuzz_state(state: &mut u64) -> u64 {
        *state ^= state.wrapping_shl(13);
        *state ^= state.wrapping_shr(7);
        *state ^= state.wrapping_shl(17);
        *state
    }

    #[test]
    fn unpacked_image_mutation_repack_read_and_write_fuzz_smoke() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let source = Save::read(source_bytes)?;
        let mut random = 0x0058_425a_9151_d001_u64;
        let mut repacked_count = 0_usize;
        let mut parsed_count = 0_usize;
        let mut writer_count = 0_usize;
        let mut written_count = 0_usize;

        for case in 0..512_usize {
            let mut raw = source.raw_image().to_vec();
            if case.checked_rem(2).unwrap_or_default() == 0 {
                let money = u32::try_from(next_fuzz_state(&mut random) % 2_000_000_001_u64).unwrap_or_default();
                let start = source.money_offset();
                let end = start.checked_add(4).ok_or("money range overflow")?;
                raw.get_mut(start..end)
                    .ok_or("money range should fit the unpacked fixture")?
                    .copy_from_slice(&money.to_le_bytes());
            } else if !raw.is_empty() {
                let length = u64::try_from(raw.len()).unwrap_or(u64::MAX);
                let offset = usize::try_from(next_fuzz_state(&mut random) % length).unwrap_or_default();
                let shift = u32::try_from(next_fuzz_state(&mut random) % 8).unwrap_or_default();
                if let Some(byte) = raw.get_mut(offset) {
                    *byte ^= 1_u8.checked_shl(shift).unwrap_or(1);
                }
            }

            let repacked = source.repack(&raw)?;
            repacked_count = repacked_count.saturating_add(1);
            let Ok(parsed) = Save::read(repacked.as_slice()) else {
                continue;
            };
            parsed_count = parsed_count.saturating_add(1);
            let old_value = parsed.money()?;
            let new_value = if old_value == 1 { 2 } else { 1 };
            let changes = ChangeSet::new(vec![Change::SetMoney {
                target_object: parsed.actor_id(),
                old_value,
                new_value,
            }]);
            let Ok(written) = apply(&parsed, &changes) else {
                continue;
            };
            written_count = written_count.saturating_add(1);
            let verified = Save::read(written.as_slice())?;
            assert_eq!(verified.money()?, new_value);
            writer_count = writer_count.saturating_add(1);
        }

        assert_eq!(repacked_count, 512);
        assert!(parsed_count > 0, "mutated unpacked images should reach the reader");
        assert!(written_count > 0, "at least one fuzz case should reach the writer");
        assert_eq!(writer_count, written_count);
        Ok(())
    }

    #[test]
    fn read_back_rejects_new_story_id_duplicates_but_allows_existing_ones() -> TestResult {
        let original = [Some(73), Some(73), Some(u32::MAX), None];
        let unchanged = [Some(73), Some(73), Some(u32::MAX), None];
        verify_no_new_story_id_occurrences(original, unchanged)?;
        verify_no_new_story_id_occurrences(original, [Some(73), Some(73), Some(91)])?;

        let duplicated = [Some(73), Some(73), Some(73), Some(u32::MAX), None];
        let error = verify_no_new_story_id_occurrences(original, duplicated)
            .expect_err("a new occurrence of an existing story_id must be rejected");
        assert!(error.to_string().contains("duplicate story_id"));
        Ok(())
    }

    #[test]
    fn money_writer_rejects_a_collateral_image_change() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        let expected_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-cop-expected.sav");
        let source = Save::read(source_bytes)?;
        let expected = Save::read(expected_bytes)?;
        let writes = [PendingWrite::u32(source.money_offset(), expected.money()?)];

        verify_changed_image_ranges(
            &source,
            &expected,
            expected.raw_image(),
            &writes,
            &[],
            &ChangeSet::default(),
        )?;

        let mut corrupted = expected.raw_image().to_vec();
        corrupted[0] ^= 1;
        let error = verify_changed_image_ranges(&source, &expected, &corrupted, &writes, &[], &ChangeSet::default())
            .expect_err("a collateral byte outside the money field must be rejected");
        assert!(error.to_string().contains("outside declared changed ranges"));
        Ok(())
    }

    #[test]
    fn item_writer_rejects_collateral_changes_inside_another_registry_record() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-source.sav");
        let source = Save::read(packed)?;
        let template = source
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 13398)
            .ok_or("placement fixture template should exist")?;
        let object_id = (1..u16::MAX)
            .rev()
            .find(|candidate| {
                !source
                    .registry_objects()
                    .iter()
                    .any(|record| record.object_id == *candidate)
            })
            .ok_or("placement fixture should have a free object id")?;
        let output = apply(
            &source,
            &ChangeSet::new(vec![Change::AddItem {
                template_object: template.object_id,
                item_key: template.name.clone(),
                object_id,
                quantity: 1,
            }]),
        )?;
        let replacement = Save::read(output.as_slice())?;
        let actor = replacement
            .registry_objects()
            .iter()
            .find(|record| record.object_id == replacement.actor_id())
            .ok_or("replacement actor should exist")?;
        let mut corrupted = replacement.raw_image().to_vec();
        *corrupted
            .get_mut(actor.object_id_offset)
            .ok_or("actor object id should be inside the OBJECT chunk")? ^= 1;
        let error =
            match verify_changed_image_ranges(&source, &replacement, &corrupted, &[], &[2], &ChangeSet::default()) {
                Err(error) => error,
                Ok(()) => return Err("an unrelated actor record change must be rejected".into()),
            };
        assert!(error.to_string().contains("record"), "{error}");
        Ok(())
    }

    #[test]
    fn relation_writer_rejects_collateral_changes_inside_another_character_row() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-factions/soc-source.sav");
        let source = Save::read(packed)?;
        let registry = source
            .relation_registry
            .as_ref()
            .ok_or("relation fixture should have a readable registry")?;
        let row = registry
            .relation_rows
            .iter()
            .find(|row| row.object_id != source.actor_id() && !row.communities.is_empty())
            .ok_or("relation fixture should have another character row")?;
        let community = row.communities.first().ok_or("character row should have a community")?;
        let relation_chunk = source
            .chunks()
            .iter()
            .find(|chunk| chunk.kind == 9)
            .ok_or("relation chunk should exist")?;
        let offset = relation_chunk
            .offset
            .checked_add(community.goodwill_offset)
            .ok_or("relation offset should fit")?;
        let mut corrupted = source.raw_image().to_vec();
        *corrupted
            .get_mut(offset)
            .ok_or("goodwill field should be inside the chunk")? ^= 1;

        let error = verify_changed_image_ranges(&source, &source, &corrupted, &[], &[9], &ChangeSet::default())
            .expect_err("a change to an unrelated character row must be rejected");
        assert!(error.to_string().contains("row"), "{error}");
        Ok(())
    }

    #[test]
    fn added_clone_does_not_keep_template_story_ids() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-source.sav");
        let initial = Save::read(packed)?;
        let template = initial
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 4660)
            .ok_or("add fixture template should exist")?;
        let modified_source = seed_template_metadata(&initial, template)?;
        let source = Save::read(modified_source.as_slice())?;
        let seeded_template = source
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 4660)
            .ok_or("seeded template should remain in the registry")?;
        let template_end = seeded_template
            .record_offset
            .checked_add(seeded_template.record_length)
            .ok_or("seeded template record range should not overflow")?;
        let template_bytes = source
            .raw_image()
            .get(seeded_template.record_offset..template_end)
            .ok_or("seeded template record should be in the source image")?
            .to_vec();
        assert_eq!(seeded_template.story_id, Some(73));
        assert_eq!(seeded_template.spawn_story_id, Some(91));
        assert_eq!(seeded_template.spawn_id, Some(0x1234));
        assert_eq!(seeded_template.name_replace, "quest_template");
        assert_eq!(source.custom_data(seeded_template), Some(&b"logic = true"[..]));
        assert_eq!(
            source
                .inventory()?
                .iter()
                .find(|item| item.handle == 4660)
                .and_then(|item| item.count),
            Some(30)
        );
        let changes = ChangeSet::new(vec![Change::AddItem {
            template_object: 4660,
            item_key: "ammo_9x39_pab9".to_owned(),
            object_id: 4661,
            quantity: 17,
        }]);

        let output = apply(&source, &changes)?;
        let read_back = Save::read(output.as_slice())?;
        let cloned = read_back
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 4661)
            .ok_or("added clone should be present after read-back")?;
        assert_eq!(cloned.story_id, Some(u32::MAX));
        assert_eq!(cloned.spawn_story_id, Some(u32::MAX));
        assert_eq!(cloned.spawn_id, Some(u16::MAX));
        assert_eq!(cloned.name_replace, "");
        assert_eq!(read_back.custom_data(cloned), Some(&[][..]));
        let verified_template = read_back
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 4660)
            .ok_or("template should remain in the read-back registry")?;
        let verified_template_end = verified_template
            .record_offset
            .checked_add(verified_template.record_length)
            .ok_or("verified template record range should not overflow")?;
        assert_eq!(
            read_back
                .raw_image()
                .get(verified_template.record_offset..verified_template_end)
                .ok_or("verified template record should be in the read-back image")?,
            template_bytes.as_slice()
        );
        Ok(())
    }

    #[test]
    fn addition_refuses_to_mutate_its_template() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-source.sav");
        let source = Save::read(packed)?;
        let changes = ChangeSet::new(vec![
            Change::SetStack {
                target_object: 4660,
                old_value: 30,
                new_value: 29,
            },
            Change::AddItem {
                template_object: 4660,
                item_key: "ammo_9x39_pab9".to_owned(),
                object_id: 4662,
                quantity: 17,
            },
        ]);

        let error = match apply(&source, &changes) {
            Ok(_) => return Err("an item-add operation must not mutate its template".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("template"), "{error}");
        Ok(())
    }

    fn seed_template_metadata(
        initial: &Save,
        template: &crate::RegistryObject,
    ) -> std::result::Result<sse_core::SaveBuffer, Box<dyn std::error::Error>> {
        let mut raw = initial.raw_image().to_vec();
        let (story_offset, spawn_story_offset) = state_story_offsets(&raw, template)?;
        write_u32(&mut raw, story_offset, 73)?;
        write_u32(&mut raw, spawn_story_offset, 91)?;
        write_u16(
            &mut raw,
            template.spawn_id_offset.ok_or("template should expose spawn id")?,
            0x1234,
        )?;

        let spawn_absolute = template
            .record_offset
            .checked_add(2)
            .ok_or("template SPAWN offset overflow")?;
        let record_end = template
            .record_offset
            .checked_add(template.record_length)
            .ok_or("template record range overflow")?;
        let record = raw
            .get(template.record_offset..record_end)
            .ok_or("template record should fit")?
            .to_vec();
        let spawn_length = usize::from(read_u16(&record, 0)?);
        let spawn_end = 2_usize
            .checked_add(spawn_length)
            .ok_or("template SPAWN range overflow")?;
        let update_size_end = spawn_end.checked_add(2).ok_or("template UPDATE size overflow")?;
        let update_length = usize::from(read_u16(&record, spawn_end)?);
        let update_end = update_size_end
            .checked_add(update_length)
            .ok_or("template UPDATE range overflow")?;
        let mut spawn = record
            .get(2..spawn_end)
            .ok_or("template SPAWN should fit its record")?
            .to_vec();
        let relative = |range: &std::ops::Range<usize>| -> std::result::Result<
            std::ops::Range<usize>,
            Box<dyn std::error::Error>,
        > {
            Ok(range
                .start
                .checked_sub(spawn_absolute)
                .ok_or("template metadata starts before SPAWN")?
                ..range
                    .end
                    .checked_sub(spawn_absolute)
                    .ok_or("template metadata ends before SPAWN")?)
        };
        let name_replace_range = relative(&template.name_replace_range)?;
        let custom_data_range = relative(
            template
                .custom_data_range
                .as_ref()
                .ok_or("template should expose custom data")?,
        )?;
        let mut name_replace = b"quest_template".to_vec();
        name_replace.push(0);
        let mut custom_data = b"logic = true".to_vec();
        custom_data.push(0);
        let name_delta = isize::try_from(name_replace.len())?
            .checked_sub(isize::try_from(name_replace_range.len())?)
            .ok_or("name-replacement delta overflow")?;
        let custom_delta = isize::try_from(custom_data.len())?
            .checked_sub(isize::try_from(custom_data_range.len())?)
            .ok_or("custom-data delta overflow")?;
        spawn.splice(custom_data_range, custom_data);
        spawn.splice(name_replace_range, name_replace);

        let old_state_start = template
            .state_offset
            .checked_sub(spawn_absolute)
            .ok_or("template STATE begins before SPAWN")?;
        let state_start = usize::try_from(
            isize::try_from(old_state_start)?
                .checked_add(name_delta)
                .ok_or("seeded STATE offset overflow")?,
        )?;
        let state_length = usize::try_from(
            isize::try_from(template.state_length)?
                .checked_add(custom_delta)
                .ok_or("seeded STATE length overflow")?,
        )?;
        let size_offset = state_start.checked_sub(2).ok_or("STATE size field precedes SPAWN")?;
        let size = state_length.checked_add(2).ok_or("seeded STATE size overflow")?;
        write_u16(&mut spawn, size_offset, u16::try_from(size)?)?;

        let mut replacement_record = Vec::with_capacity(4 + spawn.len() + update_length);
        replacement_record.extend_from_slice(&u16::try_from(spawn.len())?.to_le_bytes());
        replacement_record.extend_from_slice(&spawn);
        replacement_record.extend_from_slice(&u16::try_from(update_length)?.to_le_bytes());
        replacement_record.extend_from_slice(
            record
                .get(update_size_end..update_end)
                .ok_or("template UPDATE should fit its record")?,
        );

        let object_chunk = initial
            .chunks()
            .iter()
            .find(|chunk| chunk.kind == 2)
            .ok_or("template save should have an OBJECT chunk")?;
        let object_payload = initial.object_chunk_bytes(&raw)?;
        let start = template
            .record_offset
            .checked_sub(object_chunk.offset)
            .ok_or("template record precedes OBJECT chunk")?;
        let end = start
            .checked_add(template.record_length)
            .ok_or("template record range overflow")?;
        let mut replacement_payload = Vec::with_capacity(
            object_payload
                .len()
                .checked_sub(template.record_length)
                .and_then(|length| length.checked_add(replacement_record.len()))
                .ok_or("seeded OBJECT payload length overflow")?,
        );
        replacement_payload.extend_from_slice(
            object_payload
                .get(..start)
                .ok_or("template record start should fit OBJECT payload")?,
        );
        replacement_payload.extend_from_slice(&replacement_record);
        replacement_payload.extend_from_slice(
            object_payload
                .get(end..)
                .ok_or("template record end should fit OBJECT payload")?,
        );
        let rebuilt = initial.rebuild_chunks(&raw, &[(2, replacement_payload.as_slice())])?;
        Ok(initial.repack(&rebuilt)?)
    }

    fn state_story_offsets(
        raw: &[u8],
        record: &crate::RegistryObject,
    ) -> std::result::Result<(usize, usize), Box<dyn std::error::Error>> {
        let state_end = record
            .state_offset
            .checked_add(record.state_length)
            .ok_or("STATE test range overflow")?;
        let state = raw
            .get(record.state_offset..state_end)
            .ok_or("STATE test range should fit")?;
        let mut reader = Cursor::new(state);
        let version = record.version;
        if version >= 1 {
            if version > 24 {
                if version < 83 {
                    reader.skip(4)?;
                }
            } else {
                reader.skip(1)?;
            }
            if version < 4 {
                reader.skip(2)?;
            }
            reader.skip(6)?;
        }
        if version >= 4 {
            reader.skip(4)?;
        }
        if version >= 8 {
            reader.skip(4)?;
        }
        if version > 22 && version <= 79 {
            reader.skip(2)?;
        }
        if version > 23 && version < 84 {
            reader.zero_terminated(1 << 20)?;
        }
        if version > 49 {
            reader.skip(4)?;
        }
        if version > 57 {
            reader.zero_terminated(1 << 20)?;
        }
        let story_offset = record
            .state_offset
            .checked_add(reader.position())
            .ok_or("story offset overflow")?;
        reader.u32()?;
        let spawn_story_offset = record
            .state_offset
            .checked_add(reader.position())
            .ok_or("spawn story offset overflow")?;
        reader.u32()?;
        Ok((story_offset, spawn_story_offset))
    }

    fn reference_image_with_cleared_clone_metadata(
        expected: &Save,
        object_id: u16,
    ) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
        let added = expected
            .registry_objects()
            .iter()
            .find(|record| record.object_id == object_id)
            .ok_or("reference add fixture should contain the added object")?;
        let spawn_absolute = added
            .record_offset
            .checked_add(2)
            .ok_or("spawn absolute offset overflow")?;
        let record_end = added
            .record_offset
            .checked_add(added.record_length)
            .ok_or("expected added record range overflow")?;
        let record = expected
            .raw_image()
            .get(added.record_offset..record_end)
            .ok_or("reference added record should fit the image")?;
        let spawn_length = usize::from(read_u16(record, 0)?);
        let spawn_end = 2_usize
            .checked_add(spawn_length)
            .ok_or("reference SPAWN range overflow")?;
        let update_size_end = spawn_end.checked_add(2).ok_or("UPDATE size range overflow")?;
        let update_length = usize::from(read_u16(record, spawn_end)?);
        let update_end = update_size_end
            .checked_add(update_length)
            .ok_or("reference UPDATE range overflow")?;
        if update_end != record.len() {
            return Err("reference add record framing should be exact".into());
        }
        let mut spawn = record
            .get(2..spawn_end)
            .ok_or("reference SPAWN should fit its record")?
            .to_vec();
        let relative = |range: &std::ops::Range<usize>| -> std::result::Result<
            std::ops::Range<usize>,
            Box<dyn std::error::Error>,
        > {
            Ok(range
                .start
                .checked_sub(spawn_absolute)
                .ok_or("reference string begins before SPAWN")?
                ..range
                    .end
                    .checked_sub(spawn_absolute)
                    .ok_or("reference string ends before SPAWN")?)
        };
        let name_replace_range = relative(&added.name_replace_range)?;
        let custom_data_range = relative(
            added
                .custom_data_range
                .as_ref()
                .ok_or("reference added object should expose custom data")?,
        )?;
        let splices = [name_replace_range, custom_data_range];
        let mut replacement_ranges = splices.to_vec();
        replacement_ranges.sort_unstable_by_key(|range| range.start);
        if replacement_ranges.windows(2).any(|pair| pair[0].end > pair[1].start) {
            return Err("reference SPAWN metadata ranges should not overlap".into());
        }
        for range in replacement_ranges.iter().rev() {
            if range.end > spawn.len() {
                return Err("reference SPAWN metadata range should fit".into());
            }
            spawn.splice(range.clone(), [0_u8]);
        }

        let name_delta = 1_isize
            .checked_sub(isize::try_from(splices[0].len())?)
            .ok_or("name-replacement size delta overflow")?;
        let custom_delta = 1_isize
            .checked_sub(isize::try_from(splices[1].len())?)
            .ok_or("custom-data size delta overflow")?;
        let original_state_start = added
            .state_offset
            .checked_sub(spawn_absolute)
            .ok_or("reference STATE begins before SPAWN")?;
        let state_start = usize::try_from(
            isize::try_from(original_state_start)?
                .checked_add(name_delta)
                .ok_or("normalized STATE offset overflow")?,
        )?;
        let state_length = usize::try_from(
            isize::try_from(added.state_length)?
                .checked_add(custom_delta)
                .ok_or("normalized STATE length overflow")?,
        )?;
        let state_size = state_length.checked_add(2).ok_or("normalized STATE size overflow")?;
        let state_size_offset = state_start.checked_sub(2).ok_or("STATE size field precedes SPAWN")?;
        write_u16(&mut spawn, state_size_offset, u16::try_from(state_size)?)?;

        let parsed = crate::save::parse_spawn(&spawn, 0)?;
        write_u16(
            &mut spawn,
            parsed
                .spawn_id_offset
                .ok_or("reference added SPAWN should expose spawn id")?,
            u16::MAX,
        )?;
        write_u32(
            &mut spawn,
            parsed
                .story_id_offset
                .ok_or("reference added STATE should expose story id")?,
            u32::MAX,
        )?;
        write_u32(
            &mut spawn,
            parsed
                .spawn_story_id_offset
                .ok_or("reference added STATE should expose spawn story id")?,
            u32::MAX,
        )?;

        let new_spawn_length = u16::try_from(spawn.len())?;
        let mut normalized_record = Vec::with_capacity(4 + spawn.len() + update_length);
        normalized_record.extend_from_slice(&new_spawn_length.to_le_bytes());
        normalized_record.extend_from_slice(&spawn);
        normalized_record.extend_from_slice(&u16::try_from(update_length)?.to_le_bytes());
        normalized_record.extend_from_slice(
            record
                .get(update_size_end..update_end)
                .ok_or("reference UPDATE should fit its record")?,
        );

        let object_chunk = expected
            .chunks()
            .iter()
            .find(|chunk| chunk.kind == 2)
            .ok_or("reference save should contain OBJECT chunk")?;
        let expected_payload = expected.object_chunk_bytes(expected.raw_image())?;
        let start = added
            .record_offset
            .checked_sub(object_chunk.offset)
            .ok_or("added record offset precedes OBJECT payload")?;
        let end = start
            .checked_add(added.record_length)
            .ok_or("expected record range overflow")?;
        let mut payload = Vec::with_capacity(
            expected_payload
                .len()
                .checked_sub(added.record_length)
                .and_then(|length| length.checked_add(normalized_record.len()))
                .ok_or("normalized OBJECT chunk length overflow")?,
        );
        payload.extend_from_slice(
            expected_payload
                .get(..start)
                .ok_or("expected record start should fit OBJECT chunk")?,
        );
        payload.extend_from_slice(&normalized_record);
        payload.extend_from_slice(
            expected_payload
                .get(end..)
                .ok_or("expected record end should fit OBJECT chunk")?,
        );
        Ok(expected.rebuild_chunks(expected.raw_image(), &[(2, payload.as_slice())])?)
    }

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
            let bundle = CatalogBundleReader::load_embedded()
                .get(source.format().id())
                .ok_or("matching faction catalog should exist")?;
            let faction_catalog = bundle.factions.as_ref().ok_or("faction catalog should exist")?;
            let old_faction = faction_catalog
                .resolve_numeric(source.player_faction().ok_or("actor faction should be present")?)
                .ok_or("old actor faction should resolve")?;
            let bandit = faction_catalog.resolve("bandit")?;
            let old_faction_key = old_faction.key.clone();
            let bandit_id = bandit.numeric_id.ok_or("bandit numeric id should exist")?;
            let player = ChangeSet::new(vec![Change::SetPlayerFaction {
                target_object: source.actor_id(),
                old_value: source.player_faction().ok_or("actor faction should be present")?,
                faction_key: "bandit".to_owned(),
            }]);
            let player_output = apply(&source, &player)?;
            assert_eq!(player_output.as_slice(), player_expected);
            let player_read_back = Save::read(player_output.as_slice())?;
            let player_inverse = ChangeSet::new(vec![Change::SetPlayerFaction {
                target_object: source.actor_id(),
                old_value: bandit_id,
                faction_key: old_faction_key,
            }]);
            assert_eq!(apply(&player_read_back, &player_inverse)?.as_slice(), source_bytes);

            let relation = ChangeSet::new(vec![Change::SetFactionRelation {
                target_object: source.actor_id(),
                faction_key: "bandit".to_owned(),
                old_value: None,
                new_value: 375,
            }]);
            assert_eq!(apply(&source, &relation)?.as_slice(), relation_expected);
            let combined = ChangeSet::new(vec![player.changes()[0].clone(), relation.changes()[0].clone()]);
            assert_eq!(apply(&source, &combined)?.as_slice(), combined_expected);

            let existing_faction = faction_catalog
                .resolve_numeric(0)
                .ok_or("fixture relation zero should resolve")?;
            let existing_relation = ChangeSet::new(vec![Change::SetFactionRelation {
                target_object: source.actor_id(),
                faction_key: existing_faction.key.clone(),
                old_value: Some(100),
                new_value: 375,
            }]);
            let relation_output = apply(&source, &existing_relation)?;
            let relation_read_back = Save::read(relation_output.as_slice())?;
            let relation_inverse = ChangeSet::new(vec![Change::SetFactionRelation {
                target_object: source.actor_id(),
                faction_key: existing_faction.key.clone(),
                old_value: Some(375),
                new_value: 100,
            }]);
            assert_eq!(apply(&relation_read_back, &relation_inverse)?.as_slice(), source_bytes);
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
            let source_upgrade = UpgradeDefinition::new(
                "up_a_wpn_test".to_owned(),
                None,
                Some("weapon".to_owned()),
                Some(record.name.clone()),
                "synthetic fixture inverse".to_owned(),
                source.format().id().to_owned(),
                None,
                None,
                None,
                vec![record.name.clone()],
            )?;
            let inverse_upgrade = UpgradeDefinition::new(
                "up_c_wpn_test".to_owned(),
                None,
                Some("weapon".to_owned()),
                Some(record.name.clone()),
                "synthetic fixture inverse".to_owned(),
                source.format().id().to_owned(),
                None,
                None,
                None,
                vec![record.name.clone()],
            )?;
            let inverse_catalog =
                UpgradeCatalog::new(source.format().id().to_owned(), vec![source_upgrade, inverse_upgrade])?;
            let changes = ChangeSet::new(vec![Change::SetUpgrades {
                target_object: record.object_id,
                old_value: vec!["up_a_wpn_test".to_owned(), "legacy_unknown".to_owned()],
                new_value: vec!["legacy_unknown".to_owned(), "up_c_wpn_test".to_owned()],
            }]);
            let output = apply_with_catalog(&source, &changes, None, Some(&catalog))?;
            assert_eq!(output.as_slice(), expected);
            assert_eq!(Save::read(output.as_slice())?.raw_image(), expected_raw);
            let verified = Save::read(output.as_slice())?;
            let inverse = ChangeSet::new(vec![Change::SetUpgrades {
                target_object: record.object_id,
                old_value: vec!["legacy_unknown".to_owned(), "up_c_wpn_test".to_owned()],
                new_value: vec!["up_a_wpn_test".to_owned(), "legacy_unknown".to_owned()],
            }]);
            assert_eq!(
                apply_with_catalog(&verified, &inverse, None, Some(&inverse_catalog))?.as_slice(),
                source_bytes
            );
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
    fn durability_refuses_when_update_condition_position_is_not_proven() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav");
        let initial = Save::read(packed)?;
        let item = initial
            .inventory()?
            .into_iter()
            .find(|item| item.handle == 0x3456)
            .ok_or("durability fixture target should be actor-owned")?;
        let update_offset = item
            .update_condition_offset
            .ok_or("durability fixture should prove its UPDATE condition")?;
        let mut raw = initial.raw_image().to_vec();
        let update = raw
            .get_mut(update_offset)
            .ok_or("UPDATE condition offset should be in the image")?;
        *update ^= 0xFF;
        let unproven_packed = initial.repack(&raw)?;
        let source = Save::read(unproven_packed.as_slice())?;
        let item = source
            .inventory()?
            .into_iter()
            .find(|item| item.handle == 0x3456)
            .ok_or("durability fixture target should remain actor-owned")?;
        assert!(item.update_condition_offset.is_none());

        let changes = ChangeSet::new(vec![Change::SetDurability {
            target_object: 0x3456,
            old_value: 0.25,
            new_value: 0.75,
        }]);
        let error = match apply(&source, &changes) {
            Ok(_) => return Err("durability write must fail without a proven UPDATE offset".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("proven UPDATE condition field"));
        Ok(())
    }

    #[test]
    fn durability_refuses_multiple_matching_bytes_in_the_update_record() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav");
        let initial = Save::read(packed)?;
        let item = initial
            .inventory()?
            .into_iter()
            .find(|item| item.handle == 0x3456)
            .ok_or("durability fixture target should be actor-owned")?;
        let condition = item.condition.ok_or("durability fixture should expose condition")?;
        let confirmed_offset = item
            .update_condition_offset
            .ok_or("durability fixture should prove its UPDATE condition")?;
        let record = initial
            .registry_objects()
            .iter()
            .find(|record| record.object_id == item.handle)
            .ok_or("durability fixture record should exist")?;
        let update_end = record
            .update_offset
            .checked_add(record.update_length)
            .ok_or("UPDATE record range should fit usize")?;
        let first_candidate = record
            .update_offset
            .checked_add(3)
            .ok_or("first supported UPDATE offset should fit usize")?;
        let second_candidate = record
            .update_offset
            .checked_add(4)
            .ok_or("second supported UPDATE offset should fit usize")?;
        if first_candidate >= update_end || second_candidate >= update_end {
            return Err("UPDATE fixture should contain both supported condition offsets".into());
        }
        if confirmed_offset != first_candidate && confirmed_offset != second_candidate {
            return Err("UPDATE condition should be at a supported packet offset".into());
        }
        let mut raw = initial.raw_image().to_vec();
        for offset in [first_candidate, second_candidate] {
            let candidate = raw
                .get_mut(offset)
                .ok_or("supported UPDATE candidate should be in the image")?;
            *candidate = encode_condition_q8(condition);
        }
        let ambiguous_packed = initial.repack(&raw)?;
        let ambiguous = Save::read(ambiguous_packed.as_slice())?;
        let ambiguous_item = ambiguous
            .inventory()?
            .into_iter()
            .find(|item| item.handle == 0x3456)
            .ok_or("durability fixture target should remain actor-owned")?;
        assert!(ambiguous_item.update_condition_offset.is_none());
        Ok(())
    }

    #[test]
    fn placement_change_matches_the_reference_raw_and_packed_fixture() -> TestResult {
        let pairs: [(&[u8], &[u8], &[u8], Placement); 6] = [
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
    fn belt_placement_refuses_without_capacity_evidence() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-belt-source.sav");
        let source = Save::read(packed)?;
        let changes = ChangeSet::new(vec![Change::SetPlacement {
            target_object: 0x3456,
            destination: Placement::Belt,
        }]);
        let error = match apply(&source, &changes) {
            Ok(_) => return Err("belt placement must fail without capacity evidence".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("belt capacity cannot be proven"));
        Ok(())
    }

    #[test]
    fn placement_refuses_an_occupied_slot() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-source.sav");
        let initial = Save::read(packed)?;
        let target = 0x3456;
        let template = initial
            .registry_objects()
            .iter()
            .find(|record| record.object_id == target)
            .ok_or("placement fixture target should exist")?;
        let duplicated = apply(
            &initial,
            &ChangeSet::new(vec![Change::AddItem {
                template_object: target,
                item_key: template.name.clone(),
                object_id: 0x3457,
                quantity: 1,
            }]),
        )?;
        let duplicated = Save::read(duplicated.as_slice())?;
        let occupier = duplicated
            .inventory()?
            .into_iter()
            .find(|item| item.handle == 0x3457)
            .ok_or("cloned occupier should be actor-owned")?;
        assert_eq!(occupier.placement_base_slot, Some(3));
        let offset = occupier
            .placement_offset
            .ok_or("selected slot occupier should expose its place offset")?;
        let mut raw = duplicated.raw_image().to_vec();
        let occupied_slot = (3 << 10) | (3 << 4) | 1;
        write_u16(&mut raw, offset, occupied_slot)?;
        let occupied_packed = duplicated.repack(&raw)?;
        let source = Save::read(occupied_packed.as_slice())?;

        let changes = ChangeSet::new(vec![Change::SetPlacement {
            target_object: target,
            destination: Placement::Slot(3),
        }]);
        let error = match apply(&source, &changes) {
            Ok(_) => return Err("placement must fail when its destination slot is occupied".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("slot 3 is already occupied"));
        Ok(())
    }

    #[test]
    fn taking_a_stash_item_matches_the_reference_fixture() -> TestResult {
        let pairs: [(&[u8], &[u8], &[u8]); 2] = [
            (
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-soc-source.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-soc-take.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-soc-take.raw"),
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
    fn taking_a_clear_sky_stash_item_refuses_without_proven_placement() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cs-source.sav");
        let source = Save::read(packed)?;
        let error = match apply(
            &source,
            &ChangeSet::new(vec![Change::MoveItem {
                target_object: 9029,
                old_parent: 16,
                new_parent: source.actor_id(),
            }]),
        ) {
            Err(error) => error,
            Ok(_) => return Err("a stash item without a proven placement field must be refused".into()),
        };
        assert!(error.to_string().contains("placement"), "{error}");
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
    fn item_removal_refuses_objects_with_story_ids() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-soc-source.sav");
        let initial = Save::read(packed)?;
        let target = initial
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 4660)
            .ok_or("delete fixture target should exist")?;
        let story_id_offset = target.story_id_offset.ok_or("delete fixture should expose story id")?;
        let mut raw = initial.raw_image().to_vec();
        write_u32(&mut raw, story_id_offset, 73)?;
        let story_packed = initial.repack(&raw)?;
        let source = Save::read(story_packed.as_slice())?;

        let changes = ChangeSet::new(vec![Change::RemoveItem { target_object: 4660 }]);
        let error = match apply(&source, &changes) {
            Ok(_) => return Err("story-linked objects must not be removed".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("story-linked object"));
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
            let safe_source_bytes = seed_removable_source(source_bytes, 4660)?;
            let source = Save::read(safe_source_bytes.as_slice())?;
            let changes = ChangeSet::new(vec![Change::RemoveItem { target_object: 4660 }]);
            let output = apply(&source, &changes)?;
            assert_eq!(output.as_slice(), expected_bytes);
            let verified = Save::read(output.as_slice())?;
            assert_eq!(verified.raw_image(), expected_raw);
        }
        Ok(())
    }

    #[test]
    fn item_addition_matches_same_section_fixtures_and_refuses_cross_section_templates() -> TestResult {
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
            let template = source
                .registry_objects()
                .iter()
                .find(|record| record.object_id == template_object)
                .ok_or("add template should exist")?;
            if !template.name.eq_ignore_ascii_case(item_key) {
                let error = match apply(
                    &source,
                    &ChangeSet::new(vec![Change::AddItem {
                        template_object,
                        item_key: item_key.to_owned(),
                        object_id,
                        quantity,
                    }]),
                ) {
                    Ok(_) => return Err("a different-section template must be refused".into()),
                    Err(error) => error,
                };
                assert!(error.to_string().contains("template section does not match"));
                continue;
            }
            let changes = ChangeSet::new(vec![Change::AddItem {
                template_object,
                item_key: item_key.to_owned(),
                object_id,
                quantity,
            }]);
            let output = apply(&source, &changes)?;
            let actual_raw = Save::read(output.as_slice())?;
            let expected = Save::read(expected_bytes)?;
            assert_eq!(expected.raw_image(), expected_raw);
            assert_eq!(
                actual_raw.raw_image(),
                reference_image_with_cleared_clone_metadata(&expected, object_id)?.as_slice()
            );
            let added = actual_raw
                .registry_objects()
                .iter()
                .find(|record| record.object_id == object_id)
                .ok_or("added object should be in the parsed output")?;
            assert_eq!(added.story_id, Some(u32::MAX));
            assert_eq!(added.spawn_story_id, Some(u32::MAX));
            assert_eq!(added.spawn_id, Some(u16::MAX));
            assert_eq!(added.name_replace, "");
            assert_eq!(actual_raw.custom_data(added), Some(&[][..]));
            let inverse = ChangeSet::new(vec![Change::RemoveItem {
                target_object: object_id,
            }]);
            assert_eq!(apply(&actual_raw, &inverse)?.as_slice(), source_bytes);
        }
        Ok(())
    }

    #[test]
    fn item_addition_requires_proven_placement_and_preserves_durability() -> TestResult {
        let cases: [(&[u8], &str); 2] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cs-source.sav"),
                "Clear Sky",
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav"),
                "Call of Pripyat",
            ),
        ];
        for (packed, game) in cases {
            let source = Save::read(packed)?;
            let template = source
                .registry_objects()
                .iter()
                .find(|record| record.object_id == 13398)
                .ok_or_else(|| std::io::Error::other(format!("{game} weapon template should exist")))?;
            let template_item = source
                .inventory()?
                .into_iter()
                .find(|item| item.handle == template.object_id)
                .ok_or_else(|| std::io::Error::other(format!("{game} weapon should be actor-owned")))?;
            let condition = template_item
                .condition
                .ok_or_else(|| std::io::Error::other(format!("{game} weapon condition should be indexed")))?;
            let object_id = (1..u16::MAX)
                .find(|candidate| {
                    !source
                        .registry_objects()
                        .iter()
                        .any(|record| record.object_id == *candidate)
                })
                .ok_or_else(|| std::io::Error::other(format!("{game} fixture has no free object id")))?;
            let result = apply(
                &source,
                &ChangeSet::new(vec![Change::AddItem {
                    template_object: template.object_id,
                    item_key: template.name.clone(),
                    object_id,
                    quantity: 1,
                }]),
            );
            if game == "Clear Sky" {
                let error = match result {
                    Err(error) => error,
                    Ok(_) => return Err("Clear Sky's unplaced template must be refused".into()),
                };
                assert!(error.to_string().contains("placement"), "{game}: {error}");
                continue;
            }
            let output = result?;
            let verified = Save::read(output.as_slice())?;
            let added_condition = verified
                .inventory()?
                .into_iter()
                .find(|item| item.handle == object_id)
                .and_then(|item| item.condition);
            assert_eq!(added_condition, Some(condition), "{game}");
        }
        Ok(())
    }

    #[test]
    fn item_addition_rejects_sentinel_ids() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-source.sav");
        let source = Save::read(packed)?;
        let expected_message = "0xFFFF is reserved";
        let changes = ChangeSet::new(vec![Change::AddItem {
            template_object: 4660,
            item_key: "ammo_9x39_pab9".to_owned(),
            object_id: u16::MAX,
            quantity: 1,
        }]);
        let error = match apply(&source, &changes) {
            Ok(_) => return Err(format!("addition must refuse {expected_message}").into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains(expected_message), "{error}");
        Ok(())
    }

    #[test]
    fn item_addition_refuses_a_template_from_another_section() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-source.sav");
        let source = Save::read(packed)?;
        let changes = ChangeSet::new(vec![Change::AddItem {
            template_object: 4660,
            item_key: "exo_outfit".to_owned(),
            object_id: 4661,
            quantity: 1,
        }]);
        let error = match apply(&source, &changes) {
            Ok(_) => return Err("an add template from another section must be refused".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("template section does not match"), "{error}");
        Ok(())
    }

    #[test]
    fn item_addition_refuses_quantity_greater_than_one_for_non_ammunition() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-source.sav");
        let source = Save::read(packed)?;
        let template = source
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 9029)
            .ok_or("non-ammunition template should exist")?;
        let item_key = template.name.clone();
        let changes = ChangeSet::new(vec![Change::AddItem {
            template_object: 9029,
            item_key,
            object_id: 9030,
            quantity: 2,
        }]);
        let error = match apply(&source, &changes) {
            Ok(_) => return Err("non-ammunition clones must have quantity one".into()),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("non-ammo clones require quantity 1"),
            "{error}"
        );
        Ok(())
    }

    #[test]
    fn item_addition_refuses_a_template_without_proven_placement() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-source.sav");
        let initial = Save::read(packed)?;
        let template = initial
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 9029)
            .ok_or("template object should exist")?;
        let item_key = template.name.clone();
        let client_start = template
            .client_data_offset
            .ok_or("template should expose client data")?;
        let placement_offset = client_start.checked_add(1).ok_or("placement offset overflow")?;
        let placement_end = placement_offset.checked_add(2).ok_or("placement range overflow")?;
        if template.client_data_length < 3 {
            return Err("fixture template needs the packed two-byte placement layout".into());
        }
        let mut raw = initial.raw_image().to_vec();
        raw.get_mut(placement_offset..placement_end)
            .ok_or("template placement should fit")?
            .fill(0);
        let packed_unknown = initial.repack(&raw)?;
        let source = Save::read(packed_unknown.as_slice())?;
        let error = match apply(
            &source,
            &ChangeSet::new(vec![Change::AddItem {
                template_object: 9029,
                item_key,
                object_id: 9030,
                quantity: 1,
            }]),
        ) {
            Err(error) => error,
            Ok(_) => return Err("an unproven template placement must be refused".into()),
        };
        assert!(error.to_string().contains("placement"), "{error}");
        Ok(())
    }

    #[test]
    fn item_addition_sets_and_reads_back_backpack_placement() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-source.sav");
        let source = Save::read(packed)?;
        let template = source
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 13398)
            .ok_or("placement fixture template should exist")?;
        let original_condition = source
            .inventory()?
            .into_iter()
            .find(|item| item.handle == template.object_id)
            .ok_or("placement fixture template should be actor-owned")?
            .condition;
        let object_id = (1..u16::MAX)
            .find(|candidate| {
                !source
                    .registry_objects()
                    .iter()
                    .any(|record| record.object_id == *candidate)
            })
            .ok_or("placement fixture should have a free object id")?;
        let output = apply(
            &source,
            &ChangeSet::new(vec![Change::AddItem {
                template_object: template.object_id,
                item_key: template.name.clone(),
                object_id,
                quantity: 1,
            }]),
        )?;
        let verified = Save::read(output.as_slice())?;
        let item = verified
            .inventory()?
            .into_iter()
            .find(|item| item.handle == object_id)
            .ok_or("added item should be in the verified inventory")?;
        assert!(item.placement_value.is_some_and(|value| value & 0x0F == 3));
        assert_eq!(item.condition, original_condition);
        Ok(())
    }

    #[test]
    fn declared_record_bytes_outside_its_writes_are_rejected_when_the_object_chunk_is_rebuilt() -> TestResult {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-source.sav");
        let source = Save::read(packed)?;
        let actor_id = source.actor_id();
        let money = source.money()?;
        let new_money = money.checked_add(1).ok_or("money test value overflow")?;
        let changes = ChangeSet::new(vec![
            Change::SetMoney {
                target_object: actor_id,
                old_value: money,
                new_value: new_money,
            },
            Change::AddItem {
                template_object: 4660,
                item_key: "ammo_9x39_pab9".to_owned(),
                object_id: 4661,
                quantity: 17,
            },
        ]);
        let output = apply(&source, &changes)?;
        let replacement = Save::read(output.as_slice())?;
        let writes = [PendingWrite::u32(source.money_offset(), new_money)];

        verify_changed_image_ranges(&source, &replacement, replacement.raw_image(), &writes, &[2], &changes)?;

        let actor = replacement
            .registry_objects()
            .iter()
            .find(|record| record.object_id == actor_id)
            .ok_or("replacement actor should exist")?;
        let mut corrupted = replacement.raw_image().to_vec();
        *corrupted
            .get_mut(actor.state_offset)
            .ok_or("actor STATE should be inside the image")? ^= 1;
        let error = verify_changed_image_ranges(&source, &replacement, &corrupted, &writes, &[2], &changes)
            .expect_err("a byte outside the declared money write must be rejected");
        assert!(error.to_string().contains("declared writes"), "{error}");
        Ok(())
    }

    #[test]
    fn writes_outside_the_object_chunk_are_not_attributed_to_records() -> TestResult {
        let source = Save::read(include_bytes!(
            "../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav"
        ))?;
        let alife = source
            .chunks()
            .iter()
            .find(|chunk| chunk.kind == 0)
            .ok_or("ALIFE chunk should exist")?;
        let outside = [PendingWrite::u32(alife.offset, 0)];
        assert!(super::declared_record_writes(&source, &outside)?.is_empty());
        Ok(())
    }

    #[test]
    fn single_changes_paired_with_addition_or_removal_never_report_damage() -> TestResult {
        let fixtures: [&[u8]; 4] = [
            include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav"),
            include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-source.sav"),
            include_bytes!("../../../fixtures/synthetic/writer-factions/soc-source.sav"),
            include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav"),
        ];
        for fixture in fixtures {
            // An unequipped actor-owned item is removed by the pairs; the others carry the single changes and templates.
            let initial = Save::read(fixture)?;
            let removal = initial
                .inventory()?
                .iter()
                .rev()
                .find(|item| item.placement_value.is_none_or(|value| value & 0x0F != 1))
                .map(|item| item.handle);
            let seeded = match removal {
                Some(handle) => seed_removable_source(fixture, handle)?,
                None => initial.repack(initial.raw_image())?,
            };
            let source = Save::read(seeded.as_slice())?;
            let items = source.inventory()?;
            let actor_id = source.actor_id();
            let money = source.money()?;
            let mut singles = vec![Change::SetMoney {
                target_object: actor_id,
                old_value: money,
                new_value: money.checked_add(1).ok_or("money overflow")?,
            }];
            if let Some(item) = items.iter().find(|item| item.count.is_some_and(|count| count >= 2)) {
                let old_value = item.count.ok_or("stack count should be known")?;
                singles.push(Change::SetStack {
                    target_object: item.handle,
                    old_value,
                    new_value: old_value - 1,
                });
            }
            if let Some(item) = items.iter().find(|item| item.condition.is_some()) {
                let old_value = item.condition.ok_or("condition should be known")?;
                singles.push(Change::SetDurability {
                    target_object: item.handle,
                    old_value,
                    new_value: if old_value > 0.5 {
                        old_value - 0.25
                    } else {
                        old_value + 0.25
                    },
                });
            }
            if let Some(item) = items.iter().find(|item| item.placement_value.is_some()) {
                singles.push(Change::SetPlacement {
                    target_object: item.handle,
                    destination: Placement::Ruck,
                });
            }
            if let Some(old_value) = source.player_faction() {
                singles.push(Change::SetPlayerFaction {
                    target_object: actor_id,
                    old_value,
                    faction_key: "bandit".to_owned(),
                });
            }
            singles.push(Change::SetFactionRelation {
                target_object: actor_id,
                faction_key: "bandit".to_owned(),
                old_value: None,
                new_value: 375,
            });
            singles.push(Change::AddInfoPortions {
                target_object: actor_id,
                info_portions: vec!["pair_matrix_flag".to_owned()],
            });

            let ids = source
                .registry_objects()
                .iter()
                .map(|record| record.object_id)
                .collect::<std::collections::HashSet<_>>();
            let added_id = (1..u16::MAX)
                .rev()
                .find(|candidate| !ids.contains(candidate))
                .ok_or("fixture should have a free object id")?;
            let template = items
                .iter()
                .find(|item| Some(item.handle) != removal && item.count.is_none())
                .or_else(|| items.iter().find(|item| Some(item.handle) != removal));
            let mut partners = Vec::new();
            if let Some(handle) = removal {
                partners.push(Change::RemoveItem { target_object: handle });
            }
            if let Some(template) = template {
                let quantity = if template.section.to_ascii_lowercase().starts_with("ammo_") {
                    17
                } else {
                    1
                };
                partners.push(Change::AddItem {
                    template_object: template.handle,
                    item_key: template.section.clone(),
                    object_id: added_id,
                    quantity,
                });
            }

            let mut accepted = 0_usize;
            for single in &singles {
                for partner in &partners {
                    let changes = ChangeSet::new(vec![single.clone(), partner.clone()]);
                    match apply(&source, &changes) {
                        Ok(_) => accepted = accepted.saturating_add(1),
                        Err(sse_core::Error::Damaged(message)) => {
                            return Err(format!("{single:?} with {partner:?} reported damage: {message}").into());
                        }
                        Err(_) => {}
                    }
                }
            }
            assert!(accepted > 0, "at least one pair should be applicable to this fixture");
        }
        Ok(())
    }

    fn seed_removable_source(
        packed: &[u8],
        target_object: u16,
    ) -> std::result::Result<sse_core::SaveBuffer, Box<dyn std::error::Error>> {
        let initial = Save::read(packed)?;
        let record = initial
            .registry_objects()
            .iter()
            .find(|record| record.object_id == target_object)
            .ok_or("delete fixture target should exist")?;
        let story_id_offset = record.story_id_offset.ok_or("delete fixture should expose story id")?;
        let mut raw = initial.raw_image().to_vec();
        write_u32(&mut raw, story_id_offset, u32::MAX)?;
        Ok(initial.repack(&raw)?)
    }
}
