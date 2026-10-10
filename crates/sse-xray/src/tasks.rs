//! PDA tasks of the actor, with their game-time receive and finish times.
//!
//! The task registry is the registry before the statistics one in the ALife registry chunk (9). Its start is
//! not stored, so it is located as an offset from which the registry parses completely and ends within 128
//! bytes before the statistics (or at the chunk end when there are none). Port of `XRayProgressReader` from the
//! C# 1.3.1 core; Shadow of Chernobyl records have no task-level state, and Enhanced Edition Clear Sky and Call
//! of Pripyat add one trailing byte to every task.

use crate::registry::{actor_prefix, latin1, Reader};
use crate::statistics::{find_statistics, Line, MINIMUM_TAIL};

/// Longest gap in bytes accepted between the task registry and the statistics registry (the C# reader's limit).
const MAXIMUM_GAP: usize = 128;

/// State of a task or of one of its objectives, as the game stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// The task failed.
    Failed,
    /// The task is in progress.
    InProgress,
    /// The task is complete.
    Completed,
    /// The task was skipped.
    Skipped,
    /// A state code the reader does not know.
    Unknown,
}

impl TaskState {
    const fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Failed,
            1 => Self::InProgress,
            2 => Self::Completed,
            3 => Self::Skipped,
            _ => Self::Unknown,
        }
    }
}

/// One PDA task of the actor. Times are the save's game-time values in milliseconds; 0 means not set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Task identifier, as the game writes it.
    pub id: String,
    /// Title, decoded as Windows-1251 (the game's text encoding).
    pub title: String,
    /// Task state.
    pub state: TaskState,
    /// Game time the task was received.
    pub receive_time: u64,
    /// Game time the task finished (0 when it did not).
    pub finish_time: u64,
}

/// What the registry chunk holds for the actor: the statistics lines, and the tasks when their registry was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Progress {
    /// Tasks of the actor; `None` when only the statistics registry was found and the task registry was not.
    pub(crate) tasks: Option<Vec<Task>>,
    /// Statistics lines of the actor; empty when the save has no statistics registry.
    pub(crate) statistics: Vec<Line>,
}

/// Reads the actor's tasks and statistics from the registry chunk, or `None` when neither registry is found.
///
/// `from` is the first offset that can start a registry (the end of the relation registry, or 0). `soc` is true
/// for Shadow of Chernobyl (both editions), whose task records differ.
pub(crate) fn read_progress(chunk: &[u8], actor_id: u16, from: usize, soc: bool) -> Option<Progress> {
    let (statistics_start, statistics) = match find_statistics(chunk, actor_id, from) {
        Some(found) => (found.start, found.lines),
        // Call of Pripyat has no statistics registry: the task registry is the last one.
        None => (chunk.len(), Vec::new()),
    };
    if let Some(tasks) = find_tasks(chunk, actor_id, from, statistics_start, soc) {
        return Some(Progress {
            tasks: Some(tasks),
            statistics,
        });
    }
    (!statistics.is_empty()).then_some(Progress {
        tasks: None,
        statistics,
    })
}

/// The task registry: the latest start from which it parses and ends within the gap before `statistics_start`.
fn find_tasks(chunk: &[u8], actor_id: u16, from: usize, statistics_start: usize, soc: bool) -> Option<Vec<Task>> {
    let top = statistics_start.checked_sub(MINIMUM_TAIL)?;
    for start in (from..=top).rev() {
        if !actor_prefix(chunk, start, actor_id) {
            continue;
        }
        // Enhanced Edition Clear Sky and Call of Pripyat add one byte to every task record; try both layouts.
        let trailers: &[bool] = if soc { &[false] } else { &[false, true] };
        for trailing in trailers {
            let mut reader = Reader::new(chunk, start);
            let Some(tasks) = read_tasks(&mut reader, soc, actor_id, *trailing) else {
                continue;
            };
            if statistics_start
                .checked_sub(reader.position())
                .is_some_and(|gap| gap <= MAXIMUM_GAP)
            {
                return Some(tasks);
            }
        }
    }
    None
}

/// Parses the task records of every owner; only the actor's own tasks are kept.
fn read_tasks(reader: &mut Reader<'_>, soc: bool, actor_id: u16, trailing: bool) -> Option<Vec<Task>> {
    let mut result = Vec::new();
    for _ in 0..reader.count()? {
        let owner = reader.u16()?;
        for _ in 0..reader.count()? {
            let task = if soc {
                read_soc_task(reader)?
            } else {
                read_cs_task(reader)?
            };
            if trailing {
                reader.u8()?;
            }
            if owner == actor_id {
                result.push(task);
            }
        }
    }
    Some(result)
}

/// Shadow of Chernobyl task: the task state is the state of its first objective.
fn read_soc_task(reader: &mut Reader<'_>) -> Option<Task> {
    let id = latin1(reader.string()?);
    let receive_time = reader.u64()?;
    let finish_time = reader.u64()?;
    reader.u64()?;
    let title = decode_title(reader.string()?);
    let objectives = reader.count()?;
    let mut state = TaskState::Unknown;
    for index in 0..objectives {
        reader.i32()?; // objective index
        let objective_state = TaskState::from_code(reader.i32()?);
        if index == 0 {
            state = objective_state;
        }
        reader.string()?; // description
        reader.string()?; // map location
        reader.u16()?; // map object id
        reader.i32()?; // task state, repeated
        reader.u8()?; // default location enabled
        reader.string()?; // map hint
        reader.string()?; // icon texture
        reader.skip(16)?; // icon rectangle
        reader.string()?; // article id
        for _ in 0..4 {
            reader.skip_string_vector()?;
        }
        if reader.u8()? != 0 {
            for _ in 0..4 {
                reader.skip_string_vector()?;
            }
        }
    }
    Some(Task {
        id,
        title,
        state,
        receive_time,
        finish_time,
    })
}

/// Clear Sky and Call of Pripyat task: the task carries its own state.
fn read_cs_task(reader: &mut Reader<'_>) -> Option<Task> {
    let id = latin1(reader.string()?);
    let state = TaskState::from_code(reader.i32()?);
    reader.i32()?; // task type
    let receive_time = reader.u64()?;
    let finish_time = reader.u64()?;
    reader.u64()?; // time to complete
    reader.u64()?; // timer finish
    let title = decode_title(reader.string()?);
    reader.string()?; // description
    for _ in 0..4 {
        reader.skip_string_vector()?;
    }
    reader.string()?; // icon texture
    reader.string()?; // map hint
    reader.string()?; // map location
    reader.u16()?; // map object id
    reader.u32()?; // priority
    Some(Task {
        id,
        title,
        state,
        receive_time,
        finish_time,
    })
}

fn decode_title(bytes: &[u8]) -> String {
    crate::save::decode_cp1251(bytes)
}

#[cfg(test)]
mod tests {
    use super::{read_progress, TaskState};

    /// Clear Sky style registry for actor 7: one owner with one task, then one statistics owner with no lines.
    fn cs_chunk(state: i32, owner: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1_u32.to_le_bytes()); // owners
        out.extend_from_slice(&owner.to_le_bytes());
        out.extend_from_slice(&1_u32.to_le_bytes()); // tasks
        out.extend_from_slice(b"tsk_1\0");
        out.extend_from_slice(&state.to_le_bytes());
        out.extend_from_slice(&0_i32.to_le_bytes()); // type
        out.extend_from_slice(&100_u64.to_le_bytes()); // receive
        out.extend_from_slice(&200_u64.to_le_bytes()); // finish
        out.extend_from_slice(&0_u64.to_le_bytes());
        out.extend_from_slice(&0_u64.to_le_bytes());
        out.extend_from_slice(b"Title\0");
        out.push(0); // description
        for _ in 0..4 {
            out.extend_from_slice(&0_u32.to_le_bytes()); // empty string vector
        }
        out.extend_from_slice(&[0, 0, 0]); // icon, map hint, map location
        out.extend_from_slice(&0_u16.to_le_bytes()); // map object id
        out.extend_from_slice(&0_u32.to_le_bytes()); // priority
        out
    }

    #[test]
    fn a_clear_sky_task_is_read_with_its_times_and_state() {
        let data = cs_chunk(2, 7);
        let progress = read_progress(&data, 7, 0, false);
        let tasks = progress.and_then(|p| p.tasks).unwrap_or_default();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks.first().map(|t| t.state), Some(TaskState::Completed));
        assert_eq!(tasks.first().map(|t| (t.receive_time, t.finish_time)), Some((100, 200)));
    }

    #[test]
    fn a_registry_that_starts_with_another_owner_is_not_the_actors() {
        let data = cs_chunk(2, 9);
        assert!(read_progress(&data, 7, 0, false).is_none());
    }

    #[test]
    fn a_truncated_task_registry_is_not_found() {
        let data = cs_chunk(2, 7);
        let cut = data.get(..data.len().saturating_sub(4)).unwrap_or_default();
        assert!(read_progress(cut, 7, 0, false).is_none());
    }
}
