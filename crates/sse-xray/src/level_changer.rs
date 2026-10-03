//! Bounded reader for X-Ray level-changer state suffixes and unique destinations.

use sse_core::{Cursor, Error, Result};

use crate::save::decode_cp1251;

const MAXIMUM_STRING_LENGTH: usize = 1 << 20;
const MAXIMUM_SHAPES: usize = 32;
const SPHERE_BYTES: usize = 16;
const BOX_BYTES: usize = 48;

/// Three floating point coordinates stored by X-Ray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector3 {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
    /// Z coordinate.
    pub z: f32,
}

/// Parsed destination suffix of a level-changer state packet.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelChangerDestination {
    /// Registry object serialization version.
    pub object_version: u16,
    /// Destination game graph vertex, when this version stores it.
    pub dest_game_vertex_id: Option<u16>,
    /// Destination level graph vertex, when this version stores it.
    pub dest_level_vertex_id: Option<u32>,
    /// Destination position, when this version stores it.
    pub dest_position: Option<Vector3>,
    /// Destination direction, when this version stores it.
    pub dest_direction: Option<Vector3>,
    /// Destination level identifier.
    pub dest_level_name: String,
    /// Destination point identifier.
    pub dest_level_point_name: String,
    /// Silent transition flag, when this version stores it.
    pub silent: Option<bool>,
    /// Number of bytes consumed from the provided suffix.
    pub consumed_bytes: usize,
}

/// Parses a level-changer state suffix using its object serialization version.
pub fn parse_state_suffix(packet: &[u8], object_version: u32) -> Result<LevelChangerDestination> {
    let version =
        u16::try_from(object_version).map_err(|_| Error::damaged("invalid X-Ray level-changer object version"))?;
    let mut reader = Cursor::new(packet);
    let (game_vertex, level_vertex, position, direction) = if version < 34 {
        reader.skip(8)?;
        (None, None, None, None)
    } else {
        let game_vertex = reader.u16()?;
        let level_vertex = reader.u32()?;
        let position = read_vector(&mut reader)?;
        let direction = if version <= 53 {
            Vector3 {
                x: 0.0,
                y: reader.f32()?,
                z: 0.0,
            }
        } else {
            read_vector(&mut reader)?
        };
        (Some(game_vertex), Some(level_vertex), Some(position), Some(direction))
    };
    let level_name = read_text_string(&mut reader)?;
    let point_name = read_text_string(&mut reader)?;
    let silent = if version > 116 { Some(reader.u8()? != 0) } else { None };
    Ok(LevelChangerDestination {
        object_version: version,
        dest_game_vertex_id: game_vertex,
        dest_level_vertex_id: level_vertex,
        dest_position: position,
        dest_direction: direction,
        dest_level_name: level_name,
        dest_level_point_name: point_name,
        silent,
        consumed_bytes: reader.position(),
    })
}

fn read_vector(reader: &mut Cursor<'_>) -> Result<Vector3> {
    Ok(Vector3 {
        x: reader.f32()?,
        y: reader.f32()?,
        z: reader.f32()?,
    })
}

fn read_text_string(reader: &mut Cursor<'_>) -> Result<String> {
    let bytes = reader.zero_terminated(MAXIMUM_STRING_LENGTH)?;
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(text.to_owned()),
        Err(_) => Ok(decode_cp1251(bytes)),
    }
}

pub(crate) fn find_destination(state: &[u8], version: u16) -> Option<LevelChangerDestination> {
    let last_start = state.len().checked_sub(32)?;
    let mut found = None;
    for start in 2..last_start {
        if !shapes_end_at(state, start) {
            continue;
        }
        let Some(suffix_bytes) = state.get(start..) else {
            continue;
        };
        let Ok(suffix) = parse_state_suffix(suffix_bytes, u32::from(version)) else {
            continue;
        };
        if !is_identifier(&suffix.dest_level_name) || !is_identifier(&suffix.dest_level_point_name) {
            continue;
        }
        let Some(position) = suffix.dest_position else { continue };
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            continue;
        }
        let Some(tail_start) = start.checked_add(suffix.consumed_bytes) else {
            continue;
        };
        let Some(tail) = state.get(tail_start..) else { continue };
        if !is_string_tail(tail) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(suffix);
    }
    found
}

fn shapes_end_at(state: &[u8], end: usize) -> bool {
    let Some(restrictor_offset) = end.checked_sub(1) else {
        return false;
    };
    if state.get(restrictor_offset).copied().unwrap_or(u8::MAX) > 5 {
        return false;
    }
    for count in 1..=MAXIMUM_SHAPES {
        for boxes in 0..=count {
            let Some(sphere_count) = count.checked_sub(boxes) else {
                return false;
            };
            let Some(sphere_bytes) = sphere_count.checked_mul(SPHERE_BYTES) else {
                return false;
            };
            let Some(box_bytes) = boxes.checked_mul(BOX_BYTES) else {
                return false;
            };
            let Some(total) = 2_usize
                .checked_add(count)
                .and_then(|value| value.checked_add(sphere_bytes))
                .and_then(|value| value.checked_add(box_bytes))
            else {
                break;
            };
            let Some(first) = end.checked_sub(total) else { break };
            if state.get(first).copied() != u8::try_from(count).ok() {
                continue;
            }
            let Some(mut position) = first.checked_add(1) else {
                continue;
            };
            let mut valid = true;
            for _ in 0..count {
                let Some(kind) = state.get(position).copied() else {
                    valid = false;
                    break;
                };
                if kind > 1 {
                    valid = false;
                    break;
                }
                let shape_size = if kind == 1 { BOX_BYTES } else { SPHERE_BYTES };
                let Some(next) = position.checked_add(1).and_then(|value| value.checked_add(shape_size)) else {
                    valid = false;
                    break;
                };
                if next > restrictor_offset {
                    valid = false;
                    break;
                }
                position = next;
            }
            if valid && position == restrictor_offset {
                return true;
            }
        }
    }
    false
}

fn is_string_tail(tail: &[u8]) -> bool {
    if tail.is_empty() {
        return true;
    }
    let mut position = if tail.first().copied().unwrap_or(u8::MAX) <= 1 {
        1
    } else {
        0
    };
    for _ in 0..3 {
        if position >= tail.len() {
            break;
        }
        let Some(rest) = tail.get(position..) else { return false };
        let Some(end) = rest.iter().position(|byte| *byte == 0) else {
            break;
        };
        let Some(text) = rest.get(..end) else { return false };
        if text.iter().any(|byte| !(0x20..=0x7E).contains(byte)) {
            return false;
        }
        let Some(next) = position.checked_add(end).and_then(|value| value.checked_add(1)) else {
            return false;
        };
        position = next;
        let remaining = tail.len().saturating_sub(position);
        if remaining == 0 || remaining == 2 {
            return true;
        }
    }
    matches!(tail.len().saturating_sub(position), 0 | 2)
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text.len() < 128
        && text
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(test)]
mod tests {
    use super::{find_destination, parse_state_suffix, LevelChangerDestination, Vector3};
    use sse_core::Error;

    #[test]
    fn parses_all_golden_suffixes_byte_for_byte() {
        let cases: [(&[u8], LevelChangerDestination); 5] = [
            (
                include_bytes!("../../../fixtures/synthetic/xray-level-changer/synthetic-level-changer-soc.bin"),
                LevelChangerDestination {
                    object_version: 118,
                    dest_game_vertex_id: Some(4660),
                    dest_level_vertex_id: Some(591_751_049),
                    dest_position: Some(Vector3 {
                        x: 1.25,
                        y: -2.5,
                        z: 3.75,
                    }),
                    dest_direction: Some(Vector3 { x: 0.1, y: 0.2, z: 0.3 }),
                    dest_level_name: "garbage".to_owned(),
                    dest_level_point_name: "garbage_from_swamp".to_owned(),
                    silent: Some(true),
                    consumed_bytes: 58,
                },
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-level-changer/synthetic-level-changer-cs.bin"),
                LevelChangerDestination {
                    object_version: 53,
                    dest_game_vertex_id: Some(9),
                    dest_level_vertex_id: Some(17),
                    dest_position: Some(Vector3 { x: 4.0, y: 5.0, z: 6.0 }),
                    dest_direction: Some(Vector3 { x: 0.0, y: 1.5, z: 0.0 }),
                    dest_level_name: "level".to_owned(),
                    dest_level_point_name: "point".to_owned(),
                    silent: None,
                    consumed_bytes: 34,
                },
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-level-changer/synthetic-level-changer-cop.bin"),
                LevelChangerDestination {
                    object_version: 118,
                    dest_game_vertex_id: Some(9029),
                    dest_level_vertex_id: Some(0x1234_5678),
                    dest_position: Some(Vector3 {
                        x: -1.0,
                        y: 2.0,
                        z: 3.0,
                    }),
                    dest_direction: Some(Vector3 { x: 0.4, y: 0.5, z: 0.6 }),
                    dest_level_name: "zaton".to_owned(),
                    dest_level_point_name: "zaton_from_skadovsk".to_owned(),
                    silent: Some(false),
                    consumed_bytes: 57,
                },
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-level-changer/synthetic-level-changer-legacy.bin"),
                LevelChangerDestination {
                    object_version: 33,
                    dest_game_vertex_id: None,
                    dest_level_vertex_id: None,
                    dest_position: None,
                    dest_direction: None,
                    dest_level_name: "level".to_owned(),
                    dest_level_point_name: "point".to_owned(),
                    silent: None,
                    consumed_bytes: 20,
                },
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-level-changer/synthetic-level-changer-cp1251.bin"),
                LevelChangerDestination {
                    object_version: 118,
                    dest_game_vertex_id: Some(1),
                    dest_level_vertex_id: Some(2),
                    dest_position: Some(Vector3 { x: 0.0, y: 0.0, z: 0.0 }),
                    dest_direction: Some(Vector3 { x: 0.0, y: 0.0, z: 0.0 }),
                    dest_level_name: "болото".to_owned(),
                    dest_level_point_name: "точка".to_owned(),
                    silent: Some(true),
                    consumed_bytes: 44,
                },
            ),
        ];
        let versions = [118_u32, 53, 118, 33, 118];
        for ((packet, expected), version) in cases.into_iter().zip(versions) {
            let actual = parse_state_suffix(packet, version);
            assert_eq!(actual, Ok(expected));
        }
    }

    #[test]
    fn rejects_every_truncation_and_oversized_string() {
        let packet = include_bytes!("../../../fixtures/synthetic/xray-level-changer/synthetic-level-changer-soc.bin");
        for length in 0..packet.len() {
            let Some(prefix) = packet.get(..length) else { continue };
            assert!(matches!(parse_state_suffix(prefix, 118), Err(Error::Damaged(_))));
        }
        assert!(matches!(parse_state_suffix(packet, u32::MAX), Err(Error::Damaged(_))));
        let mut hostile = vec![0_u8; 8];
        hostile.extend(std::iter::repeat_n(b'A', (1 << 20) + 1));
        hostile.extend_from_slice(b"end\0");
        assert!(matches!(parse_state_suffix(&hostile, 33), Err(Error::Damaged(_))));
    }

    #[test]
    fn deterministic_mutations_never_panic() {
        let packet = include_bytes!("../../../fixtures/synthetic/xray-level-changer/synthetic-level-changer-soc.bin");
        for index in 0..packet.len() {
            let mut mutated = packet.to_vec();
            if let Some(byte) = mutated.get_mut(index) {
                *byte ^= 1_u8
                    .checked_shl(u32::try_from(index % 8).unwrap_or_default())
                    .unwrap_or_default();
            }
            let _ = parse_state_suffix(&mutated, 118);
        }
    }

    #[test]
    fn accepts_a_destination_only_after_one_valid_shape_list() {
        let packet = include_bytes!("../../../fixtures/synthetic/xray-level-changer/synthetic-level-changer-soc.bin");
        let mut state = vec![1, 0];
        state.extend(std::iter::repeat_n(0_u8, 16));
        state.push(0);
        state.extend_from_slice(packet);
        assert!(find_destination(&state, 118).is_some());

        assert!(find_destination(packet, 118).is_none());
    }
}
