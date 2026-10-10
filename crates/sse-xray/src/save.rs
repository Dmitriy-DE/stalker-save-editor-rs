//! Read-only indexed X-Ray save views.

use sse_core::{Cursor, Error, Result};
use std::collections::HashSet;

use crate::container::{Chunk, Container};

const MAXIMUM_OBJECTS: u32 = 1_000_000;
const MAXIMUM_VECTOR_LENGTH: u32 = 1_000_000;
const MAXIMUM_STRING_LENGTH: usize = 1 << 20;
const SPAWN_MESSAGE: u16 = 1;
const UPDATE_MESSAGE: u16 = 0;
const SPAWN_HAS_VERSION: u16 = 1 << 5;

/// The six supported game formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Shadow of Chernobyl.
    Soc,
    /// Clear Sky.
    Cs,
    /// Call of Pripyat.
    Cop,
    /// Shadow of Chernobyl Enhanced Edition.
    SocEe,
    /// Clear Sky Enhanced Edition.
    CsEe,
    /// Call of Pripyat Enhanced Edition.
    CopEe,
}

/// A read-only save image with a compact index of registry records.
#[derive(Debug)]
pub struct Save {
    format: Format,
    container: Container,
    records: Vec<RegistryObject>,
    actor_id: u16,
    actor_version: u16,
    money: u32,
    money_offset: usize,
    player_faction: Option<i32>,
    pub(crate) player_faction_offset: Option<usize>,
    pub(crate) relation_registry: Option<RelationRegistry>,
    game_time: u64,
    time_factor: f32,
    normal_time_factor: f32,
}

/// One actor-owned inventory object.
#[derive(Debug, Clone, PartialEq)]
pub struct InventoryItem {
    /// Registry handle.
    pub handle: u16,
    /// Section identifier stored in the save.
    pub section: String,
    /// Human-readable category used by the C# command line.
    pub category: String,
    /// Stack size, when the object layout proves one.
    pub count: Option<u16>,
    pub(crate) state_count_offset: Option<usize>,
    pub(crate) update_count_offset: Option<usize>,
    /// Placement name; `None` means the reader did not prove placement.
    pub placement: Option<String>,
    pub(crate) placement_offset: Option<usize>,
    pub(crate) placement_value: Option<u16>,
    pub(crate) placement_width: Option<usize>,
    pub(crate) placement_base_slot: Option<u8>,
    /// Confirmed equipment condition in the serialized STATE field.
    pub condition: Option<f32>,
    /// Durability can be changed only when the matching UPDATE byte is unique and proven.
    pub durability_editable: bool,
    pub(crate) condition_offset: Option<usize>,
    pub(crate) update_condition_offset: Option<usize>,
    pub(crate) client_condition_offset: Option<usize>,
}

pub(crate) struct PlacementFields {
    pub(crate) category: String,
    pub(crate) offset: usize,
    pub(crate) packed: u16,
    pub(crate) width: usize,
    pub(crate) base_slot: Option<u8>,
}

/// Indexed registry record. Offsets address the decompressed image and preserve all unknown bytes.
#[derive(Debug, Clone)]
pub struct RegistryObject {
    /// Object message name.
    pub name: String,
    /// Section or replacement name from the spawn record.
    pub name_replace: String,
    /// SPAWN byte range containing `name_replace`, including its zero terminator.
    pub(crate) name_replace_range: std::ops::Range<usize>,
    /// Registry object id.
    pub object_id: u16,
    /// Parent object id.
    pub parent_id: u16,
    /// Raw-image offset of the object id inside M_SPAWN.
    pub object_id_offset: usize,
    /// Raw-image offset of the parent id inside M_SPAWN.
    pub parent_id_offset: usize,
    /// Spawn serialization version.
    pub version: u16,
    /// SPAWN identifier, when the version serializes one.
    pub spawn_id: Option<u16>,
    /// Byte offset of `spawn_id` in the unpacked image.
    pub(crate) spawn_id_offset: Option<usize>,
    /// Story identifier from STATE (`u32::MAX` means no story object).
    pub story_id: Option<u32>,
    /// Byte offset of `story_id` in the unpacked image.
    pub(crate) story_id_offset: Option<usize>,
    /// Spawn story identifier from STATE (`u32::MAX` means none).
    pub spawn_story_id: Option<u32>,
    /// Byte offset of `spawn_story_id` in the unpacked image.
    pub(crate) spawn_story_id_offset: Option<usize>,
    /// SPAWN STATE custom-data byte range, including its zero terminator.
    pub(crate) custom_data_range: Option<std::ops::Range<usize>>,
    /// Complete record start in the raw image.
    pub record_offset: usize,
    /// Complete record byte length.
    pub record_length: usize,
    /// State payload start in the raw image.
    pub state_offset: usize,
    /// State payload byte length.
    pub state_length: usize,
    /// Update packet start in the raw image.
    pub update_offset: usize,
    /// Update packet byte length.
    pub update_length: usize,
    /// Client data payload start, if present.
    pub client_data_offset: Option<usize>,
    /// Client data byte length.
    pub client_data_length: usize,
}

/// Validated creature-state prefix read from one registry object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CreatureVitals<'a> {
    /// Registry object identifier.
    pub object_id: u16,
    /// Object name from the SPAWN record.
    pub name: &'a str,
    /// Replacement section name from the SPAWN record.
    pub name_replace: &'a str,
    /// Health value stored in the creature STATE.
    pub health: f32,
    /// Last killer, when this object version serializes it.
    pub killer_id: Option<u16>,
    /// Game death time, when this object version serializes it.
    pub death_time: Option<u64>,
}

impl CreatureVitals<'_> {
    /// Whether the parsed, finite health value marks this creature as dead.
    #[must_use]
    pub const fn is_dead(self) -> bool {
        self.health <= 0.0
    }
}

struct DynamicVisualFields {
    custom_data_range: Option<std::ops::Range<usize>>,
    story_id: Option<u32>,
    story_id_offset: Option<usize>,
    spawn_story_id: Option<u32>,
    spawn_story_id_offset: Option<usize>,
}

type ObjectRecord = RegistryObject;

#[derive(Debug, Clone)]
pub(crate) struct RelationRegistry {
    pub(crate) info_section_end: usize,
    pub(crate) info_rows: Vec<InfoPortionRow>,
    pub(crate) relation_rows: Vec<RelationRow>,
}

#[derive(Debug, Clone)]
pub(crate) struct InfoPortionRow {
    pub(crate) object_id: u16,
    pub(crate) count_offset: usize,
    pub(crate) end_offset: usize,
    pub(crate) names: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RelationRow {
    pub(crate) object_id: u16,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) community_count_offset: usize,
    pub(crate) communities: Vec<CommunityRelation>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CommunityRelation {
    pub(crate) community_id: i32,
    pub(crate) goodwill: i32,
    pub(crate) goodwill_offset: usize,
}

impl Format {
    /// The stable C# release identifier.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Soc => "stalker-soc",
            Self::Cs => "stalker-cs",
            Self::Cop => "stalker-cop",
            Self::SocEe => "stalker-soc-ee",
            Self::CsEe => "stalker-cs-ee",
            Self::CopEe => "stalker-cop-ee",
        }
    }
}

impl Save {
    /// Reads an X-Ray save by its contents.
    pub fn read(packed: &[u8]) -> Result<Self> {
        let container = Container::read(packed)?;
        let alife = required_chunk(&container, 0)?;
        let alife_data = container.chunk_bytes(alife)?;
        if alife_data.len() != 4 {
            return Err(Error::damaged("X-Ray ALIFE chunk must contain one u32"));
        }
        let mut alife_reader = Cursor::new(alife_data);
        let alife_version = alife_reader.u32()?;

        let time = required_chunk(&container, 5)?;
        let time_data = container.chunk_bytes(time)?;
        if time_data.len() < 16 {
            return Err(Error::damaged("X-Ray GAME_TIME chunk is shorter than 16 bytes"));
        }
        let mut time_reader = Cursor::new(time_data);
        let game_time = time_reader.u64()?;
        let time_factor = time_reader.f32()?;
        let normal_time_factor = time_reader.f32()?;
        if !time_factor.is_finite() || !normal_time_factor.is_finite() {
            return Err(Error::damaged("X-Ray GAME_TIME has a non-finite time factor"));
        }

        let object_chunk = required_chunk(&container, 2)?;
        let object_data = container.chunk_bytes(object_chunk)?;
        let format = detect_format(container.version(), alife_version, object_data)?;
        let records = parse_objects(&container, object_chunk)?;
        let mut actor: Option<ObjectRecord> = None;
        for record in &records {
            if record.name.eq_ignore_ascii_case("actor") {
                if actor.is_some() {
                    return Err(Error::damaged("X-Ray save has more than one actor object"));
                }
                actor = Some(record.clone());
            }
        }
        let actor = actor.ok_or_else(|| Error::damaged("X-Ray save has no actor object"))?;
        if !actor_version_supported(format, actor.version) {
            return Err(Error::Refused(format!(
                "unsupported actor spawn version {} for {}",
                actor.version,
                format.id()
            )));
        }
        let (money, money_offset, player_faction, player_faction_offset) =
            read_actor_fields(container.image(), &actor)?;
        let relation_registry = read_relation_registry(&container, format, actor.object_id);

        Ok(Self {
            format,
            container,
            records,
            actor_id: actor.object_id,
            actor_version: actor.version,
            money,
            money_offset,
            player_faction,
            player_faction_offset,
            relation_registry,
            game_time,
            time_factor,
            normal_time_factor,
        })
    }

    /// Detected game format.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// Container version.
    #[must_use]
    pub const fn container_version(&self) -> u32 {
        self.container.version()
    }

    /// Raw image length in bytes.
    #[must_use]
    pub fn raw_size(&self) -> usize {
        self.container.image().len()
    }

    /// Actor registry version.
    #[must_use]
    pub const fn actor_version(&self) -> u16 {
        self.actor_version
    }

    /// Registry object metadata and byte ranges for this save.
    #[must_use]
    pub fn registry_objects(&self) -> &[RegistryObject] {
        &self.records
    }

    /// Returns an object's custom data as borrowed bytes without copying the save image.
    #[must_use]
    pub fn custom_data<'a>(&'a self, object: &RegistryObject) -> Option<&'a [u8]> {
        let range = object.custom_data_range.as_ref()?;
        let content_end = range.end.checked_sub(1)?;
        self.container.image().get(range.start..content_end)
    }

    /// In-game clock value.
    #[must_use]
    pub const fn game_time(&self) -> u64 {
        self.game_time
    }

    /// In-game time factor.
    #[must_use]
    pub const fn time_factor(&self) -> f32 {
        self.time_factor
    }

    /// Normal in-game time factor.
    #[must_use]
    pub const fn normal_time_factor(&self) -> f32 {
        self.normal_time_factor
    }

    /// Actor wallet value.
    pub const fn money(&self) -> Result<u32> {
        Ok(self.money)
    }

    /// Player faction numeric identifier if its actor STATE field is recognized.
    #[must_use]
    pub const fn player_faction(&self) -> Option<i32> {
        self.player_faction
    }

    /// Actor-to-community goodwill values from the validated relation registry.
    #[must_use]
    pub fn actor_relations(&self) -> Option<Vec<(i32, i32)>> {
        let registry = self.relation_registry.as_ref()?;
        let actor = registry
            .relation_rows
            .iter()
            .find(|row| row.object_id == self.actor_id)?;
        Some(
            actor
                .communities
                .iter()
                .map(|relation| (relation.community_id, relation.goodwill))
                .collect(),
        )
    }

    /// Actor info portions from the validated relation registry, or `None` when the registry is unavailable.
    #[must_use]
    pub fn actor_known_info(&self) -> Option<&[String]> {
        let registry = self.relation_registry.as_ref()?;
        registry
            .info_rows
            .iter()
            .find(|row| row.object_id == self.actor_id)
            .map(|row| row.names.as_slice())
    }

    /// Reads supported creature-state prefixes for objects matching a known section, as in the reference reader.
    #[must_use]
    pub fn find_creature_vitals(&self, section: &str) -> Vec<CreatureVitals<'_>> {
        self.records
            .iter()
            .filter(|record| record.name == section || record.name_replace == section)
            .filter_map(|record| read_creature_vitals(self.container.image(), record))
            .collect()
    }

    /// Registry id of the save's unique actor object.
    #[must_use]
    pub const fn actor_id(&self) -> u16 {
        self.actor_id
    }

    pub(crate) fn raw_image(&self) -> &[u8] {
        self.container.image()
    }

    pub(crate) fn chunks(&self) -> &[crate::container::Chunk] {
        self.container.chunks()
    }

    pub(crate) const fn money_offset(&self) -> usize {
        self.money_offset
    }

    pub(crate) fn repack(&self, raw: &[u8]) -> Result<sse_core::SaveBuffer> {
        self.container.repack(raw)
    }

    pub(crate) fn object_chunk_bytes<'a>(&self, raw: &'a [u8]) -> Result<&'a [u8]> {
        let mut found = None;
        for chunk in self.container.chunks() {
            if chunk.kind == 2 {
                if found.is_some() {
                    return Err(Error::damaged("duplicate X-Ray OBJECT chunks"));
                }
                let end = chunk
                    .offset
                    .checked_add(chunk.length)
                    .ok_or_else(|| Error::damaged("X-Ray OBJECT chunk range overflow"))?;
                found = Some(
                    raw.get(chunk.offset..end)
                        .ok_or_else(|| Error::damaged("X-Ray OBJECT chunk is outside the image"))?,
                );
            }
        }
        found.ok_or_else(|| Error::damaged("missing X-Ray OBJECT chunk"))
    }

    #[cfg(test)]
    pub(crate) fn rebuild_chunks(&self, raw: &[u8], replacements: &[(u32, &[u8])]) -> Result<Vec<u8>> {
        self.container.rebuild_chunk_payloads(raw, replacements)
    }

    pub(crate) fn rebuild_chunks_in_place(&self, raw: &mut Vec<u8>, replacements: &[(u32, &[u8])]) -> Result<()> {
        self.container.rebuild_chunk_payloads_in_place(raw, replacements)
    }

    pub(crate) fn relation_chunk_bytes<'a>(&self, raw: &'a [u8]) -> Result<&'a [u8]> {
        let mut found = None;
        for chunk in self.container.chunks() {
            if chunk.kind == 9 {
                if found.is_some() {
                    return Err(Error::damaged("duplicate X-Ray relation chunks"));
                }
                let end = chunk
                    .offset
                    .checked_add(chunk.length)
                    .ok_or_else(|| Error::damaged("X-Ray relation chunk range overflow"))?;
                found = Some(
                    raw.get(chunk.offset..end)
                        .ok_or_else(|| Error::damaged("X-Ray relation chunk is outside the image"))?,
                );
            }
        }
        found.ok_or_else(|| Error::damaged("missing X-Ray relation chunk"))
    }

    pub(crate) const fn relation_has_timestamps(&self) -> bool {
        matches!(self.format, Format::Soc | Format::Cs)
    }

    /// Actor-owned inventory entries.
    pub fn inventory(&self) -> Result<Vec<InventoryItem>> {
        let mut items = Vec::new();
        for record in &self.records {
            if record.parent_id != self.actor_id || record.object_id == self.actor_id {
                continue;
            }
            let placement_fields = read_placement_fields(self.container.image(), record, self.format)?;
            let condition_fields = read_condition_fields(self.container.image(), record);
            let stack = read_ammo_count(self.container.image(), record);
            items.push(InventoryItem {
                handle: record.object_id,
                section: record.name.clone(),
                category: category_for_name(&record.name).to_owned(),
                count: stack.map(|value| value.0),
                state_count_offset: stack.map(|value| value.1),
                update_count_offset: stack.map(|value| value.2),
                placement: placement_fields.as_ref().map(|fields| fields.category.clone()),
                placement_offset: placement_fields.as_ref().map(|fields| fields.offset),
                placement_value: placement_fields.as_ref().map(|fields| fields.packed),
                placement_width: placement_fields.as_ref().map(|fields| fields.width),
                placement_base_slot: placement_fields.as_ref().and_then(|fields| fields.base_slot),
                condition: condition_fields.map(|fields| fields.0),
                durability_editable: condition_fields.is_some_and(|fields| fields.2.is_some()),
                condition_offset: condition_fields.map(|fields| fields.1),
                update_condition_offset: condition_fields.and_then(|fields| fields.2),
                client_condition_offset: condition_fields.and_then(|fields| fields.3),
            });
        }
        items.sort_by_key(|item| item.handle);
        Ok(items)
    }

    /// Destinations whose state suffix has one unique, structurally valid restrictor boundary.
    pub fn level_changer_destinations(&self) -> Result<Vec<(u16, crate::LevelChangerDestination)>> {
        let mut destinations = Vec::new();
        for record in &self.records {
            if !record.name.eq_ignore_ascii_case("level_changer") {
                continue;
            }
            let end = record
                .state_offset
                .checked_add(record.state_length)
                .ok_or_else(|| Error::damaged("X-Ray level-changer state range overflow"))?;
            let state = self
                .container
                .image()
                .get(record.state_offset..end)
                .ok_or_else(|| Error::damaged("X-Ray level-changer state is outside the image"))?;
            if let Some(destination) = crate::level_changer::find_destination(state, record.version) {
                destinations.push((record.object_id, destination));
            }
        }
        Ok(destinations)
    }
}

fn required_chunk(container: &Container, kind: u32) -> Result<Chunk> {
    let mut found = None;
    for chunk in container.chunks() {
        if chunk.kind == kind {
            if found.is_some() {
                return Err(Error::damaged(format!("duplicate X-Ray chunk type {kind}")));
            }
            found = Some(*chunk);
        }
    }
    found.ok_or_else(|| Error::damaged(format!("missing X-Ray chunk type {kind}")))
}

fn detect_format(container_version: u32, alife_version: u32, objects: &[u8]) -> Result<Format> {
    match (container_version, alife_version) {
        (3, 3) => Ok(Format::Soc),
        (5, 5) => Ok(Format::Cs),
        (6, 6) => Ok(Format::Cop),
        (3, 51) => Ok(Format::SocEe),
        (6, 54) => {
            // A complete NUL-terminated level name is evidence; an item section containing the word is not.
            let mut has_marsh = false;
            let mut has_zaton = false;
            for string in objects.split(|byte| *byte == 0) {
                has_marsh |= string == b"marsh";
                has_zaton |= string == b"zaton";
            }
            match (has_marsh, has_zaton) {
                (true, false) => Ok(Format::CsEe),
                (false, true) => Ok(Format::CopEe),
                _ => Err(Error::Refused(
                    "X-Ray Enhanced Edition level markers are missing or ambiguous".to_owned(),
                )),
            }
        }
        _ => Err(Error::Refused(format!(
            "unsupported X-Ray container/ALIFE version pair {container_version}/{alife_version}"
        ))),
    }
}

fn actor_version_supported(format: Format, version: u16) -> bool {
    match format {
        Format::Soc | Format::SocEe => version == 118,
        Format::Cs => [122, 123, 124].contains(&version),
        Format::Cop | Format::CsEe | Format::CopEe => version == 128,
    }
}

fn parse_objects(container: &Container, chunk: Chunk) -> Result<Vec<ObjectRecord>> {
    let data = container.chunk_bytes(chunk)?;
    let mut reader = Cursor::new(data);
    let count = reader.u32()?;
    if count == 0 || count > MAXIMUM_OBJECTS {
        return Err(Error::damaged(format!(
            "X-Ray object count {count} is outside its limit"
        )));
    }
    let count_usize =
        usize::try_from(count).map_err(|_| Error::damaged("X-Ray object count does not fit this platform"))?;
    if count_usize > reader.remaining() / 4 {
        return Err(Error::damaged(
            "X-Ray object count exceeds the available record headers",
        ));
    }
    let mut records = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for _ in 0..count {
        let record_start = reader.position();
        let spawn_length = usize::from(reader.u16()?);
        let spawn_offset = chunk
            .offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("X-Ray spawn offset overflow"))?;
        let spawn = reader.take(spawn_length)?;
        let mut record = parse_spawn(spawn, spawn_offset)?;
        let update_length = usize::from(reader.u16()?);
        let update_offset = chunk
            .offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("X-Ray update offset overflow"))?;
        let update = reader.take(update_length)?;
        let mut update_reader = Cursor::new(update);
        if update_reader.remaining() < 2 || update_reader.u16()? != UPDATE_MESSAGE {
            return Err(Error::damaged(format!(
                "object '{}' has no M_UPDATE packet",
                record.name
            )));
        }
        if !ids.insert(record.object_id) {
            return Err(Error::damaged(format!(
                "duplicate X-Ray object id 0x{:04X}",
                record.object_id
            )));
        }
        record.update_offset = update_offset;
        record.update_length = update_length;
        record.record_offset = chunk
            .offset
            .checked_add(record_start)
            .ok_or_else(|| Error::damaged("X-Ray record offset overflow"))?;
        record.record_length = reader
            .position()
            .checked_sub(record_start)
            .ok_or_else(|| Error::damaged("X-Ray record length underflow"))?;
        records.push(record);
    }
    if reader.remaining() != 0 {
        return Err(Error::damaged("X-Ray OBJECT chunk has trailing bytes"));
    }
    Ok(records)
}

pub(crate) fn parse_spawn(packet: &[u8], packet_offset: usize) -> Result<ObjectRecord> {
    let mut reader = Cursor::new(packet);
    if reader.u16()? != SPAWN_MESSAGE {
        return Err(Error::damaged("X-Ray object does not begin with M_SPAWN"));
    }
    let name = decode_cp1251(reader.zero_terminated(MAXIMUM_STRING_LENGTH)?);
    let name_replace_start = reader.position();
    let name_replace = decode_cp1251(reader.zero_terminated(MAXIMUM_STRING_LENGTH)?);
    let name_replace_range = packet_offset
        .checked_add(name_replace_start)
        .ok_or_else(|| Error::damaged("X-Ray name-replacement offset overflow"))?
        ..packet_offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("X-Ray name-replacement range overflow"))?;
    reader.skip(2)?;
    reader.skip(6 * 4)?;
    reader.skip(2)?;
    let object_id_offset = packet_offset
        .checked_add(reader.position())
        .ok_or_else(|| Error::damaged("X-Ray object id offset overflow"))?;
    let object_id = reader.u16()?;
    let parent_id_offset = packet_offset
        .checked_add(reader.position())
        .ok_or_else(|| Error::damaged("X-Ray parent id offset overflow"))?;
    let parent_id = reader.u16()?;
    reader.skip(2)?;
    let flags = reader.u16()?;
    if flags & SPAWN_HAS_VERSION == 0 {
        return Err(Error::damaged(format!("object '{name}' is missing M_SPAWN_VERSION")));
    }
    let version = reader.u16()?;
    if version < 112 {
        return Err(Error::damaged(format!(
            "object '{name}' spawn version {version} is too old"
        )));
    }
    if version > 120 {
        reader.skip(2)?;
    }
    if version > 69 {
        reader.skip(2)?;
    }
    let (client_data_offset, client_data_length) = if version > 70 {
        let length = if version > 93 {
            usize::from(reader.u16()?)
        } else {
            usize::from(reader.u8()?)
        };
        let offset = packet_offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("X-Ray client-data offset overflow"))?;
        reader.skip(length)?;
        (Some(offset), length)
    } else {
        (None, 0)
    };
    let (spawn_id, spawn_id_offset) = if version > 79 {
        let offset = packet_offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("X-Ray spawn-id offset overflow"))?;
        (Some(reader.u16()?), Some(offset))
    } else {
        (None, None)
    };
    let state_size = usize::from(reader.u16()?);
    if state_size < 2 {
        return Err(Error::damaged(format!(
            "object '{name}' STATE is shorter than its size field"
        )));
    }
    let state_length = state_size
        .checked_sub(2)
        .ok_or_else(|| Error::damaged("X-Ray state size underflow"))?;
    if reader.remaining() != state_length {
        return Err(Error::damaged(format!(
            "object '{name}' STATE length does not match its SPAWN packet"
        )));
    }
    let state_offset = packet_offset
        .checked_add(reader.position())
        .ok_or_else(|| Error::damaged("X-Ray state offset overflow"))?;
    let state = packet
        .get(reader.position()..)
        .ok_or_else(|| Error::damaged("X-Ray STATE starts outside its SPAWN packet"))?;
    let mut state_reader = Cursor::new(state);
    let dynamic_fields = read_dynamic_visual_fields(&mut state_reader, version, state_offset)?;
    reader.skip(state_length)?;
    Ok(ObjectRecord {
        name,
        name_replace,
        name_replace_range,
        object_id,
        parent_id,
        object_id_offset,
        parent_id_offset,
        version,
        spawn_id,
        spawn_id_offset,
        story_id: dynamic_fields.story_id,
        story_id_offset: dynamic_fields.story_id_offset,
        spawn_story_id: dynamic_fields.spawn_story_id,
        spawn_story_id_offset: dynamic_fields.spawn_story_id_offset,
        custom_data_range: dynamic_fields.custom_data_range,
        record_offset: 0,
        record_length: 0,
        state_offset,
        state_length,
        update_offset: 0,
        update_length: 0,
        client_data_offset,
        client_data_length,
    })
}

fn read_creature_vitals<'a>(raw: &[u8], item: &'a RegistryObject) -> Option<CreatureVitals<'a>> {
    if item.version <= 18 || item.state_length == 0 || item.version < 105 {
        return None;
    }
    let state_end = item.state_offset.checked_add(item.state_length)?;
    let state = raw.get(item.state_offset..state_end)?;
    let mut reader = Cursor::new(state);

    // Human stalkers serialize trader fields before the dynamic-visual and creature fields.
    reader.skip(4).ok()?;
    reader.zero_terminated(MAXIMUM_STRING_LENGTH).ok()?;
    reader.skip(4).ok()?;
    reader.zero_terminated(MAXIMUM_STRING_LENGTH).ok()?;
    reader.skip(12).ok()?;
    reader.zero_terminated(MAXIMUM_STRING_LENGTH).ok()?;
    if item.version > 124 {
        reader.skip(2).ok()?;
    }

    skip_dynamic_visual(&mut reader, item.version).ok()?;
    reader.skip(3).ok()?;
    let health = reader.f32().ok()?;
    if item.version < 32 {
        reader.zero_terminated(MAXIMUM_STRING_LENGTH).ok()?;
    }
    if item.version > 87 {
        skip_u16_vector(&mut reader).ok()?;
        skip_u16_vector(&mut reader).ok()?;
    }
    let killer_id = if item.version > 94 {
        Some(reader.u16().ok()?)
    } else {
        None
    };
    let death_time = if item.version > 115 {
        Some(reader.u64().ok()?)
    } else {
        None
    };
    if !health.is_finite() || !(-1.0..=1.0).contains(&health) {
        return None;
    }
    Some(CreatureVitals {
        object_id: item.object_id,
        name: &item.name,
        name_replace: &item.name_replace,
        health,
        killer_id,
        death_time,
    })
}

fn read_actor_fields(raw: &[u8], actor: &ObjectRecord) -> Result<(u32, usize, Option<i32>, Option<usize>)> {
    let state_end = actor
        .state_offset
        .checked_add(actor.state_length)
        .ok_or_else(|| Error::damaged("X-Ray actor state range overflow"))?;
    let state = raw
        .get(actor.state_offset..state_end)
        .ok_or_else(|| Error::damaged("X-Ray actor state is outside the image"))?;
    let mut reader = Cursor::new(state);
    skip_dynamic_visual(&mut reader, actor.version)?;
    reader.skip(3)?;
    if actor.version > 18 {
        let _health = reader.f32()?;
    }
    if actor.version < 32 {
        let _legacy = reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
    }
    if actor.version > 87 {
        skip_u16_vector(&mut reader)?;
        skip_u16_vector(&mut reader)?;
    }
    if actor.version > 94 {
        reader.skip(2)?;
    }
    if actor.version > 115 {
        reader.skip(8)?;
    }
    if actor.version > 19 && actor.version < 108 {
        reader.skip(4)?;
    }
    if actor.version <= 62 {
        return Err(Error::damaged("X-Ray actor version has no supported wallet field"));
    }
    let offset = actor
        .state_offset
        .checked_add(reader.position())
        .ok_or_else(|| Error::damaged("X-Ray actor money offset overflow"))?;
    let money = reader.u32()?;
    if actor.version > 75 && actor.version < 98 {
        reader.skip(4)?;
    } else if actor.version >= 98 {
        let _specific_character = reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
    }
    if actor.version > 77 {
        reader.skip(4)?;
    }
    if actor.version > 81 && actor.version < 96 {
        reader.skip(4)?;
    } else if actor.version > 95 {
        let _character_profile = reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
    }
    let (player_faction, faction_offset) = if actor.version > 85 {
        let offset = actor
            .state_offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("X-Ray actor faction offset overflow"))?;
        (Some(i32::from_le_bytes(reader.u32()?.to_le_bytes())), Some(offset))
    } else {
        (None, None)
    };
    Ok((money, offset, player_faction, faction_offset))
}

fn read_relation_registry(container: &Container, format: Format, _actor_id: u16) -> Option<RelationRegistry> {
    let has_timestamps = match format {
        Format::Soc | Format::Cs => true,
        Format::Cop => false,
        Format::SocEe | Format::CsEe | Format::CopEe => return None,
    };
    let mut relation_chunk = None;
    for chunk in container.chunks() {
        if chunk.kind == 9 {
            if relation_chunk.is_some() {
                return None;
            }
            relation_chunk = Some(chunk);
        }
    }
    let payload = container.chunk_bytes(*relation_chunk?).ok()?;
    parse_relation_registry(payload, has_timestamps).ok()
}

pub(crate) fn parse_relation_registry(payload: &[u8], has_timestamps: bool) -> Result<RelationRegistry> {
    const MAXIMUM_COUNT: u32 = 1_000_000;
    let mut reader = Cursor::new(payload);
    let info_count = reader.u32()?;
    if info_count > MAXIMUM_COUNT
        || usize::try_from(info_count)
            .ok()
            .is_none_or(|count| count > reader.remaining() / 6)
    {
        return Err(Error::damaged("X-Ray info-portion object count exceeds its chunk"));
    }
    let mut info_rows = Vec::with_capacity(usize::try_from(info_count).unwrap_or_default());
    let mut info_ids = HashSet::new();
    for _ in 0..info_count {
        let object_id = reader.u16()?;
        if !info_ids.insert(object_id) {
            return Err(Error::damaged("X-Ray info-portion map repeats an object id"));
        }
        let count_offset = reader.position();
        let count = reader.u32()?;
        let minimum_width = if has_timestamps { 9_usize } else { 1 };
        if count > MAXIMUM_COUNT
            || usize::try_from(count).ok().is_none_or(|items| {
                reader
                    .remaining()
                    .checked_div(minimum_width)
                    .is_none_or(|maximum_items| items > maximum_items)
            })
        {
            return Err(Error::damaged("X-Ray info-portion vector exceeds its chunk"));
        }
        let mut names = Vec::with_capacity(usize::try_from(count).unwrap_or_default());
        for _ in 0..count {
            let bytes = reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
            names.push(decode_cp1251(bytes));
            if has_timestamps {
                reader.skip(8)?;
            }
        }
        info_rows.push(InfoPortionRow {
            object_id,
            count_offset,
            end_offset: reader.position(),
            names,
        });
    }
    let info_section_end = reader.position();
    let row_count = reader.u32()?;
    if row_count > MAXIMUM_COUNT
        || usize::try_from(row_count)
            .ok()
            .is_none_or(|count| count > reader.remaining() / 10)
    {
        return Err(Error::damaged("X-Ray relation row count exceeds its chunk"));
    }
    let mut relation_rows = Vec::with_capacity(usize::try_from(row_count).unwrap_or_default());
    let mut character_ids = HashSet::new();
    for _ in 0..row_count {
        let start = reader.position();
        let object_id = reader.u16()?;
        if !character_ids.insert(object_id) {
            return Err(Error::damaged("X-Ray relation registry repeats a character id"));
        }
        let personal_count = reader.u32()?;
        if personal_count > MAXIMUM_COUNT
            || usize::try_from(personal_count)
                .ok()
                .is_none_or(|count| count > reader.remaining() / 6)
        {
            return Err(Error::damaged("X-Ray personal relation vector exceeds its chunk"));
        }
        let mut personal_ids = HashSet::new();
        for _ in 0..personal_count {
            let target_id = reader.u16()?;
            if !personal_ids.insert(target_id) {
                return Err(Error::damaged("X-Ray personal relation vector repeats a target id"));
            }
            reader.skip(4)?;
        }
        let community_count_offset = reader.position();
        let community_count = reader.u32()?;
        if community_count > MAXIMUM_COUNT
            || usize::try_from(community_count)
                .ok()
                .is_none_or(|count| count > reader.remaining() / 8)
        {
            return Err(Error::damaged("X-Ray community relation vector exceeds its chunk"));
        }
        let mut communities = Vec::with_capacity(usize::try_from(community_count).unwrap_or_default());
        let mut community_ids = HashSet::new();
        for _ in 0..community_count {
            let community_id = i32::from_le_bytes(reader.u32()?.to_le_bytes());
            let goodwill_offset = reader.position();
            let goodwill = i32::from_le_bytes(reader.u32()?.to_le_bytes());
            if !community_ids.insert(community_id) {
                return Err(Error::damaged("X-Ray community relation vector repeats an id"));
            }
            communities.push(CommunityRelation {
                community_id,
                goodwill,
                goodwill_offset,
            });
        }
        relation_rows.push(RelationRow {
            object_id,
            start,
            end: reader.position(),
            community_count_offset,
            communities,
        });
    }
    Ok(RelationRegistry {
        info_section_end,
        info_rows,
        relation_rows,
    })
}

pub(crate) fn skip_dynamic_visual(reader: &mut Cursor<'_>, version: u16) -> Result<()> {
    let _ = read_dynamic_visual_fields(reader, version, 0)?;
    Ok(())
}

fn read_dynamic_visual_fields(
    reader: &mut Cursor<'_>,
    version: u16,
    image_offset: usize,
) -> Result<DynamicVisualFields> {
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
        reader.skip(2 + 4)?;
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
        let _legacy = reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
    }
    if version > 49 {
        reader.skip(4)?;
    }
    let custom_data_range = if version > 57 {
        let start = image_offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("X-Ray custom-data offset overflow"))?;
        reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
        let end = image_offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("X-Ray custom-data range overflow"))?;
        Some(start..end)
    } else {
        None
    };
    let (story_id, story_id_offset) = if version > 61 {
        let offset = image_offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("X-Ray story-id offset overflow"))?;
        (Some(reader.u32()?), Some(offset))
    } else {
        (None, None)
    };
    let (spawn_story_id, spawn_story_id_offset) = if version > 111 {
        let offset = image_offset
            .checked_add(reader.position())
            .ok_or_else(|| Error::damaged("X-Ray spawn-story-id offset overflow"))?;
        (Some(reader.u32()?), Some(offset))
    } else {
        (None, None)
    };
    if version > 31 {
        let _visual = reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
        if version > 103 {
            reader.skip(1)?;
        }
    }
    Ok(DynamicVisualFields {
        custom_data_range,
        story_id,
        story_id_offset,
        spawn_story_id,
        spawn_story_id_offset,
    })
}

fn skip_u16_vector(reader: &mut Cursor<'_>) -> Result<()> {
    let count = reader.u32()?;
    if count > MAXIMUM_VECTOR_LENGTH {
        return Err(Error::damaged(format!("X-Ray vector count {count} exceeds its limit")));
    }
    let count = usize::try_from(count).map_err(|_| Error::damaged("X-Ray vector count does not fit this platform"))?;
    let length = count
        .checked_mul(2)
        .ok_or_else(|| Error::damaged("X-Ray vector byte length overflow"))?;
    reader.skip(length)
}

fn skip_string_vector(reader: &mut Cursor<'_>) -> Result<()> {
    let count = reader.u32()?;
    if count > MAXIMUM_VECTOR_LENGTH {
        return Err(Error::damaged(format!(
            "X-Ray string-vector count {count} exceeds its limit"
        )));
    }
    for _ in 0..count {
        let _value = reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
    }
    Ok(())
}

fn read_ammo_count(raw: &[u8], record: &ObjectRecord) -> Option<(u16, usize, usize)> {
    if !record.name.starts_with("ammo_") && !record.name.to_ascii_lowercase().starts_with("ammo_") {
        return None;
    }
    let end = record.state_offset.checked_add(record.state_length)?;
    let state = raw.get(record.state_offset..end)?;
    let mut reader = Cursor::new(state);
    skip_dynamic_visual(&mut reader, record.version).ok()?;
    if record.version > 52 {
        reader.skip(4).ok()?;
    }
    if record.version > 123 {
        skip_string_vector(&mut reader).ok()?;
    }
    let state_count_offset = record.state_offset.checked_add(reader.position())?;
    let count = reader.u16().ok()?;
    if record.update_length < 5 {
        return None;
    }
    let update_end = record.update_offset.checked_add(record.update_length)?;
    let update_count_offset = update_end.checked_sub(std::mem::size_of::<u16>())?;
    let update_count = raw.get(update_count_offset..update_end)?;
    <[u8; 2]>::try_from(update_count).ok()?;
    Some((count, state_count_offset, update_count_offset))
}

pub(crate) fn read_placement_fields(
    raw: &[u8],
    record: &RegistryObject,
    format: Format,
) -> Result<Option<PlacementFields>> {
    let Some(start) = record.client_data_offset else {
        return Ok(None);
    };
    let client_end = start
        .checked_add(record.client_data_length)
        .ok_or_else(|| Error::damaged("X-Ray client-data range overflows"))?;
    let Some(client_data) = raw.get(start..client_end) else {
        return Ok(None);
    };
    let offset = start
        .checked_add(1)
        .ok_or_else(|| Error::damaged("X-Ray placement offset overflow"))?;
    if format == Format::Cs && client_data.len() == 2 && client_data.first() == Some(&2) {
        let Some(value) = client_data.get(1).copied().filter(|value| (1..=3).contains(value)) else {
            return Ok(None);
        };
        let category = match value {
            1 => "slot",
            2 => "belt",
            3 => "ruck",
            _ => return Ok(None),
        };
        return Ok(Some(PlacementFields {
            category: category.to_owned(),
            offset,
            packed: u16::from(value),
            width: 1,
            base_slot: None,
        }));
    }
    if client_data.len() < 3 {
        return Ok(None);
    }
    let end = offset
        .checked_add(2)
        .ok_or_else(|| Error::damaged("X-Ray placement range overflow"))?;
    if end > client_end {
        return Ok(None);
    }
    let Some(bytes) = raw.get(offset..end) else {
        return Ok(None);
    };
    let Some(array) = <[u8; 2]>::try_from(bytes).ok() else {
        return Ok(None);
    };
    let packed = u16::from_le_bytes(array);
    let kind = packed & 0x0F;
    match kind {
        1 => {
            let slot = (packed >> 4) & 0x3F;
            let base_slot = (packed >> 10) & 0x3F;
            // Slot 0 is not a slot: C# (`XRayAddWriter.TryReadPlacement`) requires 1..14 for both.
            if (1..14).contains(&slot) && (1..14).contains(&base_slot) {
                Ok(Some(PlacementFields {
                    category: "slot".to_owned(),
                    offset,
                    packed,
                    width: 2,
                    base_slot: u8::try_from(base_slot).ok(),
                }))
            } else {
                Ok(None)
            }
        }
        2 => Ok(Some(PlacementFields {
            category: "belt".to_owned(),
            offset,
            packed,
            width: 2,
            base_slot: packed_base_slot(packed),
        })),
        3 => Ok(Some(PlacementFields {
            category: "ruck".to_owned(),
            offset,
            packed,
            width: 2,
            base_slot: packed_base_slot(packed),
        })),
        _ => Ok(None),
    }
}

fn packed_base_slot(packed: u16) -> Option<u8> {
    let base_slot = (packed >> 10) & 0x3F;
    ((1..14).contains(&base_slot))
        .then(|| u8::try_from(base_slot).ok())
        .flatten()
}

fn read_condition_fields(raw: &[u8], record: &ObjectRecord) -> Option<(f32, usize, Option<usize>, Option<usize>)> {
    if record.version <= 52 || !has_condition_family(&record.name) {
        return None;
    }
    let state_end = record.state_offset.checked_add(record.state_length)?;
    let state = raw.get(record.state_offset..state_end)?;
    let mut reader = Cursor::new(state);
    skip_dynamic_visual(&mut reader, record.version).ok()?;
    let offset = record.state_offset.checked_add(reader.position())?;
    let condition_end = offset.checked_add(4)?;
    if condition_end > state_end {
        return None;
    }
    let bytes = raw.get(offset..condition_end)?;
    let condition = f32::from_le_bytes(<[u8; 4]>::try_from(bytes).ok()?);
    if !condition.is_finite() || !(0.0..=1.0).contains(&condition) {
        return None;
    }

    let update_start = record.update_offset.checked_add(2)?;
    let update_end = record.update_offset.checked_add(record.update_length)?;
    let update_payload = raw.get(update_start..update_end)?;
    let mut update_match = None;
    let mut update_matches = 0_u8;
    for relative in [3_usize, 4] {
        let Some(byte) = relative.checked_sub(2).and_then(|offset| update_payload.get(offset)) else {
            continue;
        };
        let candidate = record.update_offset.checked_add(relative)?;
        let encoded = f32::from(*byte) / 255.0;
        if (encoded - condition).abs() <= (1.0 / 255.0) + 1.0e-6 {
            update_match = Some(candidate);
            update_matches = update_matches.saturating_add(1);
        }
    }
    if update_matches != 1 {
        update_match = None;
    }

    let mut client_match = None;
    let mut client_matches = 0_u8;
    if let Some(client_start) = record.client_data_offset {
        let client_end = client_start.checked_add(record.client_data_length)?;
        let candidate_start = client_start.checked_add(2)?;
        let candidate_end = client_end.checked_sub(3)?;
        for candidate in candidate_start..candidate_end {
            let condition_end = candidate.checked_add(4)?;
            let value = raw.get(candidate..condition_end)?;
            let value = f32::from_le_bytes(<[u8; 4]>::try_from(value).ok()?);
            if !value.is_finite() || (value - condition).abs() > 1.0e-6 {
                continue;
            }
            let placement_bytes = raw.get(candidate.checked_sub(2)?..candidate)?;
            let placement = u16::from_le_bytes(<[u8; 2]>::try_from(placement_bytes).ok()?);
            if recognized_storage(placement) {
                client_match = Some(candidate);
                client_matches = client_matches.saturating_add(1);
            }
        }
    }
    if client_matches != 1 {
        client_match = None;
    }
    Some((condition, offset, update_match, client_match))
}

fn has_condition_family(name: &str) -> bool {
    let key = name.to_ascii_lowercase();
    key.starts_with("wpn_")
        || key.starts_with("weapon_")
        || key.starts_with("outfit_")
        || key.starts_with("scientific_")
        || key.starts_with("helm_")
        || key.starts_with("armor_")
        || key.ends_with("_outfit")
        || key.ends_with("_helmet")
        || key.ends_with("_helm")
        || key.ends_with("_armor")
}

fn recognized_storage(place: u16) -> bool {
    match place & 0x0F {
        2 | 3 => true,
        1 => ((place >> 4) & 0x3F) < 14 && ((place >> 10) & 0x3F) < 14,
        _ => false,
    }
}

fn category_for_name(name: &str) -> &'static str {
    let key = name.to_ascii_lowercase();
    if key.starts_with("ammo_") {
        "Патроны"
    } else if key.starts_with("wpn_") || key.starts_with("weapon_") {
        "Оружие"
    } else if key.starts_with("outfit_")
        || key.starts_with("scientific_")
        || key.starts_with("helm_")
        || key.starts_with("armor_")
        || key.ends_with("_outfit")
        || key.ends_with("_helmet")
        || key.ends_with("_helm")
        || key.ends_with("_armor")
    {
        "Броня/экипировка"
    } else if key.starts_with("af_") || key.starts_with("artifact_") {
        "Артефакт"
    } else if key.starts_with("device_") || key.starts_with("detector_") {
        "Устройство"
    } else if key.starts_with("grenade") || key.starts_with("rgd") || key.starts_with("f1_") {
        "Гранаты/стак"
    } else if key.starts_with("medkit")
        || key.starts_with("bandage")
        || key.starts_with("antirad")
        || key.starts_with("drug_")
        || key.starts_with("food_")
        || key.starts_with("bread")
        || key.starts_with("kolbasa")
        || key.starts_with("vodka")
        || key.starts_with("energy")
    {
        "Расходник"
    } else {
        "Разное"
    }
}

pub(crate) fn decode_cp1251(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| cp1251_char(*byte)).collect()
}

fn cp1251_char(byte: u8) -> char {
    const SPECIAL: [char; 64] = [
        'Ё', 'Ѓ', '‚', 'ѓ', '„', '…', '†', '‡', '€', '‰', 'Љ', '‹', 'Њ', 'Ќ', 'Ћ', 'Џ', 'ђ', '‘', '’', '“', '”', '•',
        '–', '—', '\u{FFFD}', '™', 'љ', '›', 'њ', 'ќ', 'ћ', 'џ', '\u{00A0}', 'Ў', 'ў', 'Ј', '¤', 'Ґ', '¦', '§', 'Ё',
        '©', 'Є', '«', '¬', '\u{00AD}', '®', 'Ї', '°', '±', 'І', 'і', 'ґ', 'µ', '¶', '·', 'ё', '№', 'є', '»', '¼', '½',
        '¾', 'ї',
    ];
    match byte {
        0x00..=0x7F => char::from(byte),
        0x80..=0xBF => SPECIAL
            .get(usize::from(byte.saturating_sub(0x80)))
            .copied()
            .unwrap_or('\u{FFFD}'),
        0xC0..=0xDF => char::from_u32(
            0x0410_u32
                .checked_add(u32::from(byte.saturating_sub(0xC0)))
                .unwrap_or_default(),
        )
        .unwrap_or('\u{FFFD}'),
        0xE0..=0xFF => char::from_u32(
            0x0430_u32
                .checked_add(u32::from(byte.saturating_sub(0xE0)))
                .unwrap_or_default(),
        )
        .unwrap_or('\u{FFFD}'),
    }
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::type_complexity
)]
mod tests {
    use super::{detect_format, parse_relation_registry, parse_spawn, read_placement_fields, Format, Save};
    use crate::container::Container;
    use sse_core::Error;

    #[test]
    fn indexes_original_trilogy_relation_registry() {
        let cases: [(&[u8], bool); 3] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-factions/soc-relations.sav"),
                true,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-factions/cs-relations.sav"),
                true,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/writer-factions/cop-relations.sav"),
                false,
            ),
        ];
        for (packed, timestamps) in cases {
            let parsed = Save::read(packed);
            assert!(parsed.is_ok(), "fixture should parse: {parsed:?}");
            let Ok(save) = parsed else { continue };
            assert!(
                save.relation_registry.is_some(),
                "{} relation registry: {:?}",
                save.format().id(),
                Container::read(packed).and_then(|container| {
                    let chunk = container
                        .chunks()
                        .iter()
                        .find(|chunk| chunk.kind == 9)
                        .ok_or_else(|| Error::damaged("missing relation chunk"))?;
                    let data = container.chunk_bytes(*chunk)?;
                    parse_relation_registry(data, timestamps).map(|_| ())
                })
            );
        }
    }

    #[test]
    fn exposes_the_actor_relation_values_from_the_validated_registry(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-factions/soc-relations.sav");
        let save = Save::read(packed)?;
        let values = save
            .actor_relations()
            .ok_or("fixture should expose the actor relation row")?;
        if values.is_empty() {
            return Err("actor relation row should contain catalog values".into());
        }
        Ok(())
    }

    #[test]
    fn missing_actor_info_row_remains_unknown_instead_of_becoming_an_empty_list() {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-factions/soc-relations.sav");
        let parsed = Save::read(packed);
        assert!(parsed.is_ok(), "fixture should parse: {parsed:?}");
        let Ok(mut save) = parsed else { return };
        let actor_id = save.actor_id;
        let Some(registry) = save.relation_registry.as_mut() else {
            panic!("fixture should expose a relation registry");
        };
        registry.info_rows.retain(|row| row.object_id != actor_id);

        assert!(save.actor_known_info().is_none());
    }

    #[test]
    fn creature_state_reader_matches_the_supported_csharp_prefix_and_rejects_hostile_inputs() {
        let (state, vector_offsets) = synthetic_creature_state(1.0);
        let record = synthetic_creature_record(state.len());
        let alive = super::read_creature_vitals(&state, &record);
        assert_eq!(alive.as_ref().map(|vitals| vitals.health), Some(1.0));
        assert_eq!(alive.as_ref().and_then(|vitals| vitals.killer_id), Some(u16::MAX));
        assert_eq!(alive.as_ref().and_then(|vitals| vitals.death_time), Some(0));
        assert!(alive.is_some_and(|vitals| !vitals.is_dead()));

        let (dead_state, _) = synthetic_creature_state(0.0);
        let dead = super::read_creature_vitals(&dead_state, &record);
        assert!(dead.is_some_and(|vitals| vitals.is_dead()));

        for length in 0..state.len() {
            assert!(
                super::read_creature_vitals(&state[..length], &record).is_none(),
                "accepted truncated creature STATE of length {length}"
            );
        }

        for offset in vector_offsets {
            let mut hostile = state.clone();
            let Some(count) = hostile.get_mut(offset..offset.saturating_add(4)) else {
                panic!("synthetic vector count should fit");
            };
            count.copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(super::read_creature_vitals(&hostile, &record).is_none());
        }

        let mut seed = 0xA341_316C_u32;
        for _ in 0..256 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let mut mutated = state.clone();
            let index = usize::try_from(seed).unwrap_or_default() % mutated.len();
            let Some(byte) = mutated.get_mut(index) else { continue };
            *byte ^= 1_u8.checked_shl(seed % 8).unwrap_or_default();
            let _ = super::read_creature_vitals(&mutated, &record);
        }
    }

    fn synthetic_creature_state(health: f32) -> (Vec<u8>, [usize; 2]) {
        let mut state = Vec::new();
        state.extend_from_slice(&0_u32.to_le_bytes());
        state.push(0);
        state.extend_from_slice(&0_u32.to_le_bytes());
        state.push(0);
        state.extend_from_slice(&[0; 12]);
        state.push(0);
        state.extend_from_slice(&[0; 6]);
        state.extend_from_slice(&[0; 8]);
        state.extend_from_slice(&0_u32.to_le_bytes());
        state.push(0);
        state.extend_from_slice(&0_u32.to_le_bytes());
        state.extend_from_slice(&u32::MAX.to_le_bytes());
        state.push(0);
        state.push(0);
        state.extend_from_slice(&[0; 3]);
        state.extend_from_slice(&health.to_le_bytes());
        let first_vector = state.len();
        state.extend_from_slice(&0_u32.to_le_bytes());
        let second_vector = state.len();
        state.extend_from_slice(&0_u32.to_le_bytes());
        state.extend_from_slice(&u16::MAX.to_le_bytes());
        state.extend_from_slice(&0_u64.to_le_bytes());
        (state, [first_vector, second_vector])
    }

    fn synthetic_creature_record(state_length: usize) -> super::RegistryObject {
        super::RegistryObject {
            name: "esc_wolf".to_owned(),
            name_replace: "esc_wolf".to_owned(),
            name_replace_range: 0..0,
            object_id: 1,
            parent_id: u16::MAX,
            object_id_offset: 0,
            parent_id_offset: 0,
            version: 124,
            spawn_id: Some(u16::MAX),
            spawn_id_offset: None,
            story_id: Some(u32::MAX),
            story_id_offset: None,
            spawn_story_id: Some(u32::MAX),
            spawn_story_id_offset: None,
            custom_data_range: None,
            record_offset: 0,
            record_length: state_length,
            state_offset: 0,
            state_length,
            update_offset: 0,
            update_length: 0,
            client_data_offset: None,
            client_data_length: 0,
        }
    }

    #[test]
    fn relation_registry_rejects_truncation_and_hostile_counts_and_survives_mutations() {
        let packed = include_bytes!("../../../fixtures/synthetic/writer-factions/soc-relations.sav");
        let container = Container::read(packed);
        assert!(container.is_ok(), "fixture should decompress: {container:?}");
        let Ok(container) = container else { return };
        let chunk = container.chunks().iter().find(|chunk| chunk.kind == 9);
        assert!(chunk.is_some(), "relation chunk expected");
        let Some(chunk) = chunk else { return };
        let payload = container.chunk_bytes(*chunk);
        assert!(payload.is_ok(), "relation payload should be in bounds: {payload:?}");
        let Ok(payload) = payload else { return };
        let registry = parse_relation_registry(payload, true);
        assert!(registry.is_ok(), "relation payload should parse: {registry:?}");
        let Ok(registry) = registry else { return };
        let required_end = registry
            .relation_rows
            .last()
            .map_or(registry.info_section_end.saturating_add(4), |row| row.end);
        for length in 0..required_end {
            assert!(
                parse_relation_registry(&payload[..length], true).is_err(),
                "accepted relation prefix of length {length}"
            );
        }

        let mut hostile_info_count = payload.to_vec();
        hostile_info_count[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_relation_registry(&hostile_info_count, true).is_err());

        let relation_count_end = registry.info_section_end.saturating_add(4);
        let mut hostile_relation_count = payload.to_vec();
        let Some(relation_count) = hostile_relation_count.get_mut(registry.info_section_end..relation_count_end) else {
            panic!("relation count must be present")
        };
        relation_count.copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_relation_registry(&hostile_relation_count, true).is_err());

        for index in 0..payload.len() {
            let mut mutated = payload.to_vec();
            let Some(byte) = mutated.get_mut(index) else { continue };
            *byte ^= 1_u8
                .checked_shl(u32::try_from(index % 8).unwrap_or_default())
                .unwrap_or_default();
            let _ = parse_relation_registry(&mutated, true);
        }
    }

    #[test]
    fn update_condition_mirror_uses_only_the_supported_packet_offsets() {
        let condition = 0.75_f32;
        let mut state = synthetic_dynamic_visual_state(b"", true);
        state.extend_from_slice(&condition.to_le_bytes());
        let update_offset = state.len().saturating_add(8);
        let mut raw = vec![0_u8; update_offset.saturating_add(16)];
        raw.get_mut(..state.len())
            .expect("state should fit")
            .copy_from_slice(&state);
        let update_end = update_offset.saturating_add(16);
        let update = raw.get_mut(update_offset..update_end).expect("update should fit");
        update[..2].copy_from_slice(&0_u16.to_le_bytes());
        let encoded = 191_u8;
        update[3] = encoded;
        update[11] = encoded;

        let mut record = synthetic_creature_record(state.len());
        record.name = "wpn_test".to_owned();
        record.update_offset = update_offset;
        record.update_length = 16;

        let matches = super::read_condition_fields(&raw, &record).map(|fields| fields.2);
        assert_eq!(matches, Some(Some(update_offset.saturating_add(3))));

        let mut trailing_only = raw;
        trailing_only[update_offset.saturating_add(3)] = 0;
        let matches = super::read_condition_fields(&trailing_only, &record).map(|fields| fields.2);
        assert_eq!(matches, Some(None));
    }

    #[test]
    fn info_portion_names_are_decoded_as_windows_1251() -> sse_core::Result<()> {
        // One info row for object 5 with a single name, "\u{0410}" in Windows-1251 (0xC0), and no relation rows.
        let mut payload = 1_u32.to_le_bytes().to_vec();
        payload.extend_from_slice(&5_u16.to_le_bytes());
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&[0xc0, 0x00]);
        payload.extend_from_slice(&0_u32.to_le_bytes());

        let registry = parse_relation_registry(&payload, false)?;
        let row = registry
            .info_rows
            .first()
            .ok_or_else(|| Error::damaged("info row should parse"))?;

        assert_eq!(row.names, vec!["\u{0410}".to_owned()]);
        Ok(())
    }

    #[test]
    fn condition_is_never_read_from_bytes_after_the_state() {
        let state = synthetic_dynamic_visual_state(b"", true);
        let state_length = state.len();
        let mut raw = state;
        // The next record's bytes: a valid condition value that must not be taken as this object's condition.
        raw.extend_from_slice(&0.75_f32.to_le_bytes());
        let update_offset = raw.len();
        raw.extend_from_slice(&[0_u8; 16]);
        let mut record = synthetic_creature_record(state_length);
        record.name = "wpn_test".to_owned();
        record.update_offset = update_offset;
        record.update_length = 16;

        assert_eq!(super::read_condition_fields(&raw, &record), None);
    }

    #[test]
    fn spawn_parser_rejects_unterminated_dynamic_custom_data() {
        let packet = synthetic_spawn_packet(&synthetic_dynamic_visual_state(b"logic = true", false));
        assert!(parse_spawn(&packet, 0).is_err());
    }

    #[test]
    fn spawn_parser_reads_clone_metadata_fields() {
        let state = synthetic_dynamic_visual_state(b"logic = true", true);
        let packet = synthetic_spawn_packet(&state);
        let packet_offset = 0x1000;
        let parsed = parse_spawn(&packet, packet_offset).expect("synthetic SPAWN should parse");

        assert_eq!(parsed.name_replace, "replace_me");
        assert_eq!(parsed.spawn_id, Some(0x1234));
        assert_eq!(parsed.story_id, Some(0x5555_5555));
        assert_eq!(parsed.spawn_story_id, Some(0x4444_4444));
        let name_replace = parsed
            .name_replace_range
            .start
            .checked_sub(packet_offset)
            .expect("name replacement range should use the packet base")
            ..parsed
                .name_replace_range
                .end
                .checked_sub(packet_offset)
                .expect("name replacement range should use the packet base");
        assert_eq!(packet.get(name_replace), Some(&b"replace_me\0"[..]));
        let custom_data = parsed
            .custom_data_range
            .as_ref()
            .expect("version 128 should expose custom data");
        let custom_data = custom_data
            .start
            .checked_sub(packet_offset)
            .expect("custom-data range should use the packet base")
            ..custom_data
                .end
                .checked_sub(packet_offset)
                .expect("custom-data range should use the packet base");
        assert_eq!(packet.get(custom_data), Some(&b"logic = true\0"[..]));
    }

    #[test]
    fn spawn_parser_rejects_truncated_dynamic_story_fields() {
        let mut state = synthetic_dynamic_visual_state(b"", true);
        state.truncate(state.len().saturating_sub(5));
        let packet = synthetic_spawn_packet(&state);
        assert!(parse_spawn(&packet, 0).is_err());
    }

    #[test]
    fn spawn_parser_rejects_truncation_and_survives_deterministic_mutations() {
        let packet = synthetic_spawn_packet(&synthetic_dynamic_visual_state(b"logic = true", true));
        for end in 0..packet.len() {
            assert!(
                parse_spawn(&packet[..end], 0).is_err(),
                "accepted truncated SPAWN prefix of length {end}"
            );
        }

        let parsed = parse_spawn(&packet, 0).expect("baseline SPAWN should parse");
        let mut hostile_client_length = packet.clone();
        let client_length_offset = parsed
            .client_data_offset
            .expect("version 128 should have client data")
            .checked_sub(2)
            .expect("client length precedes payload");
        let Some(length) = hostile_client_length.get_mut(client_length_offset..client_length_offset + 2) else {
            panic!("client length field should fit the synthetic packet")
        };
        length.copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(parse_spawn(&hostile_client_length, 0).is_err());

        let mut hostile_state_length = packet.clone();
        let state_size_offset = parsed
            .state_offset
            .checked_sub(2)
            .expect("state length precedes payload");
        let Some(length) = hostile_state_length.get_mut(state_size_offset..state_size_offset + 2) else {
            panic!("state length field should fit the synthetic packet")
        };
        length.copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(parse_spawn(&hostile_state_length, 0).is_err());

        for index in 0..packet.len() {
            for bit in 0..8 {
                let mut mutated = packet.clone();
                let Some(byte) = mutated.get_mut(index) else { continue };
                *byte ^= 1_u8.checked_shl(bit).unwrap_or_default();
                let _ = parse_spawn(&mutated, 0);
            }
        }
    }

    fn synthetic_dynamic_visual_state(custom_data: &[u8], terminate: bool) -> Vec<u8> {
        let mut state = Vec::new();
        state.extend_from_slice(&[0_u8; 6 + 4 + 4 + 4]);
        state.extend_from_slice(custom_data);
        if terminate {
            state.push(0);
        }
        state.extend_from_slice(&0x5555_5555_u32.to_le_bytes());
        state.extend_from_slice(&0x4444_4444_u32.to_le_bytes());
        state.extend_from_slice(b"visual\0");
        state.push(0);
        state
    }

    fn synthetic_spawn_packet(state: &[u8]) -> Vec<u8> {
        let mut packet = Vec::new();
        packet.extend_from_slice(&1_u16.to_le_bytes());
        packet.extend_from_slice(b"item_test\0replace_me\0");
        packet.extend_from_slice(&[0_u8; 2 + 6 * 4 + 2]);
        packet.extend_from_slice(&7_u16.to_le_bytes());
        packet.extend_from_slice(&8_u16.to_le_bytes());
        packet.extend_from_slice(&9_u16.to_le_bytes());
        packet.extend_from_slice(&(1_u16 << 5).to_le_bytes());
        packet.extend_from_slice(&128_u16.to_le_bytes());
        packet.extend_from_slice(&0_u16.to_le_bytes());
        packet.extend_from_slice(&0_u16.to_le_bytes());
        packet.extend_from_slice(&0_u16.to_le_bytes());
        packet.extend_from_slice(&0x1234_u16.to_le_bytes());
        let state_size = u16::try_from(state.len().saturating_add(2)).unwrap_or_default();
        packet.extend_from_slice(&state_size.to_le_bytes());
        packet.extend_from_slice(state);
        packet
    }

    #[test]
    fn reads_the_six_formats_by_content_and_decodes_the_actor_inventory() {
        let cases: [(&[u8], Format); 6] = [
            (include_bytes!("../../../fixtures/synthetic/xray-soc.sav"), Format::Soc),
            (
                include_bytes!("../../../fixtures/synthetic/xray-clear-sky.sav"),
                Format::Cs,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat.sav"),
                Format::Cop,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-soc-ee.sav"),
                Format::SocEe,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-clear-sky-ee.sav"),
                Format::CsEe,
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat-ee.sav"),
                Format::CopEe,
            ),
        ];

        for (packed, expected_format) in cases {
            let parsed = Save::read(packed);
            assert!(parsed.is_ok(), "{} did not parse: {parsed:?}", expected_format.id());
            let Ok(save) = parsed else { continue };
            assert_eq!(save.format(), expected_format);
        }

        let packed = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");
        let parsed = Save::read(packed);
        assert!(parsed.is_ok(), "fixture save should parse: {parsed:?}");
        let Ok(save) = parsed else { return };
        assert_eq!(save.money(), Ok(1234));
        let items = save.inventory();
        assert!(items.is_ok(), "fixture inventory should parse: {items:?}");
        let Ok(items) = items else { return };
        assert_eq!(items.len(), 1);
        let Some(item) = items.first() else {
            panic!("one item expected")
        };
        assert_eq!(item.handle, 0x1234);
        assert_eq!(item.section, "ammo_9x39_pab9");
        assert_eq!(item.count, Some(30));
    }

    #[test]
    fn unsupported_enhanced_mod_without_a_supported_level_marker_is_refused() {
        let source = include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat-ee.sav");
        let container = Container::read(source).expect("enhanced fixture should decompress");
        let object_chunk = container
            .chunks()
            .iter()
            .find(|chunk| chunk.kind == 2)
            .expect("OBJECT chunk should exist");
        let mut raw = container.image().to_vec();
        let object_end = object_chunk.offset.saturating_add(object_chunk.length);
        let object_data = raw
            .get(object_chunk.offset..object_end)
            .expect("OBJECT chunk should be in bounds");
        let marker = object_data
            .windows(5)
            .position(|window| window == b"zaton")
            .and_then(|offset| object_chunk.offset.checked_add(offset))
            .expect("synthetic CoP fixture should include its level marker");
        raw.get_mut(marker..marker.saturating_add(5))
            .expect("level marker should fit")
            .copy_from_slice(b"other");

        let mod_save = pack(container.version(), &raw);
        assert!(matches!(Save::read(&mod_save), Err(Error::Refused(_))));
    }

    #[test]
    fn enhanced_level_markers_must_be_standalone_serialized_strings() {
        assert!(matches!(
            detect_format(6, 54, b"ammo_marsh_test\0"),
            Err(Error::Refused(_))
        ));
        assert_eq!(detect_format(6, 54, b"\0marsh\0"), Ok(Format::CsEe));
        assert_eq!(detect_format(6, 54, b"\0zaton\0"), Ok(Format::CopEe));
        assert!(matches!(
            detect_format(6, 54, b"\0marsh\0zaton\0"),
            Err(Error::Refused(_))
        ));
    }

    #[test]
    fn every_container_truncation_is_rejected() {
        let packed = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");
        for length in 0..packed.len() {
            let Some(prefix) = packed.get(..length) else { continue };
            assert!(
                matches!(Container::read(prefix), Err(Error::Damaged(_))),
                "prefix {length}"
            );
        }
    }

    #[test]
    fn every_save_truncation_is_rejected() {
        let packed = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");
        for length in 0..packed.len() {
            let Some(prefix) = packed.get(..length) else { continue };
            assert!(matches!(Save::read(prefix), Err(Error::Damaged(_))), "prefix {length}");
        }
    }

    #[test]
    fn hostile_unpacked_lengths_are_rejected_before_decompression() {
        let source = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");
        let mut packed = source.to_vec();
        let Some(size) = packed.get_mut(8..12) else {
            panic!("container header expected")
        };
        size.copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(Container::read(&packed), Err(Error::Damaged(_))));
    }

    #[test]
    fn hostile_object_count_and_spawn_length_are_rejected() {
        let source = include_bytes!("../../../fixtures/synthetic/xray-soc.raw");
        let parsed = Container::read(include_bytes!("../../../fixtures/synthetic/xray-soc.sav"));
        assert!(parsed.is_ok());
        let Ok(container) = parsed else { return };
        let Some(chunk) = container.chunks().iter().find(|chunk| chunk.kind == 2) else {
            panic!("OBJECT chunk expected")
        };

        let mut hostile_count = source.to_vec();
        let Some(count_end) = chunk.offset.checked_add(4) else {
            panic!("count range overflow")
        };
        let Some(count) = hostile_count.get_mut(chunk.offset..count_end) else {
            panic!("object count expected")
        };
        count.copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(Save::read(&pack(3, &hostile_count)), Err(Error::Damaged(_))));

        let mut hostile_spawn = source.to_vec();
        let Some(spawn_start) = chunk.offset.checked_add(4) else {
            panic!("spawn offset overflow")
        };
        let Some(spawn_end) = spawn_start.checked_add(2) else {
            panic!("spawn range overflow")
        };
        let Some(spawn_size) = hostile_spawn.get_mut(spawn_start..spawn_end) else {
            panic!("spawn size expected")
        };
        spawn_size.copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(matches!(Save::read(&pack(3, &hostile_spawn)), Err(Error::Damaged(_))));
    }

    #[test]
    fn deterministic_bit_flips_never_panic_or_exceed_the_image_limit() {
        let source = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");
        for index in 0..source.len() {
            let mut mutated = source.to_vec();
            let Some(byte) = mutated.get_mut(index) else { continue };
            *byte ^= 1_u8
                .checked_shl(u32::try_from(index % 8).unwrap_or_default())
                .unwrap_or_default();
            if let Ok(container) = Container::read(&mutated) {
                assert!(container.image().len() <= 256 * 1024 * 1024);
            }
            let _ = Save::read(&mutated);
        }
    }

    #[test]
    fn unsupported_item_fields_remain_unknown() {
        let packed = include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat-base-item.sav");
        let parsed = Save::read(packed);
        assert!(parsed.is_ok());
        let Ok(save) = parsed else { return };
        let items = save.inventory();
        assert!(items.is_ok());
        let Ok(items) = items else { return };
        let Some(item) = items.first() else {
            panic!("fixture item expected")
        };
        assert_eq!(item.section, "bandage_existing");
        assert_eq!(item.count, None);
    }

    #[test]
    fn clear_sky_reader_accepts_the_tagged_one_byte_placement_layout() {
        let mut record = synthetic_creature_record(0);
        record.client_data_offset = Some(0);
        record.client_data_length = 2;
        let fields = read_placement_fields(&[2, 1], &record, Format::Cs)
            .expect("synthetic placement should not be damaged")
            .expect("Clear Sky's tagged one-byte slot placement should be readable");
        assert_eq!(fields.category, "slot");
        assert_eq!(fields.packed, 1);
        assert_eq!(fields.width, 1);
    }

    #[test]
    fn clear_sky_reader_keeps_the_packed_two_byte_placement_layout() {
        let mut record = synthetic_creature_record(0);
        record.client_data_offset = Some(0);
        record.client_data_length = 3;
        let fields = read_placement_fields(&[2, 3, 0], &record, Format::Cs)
            .expect("synthetic placement should not be damaged")
            .expect("the packed two-byte placement should be readable");
        assert_eq!(fields.category, "ruck");
        assert_eq!(fields.packed, 3);
        assert_eq!(fields.width, 2);
    }

    fn pack(version: u32, raw: &[u8]) -> Vec<u8> {
        let compressed = sse_codecs::lzo1x::compress(raw);
        let capacity = 12_usize.saturating_add(compressed.len());
        let mut packed = Vec::with_capacity(capacity);
        packed.extend_from_slice(&u32::MAX.to_le_bytes());
        packed.extend_from_slice(&version.to_le_bytes());
        packed.extend_from_slice(&u32::try_from(raw.len()).unwrap_or_default().to_le_bytes());
        packed.extend_from_slice(&compressed);
        packed
    }
}
