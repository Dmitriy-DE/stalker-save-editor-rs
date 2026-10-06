//! C#-compatible local draft journals with bounded undo history.

use sse_codecs::json::{Event, Reader, Text, Writer};
use sse_core::{Error, Result};
use std::collections::{BTreeMap, HashSet};
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAXIMUM_DRAFT_BYTES: usize = 2 * 1024 * 1024;
const MAXIMUM_HISTORY_STEPS: usize = 100;
const LEGACY_PLAN_FIELDS: &[&str] = &[
    "adds",
    "attach",
    "detach",
    "durability",
    "faction_relations",
    "money",
    "moves",
    "placements",
    "player_faction",
    "raw",
    "stacks",
    "upgrades",
];
static NEXT_DRAFT_ID: AtomicU64 = AtomicU64::new(1);

/// JSON value retained for draft fields this build cannot interpret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonValue {
    /// Object members in their original order.
    Object(Vec<(String, Self)>),
    /// Array elements in their original order.
    Array(Vec<Self>),
    /// A decoded JSON string.
    String(String),
    /// A JSON number kept in its original spelling.
    Number(String),
    /// A JSON Boolean.
    Bool(bool),
    /// JSON null.
    Null,
}

/// One edit request stored in the local draft envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddRequest {
    /// Catalog key used to create an item.
    pub item_key: String,
    /// Requested stack count.
    pub quantity: u32,
    /// Destination name, normally `inventory`.
    pub destination: String,
}

impl AddRequest {
    /// Creates a validated item-add request.
    pub fn new(item_key: impl Into<String>, quantity: u32, destination: impl Into<String>) -> Result<Self> {
        let item_key = item_key.into();
        let destination = destination.into().trim().to_owned();
        if item_key.trim().is_empty() || item_key.contains('\0') {
            return Err(Error::Refused("draft add item key is empty or contains NUL".to_owned()));
        }
        if quantity == 0 || quantity > u32::from(u16::MAX) {
            return Err(Error::Refused("draft add quantity is outside 1..=65535".to_owned()));
        }
        if destination.is_empty() || destination.contains('\0') {
            return Err(Error::Refused(
                "draft add destination is empty or contains NUL".to_owned(),
            ));
        }
        Ok(Self {
            item_key,
            quantity,
            destination,
        })
    }
}

/// One item transfer into a save-resident stash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StashPut {
    /// Registry object id to move.
    pub object_id: u16,
    /// Registry object id of the destination stash.
    pub box_id: u16,
}

impl StashPut {
    /// Creates a validated stash transfer.
    pub fn new(object_id: u16, box_id: u16) -> Result<Self> {
        if object_id == 0 || object_id == u16::MAX || box_id == 0 || box_id == u16::MAX {
            return Err(Error::Refused("draft stash handles must be in 1..=65534".to_owned()));
        }
        Ok(Self { object_id, box_id })
    }
}

/// One confirmed item placement saved in the edit draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftPlacement {
    /// Move into the backpack.
    Ruck,
    /// Move onto the artifact belt.
    Belt,
    /// Equip in the validated slot number.
    Slot(u8),
}

/// One plan snapshot. `unmapped_legacy_plan` retains edits this build cannot apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftPlan {
    /// SHA-256 of the untouched source save.
    pub source_sha256: String,
    /// Requested wallet value.
    pub money: Option<u32>,
    /// Requested counts keyed by registry object id.
    pub stack_counts: BTreeMap<u32, u32>,
    /// Requested durability percentages keyed by registry object id.
    pub durability: BTreeMap<u32, u8>,
    /// Requested placement keyed by registry object id.
    pub placements: BTreeMap<u32, DraftPlacement>,
    /// Requested catalog-backed upgrades keyed by registry object id.
    pub upgrades: BTreeMap<u32, Vec<String>>,
    /// Registry handles to remove.
    pub detach_handles: Vec<u16>,
    /// Items to add.
    pub adds: Vec<AddRequest>,
    /// Stash item handles to take into the actor inventory.
    pub stash_takes: Vec<u16>,
    /// S2 stash item handles to transfer into the actor inventory.
    pub s2_stash_takes: Vec<u32>,
    /// Transfers from the actor inventory into stashes.
    pub stash_puts: Vec<StashPut>,
    /// Requested goodwill values keyed by catalog faction.
    pub faction_relations: BTreeMap<String, i32>,
    /// Confirmed level-changer object to use for actor relocation.
    pub relocate_to: Option<u16>,
    /// Original legacy plan preserved when it contains unsupported edits.
    pub unmapped_legacy_plan: Option<JsonValue>,
}

impl DraftPlan {
    /// Creates an empty edit plan for a lowercase SHA-256 source key.
    pub fn empty(source_sha256: &str) -> Result<Self> {
        validate_source_sha256(source_sha256)?;
        Ok(Self {
            source_sha256: source_sha256.to_owned(),
            money: None,
            stack_counts: BTreeMap::new(),
            durability: BTreeMap::new(),
            placements: BTreeMap::new(),
            upgrades: BTreeMap::new(),
            detach_handles: Vec::new(),
            adds: Vec::new(),
            stash_takes: Vec::new(),
            s2_stash_takes: Vec::new(),
            stash_puts: Vec::new(),
            faction_relations: BTreeMap::new(),
            relocate_to: None,
            unmapped_legacy_plan: None,
        })
    }

    fn has_changes(&self) -> bool {
        self.money.is_some()
            || !self.stack_counts.is_empty()
            || !self.durability.is_empty()
            || !self.placements.is_empty()
            || !self.upgrades.is_empty()
            || !self.detach_handles.is_empty()
            || !self.adds.is_empty()
            || !self.stash_takes.is_empty()
            || !self.s2_stash_takes.is_empty()
            || !self.stash_puts.is_empty()
            || !self.faction_relations.is_empty()
            || self.relocate_to.is_some()
            || self.unmapped_legacy_plan.is_some()
    }

    fn validate(&self) -> Result<()> {
        validate_source_sha256(&self.source_sha256)?;
        let mut detach = HashSet::new();
        if self.detach_handles.iter().any(|handle| !detach.insert(*handle)) {
            return Err(Error::Refused("draft detach handles must be unique".to_owned()));
        }
        if self
            .durability
            .iter()
            .any(|(handle, value)| !valid_draft_handle(*handle) || *value > 100)
        {
            return Err(Error::Refused(
                "draft durability must use valid handles and percentages in 0..=100".to_owned(),
            ));
        }
        if self
            .placements
            .iter()
            .any(|(handle, placement)| !valid_draft_handle(*handle) || matches!(placement, DraftPlacement::Slot(0)))
        {
            return Err(Error::Refused("draft placement is invalid".to_owned()));
        }
        for (handle, upgrades) in &self.upgrades {
            if !valid_draft_handle(*handle)
                || upgrades
                    .iter()
                    .any(|upgrade| upgrade.is_empty() || upgrade.len() > 256 || upgrade.contains('\0'))
                || upgrades.iter().collect::<HashSet<_>>().len() != upgrades.len()
            {
                return Err(Error::Refused("draft upgrades are invalid or duplicated".to_owned()));
            }
        }
        let mut takes = HashSet::new();
        if self
            .stash_takes
            .iter()
            .any(|handle| *handle == 0 || *handle == u16::MAX || !takes.insert(*handle))
        {
            return Err(Error::Refused(
                "draft stash-take handles must be unique and in 1..=65534".to_owned(),
            ));
        }
        let mut s2_takes = HashSet::new();
        if self
            .s2_stash_takes
            .iter()
            .any(|handle| !valid_draft_handle(*handle) || !s2_takes.insert(*handle))
        {
            return Err(Error::Refused(
                "S2 draft stash-take handles must be unique and valid".to_owned(),
            ));
        }
        let mut puts = HashSet::new();
        for request in &self.stash_puts {
            if request.object_id == 0
                || request.object_id == u16::MAX
                || request.box_id == 0
                || request.box_id == u16::MAX
                || !puts.insert(request.object_id)
            {
                return Err(Error::Refused(
                    "draft stash-put request is invalid or duplicated".to_owned(),
                ));
            }
        }
        if self
            .faction_relations
            .iter()
            .any(|(key, _)| key.trim().is_empty() || key.len() > 256 || key.contains('\0'))
        {
            return Err(Error::Refused("draft faction relation key is invalid".to_owned()));
        }
        if self.relocate_to.is_some_and(|handle| handle == 0 || handle == u16::MAX) {
            return Err(Error::Refused("draft relocation handle is invalid".to_owned()));
        }
        if takes
            .iter()
            .any(|handle| puts.contains(handle) || detach.contains(handle))
            || puts.iter().any(|handle| detach.contains(handle))
        {
            return Err(Error::Refused(
                "draft item is both removed and moved to or from a stash".to_owned(),
            ));
        }
        for addition in &self.adds {
            AddRequest::new(&addition.item_key, addition.quantity, &addition.destination)?;
        }
        Ok(())
    }
}

fn valid_draft_handle(handle: u32) -> bool {
    handle != 0 && handle != u32::MAX
}

/// Bounded edit history with undo, redo, and branch support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftJournal {
    plans: Vec<DraftPlan>,
    index: usize,
}

impl DraftJournal {
    /// Creates a journal whose plans all target the same source save.
    pub fn new(plans: Vec<DraftPlan>, index: usize) -> Result<Self> {
        if plans.is_empty() || plans.iter().any(|plan| plan.validate().is_err()) {
            return Err(Error::Refused("draft journal must contain valid edit plans".to_owned()));
        }
        let source_sha256 = &plans
            .first()
            .ok_or_else(|| Error::Refused("draft journal has no edit plans".to_owned()))?
            .source_sha256;
        if plans.iter().any(|plan| &plan.source_sha256 != source_sha256) {
            return Err(Error::Refused("draft plans must share one source SHA-256".to_owned()));
        }
        if index >= plans.len() {
            return Err(Error::Refused("draft journal index is out of range".to_owned()));
        }
        Ok(Self { plans, index })
    }

    /// All retained snapshots, starting with the untouched save state.
    #[must_use]
    pub fn plans(&self) -> &[DraftPlan] {
        &self.plans
    }

    /// Current position in the undo history.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Current edit plan.
    #[must_use]
    pub fn current(&self) -> Option<&DraftPlan> {
        self.plans.get(self.index)
    }

    /// Whether the current plan can safely be handed to a writer.
    #[must_use]
    pub fn can_apply_current(&self) -> bool {
        self.current().is_some_and(|plan| plan.unmapped_legacy_plan.is_none())
    }

    /// Whether an earlier snapshot is available.
    #[must_use]
    pub const fn can_undo(&self) -> bool {
        self.index > 0
    }

    /// Whether a later snapshot is available.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.index < self.plans.len().saturating_sub(1)
    }

    /// Returns the previous snapshot, or this journal at the first snapshot.
    #[must_use]
    pub fn undo(&self) -> Self {
        if self.can_undo() {
            Self {
                plans: self.plans.clone(),
                index: self.index.checked_sub(1).unwrap_or(self.index),
            }
        } else {
            self.clone()
        }
    }

    /// Returns the next snapshot, or this journal at the last snapshot.
    #[must_use]
    pub fn redo(&self) -> Self {
        if self.can_redo() {
            Self {
                plans: self.plans.clone(),
                index: self.index.checked_add(1).unwrap_or(self.index),
            }
        } else {
            self.clone()
        }
    }

    /// Records a new snapshot, drops any redo tail, and retains the untouched state plus at most 100 edits.
    pub fn record(&self, plan: DraftPlan, discard_unmapped_edits: bool) -> Result<Self> {
        plan.validate()?;
        let current = self
            .current()
            .ok_or_else(|| Error::Refused("draft journal index is out of range".to_owned()))?;
        if current.unmapped_legacy_plan.is_some() && !discard_unmapped_edits {
            return Err(Error::Refused(
                "current draft has legacy edits this build cannot interpret".to_owned(),
            ));
        }
        if plan.source_sha256 != current.source_sha256 {
            return Err(Error::Refused(
                "recorded draft plan has a different source SHA-256".to_owned(),
            ));
        }
        let kept = self.index.min(MAXIMUM_HISTORY_STEPS - 1);
        let first_kept = self
            .index
            .checked_sub(kept)
            .ok_or_else(|| Error::Refused("draft history index underflow".to_owned()))?;
        let capacity = kept
            .checked_add(2)
            .ok_or_else(|| Error::Refused("draft history length overflow".to_owned()))?;
        let first = self
            .plans
            .first()
            .ok_or_else(|| Error::Refused("draft journal has no initial state".to_owned()))?
            .clone();
        let mut plans = Vec::with_capacity(capacity);
        plans.push(first);
        if kept > 0 {
            let previous_start = first_kept
                .checked_add(1)
                .ok_or_else(|| Error::Refused("draft history range overflow".to_owned()))?;
            let previous = self
                .plans
                .get(previous_start..=self.index)
                .ok_or_else(|| Error::Refused("draft history range is invalid".to_owned()))?;
            plans.extend(previous.iter().cloned());
        }
        plans.push(plan);
        let index = plans
            .len()
            .checked_sub(1)
            .ok_or_else(|| Error::Refused("draft history is empty after recording".to_owned()))?;
        Ok(Self { plans, index })
    }
}

/// Local draft store keyed only by the SHA-256 of the source save.
#[derive(Debug, Clone)]
pub struct DraftStore {
    directory: PathBuf,
}

impl DraftStore {
    /// Creates a store at `directory`.
    #[must_use]
    pub fn new(directory: impl AsRef<Path>) -> Self {
        Self {
            directory: directory.as_ref().to_path_buf(),
        }
    }

    /// Returns the path for a lowercase SHA-256 key.
    pub fn path_for(&self, source_sha256: &str) -> Result<PathBuf> {
        validate_source_sha256(source_sha256)?;
        Ok(self.directory.join(format!("{source_sha256}.json")))
    }

    /// Loads schemas 1–5, returning `None` for a missing, invalid, or oversized draft.
    pub fn load(&self, source_sha256: &str) -> Result<Option<DraftJournal>> {
        let path = self.path_for(source_sha256)?;
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Ok(None);
        }
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Ok(None),
        };
        if metadata.len() > MAXIMUM_DRAFT_BYTES as u64 {
            return Ok(None);
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        Ok(parse_journal(&bytes, source_sha256).ok())
    }

    /// Saves a journal atomically; when history exceeds 2 MiB, keeps only the untouched and current states.
    pub fn save(&self, journal: DraftJournal) -> Result<DraftJournal> {
        validate_journal(&journal)?;
        let current = journal
            .current()
            .ok_or_else(|| Error::Refused("draft journal index is out of range".to_owned()))?;
        let source_sha256 = current.source_sha256.clone();
        let path = self.path_for(&source_sha256)?;
        if !current.has_changes() {
            match fs::remove_file(&path) {
                Ok(()) => sync_directory(&self.directory),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            return Ok(journal);
        }
        let mut persisted = journal;
        let mut bytes = serialize_journal(&persisted)?;
        if bytes.len() > MAXIMUM_DRAFT_BYTES {
            let untouched = DraftPlan::empty(&source_sha256)?;
            let current = persisted
                .current()
                .ok_or_else(|| Error::Refused("draft journal index is out of range".to_owned()))?
                .clone();
            persisted = DraftJournal::new(vec![untouched, current], 1)?;
            bytes = serialize_journal(&persisted)?;
            if bytes.len() > MAXIMUM_DRAFT_BYTES {
                return Err(Error::Refused(
                    "current draft exceeds the 2 MiB storage limit".to_owned(),
                ));
            }
        }
        write_durable(&self.directory, &path, &bytes)?;
        Ok(persisted)
    }

    /// Moves a stored draft aside so unsupported content can be recovered later.
    pub fn set_aside(&self, source_sha256: &str) -> Result<Option<PathBuf>> {
        let path = self.path_for(source_sha256)?;
        if !path.exists() {
            return Ok(None);
        }
        let id = NEXT_DRAFT_ID.fetch_add(1, Ordering::Relaxed);
        let kept = path.with_extension(format!("json.unsupported-{}-{id}", std::process::id()));
        fs::rename(&path, &kept)?;
        sync_directory(&self.directory);
        Ok(Some(kept))
    }
}

fn validate_source_sha256(source_sha256: &str) -> Result<()> {
    if source_sha256.len() != 64
        || !source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Refused("draft key must be a lowercase SHA-256".to_owned()));
    }
    Ok(())
}

fn validate_journal(journal: &DraftJournal) -> Result<()> {
    if journal.plans.is_empty() || journal.index >= journal.plans.len() {
        return Err(Error::Refused("draft journal index is out of range".to_owned()));
    }
    let source_sha256 = &journal
        .plans
        .first()
        .ok_or_else(|| Error::Refused("draft journal has no edit plans".to_owned()))?
        .source_sha256;
    for plan in &journal.plans {
        plan.validate()?;
        if &plan.source_sha256 != source_sha256 {
            return Err(Error::Refused("draft plans must share one source SHA-256".to_owned()));
        }
    }
    Ok(())
}

fn parse_journal(bytes: &[u8], expected_sha256: &str) -> Result<DraftJournal> {
    let value = parse_json(bytes)?;
    let root = object_members(&value, &["index", "plans", "schema", "source_sha256"])?;
    if string_field(root, "source_sha256")? != expected_sha256 {
        return Err(Error::Refused("draft belongs to a different source save".to_owned()));
    }
    let schema = number_u32(field(root, "schema")?)?;
    let plans = array_field(root, "plans")?;
    let index = usize::try_from(number_u32(field(root, "index")?)?)
        .map_err(|_| Error::damaged("draft index is outside the platform range"))?;
    let parsed = match schema {
        1 => plans
            .iter()
            .map(|plan| parse_legacy_plan(plan, expected_sha256))
            .collect::<Result<Vec<_>>>()?,
        2 => plans
            .iter()
            .map(|plan| parse_current_plan(plan, expected_sha256, false, false, false))
            .collect::<Result<Vec<_>>>()?,
        3 => plans
            .iter()
            .map(|plan| {
                let includes_s2_stash_takes = object_entries(plan)?.iter().any(|(name, _)| name == "s2StashTakes");
                parse_current_plan(plan, expected_sha256, true, includes_s2_stash_takes, false)
            })
            .collect::<Result<Vec<_>>>()?,
        4 => plans
            .iter()
            .map(|plan| parse_current_plan(plan, expected_sha256, true, true, false))
            .collect::<Result<Vec<_>>>()?,
        5 => plans
            .iter()
            .map(|plan| parse_current_plan(plan, expected_sha256, true, true, true))
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(Error::Refused("unsupported draft schema".to_owned())),
    };
    let journal = DraftJournal::new(parsed, index)?;
    if !journal.current().is_some_and(DraftPlan::has_changes) {
        return Err(Error::Refused("draft has no current edits".to_owned()));
    }
    Ok(journal)
}

fn parse_current_plan(
    value: &JsonValue,
    expected_sha256: &str,
    extended: bool,
    s2_stashes: bool,
    operations: bool,
) -> Result<DraftPlan> {
    let base_fields = [
        "sourceSha256",
        "money",
        "stackCounts",
        "detachHandles",
        "adds",
        "stashTakes",
        "stashPuts",
        "unmappedLegacyPlan",
    ];
    let extended_fields = [
        "sourceSha256",
        "money",
        "stackCounts",
        "detachHandles",
        "adds",
        "stashTakes",
        "stashPuts",
        "durability",
        "placements",
        "upgrades",
        "unmappedLegacyPlan",
    ];
    let full_fields = [
        "sourceSha256",
        "money",
        "stackCounts",
        "detachHandles",
        "adds",
        "stashTakes",
        "s2StashTakes",
        "stashPuts",
        "durability",
        "placements",
        "upgrades",
        "unmappedLegacyPlan",
    ];
    let operation_fields = [
        "sourceSha256",
        "money",
        "stackCounts",
        "detachHandles",
        "adds",
        "stashTakes",
        "s2StashTakes",
        "stashPuts",
        "durability",
        "placements",
        "upgrades",
        "factionRelations",
        "relocateTo",
        "unmappedLegacyPlan",
    ];
    let members = if operations {
        object_members(value, &operation_fields)?
    } else if s2_stashes {
        object_members(value, &full_fields)?
    } else if extended {
        object_members(value, &extended_fields)?
    } else {
        object_members(value, &base_fields)?
    };
    if string_field(members, "sourceSha256")? != expected_sha256 {
        return Err(Error::Refused(
            "draft plan belongs to a different source save".to_owned(),
        ));
    }
    let mut plan = DraftPlan::empty(expected_sha256)?;
    plan.money = nullable_u32(field(members, "money")?)?;
    for (key, value) in object_entries(field(members, "stackCounts")?)? {
        let handle = key
            .parse::<u32>()
            .map_err(|_| Error::damaged("draft stack handle is not an unsigned 32-bit integer"))?;
        let count = number_u32(value)?;
        if plan.stack_counts.insert(handle, count).is_some() {
            return Err(Error::damaged("draft repeats a stack handle"));
        }
    }
    if extended {
        let value = field(members, "durability")?;
        for (key, value) in object_entries(value)? {
            let handle = draft_handle_key(key)?;
            let durability = number_u8(value)?;
            if plan.durability.insert(handle, durability).is_some() {
                return Err(Error::damaged("draft repeats a durability handle"));
            }
        }
        let value = field(members, "placements")?;
        for (key, value) in object_entries(value)? {
            let handle = draft_handle_key(key)?;
            let placement = match string_value(value)? {
                "ruck" => DraftPlacement::Ruck,
                "belt" => DraftPlacement::Belt,
                encoded if encoded.starts_with("slot:") => {
                    let slot = encoded
                        .get(5..)
                        .and_then(|value| value.parse::<u8>().ok())
                        .filter(|slot| *slot > 0)
                        .ok_or_else(|| Error::damaged("draft slot placement is invalid"))?;
                    DraftPlacement::Slot(slot)
                }
                _ => return Err(Error::damaged("draft placement is unknown")),
            };
            if plan.placements.insert(handle, placement).is_some() {
                return Err(Error::damaged("draft repeats a placement handle"));
            }
        }
        let value = field(members, "upgrades")?;
        for (key, value) in object_entries(value)? {
            let handle = draft_handle_key(key)?;
            let upgrades = array_value(value)?
                .iter()
                .map(|value| string_value(value).map(str::to_owned))
                .collect::<Result<Vec<_>>>()?;
            if plan.upgrades.insert(handle, upgrades).is_some() {
                return Err(Error::damaged("draft repeats an upgrades handle"));
            }
        }
    }
    plan.detach_handles = array_field(members, "detachHandles")?
        .iter()
        .map(number_u16)
        .collect::<Result<Vec<_>>>()?;
    plan.adds = array_field(members, "adds")?
        .iter()
        .map(parse_current_add)
        .collect::<Result<Vec<_>>>()?;
    plan.stash_takes = array_field(members, "stashTakes")?
        .iter()
        .map(number_u16)
        .collect::<Result<Vec<_>>>()?;
    if s2_stashes {
        plan.s2_stash_takes = array_field(members, "s2StashTakes")?
            .iter()
            .map(number_u32)
            .collect::<Result<Vec<_>>>()?;
    }
    plan.stash_puts = array_field(members, "stashPuts")?
        .iter()
        .map(parse_stash_put)
        .collect::<Result<Vec<_>>>()?;
    if operations {
        for (key, value) in object_entries(field(members, "factionRelations")?)? {
            let relation = number_i32(value)?;
            if plan.faction_relations.insert(key.to_owned(), relation).is_some() {
                return Err(Error::damaged("draft repeats a faction relation key"));
            }
        }
        plan.relocate_to = nullable_u16(field(members, "relocateTo")?)?;
    }
    plan.unmapped_legacy_plan = match field(members, "unmappedLegacyPlan")? {
        JsonValue::Null => None,
        value => Some(value.clone()),
    };
    plan.validate()?;
    Ok(plan)
}

fn parse_current_add(value: &JsonValue) -> Result<AddRequest> {
    let members = object_members(value, &["itemKey", "quantity", "destination"])?;
    AddRequest::new(
        string_field(members, "itemKey")?,
        number_u32(field(members, "quantity")?)?,
        string_field(members, "destination")?,
    )
}

fn parse_stash_put(value: &JsonValue) -> Result<StashPut> {
    let members = object_members(value, &["objectId", "boxId"])?;
    StashPut::new(
        number_u16(field(members, "objectId")?)?,
        number_u16(field(members, "boxId")?)?,
    )
}

fn parse_legacy_plan(value: &JsonValue, source_sha256: &str) -> Result<DraftPlan> {
    let members = object_entries(value)?;
    let known: HashSet<&str> = LEGACY_PLAN_FIELDS.iter().copied().collect();
    let mut unsupported = members.iter().any(|(key, _)| !known.contains(key.as_str()));
    let mut plan = DraftPlan::empty(source_sha256)?;
    if let Some(money) = optional_field(members, "money") {
        plan.money = nullable_u32(money)?;
    }
    if let Some(stacks) = optional_field(members, "stacks") {
        for row in array_value(stacks)? {
            let tuple = tuple(row, 2)?;
            let handle = number_u32(tuple.first().ok_or_else(|| Error::damaged("stack handle is missing"))?)?;
            let count = number_u32(tuple.get(1).ok_or_else(|| Error::damaged("stack count is missing"))?)?;
            if plan.stack_counts.insert(handle, count).is_some() {
                return Err(Error::damaged("legacy draft repeats a stack handle"));
            }
        }
    }
    if let Some(detach) = optional_field(members, "detach") {
        for row in array_value(detach)? {
            let tuple = tuple(row, 2)?;
            let handle = number_u32(
                tuple
                    .first()
                    .ok_or_else(|| Error::damaged("detach handle is missing"))?,
            )?;
            if handle == 0 || handle == u32::from(u16::MAX) || handle > u32::from(u16::MAX) {
                return Err(Error::damaged("legacy draft detach handle is out of range"));
            }
            let deep = bool_value(tuple.get(1).ok_or_else(|| Error::damaged("detach flag is missing"))?)?;
            if deep {
                unsupported = true;
            } else {
                plan.detach_handles.push(
                    u16::try_from(handle).map_err(|_| Error::damaged("legacy draft detach handle exceeds 16 bits"))?,
                );
            }
        }
    }
    if let Some(adds) = optional_field(members, "adds") {
        for row in array_value(adds)? {
            let tuple = tuple(row, 3)?;
            plan.adds.push(AddRequest::new(
                string_value(tuple.first().ok_or_else(|| Error::damaged("add item key is missing"))?)?,
                number_u32(tuple.get(1).ok_or_else(|| Error::damaged("add quantity is missing"))?)?,
                string_value(
                    tuple
                        .get(2)
                        .ok_or_else(|| Error::damaged("add destination is missing"))?,
                )?,
            )?);
        }
    }
    for name in [
        "attach",
        "durability",
        "faction_relations",
        "moves",
        "placements",
        "raw",
        "upgrades",
    ] {
        if let Some(value) = optional_field(members, name) {
            if !array_value(value)?.is_empty() {
                unsupported = true;
            }
        }
    }
    if let Some(value) = optional_field(members, "player_faction") {
        match value {
            JsonValue::Null => {}
            JsonValue::String(_) => unsupported = true,
            _ => return Err(Error::damaged("legacy player faction must be text or null")),
        }
    }
    plan.validate()?;
    if unsupported {
        plan.unmapped_legacy_plan = Some(value.clone());
    }
    Ok(plan)
}

fn serialize_journal(journal: &DraftJournal) -> Result<Vec<u8>> {
    validate_journal(journal)?;
    let operations = journal
        .plans
        .iter()
        .any(|plan| !plan.faction_relations.is_empty() || plan.relocate_to.is_some());
    let extended = operations
        || journal.plans.iter().any(|plan| {
            !plan.durability.is_empty()
                || !plan.placements.is_empty()
                || !plan.upgrades.is_empty()
                || !plan.s2_stash_takes.is_empty()
        });
    let schema = if operations {
        5
    } else if extended {
        3
    } else {
        2
    };
    let mut writer = Writer::compact();
    writer.object_start()?;
    writer.key("index")?;
    writer.u64(journal.index as u64)?;
    writer.key("plans")?;
    writer.array_start()?;
    for plan in &journal.plans {
        write_current_plan(&mut writer, plan, extended, operations)?;
    }
    writer.array_end()?;
    writer.key("schema")?;
    writer.u64(schema)?;
    writer.key("source_sha256")?;
    writer.string(
        &journal
            .current()
            .ok_or_else(|| Error::Refused("draft journal index is out of range".to_owned()))?
            .source_sha256,
    )?;
    writer.object_end()?;
    writer.finish()
}

fn write_current_plan(writer: &mut Writer, plan: &DraftPlan, extended: bool, operations: bool) -> Result<()> {
    writer.object_start()?;
    writer.key("sourceSha256")?;
    writer.string(&plan.source_sha256)?;
    writer.key("money")?;
    if let Some(money) = plan.money {
        writer.u64(u64::from(money))?;
    } else {
        writer.null()?;
    }
    writer.key("stackCounts")?;
    writer.object_start()?;
    for (handle, count) in &plan.stack_counts {
        writer.key(&handle.to_string())?;
        writer.u64(u64::from(*count))?;
    }
    writer.object_end()?;
    writer.key("detachHandles")?;
    writer.array_start()?;
    for handle in &plan.detach_handles {
        writer.u64(u64::from(*handle))?;
    }
    writer.array_end()?;
    writer.key("adds")?;
    writer.array_start()?;
    for addition in &plan.adds {
        writer.object_start()?;
        writer.key("itemKey")?;
        writer.string(&addition.item_key)?;
        writer.key("quantity")?;
        writer.u64(u64::from(addition.quantity))?;
        writer.key("destination")?;
        writer.string(&addition.destination)?;
        writer.object_end()?;
    }
    writer.array_end()?;
    writer.key("stashTakes")?;
    writer.array_start()?;
    for handle in &plan.stash_takes {
        writer.u64(u64::from(*handle))?;
    }
    writer.array_end()?;
    if extended {
        writer.key("s2StashTakes")?;
        writer.array_start()?;
        for handle in &plan.s2_stash_takes {
            writer.u64(u64::from(*handle))?;
        }
        writer.array_end()?;
    }
    writer.key("stashPuts")?;
    writer.array_start()?;
    for transfer in &plan.stash_puts {
        writer.object_start()?;
        writer.key("objectId")?;
        writer.u64(u64::from(transfer.object_id))?;
        writer.key("boxId")?;
        writer.u64(u64::from(transfer.box_id))?;
        writer.object_end()?;
    }
    writer.array_end()?;
    if operations {
        writer.key("factionRelations")?;
        writer.object_start()?;
        for (key, relation) in &plan.faction_relations {
            writer.key(key)?;
            writer.i64(i64::from(*relation))?;
        }
        writer.object_end()?;
        writer.key("relocateTo")?;
        if let Some(handle) = plan.relocate_to {
            writer.u64(u64::from(handle))?;
        } else {
            writer.null()?;
        }
    }
    if extended {
        writer.key("durability")?;
        writer.object_start()?;
        for (handle, durability) in &plan.durability {
            writer.key(&handle.to_string())?;
            writer.u64(u64::from(*durability))?;
        }
        writer.object_end()?;
        writer.key("placements")?;
        writer.object_start()?;
        for (handle, placement) in &plan.placements {
            writer.key(&handle.to_string())?;
            let encoded = match placement {
                DraftPlacement::Ruck => "ruck".to_owned(),
                DraftPlacement::Belt => "belt".to_owned(),
                DraftPlacement::Slot(slot) => format!("slot:{slot}"),
            };
            writer.string(&encoded)?;
        }
        writer.object_end()?;
        writer.key("upgrades")?;
        writer.object_start()?;
        for (handle, upgrades) in &plan.upgrades {
            writer.key(&handle.to_string())?;
            writer.array_start()?;
            for upgrade in upgrades {
                writer.string(upgrade)?;
            }
            writer.array_end()?;
        }
        writer.object_end()?;
    }
    writer.key("unmappedLegacyPlan")?;
    if let Some(unmapped) = &plan.unmapped_legacy_plan {
        write_json_value(writer, unmapped)?;
    } else {
        writer.null()?;
    }
    writer.object_end()
}

fn write_json_value(writer: &mut Writer, value: &JsonValue) -> Result<()> {
    match value {
        JsonValue::Object(members) => {
            writer.object_start()?;
            for (key, value) in members {
                writer.key(key)?;
                write_json_value(writer, value)?;
            }
            writer.object_end()
        }
        JsonValue::Array(values) => {
            writer.array_start()?;
            for value in values {
                write_json_value(writer, value)?;
            }
            writer.array_end()
        }
        JsonValue::String(value) => writer.string(value),
        JsonValue::Number(value) => writer.number(value),
        JsonValue::Bool(value) => writer.bool(*value),
        JsonValue::Null => writer.null(),
    }
}

fn parse_json(bytes: &[u8]) -> Result<JsonValue> {
    let mut reader = Reader::new(bytes);
    let first = reader
        .next_event()?
        .ok_or_else(|| Error::damaged("draft JSON is empty"))?;
    let value = parse_json_event(&mut reader, first)?;
    if reader.next_event()?.is_some() {
        return Err(Error::damaged("draft JSON has trailing events"));
    }
    Ok(value)
}

fn parse_json_event(reader: &mut Reader<'_>, event: Event<'_>) -> Result<JsonValue> {
    match event {
        Event::ObjectStart => {
            let mut members = Vec::new();
            let mut seen = HashSet::new();
            loop {
                match reader
                    .next_event()?
                    .ok_or_else(|| Error::damaged("draft JSON object is truncated"))?
                {
                    Event::ObjectEnd => return Ok(JsonValue::Object(members)),
                    Event::Key(key) => {
                        let key = key.into_owned();
                        if !seen.insert(key.clone()) {
                            return Err(Error::damaged("draft JSON object repeats a key"));
                        }
                        let value = reader
                            .next_event()?
                            .ok_or_else(|| Error::damaged("draft JSON member has no value"))?;
                        members.push((key, parse_json_event(reader, value)?));
                    }
                    _ => return Err(Error::damaged("draft JSON object has an invalid member")),
                }
            }
        }
        Event::ArrayStart => {
            let mut values = Vec::new();
            loop {
                let event = reader
                    .next_event()?
                    .ok_or_else(|| Error::damaged("draft JSON array is truncated"))?;
                if event == Event::ArrayEnd {
                    return Ok(JsonValue::Array(values));
                }
                values.push(parse_json_event(reader, event)?);
            }
        }
        Event::String(value) => Ok(JsonValue::String(value.into_owned())),
        Event::Number(value) => Ok(JsonValue::Number(value.to_owned())),
        Event::Bool(value) => Ok(JsonValue::Bool(value)),
        Event::Null => Ok(JsonValue::Null),
        Event::Key(_) | Event::ObjectEnd | Event::ArrayEnd => Err(Error::damaged("draft JSON has an invalid value")),
    }
}

fn object_members<'a>(value: &'a JsonValue, expected: &[&str]) -> Result<&'a [(String, JsonValue)]> {
    let members = object_entries(value)?;
    if members.len() != expected.len() || members.iter().any(|(key, _)| !expected.contains(&key.as_str())) {
        return Err(Error::damaged("draft object has unknown or missing fields"));
    }
    Ok(members)
}

fn draft_handle_key(key: &str) -> Result<u32> {
    let handle = key
        .parse::<u32>()
        .map_err(|_| Error::damaged("draft item handle is not an unsigned 32-bit integer"))?;
    if !valid_draft_handle(handle) {
        return Err(Error::damaged("draft item handle is outside the supported range"));
    }
    Ok(handle)
}

fn object_entries(value: &JsonValue) -> Result<&[(String, JsonValue)]> {
    match value {
        JsonValue::Object(members) => Ok(members),
        _ => Err(Error::damaged("draft value must be a JSON object")),
    }
}

fn array_field<'a>(members: &'a [(String, JsonValue)], name: &str) -> Result<&'a [JsonValue]> {
    array_value(field(members, name)?)
}

fn array_value(value: &JsonValue) -> Result<&[JsonValue]> {
    match value {
        JsonValue::Array(values) => Ok(values),
        _ => Err(Error::damaged("draft field must be an array")),
    }
}

fn field<'a>(members: &'a [(String, JsonValue)], name: &str) -> Result<&'a JsonValue> {
    optional_field(members, name).ok_or_else(|| Error::damaged(format!("draft field {name} is missing")))
}

fn optional_field<'a>(members: &'a [(String, JsonValue)], name: &str) -> Option<&'a JsonValue> {
    members.iter().find(|(key, _)| key == name).map(|(_, value)| value)
}

fn string_field<'a>(members: &'a [(String, JsonValue)], name: &str) -> Result<&'a str> {
    string_value(field(members, name)?)
}

fn string_value(value: &JsonValue) -> Result<&str> {
    match value {
        JsonValue::String(value) => Ok(value),
        _ => Err(Error::damaged("draft field must be text")),
    }
}

fn bool_value(value: &JsonValue) -> Result<bool> {
    match value {
        JsonValue::Bool(value) => Ok(*value),
        _ => Err(Error::damaged("draft field must be Boolean")),
    }
}

fn nullable_u32(value: &JsonValue) -> Result<Option<u32>> {
    if value == &JsonValue::Null {
        Ok(None)
    } else {
        number_u32(value).map(Some)
    }
}

fn nullable_u16(value: &JsonValue) -> Result<Option<u16>> {
    if value == &JsonValue::Null {
        Ok(None)
    } else {
        number_u16(value).map(Some)
    }
}

fn number_u32(value: &JsonValue) -> Result<u32> {
    number_string(value)?
        .parse::<u32>()
        .map_err(|_| Error::damaged("draft number is not an unsigned 32-bit integer"))
}

fn number_u16(value: &JsonValue) -> Result<u16> {
    number_string(value)?
        .parse::<u16>()
        .map_err(|_| Error::damaged("draft number is not an unsigned 16-bit integer"))
}

fn number_u8(value: &JsonValue) -> Result<u8> {
    number_string(value)?
        .parse::<u8>()
        .map_err(|_| Error::damaged("draft number is not an unsigned 8-bit integer"))
}

fn number_i32(value: &JsonValue) -> Result<i32> {
    number_string(value)?
        .parse::<i32>()
        .map_err(|_| Error::damaged("draft number is not a signed 32-bit integer"))
}

fn number_string(value: &JsonValue) -> Result<&str> {
    match value {
        JsonValue::Number(value) => Ok(value),
        _ => Err(Error::damaged("draft field must be a JSON number")),
    }
}

fn tuple(value: &JsonValue, length: usize) -> Result<&[JsonValue]> {
    let values = array_value(value)?;
    if values.len() != length {
        return Err(Error::damaged("legacy draft row has an invalid tuple shape"));
    }
    Ok(values)
}

fn write_durable(directory: &Path, destination: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(directory)?;
    let id = NEXT_DRAFT_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(".draft-{}-{id}.tmp", std::process::id()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(&temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if fs::symlink_metadata(destination).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(Error::Refused("draft path is a symbolic link".to_owned()));
        }
        fs::rename(&temporary, destination)?;
        sync_directory(directory);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn sync_directory(directory: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = File::open(directory) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = directory;
}

impl From<Text<'_>> for JsonValue {
    fn from(value: Text<'_>) -> Self {
        Self::String(value.into_owned())
    }
}
