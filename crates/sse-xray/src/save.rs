//! Read-only indexed X-Ray save views.

use sse_core::{Cursor, Error, Result};

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
    game_time: u64,
    time_factor: f32,
    normal_time_factor: f32,
}

/// One actor-owned inventory object.
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

/// Indexed registry record. Offsets address the decompressed image and preserve all unknown bytes.
#[derive(Debug, Clone)]
pub struct RegistryObject {
    /// Object message name.
    pub name: String,
    /// Section or replacement name from the spawn record.
    pub name_replace: String,
    /// Registry object id.
    pub object_id: u16,
    /// Parent object id.
    pub parent_id: u16,
    /// Spawn serialization version.
    pub version: u16,
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

type ObjectRecord = RegistryObject;

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
            return Err(Error::damaged(format!(
                "actor spawn version {} is not supported for {}",
                actor.version,
                format.id()
            )));
        }
        let (money, money_offset) = read_actor_money(container.image(), &actor)?;

        Ok(Self {
            format,
            container,
            records,
            actor_id: actor.object_id,
            actor_version: actor.version,
            money,
            money_offset,
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

    pub(crate) const fn actor_id(&self) -> u16 {
        self.actor_id
    }

    pub(crate) fn raw_image(&self) -> &[u8] {
        self.container.image()
    }

    pub(crate) const fn money_offset(&self) -> usize {
        self.money_offset
    }

    pub(crate) fn repack(&self, raw: &[u8]) -> Result<sse_core::SaveBuffer> {
        self.container.repack(raw)
    }

    /// Actor-owned inventory entries.
    pub fn inventory(&self) -> Result<Vec<InventoryItem>> {
        let mut items = Vec::new();
        for record in &self.records {
            if record.parent_id != self.actor_id || record.object_id == self.actor_id {
                continue;
            }
            let placement = read_placement(self.container.image(), record)?;
            let stack = read_ammo_count(self.container.image(), record);
            items.push(InventoryItem {
                handle: record.object_id,
                section: record.name.clone(),
                category: category_for_name(&record.name).to_owned(),
                count: stack.map(|value| value.0),
                state_count_offset: stack.map(|value| value.1),
                update_count_offset: stack.map(|value| value.2),
                placement,
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
            let has_marsh = objects.windows(5).any(|window| window == b"marsh");
            let has_zaton = objects.windows(5).any(|window| window == b"zaton");
            match (has_marsh, has_zaton) {
                (true, false) => Ok(Format::CsEe),
                (false, true) => Ok(Format::CopEe),
                _ => Err(Error::damaged("X-Ray Enhanced Edition markers are ambiguous")),
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

fn parse_spawn(packet: &[u8], packet_offset: usize) -> Result<ObjectRecord> {
    let mut reader = Cursor::new(packet);
    if reader.u16()? != SPAWN_MESSAGE {
        return Err(Error::damaged("X-Ray object does not begin with M_SPAWN"));
    }
    let name = decode_cp1251(reader.zero_terminated(MAXIMUM_STRING_LENGTH)?);
    let name_replace = decode_cp1251(reader.zero_terminated(MAXIMUM_STRING_LENGTH)?);
    reader.skip(2)?;
    reader.skip(6 * 4)?;
    reader.skip(2)?;
    let object_id = reader.u16()?;
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
    if version > 79 {
        reader.skip(2)?;
    }
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
    reader.skip(state_length)?;
    Ok(ObjectRecord {
        name,
        name_replace,
        object_id,
        parent_id,
        version,
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

fn read_actor_money(raw: &[u8], actor: &ObjectRecord) -> Result<(u32, usize)> {
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
    Ok((reader.u32()?, offset))
}

fn skip_dynamic_visual(reader: &mut Cursor<'_>, version: u16) -> Result<()> {
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
    if version > 57 {
        let _ini = reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
    }
    if version > 61 {
        reader.skip(4)?;
    }
    if version > 111 {
        reader.skip(4)?;
    }
    if version > 31 {
        let _visual = reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
        if version > 103 {
            reader.skip(1)?;
        }
    }
    Ok(())
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

fn read_placement(raw: &[u8], record: &ObjectRecord) -> Result<Option<String>> {
    if record.client_data_length < 3 {
        return Ok(None);
    }
    let Some(start) = record.client_data_offset else {
        return Ok(None);
    };
    let offset = start
        .checked_add(1)
        .ok_or_else(|| Error::damaged("X-Ray placement offset overflow"))?;
    let end = offset
        .checked_add(2)
        .ok_or_else(|| Error::damaged("X-Ray placement range overflow"))?;
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
                Ok(Some("slot".to_owned()))
            } else {
                Ok(None)
            }
        }
        2 => Ok(Some("belt".to_owned())),
        3 => Ok(Some("ruck".to_owned())),
        _ => Ok(None),
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
mod tests {
    use super::{Format, Save};
    use crate::container::Container;
    use sse_core::Error;

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
                assert!(container.image().len() <= 512 * 1024 * 1024);
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
