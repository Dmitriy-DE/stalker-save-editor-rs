//! Actor kill counts from the statistics registry (PDA "Statistics").
//!
//! The statistics registry is the last registry of the ALife registry chunk (9), after the task registry.
//! Its start is not stored, so it is located as the offset from which the registry parses completely and
//! ends exactly at the chunk end. Port of `XRayProgressReader` (statistics part) from the C# 1.3.1 core.

/// Longest zero-terminated string accepted in the registry (the C# reader's limit).
const MAXIMUM_STRING: usize = 4096;
/// Largest element count accepted in the registry (the C# reader's limit).
const MAXIMUM_COUNT: u32 = 100_000;
/// Smallest registry prefix: the element count, the owner id and at least one more field.
const MINIMUM_TAIL: usize = 10;

/// Kills of the actor, from the two statistics sections the game keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kills {
    /// Kills of stalkers and bandits (section `stalkerkills`).
    pub stalkers: i64,
    /// Kills of monsters (section `monsterkills`).
    pub monsters: i64,
}

struct Line {
    section: String,
    count: i32,
}

/// Reads the actor's kill counts from the registry chunk, or `None` when no statistics registry parses there.
///
/// `from` is the first offset that can start the registry (the end of the relation registry, or 0).
pub(crate) fn read_kills(chunk: &[u8], actor_id: u16, from: usize) -> Option<Kills> {
    let top = chunk.len().checked_sub(MINIMUM_TAIL)?;
    for start in (from..=top).rev() {
        if !actor_prefix(chunk, start, actor_id) {
            continue;
        }
        let mut reader = Reader {
            data: chunk,
            position: start,
        };
        if let Some(lines) = read_statistics(&mut reader, actor_id) {
            if reader.position == chunk.len() {
                return Some(Kills {
                    stalkers: sum_section(&lines, "stalkerkills"),
                    monsters: sum_section(&lines, "monsterkills"),
                });
            }
        }
    }
    None
}

fn sum_section(lines: &[Line], section: &str) -> i64 {
    lines
        .iter()
        .filter(|line| line.section == section)
        .map(|line| i64::from(line.count))
        .sum()
}

/// The first `N` bytes as a fixed array, or `None` when fewer are left.
fn le_bytes<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    bytes.get(..N)?.try_into().ok()
}

fn actor_prefix(chunk: &[u8], start: usize, actor_id: u16) -> bool {
    let Some(rest) = chunk.get(start..) else {
        return false;
    };
    let Some(count) = le_bytes::<4>(rest).map(u32::from_le_bytes) else {
        return false;
    };
    let Some(owner) = rest.get(4..).and_then(le_bytes::<2>).map(u16::from_le_bytes) else {
        return false;
    };
    (1..=16).contains(&count) && owner == actor_id
}

/// Parses every statistics line that belongs to `actor_id`; `None` on truncation or a malformed field.
fn read_statistics(reader: &mut Reader<'_>, actor_id: u16) -> Option<Vec<Line>> {
    let mut result = Vec::new();
    let objects = reader.count()?;
    for _ in 0..objects {
        let owner = reader.u16()?;
        let sections = reader.count()?;
        for _ in 0..sections {
            let details = reader.count()?;
            let mut lines = Vec::new();
            for _ in 0..details {
                let _key = reader.string()?;
                let count = reader.i32()?;
                let _points = reader.i32()?;
                let _ = reader.string()?;
                lines.push(count);
            }
            let section = reader.string()?;
            if owner == actor_id {
                result.extend(lines.into_iter().map(|count| Line {
                    section: section.clone(),
                    count,
                }));
            }
        }
    }
    Some(result)
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl Reader<'_> {
    fn take(&mut self, length: usize) -> Option<&[u8]> {
        let end = self.position.checked_add(length)?;
        let bytes = self.data.get(self.position..end)?;
        self.position = end;
        Some(bytes)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(le_bytes::<2>(self.take(2)?)?))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(le_bytes::<4>(self.take(4)?)?))
    }

    fn count(&mut self) -> Option<usize> {
        let value = u32::from_le_bytes(le_bytes::<4>(self.take(4)?)?);
        if value > MAXIMUM_COUNT {
            return None;
        }
        usize::try_from(value).ok()
    }

    /// Zero-terminated string within the first `MAXIMUM_STRING` bytes; control characters are refused.
    /// Text is Latin-1 (one char per byte), as the C# reader decodes it.
    fn string(&mut self) -> Option<String> {
        let rest = self.data.get(self.position..)?;
        let window = rest.get(..rest.len().min(MAXIMUM_STRING))?;
        let end = window.iter().position(|byte| *byte == 0)?;
        let bytes = window.get(..end)?;
        if bytes.iter().any(|byte| *byte < 0x20) {
            return None;
        }
        self.position = self.position.checked_add(end)?.checked_add(1)?;
        Some(bytes.iter().map(|byte| char::from(*byte)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{read_kills, Kills};

    /// One object owned by actor 7 with `stalkerkills` = 5 and `monsterkills` = 2, ending at the chunk end.
    fn chunk(owner: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1_u32.to_le_bytes()); // objects
        out.extend_from_slice(&owner.to_le_bytes());
        out.extend_from_slice(&2_u32.to_le_bytes()); // sections
        for (section, count) in [("stalkerkills", 5_i32), ("monsterkills", 2_i32)] {
            out.extend_from_slice(&1_u32.to_le_bytes()); // details
            out.extend_from_slice(b"bandit\0");
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&0_i32.to_le_bytes());
            out.push(0);
            out.extend_from_slice(section.as_bytes());
            out.push(0);
        }
        out
    }

    #[test]
    fn kills_are_summed_per_section_for_the_actor() {
        let data = chunk(7);
        // The prefix needs the actor id at offset 4 of the registry, so the registry starts at offset 0.
        assert_eq!(
            read_kills(&data, 7, 0),
            Some(Kills {
                stalkers: 5,
                monsters: 2
            })
        );
    }

    #[test]
    fn statistics_of_another_owner_are_not_counted() {
        let data = chunk(9);
        assert_eq!(read_kills(&data, 7, 0), None);
    }

    #[test]
    fn truncated_registry_is_not_an_error_and_not_a_count() {
        let data = chunk(7);
        let cut = data.get(..data.len() - 3).unwrap_or_default();
        assert_eq!(read_kills(cut, 7, 0), None);
        assert_eq!(read_kills(&[], 7, 0), None);
    }
}
