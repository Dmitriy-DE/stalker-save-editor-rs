//! UI-independent coordination for save writes, restores, drafts, and file checks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

#[derive(Clone)]
/// Shared, UI-independent state machine for save-file operations and drafts.
pub struct SaveSession {
    inner: Arc<SessionInner>,
}

struct SessionInner {
    state: Mutex<SessionState>,
    idle: Condvar,
}

#[derive(Default)]
struct SessionState {
    next_operation_id: u64,
    latest_operation_id: Option<SaveOperationId>,
    active_operation: Option<ActiveOperation>,
    close_requested: bool,
    next_draft_generation: u64,
    draft_generations: HashMap<String, DraftGeneration>,
    next_file_check_id: u64,
    file_check: Option<ActiveFileCheck>,
}

#[derive(Clone, Copy)]
struct ActiveOperation {
    id: SaveOperationId,
    kind: SaveOperationKind,
}

struct ActiveFileCheck {
    token: FileCheckToken,
    path: PathBuf,
}

/// Kind of exclusive operation currently using a save file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveOperationKind {
    /// A save edit or quest repair is being written.
    Save,
    /// A backup is being restored over its original save.
    Restore,
}

/// Unique identity for a save or restore operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SaveOperationId(u64);

impl SaveOperationId {
    /// Returns the stable numeric request identifier.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// RAII lease for an exclusive save write or in-place restore.
///
/// Dropping the lease always releases the operation, including on early return or panic.
pub struct SaveOperationGuard {
    inner: Arc<SessionInner>,
    id: SaveOperationId,
    kind: SaveOperationKind,
}

impl SaveOperationGuard {
    /// Returns the unique operation identifier.
    #[must_use]
    pub const fn id(&self) -> SaveOperationId {
        self.id
    }

    /// Returns the operation kind.
    #[must_use]
    pub const fn kind(&self) -> SaveOperationKind {
        self.kind
    }
}

impl Drop for SaveOperationGuard {
    fn drop(&mut self) {
        let mut state = lock(&self.inner.state);
        if state.active_operation.is_some_and(|operation| operation.id == self.id) {
            state.active_operation = None;
            self.inner.idle.notify_all();
        }
    }
}

/// Result of requesting application close while save I/O may still be active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseDecision {
    /// There is no exclusive save operation; the caller may close immediately.
    Allowed,
    /// A write or in-place restore must finish before the pending close can proceed.
    Deferred,
}

/// Generation assigned to a pending draft write for one or more source saves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DraftGeneration(u64);

impl DraftGeneration {
    /// Returns the numeric generation for diagnostics and request payloads.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Identity for one background file-monitor check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileCheckToken(u64);

/// RAII lease for one background file-monitor check.
///
/// Dropping an unfinished lease clears its request, including on cancellation or panic.
pub struct FileCheckGuard {
    inner: Arc<SessionInner>,
    token: FileCheckToken,
}

impl FileCheckGuard {
    /// Finishes the check and returns whether its result is still current.
    #[must_use]
    pub fn finish(&mut self) -> bool {
        let mut state = lock(&self.inner.state);
        let Some(active) = state.file_check.as_ref() else {
            return false;
        };
        if active.token != self.token {
            return false;
        }
        state.file_check = None;
        state.active_operation.is_none()
    }
}

impl Drop for FileCheckGuard {
    fn drop(&mut self) {
        let mut state = lock(&self.inner.state);
        if state
            .file_check
            .as_ref()
            .is_some_and(|active| active.token == self.token)
        {
            state.file_check = None;
        }
    }
}

impl Default for SaveSession {
    fn default() -> Self {
        Self::new()
    }
}

impl SaveSession {
    /// Creates an idle save-operation state machine.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SessionInner {
                state: Mutex::new(SessionState::default()),
                idle: Condvar::new(),
            }),
        }
    }

    /// Starts a save write and invalidates any monitor result for `path`.
    ///
    /// Returns `None` if another operation is active or an identifier is exhausted.
    pub fn begin_save(&self, path: &Path) -> Option<SaveOperationGuard> {
        self.begin_operation(path, SaveOperationKind::Save)
    }

    /// Starts an in-place restore and invalidates any monitor result for `path`.
    ///
    /// Returns `None` if another operation is active or an identifier is exhausted.
    pub fn begin_restore(&self, path: &Path) -> Option<SaveOperationGuard> {
        self.begin_operation(path, SaveOperationKind::Restore)
    }

    fn begin_operation(&self, path: &Path, kind: SaveOperationKind) -> Option<SaveOperationGuard> {
        let mut state = lock(&self.inner.state);
        if state.active_operation.is_some() {
            return None;
        }
        let id = SaveOperationId(state.next_operation_id.checked_add(1)?);
        state.next_operation_id = id.get();
        state.latest_operation_id = Some(id);
        if state.file_check.as_ref().is_some_and(|check| check.path == path) {
            state.file_check = None;
        }
        state.active_operation = Some(ActiveOperation { id, kind });
        Some(SaveOperationGuard {
            inner: Arc::clone(&self.inner),
            id,
            kind,
        })
    }

    /// Returns whether a save write is active.
    #[must_use]
    pub fn is_saving(&self) -> bool {
        lock(&self.inner.state)
            .active_operation
            .is_some_and(|operation| operation.kind == SaveOperationKind::Save)
    }

    /// Returns whether an in-place restore is active.
    #[must_use]
    pub fn is_restoring(&self) -> bool {
        lock(&self.inner.state)
            .active_operation
            .is_some_and(|operation| operation.kind == SaveOperationKind::Restore)
    }

    /// Returns whether a save write or in-place restore is active.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        lock(&self.inner.state).active_operation.is_some()
    }

    /// Tests whether `id` is still the latest accepted operation.
    ///
    /// This lets a screen ignore an old completion after a newer operation began.
    #[must_use]
    pub fn is_latest_operation(&self, id: SaveOperationId) -> bool {
        lock(&self.inner.state).latest_operation_id == Some(id)
    }

    /// Records a close request, deferring it until the active write or restore ends.
    pub fn request_close(&self) -> CloseDecision {
        let mut state = lock(&self.inner.state);
        if state.active_operation.is_some() {
            state.close_requested = true;
            CloseDecision::Deferred
        } else {
            state.close_requested = false;
            CloseDecision::Allowed
        }
    }

    /// Returns whether a deferred close can proceed without consuming the request.
    #[must_use]
    pub fn deferred_close_ready(&self) -> bool {
        let state = lock(&self.inner.state);
        state.close_requested && state.active_operation.is_none()
    }

    /// Returns and clears a deferred close once save I/O has ended.
    pub fn take_deferred_close_ready(&self) -> bool {
        let mut state = lock(&self.inner.state);
        if state.close_requested && state.active_operation.is_none() {
            state.close_requested = false;
            true
        } else {
            false
        }
    }

    /// Blocks the calling shutdown thread until an active write or in-place restore ends.
    pub fn wait_until_idle(&self) {
        let mut state = lock(&self.inner.state);
        while state.active_operation.is_some() {
            state = self
                .inner
                .idle
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Starts a generation for persisting one draft journal.
    pub fn next_draft_generation(&self, source_sha256: &str) -> Option<DraftGeneration> {
        self.next_draft_generation_for(std::iter::once(source_sha256))
    }

    /// Starts one generation for a group of journals that must be written together.
    pub fn next_draft_generation_for<'a>(
        &self,
        source_sha256s: impl IntoIterator<Item = &'a str>,
    ) -> Option<DraftGeneration> {
        let hashes: Vec<&str> = source_sha256s.into_iter().collect();
        if hashes.is_empty() {
            return None;
        }
        let mut state = lock(&self.inner.state);
        let generation = DraftGeneration(state.next_draft_generation.checked_add(1)?);
        state.next_draft_generation = generation.get();
        for hash in hashes {
            state.draft_generations.insert(hash.to_owned(), generation);
        }
        Some(generation)
    }

    /// Returns the latest generation for `source_sha256`, if a write is pending.
    #[must_use]
    pub fn draft_generation(&self, source_sha256: &str) -> Option<DraftGeneration> {
        lock(&self.inner.state).draft_generations.get(source_sha256).copied()
    }

    /// Returns whether a pending write still owns the current generation for a save.
    #[must_use]
    pub fn is_current_draft_generation(&self, source_sha256: &str, generation: DraftGeneration) -> bool {
        self.draft_generation(source_sha256) == Some(generation)
    }

    /// Clears a draft generation only if no newer write replaced it.
    pub fn clear_draft_if_current(&self, source_sha256: &str, generation: DraftGeneration) -> bool {
        let mut state = lock(&self.inner.state);
        if state.draft_generations.get(source_sha256).copied() != Some(generation) {
            return false;
        }
        state.draft_generations.remove(source_sha256);
        true
    }

    /// Starts one file check unless another check or save operation is active.
    pub fn begin_file_check(&self, path: &Path) -> Option<FileCheckGuard> {
        let mut state = lock(&self.inner.state);
        if state.active_operation.is_some() || state.file_check.is_some() {
            return None;
        }
        let token = FileCheckToken(state.next_file_check_id.checked_add(1)?);
        state.next_file_check_id = token.0;
        state.file_check = Some(ActiveFileCheck {
            token,
            path: path.to_path_buf(),
        });
        Some(FileCheckGuard {
            inner: Arc::clone(&self.inner),
            token,
        })
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{CloseDecision, SaveOperationKind, SaveSession};
    use std::path::PathBuf;

    fn required<T>(value: Option<T>, reason: &str) -> T {
        match value {
            Some(value) => value,
            None => panic!("{reason}"),
        }
    }

    #[test]
    fn save_and_restore_are_mutually_exclusive_and_guards_release_on_drop() {
        let session = SaveSession::new();
        let path = PathBuf::from("fixture.sav");
        let save = required(session.begin_save(&path), "save should start while idle");
        assert_eq!(save.kind(), SaveOperationKind::Save);
        assert!(session.is_saving());
        assert!(session.is_busy());
        assert!(session.begin_save(&path).is_none());
        assert!(session.begin_restore(&path).is_none());

        drop(save);
        assert!(!session.is_busy());

        let restore = required(session.begin_restore(&path), "restore should start while idle");
        assert_eq!(restore.kind(), SaveOperationKind::Restore);
        assert!(session.is_restoring());
        assert!(session.begin_save(&path).is_none());
        assert!(session.begin_restore(&path).is_none());

        drop(restore);
        assert!(!session.is_busy());
    }

    #[test]
    fn only_the_newest_operation_id_is_current() {
        let session = SaveSession::new();
        let first = required(
            session.begin_save(&PathBuf::from("first.sav")),
            "first save should start",
        );
        let first_id = first.id();
        assert!(session.is_latest_operation(first_id));
        drop(first);

        let second = required(
            session.begin_restore(&PathBuf::from("second.sav")),
            "restore should start after the first operation ends",
        );
        let second_id = second.id();
        assert!(second_id.get() > first_id.get());
        assert!(!session.is_latest_operation(first_id));
        assert!(session.is_latest_operation(second_id));
    }

    #[test]
    fn stale_draft_generation_cannot_clear_a_newer_generation() {
        let session = SaveSession::new();
        let hash = "ab".repeat(32);
        let old = required(session.next_draft_generation(&hash), "generation should be allocated");
        let current = required(session.next_draft_generation(&hash), "generation should advance");

        assert!(!session.is_current_draft_generation(&hash, old));
        assert!(session.is_current_draft_generation(&hash, current));
        assert!(!session.clear_draft_if_current(&hash, old));
        assert!(session.is_current_draft_generation(&hash, current));
        assert!(session.clear_draft_if_current(&hash, current));
        assert!(!session.is_current_draft_generation(&hash, current));
    }

    #[test]
    fn draft_generations_for_different_saves_are_independent() {
        let session = SaveSession::new();
        let first_hash = "11".repeat(32);
        let second_hash = "22".repeat(32);
        let first = required(session.next_draft_generation(&first_hash), "first generation");
        let second = required(session.next_draft_generation(&second_hash), "second generation");

        assert!(session.is_current_draft_generation(&first_hash, first));
        assert!(session.is_current_draft_generation(&second_hash, second));
        assert!(session.clear_draft_if_current(&first_hash, first));
        assert!(session.is_current_draft_generation(&second_hash, second));
    }

    #[test]
    fn grouped_draft_generation_updates_all_sources_together() {
        let session = SaveSession::new();
        let first_hash = "33".repeat(32);
        let second_hash = "44".repeat(32);
        let generation = required(
            session.next_draft_generation_for([first_hash.as_str(), second_hash.as_str()]),
            "group generation should be allocated",
        );

        assert_eq!(session.draft_generation(&first_hash), Some(generation));
        assert_eq!(session.draft_generation(&second_hash), Some(generation));
        assert!(session.next_draft_generation_for(std::iter::empty()).is_none());
        assert_eq!(session.draft_generation(&first_hash), Some(generation));
    }

    #[test]
    fn close_request_is_deferred_until_write_or_restore_guard_drops() {
        let session = SaveSession::new();
        let path = PathBuf::from("fixture.sav");
        let operation = required(session.begin_save(&path), "save should start");

        assert_eq!(session.request_close(), CloseDecision::Deferred);
        assert_eq!(session.request_close(), CloseDecision::Deferred);
        assert!(!session.deferred_close_ready());
        assert!(!session.take_deferred_close_ready());
        drop(operation);
        assert!(session.deferred_close_ready());
        assert!(session.deferred_close_ready());
        assert!(session.take_deferred_close_ready());
        assert!(!session.deferred_close_ready());
        assert!(!session.take_deferred_close_ready());
        assert_eq!(session.request_close(), CloseDecision::Allowed);
    }

    #[test]
    fn a_new_idle_close_request_clears_an_old_deferred_request() {
        let session = SaveSession::new();
        let path = PathBuf::from("fixture.sav");
        let operation = required(session.begin_restore(&path), "restore should start");

        assert_eq!(session.request_close(), CloseDecision::Deferred);
        drop(operation);
        assert!(session.deferred_close_ready());
        assert_eq!(session.request_close(), CloseDecision::Allowed);
        assert!(!session.deferred_close_ready());
    }

    #[test]
    fn save_and_restore_invalidate_file_checks_for_their_path() {
        for kind in [SaveOperationKind::Save, SaveOperationKind::Restore] {
            let session = SaveSession::new();
            let path = PathBuf::from("fixture.sav");
            let mut stale = required(session.begin_file_check(&path), "file check should start while idle");
            assert!(session.begin_file_check(&path).is_none());
            let operation = required(
                match kind {
                    SaveOperationKind::Save => session.begin_save(&path),
                    SaveOperationKind::Restore => session.begin_restore(&path),
                },
                "operation should start while a monitor check is active",
            );

            assert!(!stale.finish());
            assert!(session.begin_file_check(&path).is_none());
            drop(operation);
            let mut current = required(session.begin_file_check(&path), "check should resume after operation");
            assert!(current.finish());
        }
    }

    #[test]
    fn an_operation_on_another_path_keeps_a_file_check_current() {
        let session = SaveSession::new();
        let mut check = required(
            session.begin_file_check(&PathBuf::from("selected.sav")),
            "check should start",
        );
        let operation = required(
            session.begin_save(&PathBuf::from("other.sav")),
            "unrelated save should start",
        );
        drop(operation);

        assert!(check.finish());
    }

    #[test]
    fn file_check_result_is_consumed_once_and_invalidated_by_a_later_check() {
        let session = SaveSession::new();
        let path = PathBuf::from("fixture.sav");
        let mut first = required(session.begin_file_check(&path), "first check should start");

        assert!(first.finish());

        let first = required(session.begin_file_check(&path), "second check should start");
        let restore = required(session.begin_restore(&path), "restore should start");
        drop(restore);
        let mut second = required(session.begin_file_check(&path), "check should restart after restore");
        let mut first = first;
        assert!(!first.finish());
        assert!(second.finish());
    }

    #[test]
    fn dropping_or_unwinding_a_file_check_releases_its_slot() {
        let session = SaveSession::new();
        let path = PathBuf::from("fixture.sav");
        let stale = required(session.begin_file_check(&path), "check should start");
        drop(stale);
        assert!(session.begin_file_check(&path).is_some());

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _check = required(session.begin_file_check(&path), "previous check should be dropped");
            panic!("simulate cancelled monitor worker");
        }));
        assert!(result.is_err());
        assert!(session.begin_file_check(&path).is_some());
    }

    #[test]
    fn guard_drop_releases_state_during_unwind() {
        let session = SaveSession::new();
        let path = PathBuf::from("fixture.sav");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _operation = required(session.begin_restore(&path), "restore should start");
            panic!("simulate worker panic");
        }));

        assert!(result.is_err());
        assert!(!session.is_busy());
    }
}
