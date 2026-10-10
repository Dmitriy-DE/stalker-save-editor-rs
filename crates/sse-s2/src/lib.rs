//! S.T.A.L.K.E.R. 2 containers and read-only save indexes.

use sse_core::fields::{read_u16, read_u32};
use sse_core::ranges::{verify_unchanged_outside_ranges, ChangedRange};
use sse_core::{Error, Result, SaveBuffer};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Maximum decoded S2 image admitted by the bounded Rust reader.
pub const MAXIMUM_UNPACKED_SIZE: usize = sse_core::limits::MAXIMUM_UNPACKED_BYTES;
const MAXIMUM_MONEY: u32 = 2_000_000_000;
const MAXIMUM_EDITABLE_STACK_COUNT: u32 = 1_000_000;
const MAXIMUM_OWNED_HANDLES: usize = 4096;
const MAXIMUM_GRID_CELLS: usize = 8192;
// Consumers only need to distinguish one unambiguous record from an ambiguous handle.
const MAXIMUM_OBJECT_CANDIDATES_PER_HANDLE: usize = 2;
const KRAKEN_BLOCK_SIZE: usize = 0x4_0000;
const GRID_WIDTH: u16 = 8;
const WALLET_ANCHOR: [u8; 32] = [
    0x00, 0x38, 0x01, 0x00, 0x00, 0x00, 0x01, 0x10, 0xca, 0xcf, 0xa8, 0x48, 0xc8, 0x95, 0x21, 0x49, 0xb5, 0x1b, 0x94,
    0x44, 0x00, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00,
];
const LEGACY_CONTAINER_ID: [u8; 12] = [0xca, 0xcf, 0xa8, 0x48, 0xc8, 0x95, 0x21, 0x49, 0xb5, 0x1b, 0x94, 0x44];
const STASH_MARKER: [u8; 10] = [0xff, 0xff, 0xff, 0xff, 0x06, 0x01, 0x00, 0x00, 0x00, 0x06];
const STASH_HEADER_TAIL: [u8; 4] = [0x03, 0x00, 0x00, 0x00];

/// Change supported by the compiled S2 writer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum S2Change {
    /// Replace the player wallet balance.
    SetMoney {
        /// The wallet balance the edit was prepared against; the write is refused if the save no longer has it.
        old_value: u32,
        /// The requested wallet balance.
        new_value: u32,
    },
    /// Replace a validated stack count and scale its total weight.
    SetStackCount {
        /// Unique object handle.
        handle: u32,
        /// New positive stack count.
        count: u32,
    },
    /// Replace a validated equipped-item condition in the inclusive 0..=1 range.
    SetDurability {
        /// Unique equipped-item handle.
        handle: u32,
        /// New durability fraction from zero through one.
        condition: f32,
    },
    /// Move an item from the stash into the first fitting backpack cells.
    MoveStashToBackpack {
        /// Unique item handle owned by the stash.
        handle: u32,
    },
}

/// One S2 container's unpacked image and integrity metadata.
pub struct S2Container {
    image: SaveBuffer,
    packed_size: usize,
    stored_crc32: u32,
    computed_crc32: u32,
}

impl S2Container {
    /// Validates the trailer CRC and decodes the bounded Kraken image.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 8 {
            return Err(Error::damaged("S2 container is shorter than its size and CRC fields"));
        }
        let trailer_offset = data
            .len()
            .checked_sub(4)
            .ok_or_else(|| Error::damaged("S2 container is shorter than its CRC trailer"))?;
        let stored_crc32 = read_u32(data, trailer_offset)?;
        let crc_body = data
            .get(..trailer_offset)
            .ok_or_else(|| Error::damaged("S2 CRC range is out of bounds"))?;
        let computed_crc32 = sse_codecs::crc32::crc32(crc_body);
        if stored_crc32 != computed_crc32 {
            return Err(Error::damaged("S2 container CRC-32 mismatch"));
        }

        let unpacked_u32 = read_u32(data, 0)?;
        let unpacked_size =
            usize::try_from(unpacked_u32).map_err(|_| Error::damaged("S2 unpacked size does not fit this platform"))?;
        if unpacked_size == 0 || unpacked_size > MAXIMUM_UNPACKED_SIZE {
            return Err(Error::damaged("S2 unpacked size is outside the supported limit"));
        }
        let stream = data
            .get(4..trailer_offset)
            .ok_or_else(|| Error::damaged("S2 Kraken stream range is out of bounds"))?;
        sse_codecs::validate_declared_output_size(stream.len(), unpacked_size, "S2 Kraken")?;
        let mut image = vec![0_u8; unpacked_size];
        sse_codecs::kraken::decompress_into(stream, &mut image)?;
        Ok(Self {
            image: SaveBuffer::from_vec(image),
            packed_size: data.len(),
            stored_crc32,
            computed_crc32,
        })
    }

    /// Unpacked image bytes.
    #[must_use]
    pub fn image(&self) -> &[u8] {
        self.image.as_slice()
    }

    /// Packed file size.
    #[must_use]
    pub const fn packed_size(&self) -> usize {
        self.packed_size
    }

    /// Stored CRC-32 trailer.
    #[must_use]
    pub const fn stored_crc32(&self) -> u32 {
        self.stored_crc32
    }

    /// Computed CRC-32 over all packed bytes except the trailer.
    #[must_use]
    pub const fn computed_crc32(&self) -> u32 {
        self.computed_crc32
    }
}

/// Read-only indexed S2 save.
pub struct S2Save {
    container: S2Container,
    index: S2InventoryIndex,
    objects: S2ObjectIndex,
    names: Option<S2NameTables>,
    unresolved_handles: Vec<u32>,
    warnings: Vec<String>,
}

impl S2Save {
    /// Reads a packed save and builds its bounded inventory index.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let container = S2Container::from_bytes(data)?;
        Self::from_container(container)
    }

    fn from_container(container: S2Container) -> Result<Self> {
        let index = S2InventoryIndex::locate(container.image())?;
        let mut referenced_handles = index
            .owned_handles
            .iter()
            .copied()
            .chain(index.grid_cells.iter().map(|cell| cell.handle))
            .collect::<HashSet<_>>();
        referenced_handles.remove(&u32::MAX);
        if !index.is_legacy {
            if let Ok(stash) = S2StashLayout::locate(container.image(), &index) {
                referenced_handles.extend(stash.live_handles().iter().copied());
            }
        }
        let objects = S2ObjectIndex::build(container.image(), index.is_legacy, &referenced_handles)?;
        let names = S2NameTables::locate(container.image(), &objects, index.is_legacy)?;
        let (unresolved_handles, warnings) = collect_inventory_warnings(container.image(), &index, &objects)?;
        Ok(Self {
            container,
            index,
            objects,
            names,
            unresolved_handles,
            warnings,
        })
    }

    /// Container metadata and unpacked save image.
    #[must_use]
    pub const fn container(&self) -> &S2Container {
        &self.container
    }

    /// Inventory index with offsets into the unpacked image.
    #[must_use]
    pub const fn index(&self) -> &S2InventoryIndex {
        &self.index
    }

    /// Wallet balance.
    ///
    /// # Errors
    /// Returns `Error::Damaged` when the wallet field cannot be read from the image.
    pub fn money(&self) -> Result<u32> {
        read_u32(self.container.image(), self.index.money_offset)
    }

    /// Read-only inventory item views assembled from the image and its index.
    #[must_use]
    pub fn items(&self) -> Vec<S2InventoryItem> {
        build_inventory_items(self.container.image(), &self.index, &self.objects, self.names.as_ref())
    }

    /// Owned records that do not map to a grid or confirmed equipment record.
    #[must_use]
    pub fn orphans(&self) -> Vec<S2OrphanItem> {
        build_orphans(self.container.image(), &self.index, &self.objects)
    }

    /// Unresolved handles from grid references, record scans, and unknown kinds.
    #[must_use]
    pub fn unresolved_handles(&self) -> &[u32] {
        &self.unresolved_handles
    }

    /// Warnings produced while validating the inventory index.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Save-local display name tables, when a valid table was found.
    #[must_use]
    pub const fn name_tables(&self) -> Option<&S2NameTables> {
        self.names.as_ref()
    }

    /// Reads the unique stash block near the save's player inventory.
    pub fn stash(&self) -> Result<S2StashLayout> {
        S2StashLayout::locate(self.container.image(), &self.index)
    }

    /// Reads item records referenced by the validated save-resident stash.
    ///
    /// # Errors
    /// Returns an error when the stash or any live stash item cannot be uniquely indexed.
    pub fn stash_items(&self) -> Result<Vec<S2StashItem>> {
        let stash = self.stash()?;
        let mut items = Vec::with_capacity(stash.live_handles().len());
        for handle in stash.live_handles() {
            let record = self
                .objects
                .unique(*handle)
                .ok_or_else(|| Error::Refused(format!("S2 stash item 0x{handle:08X} is missing or ambiguous")))?;
            let mut cells = stash
                .grid_cells()
                .iter()
                .filter(|cell| cell.handle == *handle)
                .copied()
                .collect::<Vec<_>>();
            if cells.is_empty() {
                return Err(Error::damaged(format!(
                    "S2 stash item 0x{handle:08X} has no grid cells"
                )));
            }
            cells.sort_by_key(|cell| (cell.y, cell.x));
            let x = cells.iter().map(|cell| cell.x).min().unwrap_or_default();
            let y = cells.iter().map(|cell| cell.y).min().unwrap_or_default();
            let max_x = cells.iter().map(|cell| cell.x).max().unwrap_or_default();
            let max_y = cells.iter().map(|cell| cell.y).max().unwrap_or_default();
            items.push(S2StashItem {
                handle: *handle,
                x,
                y,
                width: max_x.saturating_sub(x).saturating_add(1),
                height: max_y.saturating_sub(y).saturating_add(1),
                cells,
                count: record.count,
                total_weight: record.total_weight,
                kind_code: record.kind_code,
                type_key: record.type_key,
                display_name: self
                    .names
                    .as_ref()
                    .and_then(|names| names.resolve(&record.type_key))
                    .map(str::to_owned),
            });
        }
        Ok(items)
    }

    /// Applies supported changes and encodes a packed S2 save with a CRC-checked read-back.
    ///
    /// # Errors
    /// Reports invalid, ambiguous, or unsupported edit requests without modifying this parsed save.
    pub fn write_changes(&self, changes: &[S2Change]) -> Result<Vec<u8>> {
        let durability_changes = changes
            .iter()
            .filter_map(|change| match change {
                S2Change::SetDurability { handle, condition } => Some((*handle, *condition)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let source_items = self.items();
        let (image, changed_ranges) = apply_changes_with_items(self, &source_items, changes)?;
        let (packed, verified_container) = pack_and_verify_s2_image(self.container.image(), &image, &changed_ranges)?;
        let verified = Self::from_container(verified_container)?;
        let verified_items = verified.items();
        verify_changes_readback_with_items(self, &source_items, &verified, &verified_items, changes)?;
        if !durability_changes.is_empty() {
            verify_durability_values(
                &condition_pairs(&source_items),
                &condition_pairs(&verified_items),
                &durability_changes,
            )?;
        }
        Ok(packed)
    }
}

/// Why a stash item was left in the stash by [`transfer_stash_items_to_backpack`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S2SkipReason {
    /// The backpack has no free place for the item's footprint.
    NoRoom,
    /// The item's kind or layout is not one the editor can move.
    Unsupported,
    /// Any other refusal of the single-item write.
    Other,
}

/// One item the writer refused before accepting any bytes; the previous image stays in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S2RefusedMove {
    /// Handle of the refused item.
    pub handle: u32,
    /// Category used in the summary.
    pub reason: S2SkipReason,
    /// Writer message.
    pub message: String,
}

/// Outcome of moving several stash items, one verified write at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S2StashTransfer {
    /// Packed save after the last accepted write; `None` when nothing was written.
    pub packed: Option<Vec<u8>>,
    /// Handles moved into the backpack, in request order.
    pub moved: Vec<u32>,
    /// Handles skipped because the record is not marked as stash-owned.
    pub skipped: Vec<u32>,
    /// Handles the writer refused; the transfer went on with the next item.
    pub refused: Vec<S2RefusedMove>,
    /// Error that stopped the transfer. Only an accepted result that cannot be read back stops it.
    pub stopped: Option<String>,
}

impl S2StashTransfer {
    /// Number of refused items in one category.
    #[must_use]
    pub fn refused_count(&self, reason: S2SkipReason) -> usize {
        self.refused.iter().filter(|refused| refused.reason == reason).count()
    }
}

fn classify_refusal(message: &str) -> S2SkipReason {
    if message.contains("does not fit a backpack")
        || message.contains("no fitting free cells")
        || message.contains("grid-cell limit")
    {
        S2SkipReason::NoRoom
    } else if message.contains("kind is not confirmed")
        || message.contains("footprint is missing")
        || message.contains("unresolved backpack cells")
    {
        S2SkipReason::Unsupported
    } else {
        S2SkipReason::Other
    }
}

/// Applies `changes` (if any), then moves each requested stash item with its own write and re-read.
///
/// An item that is not in use or not marked stash-owned (record byte 28, bit `0x08`) is skipped.
/// A refusal of a single-item write leaves the image as it was, so that item is recorded in
/// `refused` and the transfer goes on. Only an accepted result that cannot be parsed again stops
/// the transfer; the writes before it stay in `packed`, and the error is in `stopped`.
///
/// # Errors
/// Returns an error when `changes` cannot be written, or when the inventory is not fully resolved or is 1.0.x.
pub fn transfer_stash_items_to_backpack(
    save: &S2Save,
    changes: &[S2Change],
    handles: &[u32],
) -> Result<S2StashTransfer> {
    if save.index.is_legacy {
        return Err(Error::Refused(
            "This save was written by game version 1.0.x. It can be read, but its layout is not supported for editing; load it in the current game and save again."
                .to_owned(),
        ));
    }
    if !handles.is_empty() && !save.unresolved_handles.is_empty() {
        return Err(Error::Refused(
            "S2 stash move requires a fully resolved inventory".to_owned(),
        ));
    }
    let mut packed = None;
    let mut current = if changes.is_empty() {
        None
    } else {
        let written = save.write_changes(changes)?;
        let parsed = S2Save::from_bytes(&written)?;
        packed = Some(written);
        Some(parsed)
    };
    let mut moved = Vec::new();
    let mut skipped = Vec::new();
    let mut refused = Vec::new();
    for &handle in handles {
        let source = current.as_ref().unwrap_or(save);
        let listed = source.stash_items()?.iter().any(|item| item.handle == handle);
        if !listed || !is_marked_stash_owned(source, handle) {
            skipped.push(handle);
            continue;
        }
        match source.write_changes(&[S2Change::MoveStashToBackpack { handle }]) {
            Ok(written) => match S2Save::from_bytes(&written) {
                Ok(parsed) => {
                    current = Some(parsed);
                    packed = Some(written);
                    moved.push(handle);
                }
                Err(error) => {
                    return Ok(S2StashTransfer {
                        packed,
                        moved,
                        skipped,
                        refused,
                        stopped: Some(error.to_string()),
                    });
                }
            },
            Err(error) => {
                let message = error.to_string();
                refused.push(S2RefusedMove {
                    handle,
                    reason: classify_refusal(&message),
                    message,
                });
            }
        }
    }
    Ok(S2StashTransfer {
        packed,
        moved,
        skipped,
        refused,
        stopped: None,
    })
}

/// True when the record is in use (byte 15 is 1) and carries the stash-owned flag (byte 28, bit `0x08`).
fn is_marked_stash_owned(save: &S2Save, handle: u32) -> bool {
    let Some(record) = save.objects.unique(handle) else {
        return false;
    };
    let image = save.container.image();
    matches!(
        (
            read_u8(image, record.record_offset.saturating_add(15)),
            read_u8(image, record.record_offset.saturating_add(28)),
        ),
        (Ok(1), Ok(flags)) if flags & 0x08 != 0
    )
}

/// One validated grid cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S2GridCell {
    /// Object handle.
    pub handle: u32,
    /// Horizontal grid coordinate.
    pub x: u16,
    /// Vertical grid coordinate.
    pub y: u16,
}

/// A validated object-record candidate in the unpacked save image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct S2ObjectRecordIndex {
    /// Object handle.
    pub handle: u32,
    /// Start offset of this record candidate.
    pub record_offset: usize,
    /// Stack count field offset.
    pub count_offset: usize,
    /// Weight field offset.
    pub weight_offset: usize,
    /// Item type-key offset.
    pub type_key_offset: usize,
    /// Item kind code.
    pub kind_code: u8,
    /// Stack count.
    pub count: u32,
    /// Total weight read from the record.
    pub total_weight: f32,
    /// Three-byte save-local name key.
    pub type_key: [u8; 3],
}

/// One item view decoded from an indexed object record.
#[derive(Debug, Clone, PartialEq)]
pub struct S2InventoryItem {
    /// Object handle.
    pub handle: u32,
    /// Horizontal grid coordinate, if placed in the backpack.
    pub x: Option<u16>,
    /// Vertical grid coordinate, if placed in the backpack.
    pub y: Option<u16>,
    /// Grid width for a rectangular footprint.
    pub width: Option<u16>,
    /// Grid height for a rectangular footprint.
    pub height: Option<u16>,
    /// Grid cells occupied by this item.
    pub cells: Vec<S2GridCell>,
    /// Stack count.
    pub count: u32,
    /// Total weight.
    pub total_weight: f32,
    /// Object kind.
    pub kind_code: u8,
    /// Three-byte save-local name key.
    pub type_key: [u8; 3],
    /// Name resolved from the save's name tables.
    pub display_name: Option<String>,
    /// Whether the stack count is supported by known S2 layout evidence.
    pub editable_count: bool,
    /// Validated durability value for confirmed equipped armor.
    pub condition: Option<f32>,
    /// Unpacked image offset of the validated durability value.
    pub condition_offset: Option<usize>,
    /// Directly attached weapon modules that passed name-table checks.
    pub modules: Vec<String>,
    /// Weapon upgrades that passed name-table and family checks.
    pub upgrades: Vec<String>,
    /// Start offset in the unpacked image.
    pub record_offset: usize,
    /// Stack-count offset in the unpacked image.
    pub count_offset: usize,
}

/// One uniquely indexed item from a validated S2 stash.
#[derive(Debug, Clone, PartialEq)]
pub struct S2StashItem {
    /// Object handle.
    pub handle: u32,
    /// Leftmost stash grid column.
    pub x: u16,
    /// Topmost stash grid row.
    pub y: u16,
    /// Width of the occupied bounding box.
    pub width: u16,
    /// Height of the occupied bounding box.
    pub height: u16,
    /// Occupied stash cells.
    pub cells: Vec<S2GridCell>,
    /// Stack count from the uniquely indexed object record.
    pub count: u32,
    /// Total weight from the uniquely indexed object record.
    pub total_weight: f32,
    /// Object kind.
    pub kind_code: u8,
    /// Three-byte save-local name key.
    pub type_key: [u8; 3],
    /// Name resolved from the save's name tables.
    pub display_name: Option<String>,
}

/// Owned object record not mapped to the backpack grid.
#[derive(Debug, Clone, PartialEq)]
pub struct S2OrphanItem {
    /// Object handle.
    pub handle: u32,
    /// Record start offset.
    pub record_offset: usize,
    /// Record position X.
    pub x: u16,
    /// Record position Y.
    pub y: u16,
    /// Stack count.
    pub count: u32,
    /// Total weight.
    pub total_weight: f32,
    /// Object kind.
    pub kind_code: u8,
    /// Save-local name key.
    pub type_key: [u8; 3],
}

/// Validated stash handle and grid offsets.
pub struct S2StashLayout {
    owned_count_offset: usize,
    owned_handles_offset: usize,
    owned_handles: Vec<u32>,
    live_handles: Vec<u32>,
    grid_count_offset: usize,
    grid_offset: usize,
    grid_count: usize,
    grid_cells: Vec<S2GridCell>,
    grid_end_offset: usize,
}

impl S2StashLayout {
    /// Locates the unique stash block within 512 bytes of the player grid end.
    pub fn locate(raw: &[u8], player: &S2InventoryIndex) -> Result<Self> {
        let search_start = player.grid_end_offset;
        let search_end = raw
            .len()
            .min(checked_add(search_start, 512, "S2 stash search range overflow")?);
        let mut found = None;
        let mut search_from = search_start;
        while let Some(offset) = find_subslice(raw, &STASH_MARKER, search_from) {
            if offset >= search_end {
                break;
            }
            if found.is_some() {
                return Err(Error::damaged("S2 stash header is ambiguous"));
            }
            found = Some(offset);
            search_from = checked_add(offset, 1, "S2 stash marker offset overflow")?;
        }
        let marker_offset = found.ok_or_else(|| Error::damaged("S2 stash header was not found"))?;
        require_range(raw, marker_offset, 20, "S2 stash header is truncated")?;
        let tail_offset = checked_add(marker_offset, 16, "S2 stash header offset overflow")?;
        let tail_end = checked_add(tail_offset, STASH_HEADER_TAIL.len(), "S2 stash header end overflow")?;
        if raw.get(tail_offset..tail_end) != Some(STASH_HEADER_TAIL.as_slice()) {
            return Err(Error::damaged("S2 stash header shape is invalid"));
        }
        let owned_count_offset = checked_add(marker_offset, 20, "S2 stash count offset overflow")?;
        let owned_count = usize::from(read_u16(raw, owned_count_offset)?);
        if owned_count > MAXIMUM_OWNED_HANDLES {
            return Err(Error::damaged("S2 stash owned-handle count exceeds its limit"));
        }
        let handles_offset = checked_add(owned_count_offset, 2, "S2 stash handle offset overflow")?;
        let handles_bytes = owned_count
            .checked_mul(4)
            .ok_or_else(|| Error::damaged("S2 stash handle byte count overflow"))?;
        let grid_count_offset = checked_add(handles_offset, handles_bytes, "S2 stash grid count offset overflow")?;
        let handles_end = checked_add(handles_bytes, 2, "S2 stash grid count range overflow")?;
        require_range(raw, handles_offset, handles_end, "S2 stash handle array is truncated")?;
        let mut owned_handles = Vec::with_capacity(owned_count);
        for item in 0..owned_count {
            let relative = item
                .checked_mul(4)
                .ok_or_else(|| Error::damaged("S2 stash handle offset overflow"))?;
            let offset = checked_add(handles_offset, relative, "S2 stash handle offset overflow")?;
            owned_handles.push(read_u32(raw, offset)?);
        }
        let grid_count = usize::from(read_u16(raw, grid_count_offset)?);
        if grid_count > MAXIMUM_GRID_CELLS {
            return Err(Error::damaged("S2 stash grid-cell count exceeds its limit"));
        }
        let grid_offset = checked_add(grid_count_offset, 2, "S2 stash grid offset overflow")?;
        let grid_bytes = grid_count
            .checked_mul(8)
            .ok_or_else(|| Error::damaged("S2 stash grid size overflow"))?;
        require_range(raw, grid_offset, grid_bytes, "S2 stash grid is truncated")?;
        let mut grid_cells = Vec::with_capacity(grid_count);
        for item in 0..grid_count {
            let relative = item
                .checked_mul(8)
                .ok_or_else(|| Error::damaged("S2 stash grid offset overflow"))?;
            let offset = checked_add(grid_offset, relative, "S2 stash grid offset overflow")?;
            grid_cells.push(S2GridCell {
                handle: read_u32(raw, offset)?,
                x: read_u16(raw, checked_add(offset, 4, "S2 stash x offset overflow")?)?,
                y: read_u16(raw, checked_add(offset, 6, "S2 stash y offset overflow")?)?,
            });
        }
        let live_handles = owned_handles
            .iter()
            .copied()
            .filter(|handle| *handle != u32::MAX)
            .collect::<Vec<_>>();
        let live_set = live_handles.iter().copied().collect::<HashSet<_>>();
        if live_handles.iter().any(|handle| handle >> 16 != 0x3000)
            || grid_cells.iter().any(|cell| !live_set.contains(&cell.handle))
        {
            return Err(Error::damaged("S2 stash handles and grid cells are inconsistent"));
        }
        let grid_end_offset = checked_add(grid_offset, grid_bytes, "S2 stash grid end overflow")?;
        Ok(Self {
            owned_count_offset,
            owned_handles_offset: handles_offset,
            owned_handles,
            live_handles,
            grid_count_offset,
            grid_offset,
            grid_count,
            grid_cells,
            grid_end_offset,
        })
    }

    /// Owned-handle count offset.
    #[must_use]
    pub const fn owned_count_offset(&self) -> usize {
        self.owned_count_offset
    }

    /// Owned handles, including tombstones.
    #[must_use]
    pub fn owned_handles(&self) -> &[u32] {
        &self.owned_handles
    }

    /// Live handles.
    #[must_use]
    pub fn live_handles(&self) -> &[u32] {
        &self.live_handles
    }

    /// Stash grid cells.
    #[must_use]
    pub fn grid_cells(&self) -> &[S2GridCell] {
        &self.grid_cells
    }

    /// End offset of the stash grid.
    #[must_use]
    pub const fn grid_end_offset(&self) -> usize {
        self.grid_end_offset
    }
}

#[derive(Default)]
struct S2ObjectIndex {
    records: Vec<S2ObjectRecordIndex>,
    by_handle: HashMap<u32, Vec<usize>>,
    record_ends: Vec<usize>,
}

impl S2ObjectIndex {
    fn build(raw: &[u8], legacy: bool, referenced_handles: &HashSet<u32>) -> Result<Self> {
        let record_shift = if legacy { 3_usize } else { 0 };
        let marker_offset = 18_usize
            .checked_sub(record_shift)
            .ok_or_else(|| Error::damaged("S2 object record shift is invalid"))?;
        let count_relative = 19_usize
            .checked_sub(record_shift)
            .ok_or_else(|| Error::damaged("S2 count offset is invalid"))?;
        let weight_relative = 24_usize
            .checked_sub(record_shift)
            .ok_or_else(|| Error::damaged("S2 weight offset is invalid"))?;
        let kind_relative = 31_usize
            .checked_sub(record_shift)
            .ok_or_else(|| Error::damaged("S2 kind offset is invalid"))?;
        let mut result = Self::default();
        let mut previous_candidate: Option<usize> = None;

        // Scan every plausible record for boundaries, but retain records only for referenced handles.
        for (marker_position, marker) in raw.iter().enumerate().skip(marker_offset) {
            if *marker != 0x38 {
                continue;
            }
            let Some(record_offset) = marker_position.checked_sub(marker_offset) else {
                continue;
            };
            if raw.len().saturating_sub(record_offset) < 36 {
                continue;
            }
            let count_offset = checked_add(record_offset, count_relative, "S2 count offset overflow")?;
            let count = read_u32(raw, count_offset)?;
            if !(1..=10_000_000).contains(&count) {
                continue;
            }
            let weight_offset = checked_add(record_offset, weight_relative, "S2 weight offset overflow")?;
            let weight = f32::from_bits(read_u32(raw, weight_offset)?);
            if !weight.is_finite() || !(0.0..=10_000_000.0).contains(&weight) {
                continue;
            }
            let handle = read_u32(raw, record_offset)?;
            if let Some(previous_index) = previous_candidate.take() {
                if let Some(end) = result.record_ends.get_mut(previous_index) {
                    *end = record_offset;
                }
            }
            if !referenced_handles.contains(&handle)
                || result
                    .by_handle
                    .get(&handle)
                    .is_some_and(|candidates| candidates.len() >= MAXIMUM_OBJECT_CANDIDATES_PER_HANDLE)
            {
                continue;
            }
            let type_key_offset = checked_add(record_offset, 8, "S2 type key offset overflow")?;
            let type_key_slice = raw
                .get(type_key_offset..checked_add(type_key_offset, 3, "S2 type key end overflow")?)
                .ok_or_else(|| Error::damaged("S2 object type key is truncated"))?;
            let type_key: [u8; 3] = type_key_slice
                .try_into()
                .map_err(|_| Error::damaged("S2 object type key is truncated"))?;
            let kind_offset = checked_add(record_offset, kind_relative, "S2 kind offset overflow")?;
            let kind_code = read_u8(raw, kind_offset)?;
            let index = result.records.len();
            result.records.push(S2ObjectRecordIndex {
                handle,
                record_offset,
                count_offset,
                weight_offset,
                type_key_offset,
                kind_code,
                count,
                total_weight: weight,
                type_key,
            });
            result.by_handle.entry(handle).or_default().push(index);
            result.record_ends.push(usize::MAX);
            previous_candidate = Some(index);
        }
        if let Some(previous_index) = previous_candidate {
            if let (Some(previous), Some(end)) = (
                result.records.get(previous_index),
                result.record_ends.get_mut(previous_index),
            ) {
                *end = raw.len().min(previous.record_offset.saturating_add(4096));
            }
        }
        Ok(result)
    }

    fn candidates(&self, handle: u32) -> &[usize] {
        self.by_handle.get(&handle).map(Vec::as_slice).unwrap_or_default()
    }

    fn unique(&self, handle: u32) -> Option<&S2ObjectRecordIndex> {
        let candidates = self.candidates(handle);
        if candidates.len() != 1 {
            return None;
        }
        candidates.first().and_then(|index| self.records.get(*index))
    }
}

/// Parsed names embedded in an S2 save.
pub struct S2NameTables {
    tables: Vec<Vec<String>>,
    single_table: bool,
}

impl S2NameTables {
    fn locate(raw: &[u8], objects: &S2ObjectIndex, legacy: bool) -> Result<Option<Self>> {
        if legacy {
            let mut search_from = 0_usize;
            while let Some(name_start) = find_subslice(raw, b"Player", search_from) {
                let table_start = name_start.checked_sub(4);
                if let Some(table_start) = table_start {
                    if let Some((names, end)) = parse_name_table(raw, table_start, usize::from(u16::MAX)) {
                        if end == raw.len() && names.first().map(String::as_str) == Some("Player") {
                            return Ok(Some(Self {
                                tables: vec![names],
                                single_table: true,
                            }));
                        }
                    }
                }
                search_from = checked_add(name_start, 1, "S2 name search offset overflow")?;
            }
            return Ok(None);
        }
        let keys = objects.records.iter().map(|record| record.type_key).collect::<Vec<_>>();
        // Every block that fits the save's keys is a candidate; the names are used only if they agree on all keys.
        let mut candidates = Vec::new();
        let mut search_from = 0_usize;
        while let Some(name_start) = find_subslice(raw, b"GunAK74_ST", search_from) {
            let Some(table_start) = name_start.checked_sub(4) else {
                search_from = checked_add(name_start, 1, "S2 name search offset overflow")?;
                continue;
            };
            if let Some((first_table, mut offset)) = parse_name_table(raw, table_start, 8192) {
                if first_table.first().map(String::as_str) == Some("GunAK74_ST") {
                    let mut tables = vec![first_table];
                    while tables.len() < 16 {
                        let Some((table, next_offset)) = parse_name_table(raw, offset, 8192) else {
                            break;
                        };
                        tables.push(table);
                        offset = next_offset;
                    }
                    let result = Self {
                        tables,
                        single_table: false,
                    };
                    if keys.is_empty() || keys.iter().any(|key| result.resolve(key).is_some()) {
                        candidates.push(result);
                    }
                }
            }
            search_from = checked_add(name_start, 1, "S2 name search offset overflow")?;
        }
        let resolution = |table: &Self| {
            keys.iter()
                .map(|key| table.resolve(key).map(str::to_owned))
                .collect::<Vec<_>>()
        };
        let Some(first) = candidates.first().map(resolution) else {
            return Ok(None);
        };
        if candidates.iter().all(|candidate| resolution(candidate) == first) {
            Ok(candidates.into_iter().next())
        } else {
            Ok(None)
        }
    }

    fn resolve(&self, type_key: &[u8]) -> Option<&str> {
        let (table_index, name_index) = if self.single_table {
            if type_key.len() < 2 {
                return None;
            }
            (
                0,
                usize::from(u16::from_le_bytes([*type_key.first()?, *type_key.get(1)?])),
            )
        } else {
            if type_key.len() != 3 || *type_key.first()? < 4 {
                return None;
            }
            (
                usize::from(*type_key.first()?).checked_sub(4)?,
                usize::from(u16::from_le_bytes([*type_key.get(1)?, *type_key.get(2)?])),
            )
        };
        self.tables.get(table_index)?.get(name_index).map(String::as_str)
    }

    /// Number of parsed name tables.
    #[must_use]
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }
}

fn parse_name_table(raw: &[u8], start: usize, maximum_entries: usize) -> Option<(Vec<String>, usize)> {
    let count = usize::from(read_u16(raw, start).ok()?);
    if count == 0 || count > maximum_entries {
        return None;
    }
    let mut offset = start.checked_add(2)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let length = usize::from(read_u16(raw, offset).ok()?);
        if length > 4096 {
            return None;
        }
        offset = offset.checked_add(2)?;
        let end = offset.checked_add(length)?;
        let encoded = raw.get(offset..end)?;
        let name = std::str::from_utf8(encoded).ok()?;
        if !is_printable_name(name) {
            return None;
        }
        values.push(name.to_owned());
        offset = end;
    }
    Some((values, offset))
}

fn is_printable_name(value: &str) -> bool {
    value.chars().all(|character| {
        character == '\t' || character == ' ' || (!character.is_control() && !character.is_whitespace())
    })
}

fn find_subslice(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start > haystack.len() || haystack.len().saturating_sub(start) < needle.len() {
        return None;
    }
    haystack
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .and_then(|relative| start.checked_add(relative))
}

fn collect_inventory_warnings(
    raw: &[u8],
    index: &S2InventoryIndex,
    objects: &S2ObjectIndex,
) -> Result<(Vec<u32>, Vec<String>)> {
    let mut unresolved = index.unresolved_handles.iter().copied().collect::<BTreeSet<_>>();
    let mut warnings = index.warnings.clone();
    let mut cells_by_handle = BTreeMap::<u32, Vec<S2GridCell>>::new();
    for cell in &index.grid_cells {
        cells_by_handle.entry(cell.handle).or_default().push(*cell);
    }
    for (handle, cells) in &cells_by_handle {
        let candidates = objects.candidates(*handle);
        if candidates.len() != 1 {
            unresolved.insert(*handle);
            warnings.push(format!(
                "Inventory handle 0x{handle:08X}: object record candidates={}",
                candidates.len()
            ));
            continue;
        }
        let Some(candidate) = candidates.first().and_then(|position| objects.records.get(*position)) else {
            unresolved.insert(*handle);
            continue;
        };
        if !is_known_kind(candidate.kind_code) {
            unresolved.insert(*handle);
            warnings.push(format!(
                "Handle 0x{handle:08X}: неизвестный object kind={}, только read-only",
                candidate.kind_code
            ));
        }
        let min_x = cells.iter().map(|cell| cell.x).min().unwrap_or_default();
        let max_x = cells.iter().map(|cell| cell.x).max().unwrap_or_default();
        let min_y = cells.iter().map(|cell| cell.y).min().unwrap_or_default();
        let max_y = cells.iter().map(|cell| cell.y).max().unwrap_or_default();
        let width = usize::from(max_x.saturating_sub(min_x)).saturating_add(1);
        let height = usize::from(max_y.saturating_sub(min_y)).saturating_add(1);
        let expected = width
            .checked_mul(height)
            .ok_or_else(|| Error::damaged("S2 grid footprint size overflow"))?;
        let unique_cells = cells.iter().map(|cell| (cell.x, cell.y)).collect::<HashSet<_>>();
        if unique_cells.len() != cells.len() || unique_cells.len() != expected {
            unresolved.insert(*handle);
            warnings.push(format!(
                "Handle 0x{handle:08X}: footprint/коллизия grid cells не образует полный прямоугольник"
            ));
        }
    }

    let grid_handles = index.grid_cells.iter().map(|cell| cell.handle).collect::<HashSet<_>>();
    if !index.is_legacy {
        for handle in &index.owned_handles {
            if *handle == u32::MAX || grid_handles.contains(handle) || unresolved.contains(handle) {
                continue;
            }
            let candidates = objects.candidates(*handle);
            if candidates.len() != 1 {
                unresolved.insert(*handle);
                warnings.push(format!(
                    "Owned handle 0x{handle:08X}: отсутствует однозначный object record"
                ));
                continue;
            }
            let Some(record) = objects.unique(*handle) else {
                continue;
            };
            if !has_equipment_shape(raw, *handle, record) && !is_known_kind(record.kind_code) {
                unresolved.insert(*handle);
                warnings.push(format!(
                    "Handle 0x{handle:08X}: неизвестный orphan object kind={}, только read-only",
                    record.kind_code
                ));
            }
        }
    }
    Ok((unresolved.into_iter().collect(), warnings))
}

fn build_inventory_items(
    raw: &[u8],
    index: &S2InventoryIndex,
    objects: &S2ObjectIndex,
    names: Option<&S2NameTables>,
) -> Vec<S2InventoryItem> {
    let record_ends = record_end_guesses(raw.len(), index, objects);
    let mut cells_by_handle = BTreeMap::<u32, Vec<S2GridCell>>::new();
    for cell in &index.grid_cells {
        cells_by_handle.entry(cell.handle).or_default().push(*cell);
    }
    let mut items = Vec::new();
    for (handle, mut cells) in cells_by_handle {
        let Some(record) = objects.unique(handle) else { continue };
        cells.sort_by_key(|cell| (cell.y, cell.x));
        let x = cells.iter().map(|cell| cell.x).min().unwrap_or_default();
        let y = cells.iter().map(|cell| cell.y).min().unwrap_or_default();
        let max_x = cells.iter().map(|cell| cell.x).max().unwrap_or_default();
        let max_y = cells.iter().map(|cell| cell.y).max().unwrap_or_default();
        let display_name = names
            .and_then(|table| table.resolve(&record.type_key))
            .map(str::to_owned);
        let weapon_state = if !index.is_legacy && record.kind_code == 0 {
            names.and_then(|table| {
                read_weapon_state(
                    raw,
                    handle,
                    record.record_offset,
                    record_ends
                        .get(&handle)
                        .copied()
                        .unwrap_or_else(|| raw.len().min(record.record_offset.saturating_add(512))),
                    table,
                )
            })
        } else {
            None
        };
        let (condition, condition_offset, modules, upgrades) = match weapon_state {
            Some((offset, value, modules, upgrades)) => (Some(value), Some(offset), modules, upgrades),
            None => (None, None, Vec::new(), Vec::new()),
        };
        items.push(S2InventoryItem {
            handle,
            x: Some(x),
            y: Some(y),
            width: Some(max_x.saturating_sub(x).saturating_add(1)),
            height: Some(max_y.saturating_sub(y).saturating_add(1)),
            cells,
            count: record.count,
            total_weight: record.total_weight,
            kind_code: record.kind_code,
            type_key: record.type_key,
            display_name,
            editable_count: !index.is_legacy && is_editable_stack(record.kind_code, record.count),
            condition,
            condition_offset,
            modules,
            upgrades,
            record_offset: record.record_offset,
            count_offset: record.count_offset,
        });
    }

    if !index.is_legacy {
        let grid_handles = index.grid_cells.iter().map(|cell| cell.handle).collect::<HashSet<_>>();
        for handle in &index.owned_handles {
            if *handle == u32::MAX || grid_handles.contains(handle) {
                continue;
            }
            let Some(record) = objects.unique(*handle) else {
                continue;
            };
            let carried = matches!(record.kind_code, 6 | 8 | 10 | 11);
            if !carried && !has_equipment_shape(raw, *handle, record) {
                continue;
            }
            let display_name = names
                .and_then(|table| table.resolve(&record.type_key))
                .map(str::to_owned);
            let armor_state = (!carried && record.kind_code == 1)
                .then(|| read_armor_condition(raw, *handle, record.record_offset, record.kind_code))
                .flatten();
            let armor_upgrades = armor_state
                .and_then(|(value_offset, _)| {
                    names.and_then(|table| read_armor_upgrades(raw, record.record_offset, value_offset, table))
                })
                .unwrap_or_default();
            let weapon_state = if record.kind_code == 0 {
                names.and_then(|table| {
                    read_weapon_state(
                        raw,
                        *handle,
                        record.record_offset,
                        record_ends
                            .get(handle)
                            .copied()
                            .unwrap_or_else(|| raw.len().min(record.record_offset.saturating_add(512))),
                        table,
                    )
                })
            } else {
                None
            };
            let (condition, condition_offset, modules, upgrades) = match weapon_state {
                Some((offset, value, modules, upgrades)) => (Some(value), Some(offset), modules, upgrades),
                None => (
                    armor_state.map(|state| state.1),
                    armor_state.map(|state| state.0),
                    Vec::new(),
                    armor_upgrades,
                ),
            };
            items.push(S2InventoryItem {
                handle: *handle,
                x: None,
                y: None,
                width: None,
                height: None,
                cells: Vec::new(),
                count: record.count,
                total_weight: record.total_weight,
                kind_code: record.kind_code,
                type_key: record.type_key,
                display_name,
                editable_count: false,
                condition,
                condition_offset,
                modules,
                upgrades,
                record_offset: record.record_offset,
                count_offset: record.count_offset,
            });
        }
    }
    items.sort_by_key(|item| {
        (
            item.y.is_none(),
            item.y.unwrap_or_default(),
            item.x.is_none(),
            item.x.unwrap_or_default(),
            item.handle,
        )
    });
    items
}

fn record_end_guesses(raw_length: usize, index: &S2InventoryIndex, objects: &S2ObjectIndex) -> HashMap<u32, usize> {
    let mut ends = HashMap::with_capacity(index.owned_handles.len());
    for handle in &index.owned_handles {
        let candidates = objects.candidates(*handle);
        if candidates.len() != 1 {
            continue;
        }
        if let Some(record_index) = candidates.first().copied() {
            if let Some(record) = objects.records.get(record_index) {
                let fallback = raw_length.min(record.record_offset.saturating_add(4096));
                let end = objects
                    .record_ends
                    .get(record_index)
                    .copied()
                    .unwrap_or(fallback)
                    .min(raw_length);
                ends.insert(*handle, end);
            }
        }
    }
    ends
}

fn build_orphans(raw: &[u8], index: &S2InventoryIndex, objects: &S2ObjectIndex) -> Vec<S2OrphanItem> {
    if index.is_legacy {
        return Vec::new();
    }
    let grid_handles = index.grid_cells.iter().map(|cell| cell.handle).collect::<HashSet<_>>();
    let unresolved = index.unresolved_handles.iter().copied().collect::<HashSet<_>>();
    let mut result = Vec::new();
    for handle in &index.owned_handles {
        if *handle == u32::MAX || grid_handles.contains(handle) || unresolved.contains(handle) {
            continue;
        }
        let Some(record) = objects.unique(*handle) else {
            continue;
        };
        if has_equipment_shape(raw, *handle, record) {
            continue;
        }
        let x = read_u16(raw, record.record_offset.saturating_add(11)).unwrap_or_default();
        let y = read_u16(raw, record.record_offset.saturating_add(13)).unwrap_or_default();
        result.push(S2OrphanItem {
            handle: *handle,
            record_offset: record.record_offset,
            x,
            y,
            count: record.count,
            total_weight: record.total_weight,
            kind_code: record.kind_code,
            type_key: record.type_key,
        });
    }
    result
}

fn has_equipment_shape(raw: &[u8], handle: u32, record: &S2ObjectRecordIndex) -> bool {
    if !matches!(record.kind_code, 0..=2) {
        return false;
    }
    let Some(offset) = record.record_offset.checked_add(0x23) else {
        return false;
    };
    read_u32(raw, record.record_offset).ok() == Some(handle) && read_u32(raw, offset).ok() == Some(handle)
}

fn read_armor_condition(raw: &[u8], handle: u32, record_offset: usize, kind_code: u8) -> Option<(usize, f32)> {
    if kind_code != 1 || read_u32(raw, record_offset).ok()? != handle {
        return None;
    }
    let nested_offset = record_offset.checked_add(0x23)?;
    if read_u32(raw, nested_offset).ok()? != handle {
        return None;
    }
    let value_offset = nested_offset.checked_add(4)?;
    let value = f32::from_bits(read_u32(raw, value_offset).ok()?);
    (value.is_finite() && (0.0..=1.0).contains(&value)).then_some((value_offset, value))
}

fn read_armor_upgrades(
    raw: &[u8],
    record_offset: usize,
    value_offset: usize,
    names: &S2NameTables,
) -> Option<Vec<String>> {
    const MAXIMUM_UPGRADES: usize = 64;
    let key_offset = record_offset.checked_add(8)?;
    let own_name = names.resolve(raw.get(key_offset..key_offset.checked_add(3)?)?)?;
    let count_offset = value_offset.checked_add(4)?;
    let count = usize::from(read_u16(raw, count_offset).ok()?);
    if count == 0 {
        return Some(Vec::new());
    }
    if count > MAXIMUM_UPGRADES {
        return None;
    }
    let values_start = count_offset.checked_add(2)?;
    let bytes = count.checked_mul(3)?;
    let end = values_start.checked_add(bytes)?;
    if end > raw.len() {
        return None;
    }
    let mut upgrades = Vec::with_capacity(count);
    for index in 0..count {
        let offset = values_start.checked_add(index.checked_mul(3)?)?;
        let name = names.resolve(raw.get(offset..offset.checked_add(3)?)?)?;
        if !starts_with_family(name, own_name) {
            return None;
        }
        upgrades.push(name.to_owned());
    }
    Some(upgrades)
}

fn read_weapon_state(
    raw: &[u8],
    handle: u32,
    record_offset: usize,
    record_end: usize,
    names: &S2NameTables,
) -> Option<(usize, f32, Vec<String>, Vec<String>)> {
    if read_u32(raw, record_offset).ok()? != handle {
        return None;
    }
    const MINIMUM_OFFSET: usize = 0x30;
    const MAXIMUM_UPGRADES: usize = 64;
    let minimum_offset = record_offset.checked_add(MINIMUM_OFFSET)?;
    let limit = raw.len().min(record_end.max(record_offset));
    if limit <= minimum_offset.saturating_add(6) {
        return None;
    }

    let mut candidates = Vec::new();
    let mut value_offset = minimum_offset;
    while value_offset.saturating_add(6) < limit {
        let value = f32::from_bits(read_u32(raw, value_offset).ok()?);
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            let vector_offset = value_offset.checked_add(4)?;
            if let Some((upgrades, upgrades_end)) =
                read_upgrade_vector(raw, vector_offset, limit, names, MAXIMUM_UPGRADES)
            {
                let modules = read_direct_modules(raw, value_offset, record_offset, names);
                let module_count = modules.len();
                let counted_run = has_counted_module_run(raw, value_offset, module_count, record_offset);
                if upgrades.is_empty() {
                    if counted_run && modules.iter().any(|module| contains_ascii(module, "_mag")) {
                        candidates.push((value_offset, value, modules, upgrades, upgrades_end));
                    }
                } else if (upgrades.len() >= 2 || counted_run) && !modules.is_empty() {
                    let first_upgrade = upgrades.first()?;
                    if let Some(separator) = find_ascii(first_upgrade, "_upgrade_") {
                        if separator > 0 {
                            let family = first_upgrade.get(..separator)?;
                            if modules.iter().any(|module| starts_with_family(module, family)) {
                                candidates.push((value_offset, value, modules, upgrades, upgrades_end));
                            }
                        }
                    }
                }
            }
        }
        value_offset = value_offset.checked_add(1)?;
    }

    if candidates.len() != 1 {
        return None;
    }
    let (offset, value, modules, upgrades, _) = candidates.pop()?;
    Some((offset, value, modules, upgrades))
}

fn read_upgrade_vector(
    raw: &[u8],
    offset: usize,
    limit: usize,
    names: &S2NameTables,
    maximum: usize,
) -> Option<(Vec<String>, usize)> {
    if offset > limit || limit.saturating_sub(offset) < 2 {
        return None;
    }
    let count = usize::from(read_u16(raw, offset).ok()?);
    if count == 0 {
        return Some((Vec::new(), offset.checked_add(2)?));
    }
    if count > maximum {
        return None;
    }
    let values_start = offset.checked_add(2)?;
    let byte_count = count.checked_mul(3)?;
    let end = values_start.checked_add(byte_count)?;
    if end > limit {
        return None;
    }
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let start = values_start.checked_add(index.checked_mul(3)?)?;
        let name = names.resolve(raw.get(start..start.checked_add(3)?)?)?;
        if !contains_ascii(name, "_upgrade_") {
            return None;
        }
        values.push(name.to_owned());
    }
    Some((values, end))
}

fn read_direct_modules(raw: &[u8], value_offset: usize, record_offset: usize, names: &S2NameTables) -> Vec<String> {
    let Some(minimum) = record_offset.checked_add(0x30) else {
        return Vec::new();
    };
    let Some(mut offset) = value_offset.checked_sub(3) else {
        return Vec::new();
    };
    let mut reversed = Vec::new();
    while offset >= minimum {
        let Some(end) = offset.checked_add(3) else { break };
        let Some(key) = raw.get(offset..end) else { break };
        let Some(name) = names.resolve(key) else { break };
        if !is_direct_module_name(name) {
            break;
        }
        reversed.push(name.to_owned());
        let Some(previous) = offset.checked_sub(3) else { break };
        offset = previous;
    }
    reversed.reverse();
    reversed
}

fn has_counted_module_run(raw: &[u8], value_offset: usize, module_count: usize, record_offset: usize) -> bool {
    if module_count == 0 {
        return false;
    }
    let Some(module_bytes) = module_count.checked_mul(3) else {
        return false;
    };
    let Some(count_offset) = value_offset.checked_sub(module_bytes.saturating_add(2)) else {
        return false;
    };
    count_offset >= record_offset.saturating_add(0x30)
        && read_u16(raw, count_offset).ok().map(usize::from) == Some(module_count)
}

fn is_direct_module_name(name: &str) -> bool {
    !contains_ascii(name, "_upgrade_")
        && (["en_", "hp_", "ru_", "toprail"].iter().any(|prefix| {
            name.get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
        }) || contains_ascii(name, "_mag")
            || ends_with_ascii(name, "_screw"))
}

fn contains_ascii(value: &str, needle: &str) -> bool {
    !needle.is_empty()
        && value
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn find_ascii(value: &str, needle: &str) -> Option<usize> {
    (!needle.is_empty())
        .then(|| {
            value
                .as_bytes()
                .windows(needle.len())
                .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
        })
        .flatten()
}

fn ends_with_ascii(value: &str, suffix: &str) -> bool {
    value.len() >= suffix.len()
        && value
            .get(value.len().saturating_sub(suffix.len())..)
            .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
}

fn starts_with_family(value: &str, family: &str) -> bool {
    value
        .get(..family.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(family))
        && value.as_bytes().get(family.len()) == Some(&b'_')
}

fn is_known_kind(kind: u8) -> bool {
    matches!(kind, 0 | 1 | 2 | 4 | 5 | 6 | 7 | 8 | 10 | 11)
}

fn is_editable_stack(kind: u8, count: u32) -> bool {
    (count > 1 && matches!(kind, 4 | 5 | 7 | 8)) || (count >= 1 && matches!(kind, 4 | 5 | 7))
}

fn has_unique_owned_handle(index: &S2InventoryIndex, handle: u32) -> bool {
    index.owned_handles.iter().filter(|owned| **owned == handle).count() == 1
}

fn has_unique_backpack_grid_handle(index: &S2InventoryIndex, handle: u32) -> bool {
    has_unique_owned_handle(index, handle) && index.grid_cells.iter().filter(|cell| cell.handle == handle).count() == 1
}

/// Wallet and grid offsets plus validated handle references.
pub struct S2InventoryIndex {
    money_offset: usize,
    owned_count_offset: usize,
    owned_handles_offset: usize,
    owned_handles: Vec<u32>,
    grid_count_offset: usize,
    grid_offset: usize,
    grid_count: usize,
    grid_cells: Vec<S2GridCell>,
    grid_end_offset: usize,
    grid_handle_count: usize,
    unresolved_handles: Vec<u32>,
    warnings: Vec<String>,
    is_legacy: bool,
}

impl S2InventoryIndex {
    /// Locates the wallet anchor and checks owned handles and grid cells.
    pub fn locate(raw: &[u8]) -> Result<Self> {
        let anchors = find_all(raw, &WALLET_ANCHOR);
        let (money_offset, is_legacy) = match anchors.as_slice() {
            [anchor] => (
                checked_add(*anchor, WALLET_ANCHOR.len(), "S2 wallet offset overflow")?,
                false,
            ),
            [] => (locate_legacy_wallet(raw)?, true),
            _ => return Err(Error::damaged("S2 wallet anchor is ambiguous")),
        };
        let owned_count_offset = checked_add(money_offset, 8, "S2 owned count offset overflow")?;
        let owned_count = usize::from(read_u16(raw, owned_count_offset)?);
        if owned_count > MAXIMUM_OWNED_HANDLES {
            return Err(Error::damaged("S2 owned-handle count exceeds its limit"));
        }
        let owned_handles_offset = checked_add(owned_count_offset, 2, "S2 owned array offset overflow")?;
        let owned_bytes = owned_count
            .checked_mul(4)
            .ok_or_else(|| Error::damaged("S2 owned-handle byte count overflow"))?;
        require_range(
            raw,
            owned_handles_offset,
            checked_add(owned_bytes, 2, "S2 owned grid count range overflow")?,
            "S2 owned-handle array is truncated",
        )?;
        let mut owned_handles = Vec::with_capacity(owned_count);
        for item in 0..owned_count {
            let relative = item
                .checked_mul(4)
                .ok_or_else(|| Error::damaged("S2 owned-handle offset overflow"))?;
            let offset = checked_add(owned_handles_offset, relative, "S2 owned-handle offset overflow")?;
            owned_handles.push(read_u32(raw, offset)?);
        }
        validate_owned_handles(&owned_handles, is_legacy)?;

        let grid_count_offset = checked_add(owned_handles_offset, owned_bytes, "S2 grid count offset overflow")?;
        let grid_count = usize::from(read_u16(raw, grid_count_offset)?);
        if grid_count > MAXIMUM_GRID_CELLS {
            return Err(Error::damaged("S2 grid-cell count exceeds its limit"));
        }
        let grid_offset = checked_add(grid_count_offset, 2, "S2 grid offset overflow")?;
        let grid_record_size = if is_legacy { 6 } else { 8 };
        let grid_bytes = grid_count
            .checked_mul(grid_record_size)
            .ok_or_else(|| Error::damaged("S2 grid byte count overflow"))?;
        require_range(raw, grid_offset, grid_bytes, "S2 inventory grid is truncated")?;

        let owned_set = owned_handles.iter().copied().collect::<std::collections::HashSet<_>>();
        let mut all_grid_handles = std::collections::HashSet::new();
        let mut unresolved = std::collections::BTreeSet::new();
        let mut warnings = Vec::new();
        let mut seen_owned = HashSet::new();
        for handle in &owned_handles {
            if *handle != u32::MAX && !seen_owned.insert(*handle) {
                unresolved.insert(*handle);
                warnings.push(format!("Owned handle list contains duplicate 0x{handle:08X}"));
            }
        }
        let mut grid_positions = std::collections::HashMap::new();
        let mut grid_cells = Vec::with_capacity(grid_count);
        for cell_index in 0..grid_count {
            let relative = cell_index
                .checked_mul(grid_record_size)
                .ok_or_else(|| Error::damaged("S2 grid cell offset overflow"))?;
            let offset = checked_add(grid_offset, relative, "S2 grid cell offset overflow")?;
            let handle = read_u32(raw, offset)?;
            let x_offset = checked_add(offset, 4, "S2 grid coordinate offset overflow")?;
            let (x, y) = if is_legacy {
                (
                    u16::from(read_u8(raw, x_offset)?),
                    u16::from(read_u8(raw, checked_add(x_offset, 1, "S2 grid y offset overflow")?)?),
                )
            } else {
                (
                    read_u16(raw, x_offset)?,
                    read_u16(raw, checked_add(x_offset, 2, "S2 grid y offset overflow")?)?,
                )
            };
            all_grid_handles.insert(handle);
            if !owned_set.contains(&handle) {
                unresolved.insert(handle);
                warnings.push(format!(
                    "grid cell {cell_index} refers to a handle outside the owned list"
                ));
                continue;
            }
            if handle >> 24 != 0x30 || x >= GRID_WIDTH || y >= 128 {
                unresolved.insert(handle);
                warnings.push(format!("grid cell {cell_index} is outside supported bounds"));
                continue;
            }
            if let Some(previous) = grid_positions.get(&(x, y)).copied() {
                unresolved.insert(previous);
                unresolved.insert(handle);
                warnings.push(format!("grid position {x},{y} is duplicated"));
                continue;
            }
            grid_positions.insert((x, y), handle);
            grid_cells.push(S2GridCell { handle, x, y });
        }
        if grid_count == 0 {
            warnings.push("inventory grid is empty; only owned handles are indexed".to_owned());
        }
        if is_legacy {
            warnings.push(
                "Save uses the game 1.0.x layout: items are read-only and state fields are not indexed.".to_owned(),
            );
        }
        let grid_end_offset = checked_add(grid_offset, grid_bytes, "S2 grid end offset overflow")?;
        Ok(Self {
            money_offset,
            owned_count_offset,
            owned_handles_offset,
            owned_handles,
            grid_count_offset,
            grid_offset,
            grid_count,
            grid_cells,
            grid_end_offset,
            grid_handle_count: all_grid_handles.len(),
            unresolved_handles: unresolved.into_iter().collect(),
            warnings,
            is_legacy,
        })
    }

    /// Money offset in the unpacked image.
    #[must_use]
    pub const fn money_offset(&self) -> usize {
        self.money_offset
    }

    /// Owned-list count offset.
    #[must_use]
    pub const fn owned_count_offset(&self) -> usize {
        self.owned_count_offset
    }

    /// Owned handle array offset.
    #[must_use]
    pub const fn owned_handles_offset(&self) -> usize {
        self.owned_handles_offset
    }

    /// Handles in the owned array, including tombstones.
    #[must_use]
    pub fn owned_handles(&self) -> &[u32] {
        &self.owned_handles
    }

    /// Grid count field offset.
    #[must_use]
    pub const fn grid_count_offset(&self) -> usize {
        self.grid_count_offset
    }

    /// Grid cell array offset.
    #[must_use]
    pub const fn grid_offset(&self) -> usize {
        self.grid_offset
    }

    /// Validated grid cells.
    #[must_use]
    pub fn grid_cells(&self) -> &[S2GridCell] {
        &self.grid_cells
    }

    /// End of the validated grid cell array.
    #[must_use]
    pub const fn grid_end_offset(&self) -> usize {
        self.grid_end_offset
    }

    /// Number of distinct grid handles.
    #[must_use]
    pub const fn grid_handle_count(&self) -> usize {
        self.grid_handle_count
    }

    /// Handles with invalid or ambiguous grid references.
    #[must_use]
    pub fn unresolved_handles(&self) -> &[u32] {
        &self.unresolved_handles
    }

    /// Inventory layout warnings.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// True for the read-only game 1.0.x layout.
    #[must_use]
    pub const fn is_legacy(&self) -> bool {
        self.is_legacy
    }
}

fn read_u8(bytes: &[u8], offset: usize) -> Result<u8> {
    bytes
        .get(offset)
        .copied()
        .ok_or_else(|| Error::damaged("S2 field is truncated"))
}

fn checked_add(left: usize, right: usize, message: &'static str) -> Result<usize> {
    left.checked_add(right).ok_or_else(|| Error::damaged(message))
}

fn require_range(bytes: &[u8], offset: usize, length: usize, message: &'static str) -> Result<()> {
    let end = checked_add(offset, length, "S2 byte range overflow")?;
    if end > bytes.len() {
        return Err(Error::damaged(message));
    }
    Ok(())
}

#[cfg(test)]
fn apply_changes_to_image(save: &S2Save, changes: &[S2Change]) -> Result<(Vec<u8>, Vec<ChangedRange>)> {
    apply_changes_with_items(save, &save.items(), changes)
}

fn apply_changes_with_items(
    save: &S2Save,
    source_items: &[S2InventoryItem],
    changes: &[S2Change],
) -> Result<(Vec<u8>, Vec<ChangedRange>)> {
    if changes.is_empty() {
        return Err(Error::Refused("S2 write requires at least one change".to_owned()));
    }
    if save.index.is_legacy {
        return Err(Error::Refused(
            "This save was written by game version 1.0.x. It can be read, but its layout is not supported for editing; load it in the current game and save again."
                .to_owned(),
        ));
    }
    if changes
        .iter()
        .filter(|change| matches!(change, S2Change::MoveStashToBackpack { .. }))
        .count()
        > 1
    {
        return Err(Error::Refused(
            "one S2 edit batch may move at most one stash item".to_owned(),
        ));
    }

    let durability_items = changes
        .iter()
        .any(|change| matches!(change, S2Change::SetDurability { .. }))
        .then_some(source_items);
    let mut image = save.container.image().to_vec();
    let mut changed_ranges = Vec::with_capacity(changes.len().saturating_add(3));
    let mut stash_move = None;

    for change in changes {
        match *change {
            S2Change::SetMoney { old_value, new_value } => {
                if new_value > MAXIMUM_MONEY {
                    return Err(Error::Refused("S2 money is outside the supported range".to_owned()));
                }
                if save.money()? != old_value {
                    return Err(Error::Refused(
                        "S2 money changed since the edit was prepared; reload the save".to_owned(),
                    ));
                }
                write_u32_at(&mut image, save.index.money_offset, new_value)?;
                changed_ranges.push(ChangedRange {
                    before: save.index.money_offset..save.index.money_offset.saturating_add(4),
                    after: save.index.money_offset..save.index.money_offset.saturating_add(4),
                });
            }
            S2Change::SetStackCount { handle, count } => {
                if !(1..=MAXIMUM_EDITABLE_STACK_COUNT).contains(&count) {
                    return Err(Error::Refused(
                        "S2 stack count is outside the supported range".to_owned(),
                    ));
                }
                if !has_unique_backpack_grid_handle(&save.index, handle) {
                    return Err(Error::Refused(
                        "S2 stack handle is not uniquely owned by the backpack grid".to_owned(),
                    ));
                }
                let record = save
                    .objects
                    .unique(handle)
                    .ok_or_else(|| Error::Refused("S2 stack handle is missing or ambiguous".to_owned()))?;
                if save.unresolved_handles.contains(&handle) || !is_editable_stack(record.kind_code, record.count) {
                    return Err(Error::Refused(
                        "S2 stack kind or layout is not confirmed editable".to_owned(),
                    ));
                }
                if record.kind_code == 8 && count == 1 {
                    return Err(Error::Refused(
                        "S2 kind-8 stacks cannot be reduced to one before packing".to_owned(),
                    ));
                }
                let unit_weight = record.total_weight / record.count as f32;
                let total_weight = unit_weight * count as f32;
                if !total_weight.is_finite() || !(0.0..=10_000_000.0).contains(&total_weight) {
                    return Err(Error::Refused(
                        "S2 stack weight would leave the supported range".to_owned(),
                    ));
                }
                write_u32_at(&mut image, record.count_offset, count)?;
                write_u32_at(&mut image, record.weight_offset, total_weight.to_bits())?;
                changed_ranges.push(ChangedRange {
                    before: record.count_offset..record.count_offset.saturating_add(4),
                    after: record.count_offset..record.count_offset.saturating_add(4),
                });
                changed_ranges.push(ChangedRange {
                    before: record.weight_offset..record.weight_offset.saturating_add(4),
                    after: record.weight_offset..record.weight_offset.saturating_add(4),
                });
            }
            S2Change::SetDurability { handle, condition } => {
                if !condition.is_finite() || !(0.0..=1.0).contains(&condition) {
                    return Err(Error::Refused(
                        "S2 durability must be finite and between zero and one".to_owned(),
                    ));
                }
                if !has_unique_owned_handle(&save.index, handle) {
                    return Err(Error::Refused("S2 durability handle is not uniquely owned".to_owned()));
                }
                let item = durability_items
                    .as_ref()
                    .and_then(|items| {
                        let mut matches = items.iter().filter(|item| item.handle == handle);
                        let only = matches.next()?;
                        matches.next().is_none().then_some(only)
                    })
                    .ok_or_else(|| Error::Refused("S2 durability handle is missing or ambiguous".to_owned()))?;
                if save.unresolved_handles.contains(&handle) {
                    return Err(Error::Refused("S2 durability item is unresolved".to_owned()));
                }
                let offset = item
                    .condition_offset
                    .filter(|_| item.condition.is_some())
                    .ok_or_else(|| Error::Refused("S2 durability field is not confirmed editable".to_owned()))?;
                write_u32_at(&mut image, offset, condition.to_bits())?;
                changed_ranges.push(ChangedRange {
                    before: offset..offset.saturating_add(4),
                    after: offset..offset.saturating_add(4),
                });
            }
            S2Change::MoveStashToBackpack { handle } => stash_move = Some(handle),
        }
    }

    if let Some(handle) = stash_move {
        let stash = save.stash()?;
        move_stash_item_to_backpack(save, &stash, handle, &mut image, &mut changed_ranges)?;
    }
    changed_ranges.sort_unstable_by_key(|range| (range.before.start, range.after.start));
    Ok((image, changed_ranges))
}

/// Returns the first backpack origin where every cell of the item's footprint is free.
///
/// `offsets` are the footprint cells relative to its top-left corner; the search scans rows first.
fn first_free_placement(
    occupied: &HashSet<(u16, u16)>,
    offsets: &[(u16, u16)],
    width: u16,
    height: u16,
) -> Option<(u16, u16)> {
    for y in 0..=128_u16.saturating_sub(height) {
        for x in 0..=GRID_WIDTH.saturating_sub(width) {
            let fits = offsets.iter().all(|&(dx, dy)| {
                x.checked_add(dx)
                    .zip(y.checked_add(dy))
                    .is_some_and(|position| !occupied.contains(&position))
            });
            if fits {
                return Some((x, y));
            }
        }
    }
    None
}

/// Offset of an object record after the edited inventory window `window` grew or shrank by `shifted`.
///
/// Object records sit before the inventory blocks in every S2 save, so a record before the window keeps its
/// offset; a record after the window moves by `shifted`. A record inside the window cannot be placed safely.
fn moved_object_offset(record_offset: usize, window: std::ops::Range<usize>, shifted: usize) -> Result<usize> {
    if record_offset < window.start {
        return Ok(record_offset);
    }
    if record_offset < window.end {
        return Err(Error::Refused(
            "S2 stash item record lies inside the edited inventory block".to_owned(),
        ));
    }
    record_offset
        .checked_add(shifted)
        .ok_or_else(|| Error::damaged("S2 moved object record offset overflows"))
}

fn move_stash_item_to_backpack(
    save: &S2Save,
    stash: &S2StashLayout,
    handle: u32,
    image: &mut Vec<u8>,
    changed_ranges: &mut Vec<ChangedRange>,
) -> Result<()> {
    if !save.unresolved_handles.is_empty() {
        return Err(Error::Refused(
            "S2 stash move requires a fully resolved inventory".to_owned(),
        ));
    }
    let record = save
        .objects
        .unique(handle)
        .ok_or_else(|| Error::Refused("S2 stash item handle is missing or ambiguous".to_owned()))?;
    let stash_owned_index = stash
        .owned_handles
        .iter()
        .position(|owned| *owned == handle)
        .ok_or_else(|| Error::Refused("S2 handle is not owned by the stash".to_owned()))?;
    if stash.owned_handles.iter().filter(|owned| **owned == handle).count() != 1 {
        return Err(Error::Refused("S2 stash handle is duplicated".to_owned()));
    }
    if save.index.owned_handles.contains(&handle) {
        return Err(Error::Refused(
            "S2 stash item is already owned by the backpack".to_owned(),
        ));
    }
    if !is_known_kind(record.kind_code) && record.kind_code != 3 {
        return Err(Error::Refused(
            "S2 stash item kind is not confirmed editable".to_owned(),
        ));
    }
    if read_u8(image, record.record_offset.saturating_add(15))? != 1
        || read_u8(image, record.record_offset.saturating_add(28))? & 0x08 == 0
    {
        return Err(Error::Refused("S2 object is not marked as stash-owned".to_owned()));
    }
    let stash_cells = stash
        .grid_cells
        .iter()
        .filter(|cell| cell.handle == handle)
        .copied()
        .collect::<Vec<_>>();
    if stash_cells.is_empty() {
        return Err(Error::Refused("S2 stash item footprint is missing".to_owned()));
    }
    let min_x = stash_cells.iter().map(|cell| cell.x).min().unwrap_or_default();
    let max_x = stash_cells.iter().map(|cell| cell.x).max().unwrap_or_default();
    let min_y = stash_cells.iter().map(|cell| cell.y).min().unwrap_or_default();
    let max_y = stash_cells.iter().map(|cell| cell.y).max().unwrap_or_default();
    let width = max_x.saturating_sub(min_x).saturating_add(1);
    let height = max_y.saturating_sub(min_y).saturating_add(1);
    let footprint = stash_cells.len();
    let unique_positions = stash_cells.iter().map(|cell| (cell.x, cell.y)).collect::<HashSet<_>>();
    if unique_positions.len() != stash_cells.len() {
        return Err(Error::Refused(
            "S2 stash item footprint contains duplicate cells".to_owned(),
        ));
    }
    if width == 0 || height == 0 || width > GRID_WIDTH || height > 128 {
        return Err(Error::Refused("S2 stash item does not fit a backpack".to_owned()));
    }
    let new_owned_count = save
        .index
        .owned_handles
        .len()
        .checked_add(1)
        .filter(|count| *count <= MAXIMUM_OWNED_HANDLES)
        .ok_or_else(|| Error::Refused("S2 backpack owned-handle limit would be exceeded".to_owned()))?;
    let new_grid_count = save
        .index
        .grid_count
        .checked_add(footprint)
        .filter(|count| *count <= MAXIMUM_GRID_CELLS && *count <= usize::from(u16::MAX))
        .ok_or_else(|| Error::Refused("S2 backpack grid-cell limit would be exceeded".to_owned()))?;
    let occupied = save
        .index
        .grid_cells
        .iter()
        .map(|cell| (cell.x, cell.y))
        .collect::<HashSet<_>>();
    let offsets = stash_cells
        .iter()
        .map(|cell| (cell.x.saturating_sub(min_x), cell.y.saturating_sub(min_y)))
        .collect::<Vec<_>>();
    let placement = first_free_placement(&occupied, &offsets, width, height);
    let (place_x, place_y) =
        placement.ok_or_else(|| Error::Refused("S2 backpack has no fitting free cells".to_owned()))?;

    let added_cells = footprint
        .checked_mul(8)
        .ok_or_else(|| Error::damaged("S2 added grid-cell byte count overflows"))?;
    let inserted_bytes = 4_usize
        .checked_add(added_cells)
        .ok_or_else(|| Error::damaged("S2 stash move insertion size overflows"))?;
    let removed_bytes = stash_cells
        .len()
        .checked_mul(8)
        .ok_or_else(|| Error::damaged("S2 removed stash-cell byte count overflows"))?;
    let final_len = image
        .len()
        .checked_add(inserted_bytes)
        .and_then(|length| length.checked_sub(removed_bytes))
        .ok_or_else(|| Error::damaged("S2 final image length overflows"))?;
    if final_len > MAXIMUM_UNPACKED_SIZE {
        return Err(Error::Refused(
            "S2 edit would exceed the maximum unpacked image size".to_owned(),
        ));
    }
    let original_range_count = changed_ranges.len();
    image
        .try_reserve(inserted_bytes)
        .map_err(|error| Error::Refused(format!("cannot reserve S2 working image: {error}")))?;

    write_u16_at(
        image,
        save.index.owned_count_offset,
        u16::try_from(new_owned_count).map_err(|_| Error::damaged("S2 owned count does not fit u16"))?,
    )?;
    insert_bytes(image, save.index.grid_count_offset, &handle.to_le_bytes())?;
    let player_grid_count_offset = save
        .index
        .grid_count_offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("S2 player grid count offset overflows"))?;
    write_u16_at(
        image,
        player_grid_count_offset,
        u16::try_from(new_grid_count).map_err(|_| Error::damaged("S2 grid count does not fit u16"))?,
    )?;
    let player_grid_insert_offset = save
        .index
        .grid_end_offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("S2 player grid insertion offset overflows"))?;
    let mut added_grid_bytes = Vec::with_capacity(added_cells);
    for cell in &stash_cells {
        let x = place_x.saturating_add(cell.x.saturating_sub(min_x));
        let y = place_y.saturating_add(cell.y.saturating_sub(min_y));
        added_grid_bytes.extend_from_slice(&handle.to_le_bytes());
        added_grid_bytes.extend_from_slice(&x.to_le_bytes());
        added_grid_bytes.extend_from_slice(&y.to_le_bytes());
    }
    insert_bytes(image, player_grid_insert_offset, &added_grid_bytes)?;

    let shifted_stash_handles = stash
        .owned_handles_offset
        .checked_add(inserted_bytes)
        .ok_or_else(|| Error::damaged("S2 shifted stash handle offset overflows"))?;
    let shifted_stash_grid_count = stash
        .grid_count_offset
        .checked_add(inserted_bytes)
        .ok_or_else(|| Error::damaged("S2 shifted stash grid count offset overflows"))?;
    let shifted_stash_grid = stash
        .grid_offset
        .checked_add(inserted_bytes)
        .ok_or_else(|| Error::damaged("S2 shifted stash grid offset overflows"))?;
    let new_stash_grid_count = stash
        .grid_count
        .checked_sub(stash_cells.len())
        .ok_or_else(|| Error::damaged("S2 stash grid count underflows"))?;
    write_u32_at(
        image,
        shifted_stash_handles
            .checked_add(stash_owned_index.saturating_mul(4))
            .ok_or_else(|| Error::damaged("S2 moved stash handle offset overflows"))?,
        u32::MAX,
    )?;
    write_u16_at(
        image,
        shifted_stash_grid_count,
        u16::try_from(new_stash_grid_count).map_err(|_| Error::damaged("S2 stash grid count does not fit u16"))?,
    )?;
    let mut cell_indexes = stash
        .grid_cells
        .iter()
        .enumerate()
        .filter_map(|(index, cell)| (cell.handle == handle).then_some(index))
        .collect::<Vec<_>>();
    cell_indexes.sort_unstable_by(|left, right| right.cmp(left));
    for cell_index in cell_indexes {
        let start = shifted_stash_grid
            .checked_add(cell_index.saturating_mul(8))
            .ok_or_else(|| Error::damaged("S2 stash cell removal offset overflows"))?;
        remove_bytes(image, start, 8)?;
    }

    let shifted_bytes = inserted_bytes
        .checked_sub(removed_bytes)
        .ok_or_else(|| Error::damaged("S2 stash transfer unexpectedly shrank the image"))?;
    let moved_record_offset = moved_object_offset(
        record.record_offset,
        save.index.owned_count_offset..stash.grid_end_offset,
        shifted_bytes,
    )?;
    let position_x_offset = moved_record_offset.saturating_add(11);
    let position_y_offset = moved_record_offset.saturating_add(13);
    let record_flag_offset = moved_record_offset.saturating_add(15);
    let flags_offset = moved_record_offset.saturating_add(28);
    write_u16_at(image, position_x_offset, place_x)?;
    write_u16_at(image, position_y_offset, place_y)?;
    write_u8_at(image, record_flag_offset, 0)?;
    let flags = read_u8(image, flags_offset)?;
    write_u8_at(image, flags_offset, flags & !0x08)?;

    let span_start = save.index.owned_count_offset;
    let span_end = stash.grid_end_offset;
    for range in changed_ranges.iter_mut().take(original_range_count) {
        if range.before.start >= span_end {
            range.after.start = range
                .after
                .start
                .checked_add(shifted_bytes)
                .ok_or_else(|| Error::damaged("S2 changed range start overflows after stash insertion"))?;
            range.after.end = range
                .after
                .end
                .checked_add(shifted_bytes)
                .ok_or_else(|| Error::damaged("S2 changed range end overflows after stash insertion"))?;
        } else if range.before.start < span_end
            && range.before.end > span_start
            && (range.before.start < span_start || range.before.end > span_end)
        {
            return Err(Error::damaged("S2 change range crosses the stash transfer window"));
        }
    }
    changed_ranges.truncate(original_range_count);
    changed_ranges.retain(|range| range.before.end <= span_start || range.before.start >= span_end);
    changed_ranges.push(ChangedRange {
        before: span_start..span_end,
        after: span_start
            ..span_end
                .checked_add(shifted_bytes)
                .ok_or_else(|| Error::damaged("S2 changed range end overflows after stash insertion"))?,
    });
    let old_record_position_x = record
        .record_offset
        .checked_add(11)
        .ok_or_else(|| Error::damaged("S2 original position offset overflows"))?;
    let old_record_flag = record
        .record_offset
        .checked_add(15)
        .ok_or_else(|| Error::damaged("S2 original record flag offset overflows"))?;
    let old_flags = record
        .record_offset
        .checked_add(28)
        .ok_or_else(|| Error::damaged("S2 original flags offset overflows"))?;
    changed_ranges.push(ChangedRange {
        before: old_record_position_x
            ..old_record_flag
                .checked_add(1)
                .ok_or_else(|| Error::damaged("S2 original position range overflows"))?,
        after: position_x_offset
            ..record_flag_offset
                .checked_add(1)
                .ok_or_else(|| Error::damaged("S2 position range overflows"))?,
    });
    changed_ranges.push(ChangedRange {
        before: old_flags
            ..old_flags
                .checked_add(1)
                .ok_or_else(|| Error::damaged("S2 original flags range overflows"))?,
        after: flags_offset
            ..flags_offset
                .checked_add(1)
                .ok_or_else(|| Error::damaged("S2 flags range overflows"))?,
    });
    Ok(())
}

fn pack_and_verify_s2_image(
    source_image: &[u8],
    image: &[u8],
    changed_ranges: &[ChangedRange],
) -> Result<(Vec<u8>, S2Container)> {
    if image.is_empty() || image.len() > MAXIMUM_UNPACKED_SIZE {
        return Err(Error::Refused(
            "S2 image is outside the supported size range".to_owned(),
        ));
    }
    verify_unchanged_outside_ranges(source_image, image, changed_ranges)?;
    let compressed = sse_codecs::kraken_encode::compress(image);
    pack_and_verify_s2_image_with_stream(image, &compressed)
}

fn pack_and_verify_s2_image_with_stream(image: &[u8], compressed: &[u8]) -> Result<(Vec<u8>, S2Container)> {
    if compressed.len() < image.len().saturating_add(2) {
        if let Ok(packed) = pack_s2_container(image, compressed) {
            if let Ok(verified) = S2Container::from_bytes(&packed) {
                if verified.image() == image {
                    return Ok((packed, verified));
                }
            }
        }
    }

    let block_count = image
        .len()
        .checked_add(KRAKEN_BLOCK_SIZE - 1)
        .ok_or_else(|| Error::damaged("S2 stored Kraken block count overflows"))?
        / KRAKEN_BLOCK_SIZE;
    let headers_size = block_count
        .checked_mul(2)
        .ok_or_else(|| Error::damaged("S2 stored Kraken header length overflows"))?;
    let stored_capacity = image
        .len()
        .checked_add(headers_size)
        .ok_or_else(|| Error::damaged("S2 stored Kraken stream length overflows"))?;
    let mut stored = Vec::with_capacity(stored_capacity);
    for block in image.chunks(KRAKEN_BLOCK_SIZE) {
        stored.extend_from_slice(&[0xcc, 0x06]);
        stored.extend_from_slice(block);
    }
    let packed = pack_s2_container(image, &stored)?;
    let verified = S2Container::from_bytes(&packed)?;
    if verified.image() != image {
        return Err(Error::damaged("S2 write verification differs from the complete image"));
    }
    Ok((packed, verified))
}

fn pack_s2_container(image: &[u8], stream: &[u8]) -> Result<Vec<u8>> {
    let unpacked_size =
        u32::try_from(image.len()).map_err(|_| Error::Refused("S2 image exceeds u32 length".to_owned()))?;
    let capacity = stream
        .len()
        .checked_add(8)
        .ok_or_else(|| Error::damaged("S2 output container length overflows"))?;
    let mut packed = Vec::with_capacity(capacity);
    packed.extend_from_slice(&unpacked_size.to_le_bytes());
    packed.extend_from_slice(stream);
    let crc = sse_codecs::crc32::crc32(&packed);
    packed.extend_from_slice(&crc.to_le_bytes());
    Ok(packed)
}

/// Checks the written save's structure against what each change asked for, and that nothing else moved.
///
/// The image comparison in `pack_and_verify_s2_image` only proves the bytes equal the intended image; this
/// reads the result back as a save: the money, the stack count, a moved item's footprint and owners, and every
/// item the change set did not name.
#[cfg(test)]
fn verify_changes_readback(source: &S2Save, verified: &S2Save, changes: &[S2Change]) -> Result<()> {
    verify_changes_readback_with_items(source, &source.items(), verified, &verified.items(), changes)
}

fn verify_changes_readback_with_items(
    source: &S2Save,
    source_items: &[S2InventoryItem],
    verified: &S2Save,
    verified_items: &[S2InventoryItem],
    changes: &[S2Change],
) -> Result<()> {
    let damaged = |message: &str| Error::damaged(format!("S2 write read-back: {message}"));
    let mut named = HashSet::new();
    for change in changes {
        match *change {
            S2Change::SetMoney { new_value, .. } => {
                if verified.money()? != new_value {
                    return Err(damaged("money differs from the requested value"));
                }
            }
            S2Change::SetStackCount { handle, count } => {
                let item = verified_items
                    .iter()
                    .find(|item| item.handle == handle)
                    .ok_or_else(|| damaged("stack item disappeared"))?;
                if item.count != count {
                    return Err(damaged("stack count differs from the requested value"));
                }
                named.insert(handle);
            }
            S2Change::SetDurability { handle, .. } => {
                named.insert(handle);
            }
            S2Change::MoveStashToBackpack { handle } => {
                let owners = verified
                    .index
                    .owned_handles()
                    .iter()
                    .filter(|owned| **owned == handle)
                    .count();
                if owners != 1 {
                    return Err(damaged("moved item is not owned exactly once by the backpack"));
                }
                if verified.unresolved_handles.contains(&handle) {
                    return Err(damaged("moved item has unresolved backpack cells"));
                }
                let stash_before = source.stash()?;
                let footprint = |cells: &[S2GridCell]| {
                    let min_x = cells.iter().map(|cell| cell.x).min().unwrap_or_default();
                    let min_y = cells.iter().map(|cell| cell.y).min().unwrap_or_default();
                    let mut shape = cells
                        .iter()
                        .map(|cell| (cell.x.saturating_sub(min_x), cell.y.saturating_sub(min_y)))
                        .collect::<Vec<_>>();
                    shape.sort_unstable();
                    shape
                };
                let stash_cells = stash_before
                    .grid_cells()
                    .iter()
                    .filter(|cell| cell.handle == handle)
                    .copied()
                    .collect::<Vec<_>>();
                let backpack_cells = verified
                    .index
                    .grid_cells()
                    .iter()
                    .filter(|cell| cell.handle == handle)
                    .copied()
                    .collect::<Vec<_>>();
                if backpack_cells.is_empty() || footprint(&backpack_cells) != footprint(&stash_cells) {
                    return Err(damaged("moved item's backpack cells do not match its stash footprint"));
                }
                if verified.stash()?.live_handles().contains(&handle) {
                    return Err(damaged("moved item is still live in the stash"));
                }
                named.insert(handle);
            }
        }
    }
    // Offsets move when a stash item is inserted into the backpack; every other field must be equal.
    let projection = |items: &[S2InventoryItem]| {
        items
            .iter()
            .filter(|item| !named.contains(&item.handle))
            .cloned()
            .map(|mut item| {
                item.record_offset = 0;
                item.count_offset = 0;
                item.condition_offset = None;
                item
            })
            .collect::<Vec<_>>()
    };
    if projection(source_items) != projection(verified_items) {
        return Err(damaged("an item the change set did not name changed"));
    }
    Ok(())
}

fn condition_pairs(items: &[S2InventoryItem]) -> Vec<(u32, Option<f32>)> {
    items.iter().map(|item| (item.handle, item.condition)).collect()
}

fn verify_durability_values(
    before: &[(u32, Option<f32>)],
    after: &[(u32, Option<f32>)],
    changes: &[(u32, f32)],
) -> Result<()> {
    let mut expected = condition_map(before)?;
    let actual = condition_map(after)?;
    let mut changed_handles = HashSet::with_capacity(changes.len());
    for (handle, condition) in changes {
        if !changed_handles.insert(*handle) {
            return Err(Error::Refused(
                "S2 durability handle is edited more than once".to_owned(),
            ));
        }
        if !expected.contains_key(handle) {
            return Err(Error::Refused(
                "S2 durability handle is missing or ambiguous".to_owned(),
            ));
        }
        expected.insert(*handle, Some(*condition));
    }
    if expected != actual {
        return Err(Error::damaged(
            "S2 durability read-back differs from the requested and unchanged item conditions",
        ));
    }
    Ok(())
}

fn condition_map(values: &[(u32, Option<f32>)]) -> Result<BTreeMap<u32, Option<f32>>> {
    let mut conditions = BTreeMap::new();
    for (handle, condition) in values {
        if conditions.insert(*handle, *condition).is_some() {
            return Err(Error::damaged(
                "S2 durability read-back contains duplicate item handles",
            ));
        }
    }
    Ok(conditions)
}

fn write_u8_at(bytes: &mut [u8], offset: usize, value: u8) -> Result<()> {
    *bytes
        .get_mut(offset)
        .ok_or_else(|| Error::damaged("S2 write offset is out of bounds"))? = value;
    Ok(())
}

fn write_u16_at(bytes: &mut [u8], offset: usize, value: u16) -> Result<()> {
    let end = checked_add(offset, 2, "S2 write range overflows")?;
    bytes
        .get_mut(offset..end)
        .ok_or_else(|| Error::damaged("S2 write range is out of bounds"))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u32_at(bytes: &mut [u8], offset: usize, value: u32) -> Result<()> {
    let end = checked_add(offset, 4, "S2 write range overflows")?;
    bytes
        .get_mut(offset..end)
        .ok_or_else(|| Error::damaged("S2 write range is out of bounds"))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn insert_bytes(image: &mut Vec<u8>, offset: usize, inserted: &[u8]) -> Result<()> {
    if offset > image.len() {
        return Err(Error::damaged("S2 insertion offset is out of bounds"));
    }
    let previous_len = image.len();
    let new_len = previous_len
        .checked_add(inserted.len())
        .ok_or_else(|| Error::damaged("S2 image length overflows during insertion"))?;
    image.resize(new_len, 0);
    image.copy_within(offset..previous_len, offset.saturating_add(inserted.len()));
    let end = offset.saturating_add(inserted.len());
    image
        .get_mut(offset..end)
        .ok_or_else(|| Error::damaged("S2 insertion range is out of bounds"))?
        .copy_from_slice(inserted);
    Ok(())
}

fn remove_bytes(image: &mut Vec<u8>, offset: usize, length: usize) -> Result<()> {
    let end = checked_add(offset, length, "S2 removal range overflows")?;
    if image.get(offset..end).is_none() {
        return Err(Error::damaged("S2 removal range is out of bounds"));
    }
    let previous_len = image.len();
    image.copy_within(end..previous_len, offset);
    image.truncate(previous_len.saturating_sub(length));
    Ok(())
}

fn find_all(bytes: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || bytes.len() < needle.len() {
        return Vec::new();
    }
    bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
        .collect()
}

fn locate_legacy_wallet(raw: &[u8]) -> Result<usize> {
    let ids = find_all(raw, &LEGACY_CONTAINER_ID);
    let [anchor] = ids.as_slice() else {
        return Err(Error::damaged("S2 wallet anchor is missing or ambiguous"));
    };
    let mut offset = checked_add(*anchor, LEGACY_CONTAINER_ID.len(), "S2 legacy wallet offset overflow")?;
    let pair_count = usize::from(read_u16(raw, offset)?);
    if pair_count > 64 {
        return Err(Error::damaged("S2 legacy sub-container count exceeds its limit"));
    }
    offset = checked_add(offset, 2, "S2 legacy wallet offset overflow")?;
    let pairs_size = pair_count
        .checked_mul(8)
        .ok_or_else(|| Error::damaged("S2 legacy pair byte count overflow"))?;
    require_range(raw, offset, pairs_size, "S2 legacy sub-container list is truncated")?;
    for pair in 0..pair_count {
        let pair_offset = checked_add(
            offset,
            pair.checked_mul(8)
                .ok_or_else(|| Error::damaged("S2 legacy pair offset overflow"))?,
            "S2 legacy pair offset overflow",
        )?;
        let type_offset = checked_add(pair_offset, 3, "S2 legacy type offset overflow")?;
        let count_offset = checked_add(pair_offset, 4, "S2 legacy count offset overflow")?;
        if read_u8(raw, type_offset)? != 0x38 || read_u32(raw, count_offset)? != 1 {
            return Err(Error::damaged("S2 legacy sub-container shape is invalid"));
        }
    }
    offset = checked_add(offset, pairs_size, "S2 legacy header offset overflow")?;
    require_range(raw, offset, 10, "S2 legacy wallet header is truncated")?;
    let zero = read_u32(raw, offset)?;
    let number_offset = checked_add(offset, 4, "S2 legacy number offset overflow")?;
    let number = read_u32(raw, number_offset)?;
    let repeated_offset = checked_add(offset, 8, "S2 legacy repeated number offset overflow")?;
    let repeated = read_u16(raw, repeated_offset)?;
    if zero != 0 || number == 0 || number > u32::from(u16::MAX) || u32::from(repeated) != number {
        return Err(Error::damaged("S2 legacy wallet header shape is invalid"));
    }
    checked_add(offset, 10, "S2 legacy money offset overflow")
}

fn validate_owned_handles(handles: &[u32], legacy: bool) -> Result<()> {
    let judged = if legacy {
        handles.iter().filter(|handle| **handle != u32::MAX).count()
    } else {
        handles.len()
    };
    let game_handles = handles.iter().filter(|handle| **handle >> 24 == 0x30).count();
    if game_handles.saturating_mul(2) < judged {
        return Err(Error::damaged("S2 owned-handle array does not resemble player handles"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        apply_changes_to_image, classify_refusal, first_free_placement, moved_object_offset, pack_and_verify_s2_image,
        transfer_stash_items_to_backpack, validate_owned_handles, S2Change, S2Container, S2InventoryIndex, S2Save,
        S2SkipReason, S2StashLayout, GRID_WIDTH,
    };
    use sse_codecs::crc32;
    use sse_core::Error;
    use std::collections::HashSet;

    const SYNTHETIC_SAVE: &[u8] = include_bytes!("../../../fixtures/synthetic/synthetic-s2.sav");
    const SYNTHETIC_RAW: &[u8] = include_bytes!("../../../fixtures/synthetic/synthetic-s2.raw");
    const SYNTHETIC_STASH_RAW: &[u8] = include_bytes!("../../../fixtures/synthetic/synthetic-s2-stash.raw");
    const WRITER_S2_MONEY_SOURCE: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-source.sav");
    const WRITER_S2_MONEY_EXPECTED: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-expected.raw");
    const WRITER_S2_STACK_SOURCE: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-stacks/s2-stacks-source.sav");
    const WRITER_S2_STACK_EXPECTED: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-stacks/s2-stacks-expected.raw");
    const WRITER_S2_ARMOR_MONEY_SOURCE: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-equipment/s2-equipment-armor-money-source.sav");
    const WRITER_S2_ARMOR_MONEY_EXPECTED: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-equipment/s2-equipment-armor-money-expected.raw");
    const WRITER_S2_WEAPON_SOURCE: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-equipment/s2-equipment-weapon-source.sav");
    const WRITER_S2_WEAPON_EXPECTED: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-equipment/s2-equipment-weapon-expected.raw");
    const WRITER_S2_STASH_SOURCE: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.raw");
    const WRITER_S2_STASH_EXPECTED: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-stash/s2-stash-expected.raw");
    const WRITER_S2_STASH_PACKED_SOURCE: &[u8] =
        include_bytes!("../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.sav");

    /// A money change from the balance the save has now to `new_value`.
    fn money_change(save: &S2Save, new_value: u32) -> S2Change {
        S2Change::SetMoney {
            old_value: save.money().unwrap_or_default(),
            new_value,
        }
    }

    fn next_fuzz_state(state: &mut u64) -> u64 {
        *state ^= state.wrapping_shl(13);
        *state ^= state.wrapping_shr(7);
        *state ^= state.wrapping_shl(17);
        *state
    }

    #[test]
    fn owned_handle_validation_accepts_one_to_three_game_handles() {
        for count in 1..=3_u32 {
            let handles = (0..count)
                .map(|handle| 0x3000_0001_u32.saturating_add(handle))
                .collect::<Vec<_>>();
            assert!(
                validate_owned_handles(&handles, false).is_ok(),
                "{count} correctly shaped player handles should not be rejected"
            );
        }
    }

    #[test]
    fn unpacked_image_mutation_repack_read_and_write_fuzz_smoke() {
        let Ok(source) = S2Save::from_bytes(WRITER_S2_MONEY_SOURCE) else {
            panic!("S2 money writer fixture should parse");
        };
        let mut random = 0x0053_329c_e42a_7401_u64;
        let mut repacked_count = 0_usize;
        let mut parsed_count = 0_usize;
        let mut written_count = 0_usize;

        for case in 0..512_usize {
            let mut raw = source.container().image().to_vec();
            if case.checked_rem(2).unwrap_or_default() == 0 {
                let money = u32::try_from(next_fuzz_state(&mut random) % 2_000_000_001_u64).unwrap_or_default();
                let start = source.index().money_offset();
                let end = start.saturating_add(4);
                let Some(range) = raw.get_mut(start..end) else {
                    panic!("money range should fit the unpacked fixture");
                };
                range.copy_from_slice(&money.to_le_bytes());
            } else if !raw.is_empty() {
                let length = u64::try_from(raw.len()).unwrap_or(u64::MAX);
                let offset = usize::try_from(next_fuzz_state(&mut random) % length).unwrap_or_default();
                let shift = u32::try_from(next_fuzz_state(&mut random) % 8).unwrap_or_default();
                if let Some(byte) = raw.get_mut(offset) {
                    *byte ^= 1_u8.checked_shl(shift).unwrap_or(1);
                }
            }

            let stream = sse_codecs::kraken_encode::compress(&raw);
            let packed = pack_kraken_stream(&raw, &stream);
            repacked_count = repacked_count.saturating_add(1);
            let Ok(parsed) = S2Save::from_bytes(&packed) else {
                continue;
            };
            parsed_count = parsed_count.saturating_add(1);
            let old_value = parsed.money().unwrap_or_default();
            let new_value = if old_value == 1 { 2 } else { 1 };
            let Ok(written) = parsed.write_changes(&[money_change(&parsed, new_value)]) else {
                continue;
            };
            let verified = S2Save::from_bytes(&written);
            assert!(verified.is_ok(), "writer output should parse");
            let Ok(verified) = verified else { continue };
            assert_eq!(verified.money().unwrap_or_default(), new_value);
            written_count = written_count.saturating_add(1);
        }

        assert_eq!(repacked_count, 512);
        assert!(parsed_count > 0, "mutated unpacked images should reach the reader");
        assert!(written_count > 0, "at least one fuzz case should reach the writer");
    }

    #[test]
    fn public_s2_writer_round_trips_money_stack_and_crc() {
        let parsed = S2Save::from_bytes(WRITER_S2_STACK_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let original_image = parsed.container().image().to_vec();
        let original_sha = sse_codecs::sha256::sha256_hex(WRITER_S2_STACK_SOURCE);
        let changes = [
            money_change(&parsed, 876_543),
            S2Change::SetStackCount {
                handle: 0x3000_0001,
                count: 7,
            },
        ];

        let packed = parsed.write_changes(&changes);
        assert!(packed.is_ok(), "writer returned {packed:?}");
        let Ok(packed) = packed else { return };
        let verified = S2Save::from_bytes(&packed);
        assert!(verified.is_ok());
        let Ok(verified) = verified else { return };
        assert_eq!(verified.money().unwrap_or_default(), 876_543);
        assert_eq!(
            verified
                .items()
                .iter()
                .find(|item| item.handle == 0x3000_0001)
                .map(|item| item.count),
            Some(7)
        );
        let trailer_offset = packed.len().saturating_sub(4);
        let stored_crc = super::read_u32(&packed, trailer_offset);
        let computed_crc = packed.get(..trailer_offset).map(crc32::crc32);
        assert!(matches!((stored_crc, computed_crc), (Ok(stored), Some(computed)) if stored == computed));
        assert_eq!(parsed.container().image(), original_image);
        assert_eq!(sse_codecs::sha256::sha256_hex(WRITER_S2_STACK_SOURCE), original_sha);
    }

    #[test]
    #[ignore = "manual Release packed S2 writer throughput measurement"]
    fn release_s2_packed_writer_throughput_measurement() {
        let parsed = S2Save::from_bytes(WRITER_S2_STACK_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let iterations = 10_000_u32;
        let started = std::time::Instant::now();
        let mut checksum = 0_u64;
        for value in 0..iterations {
            let packed = parsed.write_changes(&[
                money_change(&parsed, value),
                S2Change::SetStackCount {
                    handle: 0x3000_0001,
                    count: 7,
                },
            ]);
            assert!(packed.is_ok());
            let Ok(packed) = packed else { return };
            checksum = checksum.wrapping_add(u64::try_from(packed.len()).unwrap_or(u64::MAX));
        }
        let elapsed = started.elapsed();
        println!(
            "S2 packed writer: {iterations} money+stack edits of 350 bytes in {elapsed:?}; {:.3} us/edit; output bytes {checksum}",
            elapsed.as_secs_f64() * 1_000_000.0 / f64::from(iterations)
        );
        assert!(checksum > 0);
    }

    #[test]
    fn s2_money_writer_matches_the_reference_image_and_writes_a_container() {
        let parsed = S2Save::from_bytes(WRITER_S2_MONEY_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let result = apply_changes_to_image(&parsed, &[money_change(&parsed, 876_543)]);
        assert_eq!(result.map(|(image, _)| image), Ok(WRITER_S2_MONEY_EXPECTED.to_vec()));
        let packed = parsed.write_changes(&[money_change(&parsed, 876_543)]);
        assert!(packed.is_ok());
        let Ok(packed) = packed else { return };
        let verified = S2Save::from_bytes(&packed);
        assert!(verified.is_ok());
        let Ok(verified) = verified else { return };
        assert_eq!(verified.money().unwrap_or_default(), 876_543);
        assert_eq!(parsed.money().unwrap_or_default(), 100);
    }

    #[test]
    fn s2_money_write_refuses_a_stale_old_value() {
        let parsed = S2Save::from_bytes(WRITER_S2_MONEY_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let current = parsed.money().unwrap_or_default();

        let stale = S2Change::SetMoney {
            old_value: current.saturating_add(1),
            new_value: 5,
        };
        assert!(apply_changes_to_image(&parsed, &[stale]).is_err_and(|error| error.to_string().contains("changed")));

        let fresh = S2Change::SetMoney {
            old_value: current,
            new_value: 5,
        };
        assert!(apply_changes_to_image(&parsed, &[fresh]).is_ok());
    }

    #[test]
    fn s2_money_writer_enforces_the_reference_upper_bound() {
        let parsed = S2Save::from_bytes(WRITER_S2_MONEY_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };

        assert!(apply_changes_to_image(&parsed, &[money_change(&parsed, 2_000_000_000)]).is_ok());
        assert!(matches!(
            apply_changes_to_image(&parsed, &[money_change(&parsed, 2_000_000_001)]),
            Err(Error::Refused(_))
        ));
    }

    #[test]
    fn s2_stack_writer_enforces_the_reference_count_ceiling() {
        let parsed = S2Save::from_bytes(WRITER_S2_STACK_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };

        assert!(matches!(
            apply_changes_to_image(
                &parsed,
                &[S2Change::SetStackCount {
                    handle: 0x3000_0001,
                    count: 1_000_001,
                }]
            ),
            Err(Error::Refused(_))
        ));
    }

    #[test]
    fn s2_writer_packs_and_verifies_changed_ranges() {
        let parsed = S2Save::from_bytes(WRITER_S2_MONEY_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let image = apply_changes_to_image(&parsed, &[money_change(&parsed, 876_543)]);
        assert!(image.is_ok());
        let Ok((image, changed_ranges)) = image else { return };
        let packed = pack_and_verify_s2_image(parsed.container().image(), &image, &changed_ranges);
        assert!(packed.is_ok());
        let Ok((packed, reread)) = packed else { return };
        assert_eq!(reread.image(), image);
        assert_eq!(super::read_u32(&packed, 0), Ok(350));
        let trailer = packed.len().saturating_sub(4);
        let expected_crc = packed.get(..trailer).map(crc32::crc32);
        assert!(matches!(
            (super::read_u32(&packed, trailer), expected_crc),
            (Ok(stored), Some(computed)) if stored == computed
        ));
    }

    #[test]
    fn s2_writer_uses_a_verified_stored_block_when_kraken_rejects_the_candidate() {
        let image = b"synthetic S2 image that must survive a rejected compressed candidate";
        let rejected_candidate = [0x8c, 0x06, 0x00, 0x00, 0x05, 0x80, 0x00, 0x03, 0x20, 0x00, 0x00];

        let packed = super::pack_and_verify_s2_image_with_stream(image, &rejected_candidate);
        assert!(packed.is_ok());
        let Ok((packed, verified)) = packed else { return };
        assert_eq!(verified.image(), image);
        assert!(packed.get(4..6).is_some_and(|header| header == [0xcc, 0x06]));
        let trailer = packed.len().saturating_sub(4);
        assert!(matches!(
            (super::read_u32(&packed, trailer), packed.get(..trailer).map(crc32::crc32)),
            (Ok(stored), Some(computed)) if stored == computed
        ));
    }

    #[test]
    fn s2_writer_emits_a_stored_header_for_every_kraken_block() {
        let mut image = vec![0_u8; 0x4_0001];
        for (index, byte) in image.iter_mut().enumerate() {
            *byte = u8::try_from(index.wrapping_mul(37) & 0xff).unwrap_or_default();
        }
        let rejected_candidate = vec![0_u8; image.len().saturating_add(3)];

        let packed = super::pack_and_verify_s2_image_with_stream(&image, &rejected_candidate);
        assert!(packed.is_ok());
        let Ok((packed, verified)) = packed else { return };
        assert_eq!(verified.image(), image);
        assert!(packed.get(4..6).is_some_and(|header| header == [0xcc, 0x06]));
        assert!(packed
            .get(4 + 2 + 0x4_0000..4 + 2 + 0x4_0000 + 2)
            .is_some_and(|header| header == [0xcc, 0x06]));
    }

    #[test]
    fn s2_packer_rejects_a_collateral_image_change_before_compression() {
        let parsed = S2Save::from_bytes(WRITER_S2_MONEY_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let prepared = apply_changes_to_image(&parsed, &[money_change(&parsed, 876_543)]);
        assert!(prepared.is_ok());
        let Ok((image, changed_ranges)) = prepared else { return };
        let mut corrupted = image;
        let unchanged_offset =
            (0..corrupted.len()).find(|offset| changed_ranges.iter().all(|range| !range.after.contains(offset)));
        assert!(unchanged_offset.is_some());
        let Some(unchanged_offset) = unchanged_offset else {
            return;
        };
        let Some(byte) = corrupted.get_mut(unchanged_offset) else {
            return;
        };
        *byte ^= 1;

        assert!(
            pack_and_verify_s2_image(parsed.container().image(), &corrupted, &changed_ranges)
                .is_err_and(|error| error.to_string().contains("outside declared changed ranges"))
        );
    }

    #[test]
    #[ignore = "manual Release raw-writer throughput measurement"]
    fn release_s2_raw_writer_throughput_measurement() {
        let parsed = S2Save::from_bytes(WRITER_S2_MONEY_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let iterations = 100_000_u32;
        let started = std::time::Instant::now();
        let mut checksum = 0_u64;
        for _ in 0..iterations {
            let changed = apply_changes_to_image(&parsed, &[money_change(&parsed, 876_543)]);
            assert!(changed.is_ok());
            let Ok((image, _)) = changed else { return };
            checksum = checksum.wrapping_add(u64::from(super::read_u32(&image, 50).unwrap_or_default()));
        }
        let elapsed = started.elapsed();
        println!(
            "S2 money writer: {iterations} raw edits of 350 bytes in {elapsed:?}; {:.3} us/edit; checksum {checksum}",
            elapsed.as_secs_f64() * 1_000_000.0 / f64::from(iterations)
        );
        assert_eq!(checksum, 87_654_300_000);
    }

    #[test]
    fn s2_stack_writer_matches_reference_count_and_scaled_weight() {
        let parsed = S2Save::from_bytes(WRITER_S2_STACK_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let result = apply_changes_to_image(
            &parsed,
            &[S2Change::SetStackCount {
                handle: 0x3000_0001,
                count: 7,
            }],
        );
        assert_eq!(result.map(|(image, _)| image), Ok(WRITER_S2_STACK_EXPECTED.to_vec()));
    }

    #[test]
    fn s2_stack_writer_refuses_a_handle_that_is_not_uniquely_owned() {
        let parsed = S2Save::from_bytes(WRITER_S2_STACK_SOURCE);
        assert!(parsed.is_ok());
        let Ok(mut parsed) = parsed else { return };
        let Some(template) = parsed.objects.records.first().copied() else {
            return;
        };
        let Some(handle) = (0x3000_0000..=0x3000_FFFF)
            .find(|handle| !parsed.index.owned_handles.contains(handle) && parsed.objects.unique(*handle).is_none())
        else {
            return;
        };
        let record_index = parsed.objects.records.len();
        parsed
            .objects
            .records
            .push(super::S2ObjectRecordIndex { handle, ..template });
        parsed.objects.by_handle.insert(handle, vec![record_index]);

        assert!(apply_changes_to_image(
            &parsed,
            &[S2Change::SetStackCount {
                handle,
                count: template.count.saturating_add(1)
            }],
        )
        .is_err_and(|error| error.to_string().contains("not uniquely owned")));
    }

    #[test]
    fn s2_stack_writer_refuses_an_owned_handle_outside_the_backpack_grid() {
        let parsed = S2Save::from_bytes(WRITER_S2_STACK_SOURCE);
        assert!(parsed.is_ok());
        let Ok(mut parsed) = parsed else { return };
        let Some(template) = parsed.objects.records.first().copied() else {
            return;
        };
        let Some(handle) = (0x3000_0000..=0x3000_FFFF)
            .find(|handle| !parsed.index.owned_handles.contains(handle) && parsed.objects.unique(*handle).is_none())
        else {
            return;
        };
        let record_index = parsed.objects.records.len();
        parsed
            .objects
            .records
            .push(super::S2ObjectRecordIndex { handle, ..template });
        parsed.objects.by_handle.insert(handle, vec![record_index]);
        parsed.index.owned_handles.push(handle);

        assert!(apply_changes_to_image(
            &parsed,
            &[S2Change::SetStackCount {
                handle,
                count: template.count.saturating_add(1)
            }],
        )
        .is_err_and(|error| error.to_string().contains("backpack grid")));
    }

    #[test]
    fn stash_move_readback_rejects_a_shifted_backpack_cell() -> Result<(), Error> {
        let handle = 0x3000_0010;
        let changes = [S2Change::MoveStashToBackpack { handle }];
        let source = S2Save::from_bytes(WRITER_S2_STASH_PACKED_SOURCE)?;
        let good = S2Save::from_bytes(&source.write_changes(&changes)?)?;
        let cell_index = good
            .index()
            .grid_cells()
            .iter()
            .position(|cell| cell.handle == handle)
            .ok_or_else(|| Error::damaged("moved item should occupy backpack cells"))?;
        let cell = good
            .index()
            .grid_cells()
            .get(cell_index)
            .copied()
            .ok_or_else(|| Error::damaged("moved cell index should exist"))?;
        let x_offset = good.index().grid_offset() + cell_index * 8 + 4;
        let mut raw = good.container().image().to_vec();
        raw.get_mut(x_offset..x_offset + 2)
            .ok_or_else(|| Error::damaged("grid cell should be inside the image"))?
            .copy_from_slice(&cell.x.saturating_add(1).to_le_bytes());
        let corrupted = S2Save::from_bytes(&pack_raw(&raw))?;

        match super::verify_changes_readback(&source, &corrupted, &changes) {
            Err(Error::Damaged(_)) => Ok(()),
            Err(error) => Err(error),
            Ok(()) => Err(Error::damaged(
                "a backpack cell moved away from the stash footprint must be rejected",
            )),
        }
    }

    #[test]
    fn name_tables_with_two_disagreeing_candidates_are_refused() -> Result<(), Error> {
        fn block(names: &[&str]) -> Vec<u8> {
            let mut bytes = u16::try_from(names.len()).unwrap_or_default().to_le_bytes().to_vec();
            for name in names {
                bytes.extend_from_slice(&u16::try_from(name.len()).unwrap_or_default().to_le_bytes());
                bytes.extend_from_slice(name.as_bytes());
            }
            bytes
        }
        let mut raw = block(&["GunAK74_ST", "GunAK74_MagA"]);
        raw.extend_from_slice(&[0xff, 0xff]);
        raw.extend_from_slice(&block(&["GunAK74_ST", "GunAK74_MagB"]));
        let mut objects = super::S2ObjectIndex::default();
        objects.records.push(super::S2ObjectRecordIndex {
            handle: 0x3000_0001,
            record_offset: 0,
            count_offset: 0,
            weight_offset: 0,
            type_key_offset: 0,
            kind_code: 0,
            count: 1,
            total_weight: 1.0,
            type_key: [4, 1, 0],
        });

        assert!(super::S2NameTables::locate(&raw, &objects, false)?.is_none());
        Ok(())
    }

    #[test]
    fn durability_window_stops_before_the_next_object_index() {
        let parsed = S2Save::from_bytes(WRITER_S2_STACK_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let Some(handle) = parsed.index.owned_handles.first().copied() else {
            return;
        };
        let Some(record) = parsed.objects.unique(handle).copied() else {
            return;
        };
        let next_offset = record.record_offset.saturating_add(0x20_000);
        let Some(next_handle) = (0x3000_0000..=0x3000_FFFF).find(|candidate| {
            !parsed.index.owned_handles.contains(candidate) && parsed.objects.unique(*candidate).is_none()
        }) else {
            return;
        };
        let next = super::S2ObjectRecordIndex {
            handle: next_handle,
            record_offset: next_offset,
            ..record
        };
        let mut objects = super::S2ObjectIndex::default();
        objects.records.extend([record, next]);
        objects.by_handle.insert(handle, vec![0]);
        objects.by_handle.insert(next_handle, vec![1]);
        objects.record_ends.extend([next_offset, next_offset.saturating_add(1)]);

        let ends = super::record_end_guesses(next_offset.saturating_add(1), &parsed.index, &objects);

        assert_eq!(ends.get(&handle), Some(&next_offset));
    }

    #[test]
    fn s2_durability_and_money_share_one_working_image_and_match_reference() {
        let parsed = S2Save::from_bytes(WRITER_S2_ARMOR_MONEY_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let result = apply_changes_to_image(
            &parsed,
            &[
                money_change(&parsed, 900_000),
                S2Change::SetDurability {
                    handle: 805_308_859,
                    condition: 1.0,
                },
            ],
        );
        assert_eq!(
            result.map(|(image, _)| image),
            Ok(WRITER_S2_ARMOR_MONEY_EXPECTED.to_vec())
        );
    }

    #[test]
    fn s2_weapon_durability_writer_matches_reference_image() {
        let parsed = S2Save::from_bytes(WRITER_S2_WEAPON_SOURCE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let changes = [S2Change::SetDurability {
            handle: 805_309_098,
            condition: 0.9,
        }];
        let result = apply_changes_to_image(&parsed, &changes);
        assert_eq!(result.map(|(image, _)| image), Ok(WRITER_S2_WEAPON_EXPECTED.to_vec()));

        let packed = parsed.write_changes(&changes);
        assert!(packed.is_ok());
        let Ok(packed) = packed else { return };
        let verified = S2Save::from_bytes(&packed);
        assert!(verified.is_ok());
        let Ok(verified) = verified else { return };
        assert_eq!(
            verified
                .items()
                .iter()
                .find(|item| item.handle == 805_309_098)
                .map(|item| item.condition),
            Some(Some(0.9))
        );
    }

    #[test]
    fn s2_durability_writer_refuses_a_handle_outside_the_owned_list() {
        let parsed = S2Save::from_bytes(WRITER_S2_WEAPON_SOURCE);
        assert!(parsed.is_ok());
        let Ok(mut parsed) = parsed else { return };
        let handle = 805_309_098;
        assert!(parsed.index.owned_handles.contains(&handle));
        parsed.index.owned_handles.retain(|owned| *owned != handle);

        assert!(
            apply_changes_to_image(&parsed, &[S2Change::SetDurability { handle, condition: 0.9 }],)
                .is_err_and(|error| error.to_string().contains("not uniquely owned"))
        );
    }

    #[test]
    fn durability_readback_rejects_a_changed_condition_on_another_item() {
        let before = [
            (0x3000_0001, Some(0.75)),
            (0x3000_0002, Some(0.40)),
            (0x3000_0003, None),
        ];
        let expected = [
            (0x3000_0001, Some(0.90)),
            (0x3000_0002, Some(0.40)),
            (0x3000_0003, None),
        ];
        let collateral_change = [
            (0x3000_0001, Some(0.90)),
            (0x3000_0002, Some(0.41)),
            (0x3000_0003, None),
        ];
        let requested = [(0x3000_0001, 0.90)];

        assert!(super::verify_durability_values(&before, &expected, &requested).is_ok());
        assert!(super::verify_durability_values(&before, &collateral_change, &requested).is_err());
    }

    #[test]
    fn moved_object_offset_keeps_records_before_the_window_and_shifts_records_after_it() {
        // Window 100..200 grows by 16 bytes.
        assert_eq!(moved_object_offset(40, 100..200, 16), Ok(40));
        assert_eq!(moved_object_offset(250, 100..200, 16), Ok(266));
        assert_eq!(moved_object_offset(250, 100..200, 0), Ok(250));
        assert!(moved_object_offset(150, 100..200, 16).is_err());
        assert!(moved_object_offset(100, 100..200, 16).is_err());
    }

    #[test]
    fn free_placement_finds_the_first_origin_that_fits() {
        let occupied: HashSet<(u16, u16)> = [(0, 0), (1, 0), (0, 1)].into_iter().collect();
        assert_eq!(first_free_placement(&occupied, &[(0, 0), (1, 0)], 2, 1), Some((2, 0)));
    }

    #[test]
    fn free_placement_checks_every_footprint_cell_not_only_the_origin() {
        let occupied: HashSet<(u16, u16)> = [(0, 1)].into_iter().collect();
        assert_eq!(first_free_placement(&occupied, &[(0, 0), (0, 1)], 1, 2), Some((1, 0)));
    }

    #[test]
    fn free_placement_refuses_a_full_backpack() {
        let occupied: HashSet<(u16, u16)> = (0..GRID_WIDTH).flat_map(|x| (0..128).map(move |y| (x, y))).collect();
        assert_eq!(first_free_placement(&occupied, &[(0, 0)], 1, 1), None);
    }

    #[test]
    fn s2_stash_move_matches_reference_in_one_working_image() {
        let packed = pack_raw(WRITER_S2_STASH_SOURCE);
        let parsed = S2Save::from_bytes(&packed);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let result = apply_changes_to_image(&parsed, &[S2Change::MoveStashToBackpack { handle: 0x3000_0010 }]);
        assert_eq!(result.map(|(image, _)| image), Ok(WRITER_S2_STASH_EXPECTED.to_vec()));
    }

    #[test]
    fn public_stash_reader_exposes_the_verified_item_and_move_read_back() -> Result<(), Error> {
        let save = S2Save::from_bytes(WRITER_S2_STASH_PACKED_SOURCE)?;
        assert_eq!(save.container().image(), WRITER_S2_STASH_SOURCE);
        let items = save.stash_items()?;
        assert_eq!(items.len(), 1);
        let item = items.first().ok_or_else(|| Error::damaged("stash item is missing"))?;
        assert_eq!(item.handle, 0x3000_0010);
        assert!(!item.cells.is_empty());
        assert!(item.count > 0);

        let packed = save.write_changes(&[S2Change::MoveStashToBackpack { handle: item.handle }])?;
        let moved = S2Save::from_bytes(&packed)?;
        assert!(moved.stash_items()?.is_empty());
        assert!(moved
            .items()
            .iter()
            .any(|backpack_item| backpack_item.handle == item.handle));
        Ok(())
    }

    /// Manual run on copies of real saves: `SSE_S2_COPY_DIR=<copies> cargo test -p sse-s2 real_copies -- --ignored --nocapture`.
    /// Never point it at the originals; the test only reads the files it is given, and writes nothing to disk.
    #[test]
    #[ignore = "manual run on copies of real S2 saves (SSE_S2_COPY_DIR)"]
    fn real_copies_stash_transfer_is_consistent() -> Result<(), Error> {
        let dir =
            std::env::var("SSE_S2_COPY_DIR").map_err(|_| Error::Refused("SSE_S2_COPY_DIR is not set".to_owned()))?;
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|error| Error::System(error.to_string()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_file())
            .collect();
        paths.sort();
        let mut inconsistent = 0_usize;
        for path in &paths {
            let source = std::fs::read(path).map_err(|error| Error::System(error.to_string()))?;
            let before = S2Save::from_bytes(&source)?;
            if before.index.is_legacy {
                println!("{} legacy-1.0x skipped", path.display());
                continue;
            }
            if !before.unresolved_handles().is_empty() {
                println!("{} unresolved-inventory skipped", path.display());
                continue;
            }
            let stash = before.stash_items()?;
            if stash.is_empty() {
                println!("{} no-stash skipped", path.display());
                continue;
            }
            let handles: Vec<u32> = stash.iter().map(|item| item.handle).collect();
            let first = handles.get(..1).unwrap_or_default();
            let one = transfer_stash_items_to_backpack(&before, &[], first)?;
            let all = transfer_stash_items_to_backpack(&before, &[], &handles)?;
            for (label, transfer) in [("one", &one), ("all", &all)] {
                let packed = transfer.packed.clone().unwrap_or_else(|| source.clone());
                let after = S2Save::from_bytes(&packed)?;
                let stash_after = after.stash_items()?.len();
                let backpack_before = before.items().len();
                let backpack_after = after.items().len();
                let counts_ok = stash_after + transfer.moved.len() == stash.len()
                    && backpack_after == backpack_before + transfer.moved.len();
                if !counts_ok || !after.unresolved_handles().is_empty() {
                    inconsistent += 1;
                }
                println!(
                    "{} {label}: moved={} skipped={} refused={} (room={} unsupported={} other={}) stopped={} counts_ok={counts_ok}",
                    path.display(),
                    transfer.moved.len(),
                    transfer.skipped.len(),
                    transfer.refused.len(),
                    transfer.refused_count(S2SkipReason::NoRoom),
                    transfer.refused_count(S2SkipReason::Unsupported),
                    transfer.refused_count(S2SkipReason::Other),
                    transfer.stopped.as_deref().unwrap_or("-"),
                );
            }
        }
        println!("files={} inconsistent={inconsistent}", paths.len());
        assert_eq!(inconsistent, 0);
        Ok(())
    }

    #[test]
    fn sequential_transfer_records_a_refused_item_and_keeps_the_image() -> Result<(), Error> {
        let mut raw = WRITER_S2_STASH_SOURCE.to_vec();
        let record_save = S2Save::from_bytes(&pack_raw(&raw))?;
        let record_offset = record_save
            .objects
            .unique(0x3000_0010)
            .ok_or_else(|| Error::damaged("fixture stash record is missing"))?
            .record_offset;
        // Kind 3 is accepted by the move check but not by the reader, so the read-back refuses it.
        let kind = raw
            .get_mut(record_offset.saturating_add(31))
            .ok_or_else(|| Error::damaged("fixture kind byte is missing"))?;
        *kind = 3;
        let save = S2Save::from_bytes(&pack_raw(&raw))?;

        let transfer = transfer_stash_items_to_backpack(&save, &[], &[0x3000_0010])?;

        assert!(transfer.moved.is_empty());
        assert_eq!(transfer.packed, None);
        assert_eq!(transfer.stopped, None);
        assert_eq!(transfer.refused.len(), 1);
        assert_eq!(transfer.refused_count(S2SkipReason::Unsupported), 1);
        assert_eq!(transfer.refused_count(S2SkipReason::NoRoom), 0);
        Ok(())
    }

    #[test]
    fn refusal_messages_map_to_skip_categories() {
        assert_eq!(
            classify_refusal("S2 stash item does not fit a backpack"),
            S2SkipReason::NoRoom
        );
        assert_eq!(
            classify_refusal("S2 backpack has no fitting free cells"),
            S2SkipReason::NoRoom
        );
        assert_eq!(
            classify_refusal("S2 stash item kind is not confirmed editable"),
            S2SkipReason::Unsupported
        );
        assert_eq!(
            classify_refusal("S2 stash item footprint is missing"),
            S2SkipReason::Unsupported
        );
        assert_eq!(
            classify_refusal("S2 write read-back: moved item has unresolved backpack cells"),
            S2SkipReason::Unsupported
        );
        assert_eq!(classify_refusal("S2 stash handle is duplicated"), S2SkipReason::Other);
    }

    #[test]
    fn sequential_transfer_moves_a_marked_item_and_reparses_clean() -> Result<(), Error> {
        let save = S2Save::from_bytes(WRITER_S2_STASH_PACKED_SOURCE)?;
        let transfer = transfer_stash_items_to_backpack(&save, &[], &[0x3000_0010])?;
        assert_eq!(transfer.moved, vec![0x3000_0010]);
        assert!(transfer.skipped.is_empty());
        assert_eq!(transfer.stopped, None);
        let packed = transfer
            .packed
            .ok_or_else(|| Error::damaged("transfer wrote nothing"))?;
        let after = S2Save::from_bytes(&packed)?;
        assert!(after.stash_items()?.is_empty());
        Ok(())
    }

    #[test]
    fn sequential_transfer_skips_an_unmarked_item_and_leaves_the_bytes_unchanged() -> Result<(), Error> {
        let mut raw = WRITER_S2_STASH_SOURCE.to_vec();
        let record_save = S2Save::from_bytes(&pack_raw(&raw))?;
        let record = record_save
            .objects
            .unique(0x3000_0010)
            .ok_or_else(|| Error::damaged("fixture stash record is missing"))?;
        let flags = raw
            .get_mut(record.record_offset.saturating_add(28))
            .ok_or_else(|| Error::damaged("fixture flag byte is missing"))?;
        *flags &= !0x08;

        let unmarked = S2Save::from_bytes(&pack_raw(&raw))?;
        let transfer = transfer_stash_items_to_backpack(&unmarked, &[], &[0x3000_0010])?;
        assert!(transfer.moved.is_empty());
        assert_eq!(transfer.skipped, vec![0x3000_0010]);
        assert_eq!(transfer.stopped, None);
        assert_eq!(transfer.packed, None);
        Ok(())
    }

    #[test]
    fn stash_move_reparses_from_zero_with_consistent_counts_and_untouched_other_items() -> Result<(), Error> {
        let before = S2Save::from_bytes(WRITER_S2_STASH_PACKED_SOURCE)?;
        let stash_before = before.stash_items()?;
        let moved = stash_before
            .first()
            .ok_or_else(|| Error::damaged("fixture has no stash item"))?
            .clone();
        let packed = before.write_changes(&[S2Change::MoveStashToBackpack { handle: moved.handle }])?;

        // A fresh read re-checks the container CRC and unpacked size and rebuilds every count from the bytes.
        let after = S2Save::from_bytes(&packed)?;
        assert!(after.unresolved_handles().is_empty());
        assert_eq!(after.stash_items()?.len() + 1, stash_before.len());
        assert_eq!(after.items().len(), before.items().len() + 1);

        let placed = after
            .items()
            .into_iter()
            .find(|item| item.handle == moved.handle)
            .ok_or_else(|| Error::damaged("moved item is missing from the backpack"))?;
        assert_eq!(placed.kind_code, moved.kind_code);
        assert_eq!(placed.count, moved.count);
        assert_eq!(placed.type_key, moved.type_key);

        for item in before.items().iter().filter(|item| item.handle != moved.handle) {
            let found = after
                .items()
                .into_iter()
                .find(|candidate| candidate.handle == item.handle)
                .ok_or_else(|| Error::damaged("an untouched backpack item disappeared"))?;
            assert_eq!(found.kind_code, item.kind_code);
            assert_eq!(found.count, item.count);
            assert_eq!(found.type_key, item.type_key);
            assert_eq!(found.total_weight.to_bits(), item.total_weight.to_bits());
            assert_eq!(found.display_name, item.display_name);
            assert_eq!(found.modules, item.modules);
            assert_eq!(found.upgrades, item.upgrades);
            assert_eq!(found.cells, item.cells);
        }
        Ok(())
    }

    #[test]
    fn s2_kind_eight_stack_cannot_be_reduced_to_one() {
        let mut raw = SYNTHETIC_RAW.to_vec();
        let parsed = S2Save::from_bytes(SYNTHETIC_SAVE);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let Some(record_offset) = parsed.objects.unique(0x3000_0003).map(|record| record.record_offset) else {
            return;
        };
        let Some(kind_offset) = record_offset.checked_add(31) else {
            return;
        };
        let Some(kind) = raw.get_mut(kind_offset) else { return };
        *kind = 8;
        let Some(count_offset) = record_offset.checked_add(19) else {
            return;
        };
        let Some(count) = raw.get_mut(count_offset..count_offset.saturating_add(4)) else {
            return;
        };
        count.copy_from_slice(&2_u32.to_le_bytes());
        let packed = pack_raw(&raw);
        let parsed = S2Save::from_bytes(&packed);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        assert!(matches!(
            apply_changes_to_image(
                &parsed,
                &[S2Change::SetStackCount {
                    handle: 0x3000_0003,
                    count: 1,
                }]
            ),
            Err(Error::Refused(_))
        ));
    }

    fn legacy_synthetic_raw() -> Vec<u8> {
        let mut raw = vec![0_u8; 64];
        let records = [
            (0x3000_0101_u32, 2_u16, 0_u8, 0_u8, 10_u32, 0.5_f32, 5_u8),
            (0x3000_0102, 3, 1, 0, 31, 3.1, 5),
            (0x3000_0103, 4, 0, 1, 468, 4.68, 4),
            (0x3000_0104, 2, 1, 1, 2, 0.1, 5),
        ];
        for (handle, name_index, x, y, count, weight, kind) in records {
            let mut record = [0_u8; 64];
            record
                .get_mut(..4)
                .unwrap_or_default()
                .copy_from_slice(&handle.to_le_bytes());
            record
                .get_mut(4..8)
                .unwrap_or_default()
                .copy_from_slice(&0x4600_0000_u32.wrapping_add(handle).to_le_bytes());
            record
                .get_mut(8..10)
                .unwrap_or_default()
                .copy_from_slice(&name_index.to_le_bytes());
            record[10] = x;
            record[11] = y;
            record[15] = 0x38;
            record
                .get_mut(16..20)
                .unwrap_or_default()
                .copy_from_slice(&count.to_le_bytes());
            record
                .get_mut(21..25)
                .unwrap_or_default()
                .copy_from_slice(&weight.to_le_bytes());
            record
                .get_mut(25..28)
                .unwrap_or_default()
                .copy_from_slice(&[0x53, 0x01, 0x01]);
            record[28] = kind;
            raw.extend_from_slice(&record);
        }

        raw.extend_from_slice(&super::LEGACY_CONTAINER_ID);
        raw.extend_from_slice(&2_u16.to_le_bytes());
        raw.extend_from_slice(&[0xdf, 0x03, 0x00, 0x38, 1, 0, 0, 0]);
        raw.extend_from_slice(&[0x50, 0x26, 0x00, 0x38, 1, 0, 0, 0]);
        raw.extend_from_slice(&0_u32.to_le_bytes());
        raw.extend_from_slice(&42_u32.to_le_bytes());
        raw.extend_from_slice(&42_u16.to_le_bytes());
        raw.extend_from_slice(&85_433_u32.to_le_bytes());
        raw.extend_from_slice(&1_u32.to_le_bytes());
        raw.extend_from_slice(&6_u16.to_le_bytes());
        for handle in [
            0x3000_0101_u32,
            0x3000_0102,
            0x3000_0103,
            0x3000_0104,
            u32::MAX,
            u32::MAX,
        ] {
            raw.extend_from_slice(&handle.to_le_bytes());
        }
        raw.extend_from_slice(&4_u16.to_le_bytes());
        for (handle, x, y) in [
            (0x3000_0101_u32, 0_u8, 0_u8),
            (0x3000_0102, 1, 0),
            (0x3000_0103, 0, 1),
            (0x3000_0104, 1, 1),
        ] {
            raw.extend_from_slice(&handle.to_le_bytes());
            raw.extend_from_slice(&[x, y]);
        }
        raw.extend_from_slice(&[0_u8; 32]);
        for name in ["Player", "", "Bandage", "ArmyMedkit", "A939A"] {
            raw.extend_from_slice(&u16::try_from(name.len()).unwrap_or_default().to_le_bytes());
            raw.extend_from_slice(name.as_bytes());
        }
        let table_start = raw.len().saturating_sub(
            ["Player", "", "Bandage", "ArmyMedkit", "A939A"]
                .iter()
                .map(|name| name.len().saturating_add(2))
                .sum::<usize>(),
        );
        raw.splice(table_start..table_start, 5_u16.to_le_bytes());
        raw
    }

    fn pack_raw(raw: &[u8]) -> Vec<u8> {
        let mut packed = Vec::with_capacity(raw.len().saturating_add(10));
        packed.extend_from_slice(&u32::try_from(raw.len()).unwrap_or_default().to_le_bytes());
        packed.extend_from_slice(&[0xcc, 0x06]);
        packed.extend_from_slice(raw);
        let crc = crc32::crc32(&packed);
        packed.extend_from_slice(&crc.to_le_bytes());
        packed
    }

    fn pack_kraken_stream(raw: &[u8], stream: &[u8]) -> Vec<u8> {
        let mut packed = Vec::with_capacity(stream.len().saturating_add(8));
        packed.extend_from_slice(&u32::try_from(raw.len()).unwrap_or_default().to_le_bytes());
        packed.extend_from_slice(stream);
        let crc = crc32::crc32(&packed);
        packed.extend_from_slice(&crc.to_le_bytes());
        packed
    }

    #[test]
    fn container_uses_main_kraken_decoder_for_rle_reference_vector() {
        const RAW: &[u8] = include_bytes!("../../../fixtures/kraken/save-like-small.raw");
        const STREAM: &[u8] = include_bytes!("../../../fixtures/kraken/save-like-small-l6.kraken");

        let packed = pack_kraken_stream(RAW, STREAM);
        assert_eq!(
            S2Container::from_bytes(&packed).map(|value| value.image() == RAW),
            Ok(true)
        );
    }

    #[test]
    fn legacy_1031_reader_matches_the_synthetic_reference_values() {
        let raw = legacy_synthetic_raw();
        let packed = pack_raw(&raw);
        assert!(S2Save::from_bytes(&packed).is_ok());
        let save = S2Save::from_bytes(&packed);
        assert_eq!(
            save.map(|value| (
                value.money().unwrap_or_default(),
                value.index().is_legacy(),
                value.index().owned_handles().len(),
                value.name_tables().map(super::S2NameTables::table_count),
                value
                    .items()
                    .iter()
                    .map(|item| (
                        item.handle,
                        item.count,
                        item.total_weight,
                        item.x.unwrap_or_default(),
                        item.y.unwrap_or_default(),
                        item.display_name.clone(),
                        item.editable_count,
                    ))
                    .collect::<Vec<_>>(),
                value.orphans().len(),
                value.unresolved_handles().to_vec(),
                value.warnings().iter().any(|warning| warning.contains("1.0.x")),
            )),
            Ok((
                85_433,
                true,
                6,
                Some(1),
                vec![
                    (0x3000_0101, 10, 0.5, 0, 0, Some("Bandage".to_owned()), false),
                    (0x3000_0102, 31, 3.1, 1, 0, Some("ArmyMedkit".to_owned()), false),
                    (0x3000_0103, 468, 4.68, 0, 1, Some("A939A".to_owned()), false),
                    (0x3000_0104, 2, 0.1, 1, 1, Some("Bandage".to_owned()), false),
                ],
                0,
                Vec::new(),
                true,
            ))
        );
    }

    #[test]
    fn legacy_1031_layout_reports_that_writing_is_disabled() {
        let packed = pack_raw(&legacy_synthetic_raw());
        let parsed = S2Save::from_bytes(&packed);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };

        assert!(parsed
            .write_changes(&[money_change(&parsed, 85_434)])
            .is_err_and(|error| error.to_string().contains(
                "This save was written by game version 1.0.x. It can be read, but its layout is not supported for editing; load it in the current game and save again."
            )));
    }

    #[test]
    fn armor_state_requires_matching_nested_handle_and_finite_condition() {
        let handle = 0x3000_0201_u32;
        let record_offset = 8_usize;
        let nested_offset = record_offset.saturating_add(0x23);
        let value_offset = nested_offset.saturating_add(4);
        let mut raw = vec![0_u8; 64];
        raw.get_mut(record_offset..record_offset.saturating_add(4))
            .unwrap_or_default()
            .copy_from_slice(&handle.to_le_bytes());
        raw.get_mut(nested_offset..nested_offset.saturating_add(4))
            .unwrap_or_default()
            .copy_from_slice(&handle.to_le_bytes());
        raw.get_mut(value_offset..value_offset.saturating_add(4))
            .unwrap_or_default()
            .copy_from_slice(&0.75_f32.to_le_bytes());

        assert_eq!(
            super::read_armor_condition(&raw, handle, record_offset, 1),
            Some((value_offset, 0.75))
        );
        assert_eq!(super::read_armor_condition(&raw, handle, record_offset, 0), None);

        let mut malformed = raw.clone();
        malformed
            .get_mut(nested_offset..nested_offset.saturating_add(4))
            .unwrap_or_default()
            .copy_from_slice(&0x3000_0fff_u32.to_le_bytes());
        assert_eq!(super::read_armor_condition(&malformed, handle, record_offset, 1), None);

        for invalid in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
            let mut malformed = raw.clone();
            malformed
                .get_mut(value_offset..value_offset.saturating_add(4))
                .unwrap_or_default()
                .copy_from_slice(&invalid.to_le_bytes());
            assert_eq!(super::read_armor_condition(&malformed, handle, record_offset, 1), None);
        }

        let object = super::S2ObjectRecordIndex {
            handle,
            record_offset,
            count_offset: record_offset.saturating_add(16),
            weight_offset: record_offset.saturating_add(21),
            type_key_offset: record_offset.saturating_add(8),
            kind_code: 1,
            count: 1,
            total_weight: 1.0,
            type_key: [4, 0, 0],
        };
        let mut objects = super::S2ObjectIndex::default();
        objects.records.push(object);
        objects.by_handle.insert(handle, vec![0]);
        let index = S2InventoryIndex {
            money_offset: 0,
            owned_count_offset: 0,
            owned_handles_offset: 0,
            owned_handles: vec![handle],
            grid_count_offset: 0,
            grid_offset: 0,
            grid_count: 0,
            grid_cells: Vec::new(),
            grid_end_offset: 0,
            grid_handle_count: 0,
            unresolved_handles: Vec::new(),
            warnings: Vec::new(),
            is_legacy: false,
        };
        let items = super::build_inventory_items(&raw, &index, &objects, None);
        assert_eq!(items.len(), 1);
        assert_eq!(items.first().and_then(|item| item.condition), Some(0.75));
        assert_eq!(items.first().and_then(|item| item.condition_offset), Some(value_offset));

        let names = super::S2NameTables {
            tables: vec![vec![
                "Exoskeleton_Monolith_Armor".to_owned(),
                "Exoskeleton_Monolith_Armor_Upgrade_Fast_1".to_owned(),
                "Exoskeleton_Monolith_Armor_Upgrade_Slow_1".to_owned(),
            ]],
            single_table: false,
        };
        raw.get_mut(record_offset.saturating_add(8)..record_offset.saturating_add(11))
            .unwrap_or_default()
            .copy_from_slice(&[4, 0, 0]);
        raw.get_mut(value_offset.saturating_add(4)..value_offset.saturating_add(6))
            .unwrap_or_default()
            .copy_from_slice(&2_u16.to_le_bytes());
        raw.get_mut(value_offset.saturating_add(6)..value_offset.saturating_add(9))
            .unwrap_or_default()
            .copy_from_slice(&[4, 1, 0]);
        raw.get_mut(value_offset.saturating_add(9)..value_offset.saturating_add(12))
            .unwrap_or_default()
            .copy_from_slice(&[4, 2, 0]);
        assert_eq!(
            super::read_armor_upgrades(&raw, record_offset, value_offset, &names),
            Some(vec![
                "Exoskeleton_Monolith_Armor_Upgrade_Fast_1".to_owned(),
                "Exoskeleton_Monolith_Armor_Upgrade_Slow_1".to_owned(),
            ])
        );
        let items = super::build_inventory_items(&raw, &index, &objects, Some(&names));
        assert_eq!(
            items
                .first()
                .map(|item| item.upgrades.iter().map(String::as_str).collect::<Vec<_>>()),
            Some(vec![
                "Exoskeleton_Monolith_Armor_Upgrade_Fast_1",
                "Exoskeleton_Monolith_Armor_Upgrade_Slow_1",
            ])
        );
    }

    #[test]
    fn weapon_state_requires_a_unique_named_module_and_upgrade_vector() {
        let handle = 0x3000_0202_u32;
        let record_offset = 8_usize;
        let value_offset = record_offset.saturating_add(0x30).saturating_add(14);
        let mut raw = vec![0_u8; 128];
        raw.get_mut(record_offset..record_offset.saturating_add(4))
            .unwrap_or_default()
            .copy_from_slice(&handle.to_le_bytes());

        fn set_key(raw: &mut [u8], offset: usize, name_index: u16) {
            raw.get_mut(offset..offset.saturating_add(3))
                .unwrap_or_default()
                .copy_from_slice(&[4, name_index.to_le_bytes()[0], name_index.to_le_bytes()[1]]);
        }
        set_key(&mut raw, record_offset.saturating_add(8), 0);
        set_key(&mut raw, value_offset.saturating_sub(6), 1);
        set_key(&mut raw, value_offset.saturating_sub(3), 2);
        raw.get_mut(value_offset.saturating_sub(8)..value_offset.saturating_sub(6))
            .unwrap_or_default()
            .copy_from_slice(&2_u16.to_le_bytes());
        raw.get_mut(value_offset..value_offset.saturating_add(4))
            .unwrap_or_default()
            .copy_from_slice(&0.75_f32.to_le_bytes());
        raw.get_mut(value_offset.saturating_add(4)..value_offset.saturating_add(6))
            .unwrap_or_default()
            .copy_from_slice(&2_u16.to_le_bytes());
        set_key(&mut raw, value_offset.saturating_add(6), 3);
        set_key(&mut raw, value_offset.saturating_add(9), 4);

        let names = super::S2NameTables {
            tables: vec![vec![
                "GunKharod_ST".to_owned(),
                "GunKharod_MagDefault".to_owned(),
                "HP_Laser_1".to_owned(),
                "GunKharod_Upgrade_Stock_1".to_owned(),
                "GunKharod_Upgrade_Barrel_1".to_owned(),
            ]],
            single_table: false,
        };
        assert_eq!(
            super::read_weapon_state(&raw, handle, record_offset, raw.len(), &names),
            Some((
                value_offset,
                0.75,
                vec!["GunKharod_MagDefault".to_owned(), "HP_Laser_1".to_owned()],
                vec![
                    "GunKharod_Upgrade_Stock_1".to_owned(),
                    "GunKharod_Upgrade_Barrel_1".to_owned(),
                ],
            ))
        );

        raw.get_mut(record_offset.saturating_add(0x23)..record_offset.saturating_add(0x27))
            .unwrap_or_default()
            .copy_from_slice(&handle.to_le_bytes());
        let object = super::S2ObjectRecordIndex {
            handle,
            record_offset,
            count_offset: record_offset.saturating_add(19),
            weight_offset: record_offset.saturating_add(24),
            type_key_offset: record_offset.saturating_add(8),
            kind_code: 0,
            count: 1,
            total_weight: 4.0,
            type_key: [4, 0, 0],
        };
        let mut objects = super::S2ObjectIndex::default();
        objects.records.push(object);
        objects.by_handle.insert(handle, vec![0]);
        let index = S2InventoryIndex {
            money_offset: 0,
            owned_count_offset: 0,
            owned_handles_offset: 0,
            owned_handles: vec![handle],
            grid_count_offset: 0,
            grid_offset: 0,
            grid_count: 0,
            grid_cells: Vec::new(),
            grid_end_offset: 0,
            grid_handle_count: 0,
            unresolved_handles: Vec::new(),
            warnings: Vec::new(),
            is_legacy: false,
        };
        let items = super::build_inventory_items(&raw, &index, &objects, Some(&names));
        assert_eq!(items.len(), 1);
        assert_eq!(items.first().and_then(|item| item.condition), Some(0.75));
        assert_eq!(
            items
                .first()
                .map(|item| item.modules.iter().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["GunKharod_MagDefault", "HP_Laser_1"])
        );
        assert_eq!(
            items
                .first()
                .map(|item| item.upgrades.iter().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["GunKharod_Upgrade_Stock_1", "GunKharod_Upgrade_Barrel_1"])
        );

        let mut malformed = raw.clone();
        malformed
            .get_mut(value_offset.saturating_add(4)..value_offset.saturating_add(6))
            .unwrap_or_default()
            .copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(
            super::read_weapon_state(&malformed, handle, record_offset, malformed.len(), &names),
            None
        );

        let mut two_candidates = raw.clone();
        let secondary_value = value_offset.saturating_add(0x900);
        let source_start = value_offset.saturating_sub(8);
        let source_end = value_offset.saturating_add(12);
        let secondary_start = secondary_value.saturating_sub(8);
        two_candidates.resize(secondary_value.saturating_add(16), 0);
        let Some(source) = raw.get(source_start..source_end) else {
            return;
        };
        let Some(destination) = two_candidates.get_mut(secondary_start..secondary_start.saturating_add(source.len()))
        else {
            return;
        };
        destination.copy_from_slice(source);
        assert_eq!(
            super::read_weapon_state(&two_candidates, handle, record_offset, two_candidates.len(), &names),
            None,
            "a second candidate anywhere before the next indexed object remains ambiguous"
        );
    }

    #[test]
    fn legacy_readers_reject_truncation_and_hostile_lengths_and_survive_mutations() {
        let raw = legacy_synthetic_raw();
        let index = S2InventoryIndex::locate(&raw);
        assert!(index.is_ok());
        let Ok(index) = index else { return };
        for length in 0..index.grid_end_offset() {
            assert!(matches!(
                S2InventoryIndex::locate(raw.get(..length).unwrap_or_default()),
                Err(Error::Damaged(_))
            ));
        }

        let anchor_offset = super::find_subslice(&raw, &super::LEGACY_CONTAINER_ID, 0);
        assert!(anchor_offset.is_some());
        let Some(anchor_offset) = anchor_offset else { return };
        for offset in anchor_offset..anchor_offset.saturating_add(super::LEGACY_CONTAINER_ID.len()) {
            for bit in 0..8_u32 {
                let mut changed = raw.clone();
                let byte = changed
                    .get_mut(offset)
                    .map(|value| *value ^= 1_u8.checked_shl(bit).unwrap_or_default());
                assert!(byte.is_some());
                assert!(matches!(S2InventoryIndex::locate(&changed), Err(Error::Damaged(_))));
            }
        }

        for offset in [index.owned_count_offset(), index.grid_count_offset()] {
            let mut changed = raw.clone();
            let end = offset.saturating_add(2);
            changed
                .get_mut(offset..end)
                .unwrap_or_default()
                .copy_from_slice(&u16::MAX.to_le_bytes());
            assert!(matches!(S2InventoryIndex::locate(&changed), Err(Error::Damaged(_))));
        }

        let table_name = super::find_subslice(&raw, b"Player", 0);
        assert!(table_name.is_some());
        let Some(table_name) = table_name else { return };
        let table_start = table_name.saturating_sub(4);
        let mut hostile_names = raw.clone();
        hostile_names
            .get_mut(table_start..table_start.saturating_add(2))
            .unwrap_or_default()
            .copy_from_slice(&u16::MAX.to_le_bytes());
        let names_result = S2Save::from_bytes(&pack_raw(&hostile_names));
        assert!(names_result.is_ok());
        assert!(names_result.is_ok_and(|save| save.name_tables().is_none()));

        let mut state = 0x7e15_4a2d_u32;
        for _ in 0..512 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let offset = usize::try_from(state).unwrap_or_default() % raw.len();
            state = state.rotate_left(9).wrapping_add(0x6c8e_9cf5);
            let mut changed = raw.clone();
            let mutation = changed.get_mut(offset).map(|byte| *byte ^= (state & 0xff) as u8);
            assert!(mutation.is_some());
            assert!(std::panic::catch_unwind(|| S2InventoryIndex::locate(&changed)).is_ok());
            assert!(std::panic::catch_unwind(|| S2Save::from_bytes(&pack_raw(&changed))).is_ok());
        }
    }

    #[test]
    fn synthetic_container_and_reader_match_the_s2_fixture() {
        let container = S2Container::from_bytes(SYNTHETIC_SAVE);
        assert_eq!(
            container.map(|value| (
                value.image() == SYNTHETIC_RAW,
                value.packed_size(),
                value.stored_crc32() == value.computed_crc32(),
            )),
            Ok((true, SYNTHETIC_SAVE.len(), true))
        );

        assert!(S2Save::from_bytes(SYNTHETIC_SAVE).is_ok());
        assert_eq!(
            S2Save::from_bytes(SYNTHETIC_SAVE).map(|value| (
                value.container().image().len(),
                value.money().unwrap_or_default(),
                value.index().is_legacy(),
                value.index().owned_handles().to_vec(),
                value.index().grid_cells().to_vec(),
                value.index().grid_handle_count(),
                value
                    .items()
                    .iter()
                    .map(|item| (
                        item.handle,
                        item.count,
                        item.x.unwrap_or_default(),
                        item.y.unwrap_or_default(),
                        item.type_key,
                        item.kind_code,
                    ))
                    .collect::<Vec<_>>(),
                value
                    .orphans()
                    .iter()
                    .map(|item| (item.handle, item.kind_code))
                    .collect::<Vec<_>>(),
                value.unresolved_handles().to_vec(),
                value.name_tables().is_some(),
            )),
            Ok((
                SYNTHETIC_RAW.len(),
                100,
                false,
                vec![0x3000_0001, 0x3000_0002, 0x3000_0003, 0x3000_0004],
                vec![
                    super::S2GridCell {
                        handle: 0x3000_0001,
                        x: 0,
                        y: 0
                    },
                    super::S2GridCell {
                        handle: 0x3000_0002,
                        x: 1,
                        y: 0
                    },
                ],
                2,
                vec![
                    (0x3000_0001, 2, 0, 0, [1, 2, 3], 4),
                    (0x3000_0002, 1, 1, 0, [4, 5, 6], 5),
                ],
                vec![(0x3000_0003, 0), (0x3000_0004, 99)],
                vec![0x3000_0004],
                false,
            ))
        );
    }

    #[test]
    fn object_index_does_not_retain_candidates_for_unowned_handles() {
        let baseline = S2Save::from_bytes(SYNTHETIC_SAVE);
        assert!(baseline.is_ok());
        let Ok(baseline) = baseline else { return };
        let expected_record_count = baseline.objects.records.len();
        let expected_items = baseline.items();
        let expected_orphans = baseline.orphans();
        let mut raw = SYNTHETIC_RAW.to_vec();

        for candidate in 0..4096_u32 {
            append_object_candidate(&mut raw, 0x7000_0000_u32.saturating_add(candidate));
        }

        let parsed = S2Save::from_bytes(&pack_raw(&raw));
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        assert_eq!(
            parsed.objects.records.len(),
            expected_record_count,
            "object index should retain only records referenced by backpack or stash handles"
        );
        assert_eq!(parsed.items(), expected_items);
        assert_eq!(parsed.orphans(), expected_orphans);
    }

    #[test]
    fn object_index_stores_only_two_candidates_per_referenced_handle() {
        let baseline = S2Save::from_bytes(SYNTHETIC_SAVE);
        assert!(baseline.is_ok());
        let Ok(baseline) = baseline else { return };
        let handle = 0x3000_0002;
        assert_eq!(baseline.objects.candidates(handle).len(), 1);
        let mut raw = SYNTHETIC_RAW.to_vec();

        for _ in 0..128 {
            append_object_candidate(&mut raw, handle);
        }

        let parsed = S2Save::from_bytes(&pack_raw(&raw));
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        assert_eq!(
            parsed.objects.candidates(handle).len(),
            2,
            "two candidates are sufficient to preserve ambiguity without storing every duplicate"
        );
    }

    #[test]
    fn unretained_object_candidates_still_bound_the_previous_record() {
        let baseline = S2Save::from_bytes(SYNTHETIC_SAVE);
        assert!(baseline.is_ok());
        let Ok(baseline) = baseline else { return };
        let last_record = baseline
            .objects
            .records
            .iter()
            .max_by_key(|record| record.record_offset);
        assert!(last_record.is_some());
        let Some(last_record) = last_record else { return };
        let handle = last_record.handle;
        let mut raw = SYNTHETIC_RAW.to_vec();
        let next_record_offset = raw.len();
        append_object_candidate(&mut raw, 0x7000_0000);

        let parsed = S2Save::from_bytes(&pack_raw(&raw));
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        let candidate = parsed.objects.candidates(handle).first().copied();
        assert!(candidate.is_some());
        let Some(candidate) = candidate else { return };
        assert_eq!(parsed.objects.record_ends.get(candidate), Some(&next_record_offset));
    }

    fn append_object_candidate(raw: &mut Vec<u8>, handle: u32) {
        let offset = raw.len();
        raw.resize(offset.saturating_add(48), 0);
        raw.get_mut(offset..offset.saturating_add(4))
            .unwrap_or_default()
            .copy_from_slice(&handle.to_le_bytes());
        if let Some(marker) = raw.get_mut(offset.saturating_add(18)) {
            *marker = 0x38;
        }
        raw.get_mut(offset.saturating_add(19)..offset.saturating_add(23))
            .unwrap_or_default()
            .copy_from_slice(&1_u32.to_le_bytes());
        raw.get_mut(offset.saturating_add(24)..offset.saturating_add(28))
            .unwrap_or_default()
            .copy_from_slice(&1.0_f32.to_bits().to_le_bytes());
        if let Some(kind) = raw.get_mut(offset.saturating_add(31)) {
            *kind = 4;
        }
    }

    #[test]
    fn unknown_kind_three_item_remains_visible_but_read_only() -> sse_core::Result<()> {
        let save = S2Save::from_bytes(WRITER_S2_STACK_SOURCE)?;
        let handle = 0x3000_0001;
        let record = save
            .objects
            .unique(handle)
            .ok_or_else(|| Error::Refused("synthetic test object is missing".to_owned()))?;
        let mut image = save.container().image().to_vec();
        super::write_u8_at(&mut image, record.record_offset.saturating_add(31), 3)?;

        // Mutate the known synthetic record only in memory; this does not assert the unknown live layout.
        let packed = pack_raw(&image);
        let unknown = S2Save::from_bytes(&packed)?;
        let item = unknown
            .items()
            .into_iter()
            .find(|item| item.handle == handle)
            .ok_or_else(|| Error::Refused("synthetic kind-three item is missing".to_owned()))?;

        assert_eq!(item.kind_code, 3);
        assert!(!item.editable_count);
        assert!(unknown.unresolved_handles().contains(&handle));
        assert!(unknown
            .warnings()
            .iter()
            .any(|warning| warning.contains("object kind=3, только read-only")));

        let original_image = unknown.container().image().to_vec();
        let result = unknown.write_changes(&[S2Change::SetStackCount { handle, count: 7 }]);
        assert!(matches!(result, Err(Error::Refused(_))));
        assert_eq!(unknown.container().image(), original_image);
        Ok(())
    }

    #[test]
    fn duplicate_owned_handles_and_grid_positions_are_marked_unresolved() {
        let index = S2InventoryIndex::locate(SYNTHETIC_RAW);
        assert!(index.is_ok());
        let Ok(index) = index else { return };

        let mut duplicate_owned = SYNTHETIC_RAW.to_vec();
        let first = index.owned_handles_offset();
        let last = first.saturating_add(12);
        let first_handle = duplicate_owned
            .get(first..first.saturating_add(4))
            .unwrap_or_default()
            .to_vec();
        duplicate_owned
            .get_mut(last..last.saturating_add(4))
            .unwrap_or_default()
            .copy_from_slice(&first_handle);
        let duplicated = S2InventoryIndex::locate(&duplicate_owned);
        assert!(duplicated.is_ok());
        assert!(duplicated.is_ok_and(|value| {
            value.unresolved_handles().contains(&0x3000_0001)
                && value.warnings().iter().any(|warning| warning.contains("duplicate"))
        }));

        let mut duplicate_grid = SYNTHETIC_RAW.to_vec();
        let second_cell_x = index.grid_offset().saturating_add(8).saturating_add(4);
        duplicate_grid
            .get_mut(second_cell_x..second_cell_x.saturating_add(2))
            .unwrap_or_default()
            .copy_from_slice(&0_u16.to_le_bytes());
        let duplicated = S2InventoryIndex::locate(&duplicate_grid);
        assert!(duplicated.is_ok());
        assert!(duplicated.is_ok_and(|value| {
            value.grid_cells().len() == 1
                && value.unresolved_handles().contains(&0x3000_0001)
                && value.unresolved_handles().contains(&0x3000_0002)
        }));
    }

    #[test]
    #[ignore = "manual release throughput measurement"]
    fn release_synthetic_s2_read_timing() {
        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            let parsed = S2Save::from_bytes(std::hint::black_box(SYNTHETIC_SAVE));
            assert!(parsed.is_ok());
        }
        eprintln!(
            "10000 S2 synthetic reads ({} packed bytes, {} image bytes): {:.3} ms",
            SYNTHETIC_SAVE.len(),
            SYNTHETIC_RAW.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
    }

    #[test]
    fn every_container_truncation_and_single_bit_flip_is_rejected() {
        for length in 0..SYNTHETIC_SAVE.len() {
            let prefix = SYNTHETIC_SAVE.get(..length).unwrap_or_default();
            assert!(matches!(S2Container::from_bytes(prefix), Err(Error::Damaged(_))));
        }
        for offset in 0..SYNTHETIC_SAVE.len() {
            for bit in 0..8_u32 {
                let mut changed = SYNTHETIC_SAVE.to_vec();
                let mask = 1_u8.checked_shl(bit).unwrap_or_default();
                let mutated = changed.get_mut(offset).map(|byte| *byte ^= mask);
                assert!(mutated.is_some());
                assert!(matches!(S2Container::from_bytes(&changed), Err(Error::Damaged(_))));
            }
        }
    }

    #[test]
    fn hostile_unpacked_size_is_rejected_after_crc_without_allocating_it() {
        let mut changed = SYNTHETIC_SAVE.to_vec();
        changed
            .get_mut(..4)
            .unwrap_or_default()
            .copy_from_slice(&u32::MAX.to_le_bytes());
        let trailer_offset = changed.len().saturating_sub(4);
        let crc = crc32::crc32(changed.get(..trailer_offset).unwrap_or_default());
        changed
            .get_mut(trailer_offset..)
            .unwrap_or_default()
            .copy_from_slice(&crc.to_le_bytes());
        assert!(matches!(S2Container::from_bytes(&changed), Err(Error::Damaged(_))));
    }

    #[test]
    fn deterministic_container_mutations_never_panic() {
        let mut state = 0x517c_c1b7_u32;
        for _ in 0..512 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let offset = usize::try_from(state).unwrap_or_default() % SYNTHETIC_SAVE.len();
            state = state.rotate_left(11).wrapping_add(0x4a39_b70d);
            let mut changed = SYNTHETIC_SAVE.to_vec();
            let byte = changed.get_mut(offset).map(|value| *value ^= (state & 0xff) as u8);
            assert!(byte.is_some());
            let result = std::panic::catch_unwind(|| S2Container::from_bytes(&changed));
            assert!(result.is_ok(), "container mutation at byte {offset} panicked");
        }
    }

    #[test]
    fn synthetic_stash_handles_and_cells_match_the_fixture() {
        let player = S2InventoryIndex::locate(SYNTHETIC_STASH_RAW);
        let result = player.and_then(|index| {
            S2StashLayout::locate(SYNTHETIC_STASH_RAW, &index).map(|stash| {
                (
                    stash.owned_handles().to_vec(),
                    stash.live_handles().to_vec(),
                    stash.grid_cells().to_vec(),
                    stash.grid_end_offset() > stash.owned_count_offset(),
                )
            })
        });
        assert_eq!(
            result,
            Ok((
                vec![u32::MAX, 0x3000_0010],
                vec![0x3000_0010],
                vec![
                    super::S2GridCell {
                        handle: 0x3000_0010,
                        x: 3,
                        y: 0
                    },
                    super::S2GridCell {
                        handle: 0x3000_0010,
                        x: 3,
                        y: 1
                    },
                ],
                true,
            ))
        );
    }

    #[test]
    fn stash_reader_rejects_truncation_marker_bit_flips_and_hostile_counts() {
        let player = S2InventoryIndex::locate(SYNTHETIC_STASH_RAW);
        assert!(player.is_ok());
        let Ok(player) = player else {
            return;
        };
        let marker_offset = super::find_subslice(SYNTHETIC_STASH_RAW, &super::STASH_MARKER, player.grid_end_offset());
        assert!(marker_offset.is_some());
        let Some(marker_offset) = marker_offset else { return };
        let marker_end = marker_offset.saturating_add(super::STASH_MARKER.len());
        let stash = S2StashLayout::locate(SYNTHETIC_STASH_RAW, &player);
        assert!(stash.is_ok());
        let Ok(stash) = stash else {
            return;
        };
        for length in marker_offset..stash.grid_end_offset() {
            let prefix = SYNTHETIC_STASH_RAW.get(..length).unwrap_or_default();
            assert!(matches!(S2StashLayout::locate(prefix, &player), Err(Error::Damaged(_))));
        }
        for offset in marker_offset..marker_end {
            for bit in 0..8_u32 {
                let mut changed = SYNTHETIC_STASH_RAW.to_vec();
                let changed_byte = changed
                    .get_mut(offset)
                    .map(|byte| *byte ^= 1_u8.checked_shl(bit).unwrap_or_default());
                assert!(changed_byte.is_some());
                assert!(matches!(
                    S2StashLayout::locate(&changed, &player),
                    Err(Error::Damaged(_))
                ));
            }
        }
        let mut hostile = SYNTHETIC_STASH_RAW.to_vec();
        let count_end = stash.owned_count_offset().saturating_add(2);
        hostile
            .get_mut(stash.owned_count_offset()..count_end)
            .unwrap_or_default()
            .copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(matches!(
            S2StashLayout::locate(&hostile, &player),
            Err(Error::Damaged(_))
        ));
    }
}
