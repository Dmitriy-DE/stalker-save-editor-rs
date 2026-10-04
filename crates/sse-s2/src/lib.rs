//! S.T.A.L.K.E.R. 2 containers and read-only save indexes.

use sse_core::{Error, Result, SaveBuffer};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Maximum decoded S2 image admitted by the bounded Rust reader.
pub const MAXIMUM_UNPACKED_SIZE: usize = 256 * 1024 * 1024;
const MAXIMUM_OWNED_HANDLES: usize = 4096;
const MAXIMUM_GRID_CELLS: usize = 8192;
const GRID_WIDTH: u16 = 8;
const WALLET_ANCHOR: [u8; 32] = [
    0x00, 0x38, 0x01, 0x00, 0x00, 0x00, 0x01, 0x10, 0xca, 0xcf, 0xa8, 0x48, 0xc8, 0x95, 0x21, 0x49, 0xb5, 0x1b, 0x94,
    0x44, 0x00, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00,
];
const LEGACY_CONTAINER_ID: [u8; 12] = [0xca, 0xcf, 0xa8, 0x48, 0xc8, 0x95, 0x21, 0x49, 0xb5, 0x1b, 0x94, 0x44];
const STASH_MARKER: [u8; 10] = [0xff, 0xff, 0xff, 0xff, 0x06, 0x01, 0x00, 0x00, 0x00, 0x06];
const STASH_HEADER_TAIL: [u8; 4] = [0x03, 0x00, 0x00, 0x00];

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

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("S2 field offset overflow"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| Error::damaged("S2 container field is truncated"))?;
    let encoded: [u8; 4] = value
        .try_into()
        .map_err(|_| Error::damaged("S2 container field is truncated"))?;
    Ok(u32::from_le_bytes(encoded))
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
        let index = S2InventoryIndex::locate(container.image())?;
        let objects = S2ObjectIndex::build(container.image(), index.is_legacy)?;
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

    /// True when the packed input has the supported S2 container and wallet layout.
    #[must_use]
    pub fn detect(data: &[u8]) -> bool {
        Self::from_bytes(data).is_ok()
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
    #[must_use]
    pub fn money(&self) -> u32 {
        read_u32(self.container.image(), self.index.money_offset).unwrap_or_default()
    }

    /// Wallet-anchor occurrence count (one for current saves, zero for the legacy layout).
    #[must_use]
    pub const fn money_anchor_count(&self) -> usize {
        if self.index.is_legacy {
            0
        } else {
            1
        }
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
    owned_handles: Vec<u32>,
    live_handles: Vec<u32>,
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
            owned_handles,
            live_handles,
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
}

impl S2ObjectIndex {
    fn build(raw: &[u8], legacy: bool) -> Result<Self> {
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
                        return Ok(Some(result));
                    }
                }
            }
            search_from = checked_add(name_start, 1, "S2 name search offset overflow")?;
        }
        Ok(None)
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

    /// Resolves a save-local type key to its display name.
    #[must_use]
    pub fn display_name(&self, type_key: &[u8]) -> Option<&str> {
        self.resolve(type_key)
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
    let mut by_handle = BTreeMap::new();
    for handle in &index.owned_handles {
        if let Some(record) = objects.unique(*handle) {
            by_handle.entry(*handle).or_insert(record.record_offset);
        }
    }
    let mut starts = by_handle.into_iter().collect::<Vec<_>>();
    starts.sort_by_key(|(_, offset)| *offset);
    let mut ends = HashMap::with_capacity(starts.len());
    for (position, (handle, offset)) in starts.iter().enumerate() {
        let fallback = raw_length.min(offset.saturating_add(4096));
        let next = starts
            .get(position.saturating_add(1))
            .map_or(fallback, |(_, next)| *next);
        ends.insert(*handle, next.min(offset.saturating_add(65_536)));
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
    const SCAN_LIMIT: usize = 0x800;
    const PRIMARY_LIMIT: usize = 0x400;
    const MINIMUM_OFFSET: usize = 0x30;
    const MAXIMUM_UPGRADES: usize = 64;
    let minimum_offset = record_offset.checked_add(MINIMUM_OFFSET)?;
    let limit = raw
        .len()
        .min(record_offset.checked_add(SCAN_LIMIT)?)
        .min(record_end.max(record_offset));
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

    if candidates.len() == 1 {
        let (offset, value, modules, upgrades, _) = candidates.pop()?;
        return Some((offset, value, modules, upgrades));
    }
    let mut primary = None;
    for candidate in candidates {
        if candidate.0.saturating_sub(record_offset) >= PRIMARY_LIMIT {
            continue;
        }
        if primary.is_some() {
            return None;
        }
        primary = Some(candidate);
    }
    primary.map(|(offset, value, modules, upgrades, _)| (offset, value, modules, upgrades))
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

/// Wallet and grid offsets plus validated handle references.
pub struct S2InventoryIndex {
    money_offset: usize,
    owned_flag_offset: usize,
    owned_count_offset: usize,
    owned_handles_offset: usize,
    owned_handles: Vec<u32>,
    grid_count_offset: usize,
    grid_offset: usize,
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
        let owned_flag_offset = checked_add(money_offset, 4, "S2 owned flag offset overflow")?;
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
            owned_flag_offset,
            owned_count_offset,
            owned_handles_offset,
            owned_handles,
            grid_count_offset,
            grid_offset,
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

    /// Owned-list flag offset.
    #[must_use]
    pub const fn owned_flag_offset(&self) -> usize {
        self.owned_flag_offset
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

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let end = checked_add(offset, 2, "S2 field offset overflow")?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| Error::damaged("S2 field is truncated"))?;
    let encoded: [u8; 2] = value.try_into().map_err(|_| Error::damaged("S2 field is truncated"))?;
    Ok(u16::from_le_bytes(encoded))
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
    let minimum = judged.saturating_div(2).max(4);
    if !handles.is_empty() && game_handles < minimum {
        return Err(Error::damaged("S2 owned-handle array does not resemble player handles"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{S2Container, S2InventoryIndex, S2Save, S2StashLayout};
    use sse_codecs::crc32;
    use sse_core::Error;

    const SYNTHETIC_SAVE: &[u8] = include_bytes!("../../../fixtures/synthetic/synthetic-s2.sav");
    const SYNTHETIC_RAW: &[u8] = include_bytes!("../../../fixtures/synthetic/synthetic-s2.raw");
    const SYNTHETIC_STASH_RAW: &[u8] = include_bytes!("../../../fixtures/synthetic/synthetic-s2-stash.raw");

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
        assert!(S2Save::detect(&packed));
        let save = S2Save::from_bytes(&packed);
        assert_eq!(
            save.map(|value| (
                value.money(),
                value.money_anchor_count(),
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
                0,
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
            owned_flag_offset: 0,
            owned_count_offset: 0,
            owned_handles_offset: 0,
            owned_handles: vec![handle],
            grid_count_offset: 0,
            grid_offset: 0,
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
            owned_flag_offset: 0,
            owned_count_offset: 0,
            owned_handles_offset: 0,
            owned_handles: vec![handle],
            grid_count_offset: 0,
            grid_offset: 0,
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

        assert!(S2Save::detect(SYNTHETIC_SAVE));
        assert_eq!(
            S2Save::from_bytes(SYNTHETIC_SAVE).map(|value| (
                value.container().image().len(),
                value.money(),
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
