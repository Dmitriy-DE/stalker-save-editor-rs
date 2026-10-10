//! Actor statistics (PDA "Statistics"): kill counts by section.
//!
//! The statistics registry is the last registry of the ALife registry chunk (9), after the task registry.
//! Its start is not stored, so it is located as the latest offset from which the registry parses completely and
//! ends exactly at the chunk end. Port of `XRayProgressReader` (statistics part) from the C# 1.3.1 core.

use crate::registry::{actor_prefix, latin1, Reader};

/// Smallest registry prefix: the owner count, the owner id and at least one more field.
pub(crate) const MINIMUM_TAIL: usize = 10;

/// Kills of the actor, from the two statistics sections the game keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kills {
    /// Kills of stalkers and bandits (section `stalkerkills`).
    pub stalkers: i64,
    /// Kills of monsters (section `monsterkills`).
    pub monsters: i64,
}

/// One statistics line of the actor: its section and count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) section: String,
    pub(crate) count: i32,
}

/// The statistics registry: where it starts in the chunk and the actor's lines.
pub(crate) struct Statistics {
    pub(crate) start: usize,
    pub(crate) lines: Vec<Line>,
}

/// Locates the actor's statistics registry, or `None` when no registry parses to the chunk end.
///
/// `from` is the first offset that can start the registry (the end of the relation registry, or 0).
pub(crate) fn find_statistics(chunk: &[u8], actor_id: u16, from: usize) -> Option<Statistics> {
    let top = chunk.len().checked_sub(MINIMUM_TAIL)?;
    for start in (from..=top).rev() {
        if !actor_prefix(chunk, start, actor_id) {
            continue;
        }
        let mut reader = Reader::new(chunk, start);
        if let Some(lines) = read_statistics(&mut reader, actor_id) {
            if reader.position() == chunk.len() {
                return Some(Statistics { start, lines });
            }
        }
    }
    None
}

/// Kill counts from statistics lines.
pub(crate) fn kills(lines: &[Line]) -> Kills {
    Kills {
        stalkers: sum_section(lines, "stalkerkills"),
        monsters: sum_section(lines, "monsterkills"),
    }
}

fn sum_section(lines: &[Line], section: &str) -> i64 {
    lines
        .iter()
        .filter(|line| line.section == section)
        .map(|line| i64::from(line.count))
        .sum()
}

/// Parses every statistics line that belongs to `actor_id`; `None` on truncation or a malformed field.
fn read_statistics(reader: &mut Reader<'_>, actor_id: u16) -> Option<Vec<Line>> {
    let mut result = Vec::new();
    for _ in 0..reader.count()? {
        let owner = reader.u16()?;
        for _ in 0..reader.count()? {
            let mut counts = Vec::new();
            for _ in 0..reader.count()? {
                reader.string()?; // key
                counts.push(reader.i32()?);
                reader.i32()?; // points
                reader.string()?; // text
            }
            let section = latin1(reader.string()?);
            if owner == actor_id {
                result.extend(counts.into_iter().map(|count| Line {
                    section: section.clone(),
                    count,
                }));
            }
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::{find_statistics, kills, Kills};

    /// One owner, actor 7, with `stalkerkills` = 5 and `monsterkills` = 2 and nothing after them.
    fn chunk(owner: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1_u32.to_le_bytes()); // owners
        out.extend_from_slice(&owner.to_le_bytes());
        out.extend_from_slice(&2_u32.to_le_bytes()); // sections
        for (section, count) in [("stalkerkills", 5_i32), ("monsterkills", 2_i32)] {
            out.extend_from_slice(&1_u32.to_le_bytes()); // lines
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
        let found = find_statistics(&data, 7, 0);
        assert_eq!(found.as_ref().map(|s| s.start), Some(0));
        assert_eq!(
            found.map(|s| kills(&s.lines)),
            Some(Kills {
                stalkers: 5,
                monsters: 2
            })
        );
    }

    #[test]
    fn a_registry_that_starts_with_another_owner_is_not_the_actors() {
        let data = chunk(9);
        assert!(find_statistics(&data, 7, 0).is_none());
    }

    #[test]
    fn truncated_registry_is_not_found() {
        let data = chunk(7);
        let cut = data.get(..data.len().saturating_sub(3)).unwrap_or_default();
        assert!(find_statistics(cut, 7, 0).is_none());
        assert!(find_statistics(&[], 7, 0).is_none());
    }
}
