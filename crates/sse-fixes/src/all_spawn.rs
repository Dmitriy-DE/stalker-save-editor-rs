//! In-place editor for X-Ray `all.spawn` files.
//!
//! Port of `AllSpawnEditor.cs`. Parses chunked spawn structures and applies
//! patrol point waypoint edits and object custom data logic edits safely.

use sse_core::{Error, Result};

use crate::models::{SpawnEditKind, SpawnEditOperation};

const SPAWN_OBJECTS_CHUNK: u32 = 1;
const PATROL_PATHS_CHUNK: u32 = 3;

#[derive(Debug, Clone, Copy)]
struct SizeField {
    offset: usize,
    width: usize,
}

#[derive(Debug, Clone)]
struct Splice {
    start: usize,
    length: usize,
    bytes: Vec<u8>,
    sizes: Vec<SizeField>,
}

#[derive(Debug, Clone, Copy)]
struct Chunk {
    id: u32,
    header: usize,
    start: usize,
    size: usize,
}

/// In-place binary editor for `all.spawn`.
pub struct AllSpawnEditor;

impl AllSpawnEditor {
    /// Applies a sequence of edits to an `all.spawn` image.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on malformed chunks or overlapping edits,
    /// [`Error::Refused`] on unsupported waypoint names or coordinates.
    pub fn apply(all_spawn: &[u8], edits: &[SpawnEditOperation]) -> Result<Vec<u8>> {
        if edits.is_empty() {
            return Ok(all_spawn.to_vec());
        }

        let mut splices = Vec::with_capacity(edits.len());
        for edit in edits {
            let splice = match edit.kind {
                SpawnEditKind::PatrolPoint => patrol_point(all_spawn, edit)?,
                SpawnEditKind::CustomData => custom_data(all_spawn, edit)?,
            };
            splices.push(splice);
        }

        // Sort descending by start offset
        splices.sort_by_key(|a| std::cmp::Reverse(a.start));

        // Check for overlaps
        for (idx, prev) in splices.iter().enumerate().skip(1) {
            let prev_idx = idx.saturating_sub(1);
            if let Some(next) = splices.get(prev_idx) {
                if prev.start.saturating_add(prev.length) > next.start {
                    return Err(Error::damaged("Two all.spawn edits overlap."));
                }
            }
        }

        let mut result = all_spawn.to_vec();
        for splice in splices {
            let new_len_i64 = i64::try_from(splice.bytes.len()).unwrap_or(0);
            let old_len_i64 = i64::try_from(splice.length).unwrap_or(0);
            let delta = new_len_i64.saturating_sub(old_len_i64);

            let res_len_i64 = i64::try_from(result.len()).unwrap_or(0);
            let next_len_i64 = res_len_i64.saturating_add(delta);
            let next_len = usize::try_from(next_len_i64.max(0)).map_err(|_| Error::damaged("Invalid result length"))?;
            let mut next = Vec::with_capacity(next_len);

            next.extend_from_slice(
                result
                    .get(..splice.start)
                    .ok_or_else(|| Error::damaged("Invalid splice start offset"))?,
            );
            next.extend_from_slice(&splice.bytes);
            next.extend_from_slice(
                result
                    .get(splice.start.saturating_add(splice.length)..)
                    .ok_or_else(|| Error::damaged("Invalid splice end offset"))?,
            );

            if delta != 0 {
                for size in splice.sizes {
                    add_to_size(&mut next, size, delta)?;
                }
            }
            result = next;
        }

        Ok(result)
    }
}

fn add_to_size(data: &mut [u8], field: SizeField, delta: i64) -> Result<()> {
    if field.width == 4 {
        let end = field.offset.saturating_add(4);
        let slice = data
            .get_mut(field.offset..end)
            .ok_or_else(|| Error::damaged("Chunk size field offset out of bounds"))?;
        let arr: [u8; 4] = (*slice)
            .try_into()
            .map_err(|_| Error::damaged("Invalid 4-byte slice"))?;
        let val = u32::from_le_bytes(arr);
        let new_val = i64::from(val).saturating_add(delta);
        let new_u32 =
            u32::try_from(new_val).map_err(|_| Error::damaged("all.spawn chunk size out of range after edit"))?;
        slice.copy_from_slice(&new_u32.to_le_bytes());
    } else {
        let end = field.offset.saturating_add(2);
        let slice = data
            .get_mut(field.offset..end)
            .ok_or_else(|| Error::damaged("Packet size field offset out of bounds"))?;
        let arr: [u8; 2] = (*slice)
            .try_into()
            .map_err(|_| Error::damaged("Invalid 2-byte slice"))?;
        let val = u16::from_le_bytes(arr);
        let new_val = i64::from(val).saturating_add(delta);
        let new_u16 =
            u16::try_from(new_val).map_err(|_| Error::damaged("Spawn packet larger than 64 KB after edit"))?;
        slice.copy_from_slice(&new_u16.to_le_bytes());
    }
    Ok(())
}

fn children(data: &[u8], start: usize, size: usize) -> Result<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let mut offset = start;
    let end = start
        .checked_add(size)
        .ok_or_else(|| Error::damaged("Chunk range overflow"))?;
    if end > data.len() {
        return Err(Error::damaged("Chunk exceeds file length"));
    }

    while offset < end {
        if end.saturating_sub(offset) < 8 {
            return Err(Error::damaged("Truncated all.spawn chunk header"));
        }
        let header_slice = data
            .get(offset..offset.saturating_add(8))
            .ok_or_else(|| Error::damaged("Incomplete chunk header"))?;
        let id_arr: [u8; 4] = header_slice
            .get(..4)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| Error::damaged("Incomplete chunk ID"))?;
        let id = u32::from_le_bytes(id_arr);

        let len_arr: [u8; 4] = header_slice
            .get(4..8)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| Error::damaged("Incomplete chunk size"))?;
        let raw_len = u32::from_le_bytes(len_arr);
        let length = usize::try_from(raw_len).map_err(|_| Error::damaged("Chunk length overflow"))?;

        let remaining = end.saturating_sub(offset.saturating_add(8));
        if length > remaining {
            return Err(Error::damaged("all.spawn chunk runs past its parent"));
        }

        let chunk_start = offset.saturating_add(8);
        chunks.push(Chunk {
            id,
            header: offset,
            start: chunk_start,
            size: length,
        });
        offset = chunk_start.saturating_add(length);
    }

    Ok(chunks)
}

fn top_chunk(data: &[u8], id: u32, sizes: &mut Vec<SizeField>) -> Result<(usize, usize)> {
    let list = children(data, 0, data.len())?;
    let matching: Vec<_> = list.into_iter().filter(|c| c.id == id).collect();
    if matching.len() != 1 {
        return Err(Error::damaged(format!("all.spawn top chunk {id} not found uniquely")));
    }
    let chunk = matching
        .first()
        .copied()
        .ok_or_else(|| Error::damaged("Chunk unexpectedly missing"))?;
    sizes.push(SizeField {
        offset: chunk.header.saturating_add(4),
        width: 4,
    });
    Ok((chunk.start, chunk.size))
}

fn child_chunk(data: &[u8], start: usize, size: usize, id: u32, sizes: &mut Vec<SizeField>) -> Result<(usize, usize)> {
    let list = children(data, start, size)?;
    let matching: Vec<_> = list.into_iter().filter(|c| c.id == id).collect();
    if matching.len() != 1 {
        return Err(Error::damaged(format!("all.spawn child chunk {id} not found uniquely")));
    }
    let chunk = matching
        .first()
        .copied()
        .ok_or_else(|| Error::damaged("Chunk unexpectedly missing"))?;
    sizes.push(SizeField {
        offset: chunk.header.saturating_add(4),
        width: 4,
    });
    Ok((chunk.start, chunk.size))
}

fn read_latin1_string(data: &[u8], offset: &mut usize, end: usize) -> Option<String> {
    if *offset >= end {
        return None;
    }
    let slice = data.get(*offset..end)?;
    let null_idx = slice.iter().position(|&b| b == 0)?;
    let string_bytes = slice.get(..null_idx)?;
    let s: String = string_bytes.iter().map(|&b| char::from(b)).collect();
    *offset = offset.saturating_add(null_idx).saturating_add(1);
    Some(s)
}

fn patrol_point(data: &[u8], edit: &SpawnEditOperation) -> Result<Splice> {
    let mut sizes = Vec::new();
    let (patrol_start, patrol_size) = top_chunk(data, PATROL_PATHS_CHUNK, &mut sizes)?;
    let (list_start, list_size) = child_chunk(data, patrol_start, patrol_size, 1, &mut sizes)?;

    let mut key = edit.target.as_bytes().to_vec();
    key.push(0);

    let list = children(data, list_start, list_size)?;
    let mut found_path: Option<(usize, usize)> = None;

    for path_chunk in list {
        let sub = children(data, path_chunk.start, path_chunk.size)?;
        if let Some(name_chunk) = sub.into_iter().find(|c| c.id == 0) {
            if name_chunk.size == key.len() {
                let end = name_chunk.start.saturating_add(name_chunk.size);
                if let Some(slice) = data.get(name_chunk.start..end) {
                    if slice == key.as_slice() {
                        if found_path.is_some() {
                            return Err(Error::damaged(format!(
                                "Patrol path {} appears more than once",
                                edit.target
                            )));
                        }
                        found_path = Some((path_chunk.start, path_chunk.size));
                        sizes.push(SizeField {
                            offset: path_chunk.header.saturating_add(4),
                            width: 4,
                        });
                    }
                }
            }
        }
    }

    let (path_start, path_size) =
        found_path.ok_or_else(|| Error::damaged(format!("Patrol path {} is not in all.spawn", edit.target)))?;
    let (graph_start, graph_size) = child_chunk(data, path_start, path_size, 1, &mut sizes)?;
    let (verts_start, verts_size) = child_chunk(data, graph_start, graph_size, 1, &mut sizes)?;
    let point_u32 = u32::try_from(edit.point).map_err(|_| Error::damaged("Point index overflow"))?;
    let (vert_start, vert_size) = child_chunk(data, verts_start, verts_size, point_u32, &mut sizes)?;
    let (point_start, point_size) = child_chunk(data, vert_start, vert_size, 1, &mut sizes)?;

    let end_point = point_start.saturating_add(point_size);
    let point_slice = data
        .get(point_start..end_point)
        .ok_or_else(|| Error::damaged("Point data out of bounds"))?;
    let name_end = point_slice
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::damaged("Unterminated waypoint name"))?;

    let expected_point_size = name_end.saturating_add(1).saturating_add(22);
    if point_size != expected_point_size {
        return Err(Error::damaged("Unexpected patrol point layout"));
    }

    let name_bytes = point_slice
        .get(..name_end)
        .ok_or_else(|| Error::damaged("Invalid waypoint name slice"))?;
    let current_name: String = name_bytes.iter().map(|&b| char::from(b)).collect();
    if current_name != edit.expected {
        return Err(Error::damaged(format!(
            "Patrol point {}[{}] is '{current_name}', expected '{}'",
            edit.target, edit.point, edit.expected
        )));
    }

    let tail_start = name_end.saturating_add(1);
    let tail_end = tail_start.saturating_add(22);
    let tail_slice = point_slice
        .get(tail_start..tail_end)
        .ok_or_else(|| Error::damaged("Invalid tail slice"))?;
    let mut tail = tail_slice.to_vec();

    if let Some(pos) = edit.position {
        for (i, &coord) in pos.iter().enumerate() {
            if !coord.is_finite() {
                return Err(Error::Refused(
                    "Waypoint position coordinates must be finite".to_string(),
                ));
            }
            let bytes = coord.to_le_bytes();
            let start_b = i.saturating_mul(4);
            let end_b = start_b.saturating_add(4);
            if let Some(dest) = tail.get_mut(start_b..end_b) {
                dest.copy_from_slice(&bytes);
            }
        }
    }
    if let Some(lvl) = edit.level_vertex_id {
        if let Some(dest) = tail.get_mut(16..20) {
            dest.copy_from_slice(&lvl.to_le_bytes());
        }
    }
    if let Some(gm) = edit.game_vertex_id {
        if let Some(dest) = tail.get_mut(20..22) {
            dest.copy_from_slice(&gm.to_le_bytes());
        }
    }

    let new_name = edit.replacement.as_deref().unwrap_or(&current_name);
    if new_name.is_empty() || new_name.contains('\0') {
        return Err(Error::Refused(
            "Waypoint name must be nonempty and without NUL".to_string(),
        ));
    }

    let mut new_bytes: Vec<u8> = new_name.chars().map(|c| u8::try_from(c).unwrap_or(b'?')).collect();
    new_bytes.push(0);
    new_bytes.extend_from_slice(&tail);

    if new_bytes == point_slice {
        return Err(Error::damaged("Edit of waypoint changes nothing"));
    }

    Ok(Splice {
        start: point_start,
        length: point_size,
        bytes: new_bytes,
        sizes,
    })
}

fn custom_data(data: &[u8], edit: &SpawnEditOperation) -> Result<Splice> {
    let replacement = edit
        .replacement
        .as_deref()
        .ok_or_else(|| Error::Refused("Custom data edit requires a replacement".to_string()))?;
    if replacement.contains('\0') || edit.expected.contains('\0') {
        return Err(Error::Refused("Custom data edits cannot contain NUL".to_string()));
    }

    let mut top_sizes = Vec::new();
    let (objects_start, objects_size) = top_chunk(data, SPAWN_OBJECTS_CHUNK, &mut top_sizes)?;
    let (list_start, list_size) = child_chunk(data, objects_start, objects_size, 1, &mut top_sizes)?;

    let mut found: Option<Splice> = None;
    let list = children(data, list_start, list_size)?;

    for object_chunk in list {
        if let Some((start, length, sub_sizes)) =
            object_custom_data(data, object_chunk.start, object_chunk.size, &edit.target)?
        {
            if found.is_some() {
                return Err(Error::damaged(format!(
                    "Spawn object {} appears more than once",
                    edit.target
                )));
            }
            let mut sizes = top_sizes.clone();
            sizes.push(SizeField {
                offset: object_chunk.header.saturating_add(4),
                width: 4,
            });
            sizes.extend(sub_sizes);

            let end_custom = start.saturating_add(length);
            let slice = data
                .get(start..end_custom)
                .ok_or_else(|| Error::damaged("Custom data slice out of bounds"))?;
            let current: String = slice.iter().map(|&b| char::from(b)).collect();
            if current != edit.expected {
                return Err(Error::damaged(format!(
                    "Custom data of {} does not match expected text",
                    edit.target
                )));
            }

            let new_bytes: Vec<u8> = replacement.chars().map(|c| u8::try_from(c).unwrap_or(b'?')).collect();
            found = Some(Splice {
                start,
                length,
                bytes: new_bytes,
                sizes,
            });
        }
    }

    found.ok_or_else(|| Error::damaged(format!("Spawn object {} is not in all.spawn", edit.target)))
}

fn object_custom_data(
    data: &[u8],
    start: usize,
    size: usize,
    target: &str,
) -> Result<Option<(usize, usize, Vec<SizeField>)>> {
    let mut sizes = Vec::new();
    let body_list = children(data, start, size)?;
    let body = match body_list.into_iter().find(|c| c.id == 1) {
        Some(b) => b,
        None => return Ok(None),
    };

    let spawn_list = children(data, body.start, body.size)?;
    let spawn = match spawn_list.into_iter().find(|c| c.id == 0) {
        Some(s) if s.size >= 4 => s,
        _ => return Ok(None),
    };

    sizes.push(SizeField {
        offset: body.header.saturating_add(4),
        width: 4,
    });
    sizes.push(SizeField {
        offset: spawn.header.saturating_add(4),
        width: 4,
    });

    let spawn_end = spawn.start.saturating_add(2);
    let packet_len_bytes = data
        .get(spawn.start..spawn_end)
        .ok_or_else(|| Error::damaged("Incomplete packet length"))?;
    let packet_len_arr: [u8; 2] = packet_len_bytes
        .try_into()
        .map_err(|_| Error::damaged("Invalid packet len bytes"))?;
    let raw_packet_len = u16::from_le_bytes(packet_len_arr);
    let packet_length = usize::from(raw_packet_len);
    if packet_length.saturating_add(2) != spawn.size {
        return Ok(None);
    }

    sizes.push(SizeField {
        offset: spawn.start,
        width: 2,
    });

    let mut reader = spawn.start.saturating_add(2);
    let end = spawn.start.saturating_add(spawn.size);

    reader = reader.saturating_add(2); // M_SPAWN message id
    let section = match read_latin1_string(data, &mut reader, end) {
        Some(s) => s,
        None => return Ok(None),
    };
    let name = match read_latin1_string(data, &mut reader, end) {
        Some(s) => s,
        None => return Ok(None),
    };

    if section.is_empty() || name != target {
        return Ok(None);
    }

    // skip: game id(1) + rp(1) + pos(12) + angle(12) + respawn(2) + id(2) + parent(2) + phantom(2) = 34
    reader = reader
        .checked_add(34)
        .ok_or_else(|| Error::damaged("Spawn reader overflow"))?;
    if reader.saturating_add(4) > end {
        return Err(Error::damaged("Truncated spawn object"));
    }

    reader = reader.saturating_add(2); // flags
    let ver_bytes = data
        .get(reader..reader.saturating_add(2))
        .ok_or_else(|| Error::damaged("Truncated version"))?;
    let ver_arr: [u8; 2] = ver_bytes
        .try_into()
        .map_err(|_| Error::damaged("Invalid version bytes"))?;
    let version = u16::from_le_bytes(ver_arr);
    reader = reader.saturating_add(2);
    if version < 118 {
        return Err(Error::damaged(format!(
            "Unsupported spawn object version {version} for {target}"
        )));
    }

    reader = reader.saturating_add(2); // game type
    reader = reader.saturating_add(2); // script version

    if reader.saturating_add(2) > end {
        return Err(Error::damaged("Truncated client data"));
    }
    let cd_bytes = data
        .get(reader..reader.saturating_add(2))
        .ok_or_else(|| Error::damaged("Truncated client data bytes"))?;
    let cd_arr: [u8; 2] = cd_bytes.try_into().map_err(|_| Error::damaged("Invalid cd bytes"))?;
    let raw_cd = u16::from_le_bytes(cd_arr);
    let client_data = usize::from(raw_cd);

    reader = reader
        .checked_add(2)
        .and_then(|r| r.checked_add(client_data))
        .ok_or_else(|| Error::damaged("Client data overflow"))?;
    reader = reader.saturating_add(2); // spawn id

    let state_size_offset = reader;
    if state_size_offset.saturating_add(2) > end {
        return Err(Error::damaged("Missing state size offset"));
    }
    let ss_bytes = data
        .get(state_size_offset..state_size_offset.saturating_add(2))
        .ok_or_else(|| Error::damaged("Missing state size bytes"))?;
    let ss_arr: [u8; 2] = ss_bytes
        .try_into()
        .map_err(|_| Error::damaged("Invalid state size bytes"))?;
    let raw_ss = u16::from_le_bytes(ss_arr);
    let state_size = usize::from(raw_ss);
    if state_size_offset.saturating_add(state_size) != end {
        return Err(Error::damaged("Unexpected spawn packet layout"));
    }

    sizes.push(SizeField {
        offset: state_size_offset,
        width: 2,
    });

    // state size(2) + graph id(2) + distance(4) + direct control(4) + node id(4) + object flags(4) = 20
    reader = reader
        .checked_add(20)
        .ok_or_else(|| Error::damaged("State reader overflow"))?;
    let custom_start = reader;
    let custom = read_latin1_string(data, &mut reader, end)
        .ok_or_else(|| Error::damaged(format!("Missing custom data for {target}")))?;

    Ok(Some((custom_start, custom.len(), sizes)))
}
