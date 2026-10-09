//! Game fix installation and rollback engine.
//!
//! Provides atomic, byte-preserving patch application with preflight SHA-256 checks,
//! journaled recovery, rollback on failure, and compatibility with the reference C# manifests.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sse_catalog::parse_json;
use sse_codecs::sha256::sha256_hex;
use sse_content::file_tree::{CompanionGame, GameFileTree};

use sse_core::{Error, Result};

use crate::all_spawn::AllSpawnEditor;
use crate::catalog::GameFixCatalog;
use crate::fs_util::{
    check_no_links, normalize_relative_path, resolve_game_path, resolve_state_path, AtomicFileWriter,
};
use crate::identify::identify_game;
use crate::models::{
    FileOverlayOperation, GameFixCategory, GameFixDefinition, GameFixImplementationType, GameFixInstallResult,
    GameFixInstalledInfo, GameFixManifest, GameFixMaturity, GameFixPreset, GameFixPresetResult,
    GameFixSaveCompatibility, GameFixState, GameFixUninstallCheck, GameFixVerificationState, GameTarget,
    ManagedGameFile, SpawnEditOperation,
};
use crate::store::GameFixContentStore;

const MANIFEST_SCHEMA_VERSION: u32 = 2;
const JOURNAL_SCHEMA_VERSION: u32 = 1;
const STATE_DIRECTORY_NAME: &str = ".save-editor-game-fixes";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const JOURNAL_FILE_NAME: &str = "transaction.json";
const RECOVERY_NOTE_FILE_NAME: &str = "RECOVERY.txt";
const JOURNAL_INSTALL: &str = "install";
const JOURNAL_UNINSTALL: &str = "uninstall";
const ABSENT_SOURCE_FINGERPRINT: &str = "absent";

type OverlayReaderFn = Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

/// Game fix transaction engine.
pub struct GameFixEngine {
    allow_synthetic_definitions: bool,
    overlay_reader: OverlayReaderFn,
}

struct TextPatchTarget {
    abs_path: PathBuf,
    bytes: Vec<u8>,
    text: String,
    code_page: u32,
    existed: bool,
    fingerprint: Option<String>,
}

impl Default for GameFixEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl GameFixEngine {
    /// Creates a production game fix engine.
    #[must_use]
    pub fn new() -> Self {
        Self {
            allow_synthetic_definitions: false,
            overlay_reader: Arc::new(|sha| GameFixContentStore::read(&GameFixContentStore::default_directory(), sha)),
        }
    }

    /// Creates an engine with synthetic definitions enabled (for unit testing).
    #[must_use]
    pub fn with_synthetic(allow_synthetic_definitions: bool) -> Self {
        Self {
            allow_synthetic_definitions,
            overlay_reader: Arc::new(|sha| GameFixContentStore::read(&GameFixContentStore::default_directory(), sha)),
        }
    }

    /// Returns the current state of a fix in a game directory.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] or [`Error::Refused`] on manifest failure.
    pub fn get_status(&self, definition: &GameFixDefinition, game_dir: &Path) -> Result<GameFixState> {
        validate_definition(definition)?;
        let manifest_path = get_manifest_path(game_dir, &definition.id);
        if !manifest_path.is_file() {
            return Ok(GameFixState::NotInstalled);
        }

        let manifest = read_manifest(&manifest_path, &definition.id)?;
        if manifest.game != definition.game {
            return Err(Error::damaged("Manifest game target mismatch"));
        }
        if !manifest.installed {
            return Ok(GameFixState::Removed);
        }

        let all_match = manifest.files.iter().all(|file| {
            resolve_game_path(game_dir, &file.relative_path)
                .ok()
                .is_some_and(|p| matches_file_hash(&p, &file.after_sha256))
        });

        Ok(if all_match {
            GameFixState::Installed
        } else {
            GameFixState::Modified
        })
    }

    /// Lists all installed fixes in a game directory.
    ///
    /// # Errors
    /// Returns [`Error::System`] or [`Error::Damaged`] if strict check fails.
    pub fn list_installed(
        &self,
        game_dir: &Path,
        issues: Option<&mut Vec<String>>,
    ) -> Result<Vec<GameFixInstalledInfo>> {
        let active = read_active_manifests(game_dir, issues)?;
        let mut result = Vec::with_capacity(active.len());

        for manifest in active {
            let all_match = manifest.files.iter().all(|file| {
                resolve_game_path(game_dir, &file.relative_path)
                    .ok()
                    .is_some_and(|p| matches_file_hash(&p, &file.after_sha256))
            });

            let state = if all_match {
                GameFixState::Installed
            } else {
                GameFixState::Modified
            };

            result.push(GameFixInstalledInfo {
                id: manifest.fix_id,
                game: manifest.game,
                version: manifest.version,
                title: manifest.title,
                category: manifest.category,
                maturity: manifest.maturity,
                state,
                files: manifest.files.into_iter().map(|f| f.relative_path).collect(),
            });
        }

        Ok(result)
    }

    /// Loads the installed manifest for a fix.
    ///
    /// # Errors
    /// Returns an error if the manifest cannot be read or is invalid.
    pub fn get_manifest(&self, fix_id: &str, game_dir: &Path) -> Result<GameFixManifest> {
        let manifest_path = get_manifest_path(game_dir, fix_id);
        read_manifest(&manifest_path, fix_id)
    }

    /// Installs a game fix.
    ///
    /// # Errors
    /// Returns [`Error::Refused`], [`Error::Damaged`], or [`Error::System`] on failure.
    pub fn install(&self, definition: &GameFixDefinition, game_dir: &Path) -> Result<GameFixInstallResult> {
        self.install_internal(definition, game_dir, false)
    }

    fn install_internal(
        &self,
        definition: &GameFixDefinition,
        game_dir: &Path,
        allow_version_transition: bool,
    ) -> Result<GameFixInstallResult> {
        validate_definition(definition)?;
        let fix_dir = get_fix_directory(game_dir, &definition.id);
        check_no_links(game_dir, &fix_dir)?;
        let _ = self.recover_interrupted(game_dir)?;

        let (is_installation, steam_build_id) = identify_game(definition.game, game_dir);
        if !is_installation {
            return Err(Error::Refused(
                "The selected directory does not pass the structural game check.".to_string(),
            ));
        }
        let build_id = steam_build_id.as_deref();
        if !definition.supports_detected_build_or_hashes(build_id) {
            let message = if build_id.is_some() {
                "This fix does not list the detected Steam build as supported."
            } else {
                "Steam build ID unavailable and this fix has no exact source-file hash anchors."
            };
            return Err(Error::Refused(message.to_string()));
        }

        if !self.allow_synthetic_definitions
            && matches!(
                definition.verification_state,
                GameFixVerificationState::Research | GameFixVerificationState::SyntheticTests
            )
        {
            return Err(Error::Refused(
                "Game Fix installation requires validation against supported retail files.".to_string(),
            ));
        }

        let manifest_path = get_manifest_path(game_dir, &definition.id);
        let mut prior_manifest: Option<GameFixManifest> = None;
        let mut prior_manifest_bytes: Option<Vec<u8>> = None;

        if manifest_path.is_file() {
            let bytes = fs::read(&manifest_path).map_err(|e| Error::System(format!("Failed to read manifest: {e}")))?;
            let manifest = read_manifest(&manifest_path, &definition.id)?;
            if manifest.game != definition.game {
                return Err(Error::damaged("Manifest game target mismatch"));
            }

            if manifest.installed {
                if manifest.version != definition.version {
                    return Err(Error::Refused(
                        "A different version of this fix is installed; remove it before updating.".to_string(),
                    ));
                }
                let status = self.get_status(definition, game_dir)?;
                if status == GameFixState::Installed {
                    return Ok(GameFixInstallResult {
                        changed: false,
                        state: status,
                        files: manifest.files.into_iter().map(|f| f.relative_path).collect(),
                    });
                }
                return Err(Error::Refused(
                    "A managed game file changed after the fix was installed; refusing to overwrite it.".to_string(),
                ));
            }

            if manifest.version != definition.version && !allow_version_transition {
                return Err(Error::Refused(
                    "Reinstalling a removed fix requires the same fix version so its original backup remains authoritative."
                        .to_string(),
                ));
            }

            prior_manifest = Some(manifest);
            prior_manifest_bytes = Some(bytes);
        } else if fix_dir.is_dir() {
            return Err(Error::Refused(
                "Fix state exists without a valid manifest; refusing to reuse it.".to_string(),
            ));
        }

        let active = read_active_manifests(game_dir, None)?;
        let active_ids: HashSet<String> = active.iter().map(|m| m.fix_id.clone()).collect();

        for dep in &definition.depends_on {
            if !active_ids.contains(dep) {
                return Err(Error::Refused(format!("Required fix is not installed: {dep}")));
            }
        }
        for conflict in &definition.conflicts_with {
            if active_ids.contains(conflict) {
                return Err(Error::Refused(format!(
                    "This fix conflicts with installed fix: {conflict}"
                )));
            }
        }

        let mut managed_paths = HashSet::new();
        for m in &active {
            for f in &m.files {
                managed_paths.insert(f.relative_path.to_ascii_lowercase());
            }
        }

        let check_unmanaged = |rel: &str| -> Result<String> {
            let norm = normalize_relative_path(rel)?;
            if managed_paths.contains(&norm.to_ascii_lowercase()) {
                return Err(Error::Refused(format!(
                    "Another active Game Fix manages {norm}; layered transformations not supported"
                )));
            }
            Ok(norm)
        };

        for patch in &definition.text_patches {
            check_unmanaged(&patch.relative_path)?;
        }
        for overlay in &definition.overlays {
            check_unmanaged(&overlay.relative_path)?;
        }
        for edit in &definition.spawn_edits {
            check_unmanaged(&edit.relative_path)?;
        }

        if let Some(ref prior) = prior_manifest {
            let prior_set: HashSet<String> = prior
                .files
                .iter()
                .map(|f| f.relative_path.to_ascii_lowercase())
                .collect();
            let mut req_set = HashSet::new();
            for p in &definition.text_patches {
                req_set.insert(normalize_relative_path(&p.relative_path)?.to_ascii_lowercase());
            }
            for o in &definition.overlays {
                req_set.insert(normalize_relative_path(&o.relative_path)?.to_ascii_lowercase());
            }
            for e in &definition.spawn_edits {
                req_set.insert(normalize_relative_path(&e.relative_path)?.to_ascii_lowercase());
            }
            if prior_set != req_set {
                return Err(Error::Refused(
                    "Fix version transition must keep same managed file set".to_string(),
                ));
            }
        }

        ensure_no_companion_overlap(game_dir, definition)?;

        let mut changes = self.prepare_text_changes(definition, game_dir)?;
        for overlay in &definition.overlays {
            changes.push(self.prepare_overlay_change(definition.game, game_dir, overlay)?);
        }
        if !definition.spawn_edits.is_empty() {
            let first_edit = definition
                .spawn_edits
                .first()
                .ok_or_else(|| Error::damaged("Spawn edits unexpectedly empty"))?;
            let spawn_path = normalize_relative_path(&first_edit.relative_path)?;
            changes.push(self.prepare_spawn_change(definition.game, game_dir, &spawn_path, &definition.spawn_edits)?);
        }

        let backup_dir = fix_dir.join("backups");
        fs::create_dir_all(&backup_dir).map_err(|e| Error::System(format!("Failed to create backup dir: {e}")))?;

        let mut planned_files = Vec::with_capacity(changes.len());
        for (i, change) in changes.iter().enumerate() {
            let prior_file = prior_manifest
                .as_ref()
                .and_then(|pm| pm.files.iter().find(|f| f.relative_path == change.relative_path));
            let backup_path = prior_file
                .map(|f| f.backup_path.clone())
                .unwrap_or_else(|| format!("backups/file-{i:04}.before"));

            planned_files.push(ManagedGameFile {
                relative_path: change.relative_path.clone(),
                before_sha256: change.before_sha256.clone(),
                after_sha256: change.after_sha256.clone(),
                backup_path,
                target_existed_before: change.target_existed_before,
            });
        }

        // Write transaction journal
        let journal = crate::models::GameFixJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            kind: JOURNAL_INSTALL.to_string(),
            fresh_state: prior_manifest.is_none(),
            files: planned_files.clone(),
        };
        write_journal(&fix_dir, &journal)?;

        let mut created_backups = Vec::new();
        let mut applied: Vec<PreparedFileChange> = Vec::new();

        let execute_transaction = || -> Result<GameFixInstallResult> {
            for (change, planned) in changes.iter().zip(&planned_files) {
                let backup_full = resolve_state_path(&fix_dir, &planned.backup_path)?;
                if backup_full.is_file() {
                    let existing_backup =
                        fs::read(&backup_full).map_err(|e| Error::System(format!("Failed to read backup: {e}")))?;
                    if sha256_hex(&existing_backup) != change.before_sha256 {
                        return Err(Error::damaged(format!(
                            "Stored recovery file hash mismatch for {}",
                            change.relative_path
                        )));
                    }
                } else if prior_manifest.is_some() {
                    return Err(Error::damaged(format!(
                        "Stored recovery file is missing: {}",
                        change.relative_path
                    )));
                } else {
                    AtomicFileWriter::write(&backup_full, &change.before_bytes, false)?;
                    created_backups.push(backup_full);
                }
            }

            for change in &changes {
                if !matches_preflight_source(game_dir, definition.game, change) {
                    return Err(Error::Refused(format!(
                        "Managed game file changed after preflight: {}",
                        change.relative_path
                    )));
                }
                AtomicFileWriter::write(&change.absolute_path, &change.after_bytes, true)?;
                applied.push(change.clone());
            }

            let manifest = GameFixManifest {
                schema_version: MANIFEST_SCHEMA_VERSION,
                fix_id: definition.id.clone(),
                game: definition.game,
                steam_build_id: build_id.unwrap_or_default().to_owned(),
                version: definition.version.clone(),
                title: definition.title.clone(),
                problem: definition.problem.clone(),
                description: definition.description.clone(),
                implementation: definition.implementation,
                requires_new_game: definition.requires_new_game,
                save_compatibility: definition.save_compatibility,
                verification_state: definition.verification_state,
                detection_method: definition.detection_method.clone(),
                references: definition.references.clone(),
                category: definition.category,
                maturity: definition.maturity,
                depends_on: definition.depends_on.clone(),
                conflicts_with: definition.conflicts_with.clone(),
                source: definition.source.clone(),
                installed: true,
                files: planned_files.clone(),
            };

            let manifest_bytes = serialize_manifest(&manifest);
            AtomicFileWriter::write(&manifest_path, &manifest_bytes, true)?;
            delete_journal(&fix_dir);

            Ok(GameFixInstallResult {
                changed: true,
                state: GameFixState::Installed,
                files: planned_files.into_iter().map(|f| f.relative_path).collect(),
            })
        };

        match execute_transaction() {
            Ok(result) => Ok(result),
            Err(err) => {
                let rollback_errors = rollback_installation(&applied);
                if !rollback_errors.is_empty() {
                    write_recovery_note(&fix_dir, &definition.id, &changes, &applied, &rollback_errors);
                    return Err(Error::System(format!(
                        "Installation failed and rollback incomplete: {}",
                        rollback_errors.join("; ")
                    )));
                }

                if let Some(ref prior_bytes) = prior_manifest_bytes {
                    let _ = AtomicFileWriter::write(&manifest_path, prior_bytes, true);
                } else if manifest_path.is_file() {
                    let _ = fs::remove_file(&manifest_path);
                }

                for backup in created_backups {
                    let _ = fs::remove_file(backup);
                }
                delete_journal(&fix_dir);

                if prior_manifest_bytes.is_none() {
                    delete_empty_state_tree(&fix_dir);
                }

                Err(err)
            }
        }
    }

    /// Updates an installed fix to a newer version as one guarded transaction.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] or [`Error::System`] on failure.
    pub fn update(&self, definition: &GameFixDefinition, game_dir: &Path) -> Result<GameFixInstallResult> {
        validate_definition(definition)?;
        let manifest_path = get_manifest_path(game_dir, &definition.id);
        check_no_links(game_dir, &manifest_path)?;
        let _ = self.recover_interrupted(game_dir)?;

        if !manifest_path.is_file() {
            return self.install(definition, game_dir);
        }

        let old_manifest_bytes =
            fs::read(&manifest_path).map_err(|e| Error::System(format!("Failed to read manifest: {e}")))?;
        let old_manifest = read_manifest(&manifest_path, &definition.id)?;
        if old_manifest.game != definition.game {
            return Err(Error::damaged("Manifest game target mismatch"));
        }

        if !old_manifest.installed || old_manifest.version == definition.version {
            return self.install(definition, game_dir);
        }

        let old_v = parse_numeric_version(&old_manifest.version)?;
        let new_v = parse_numeric_version(&definition.version)?;
        if new_v <= old_v {
            return Err(Error::Refused(
                "Fix updates must use an increasing numeric version".to_string(),
            ));
        }

        let (is_install, build_id) = identify_game(definition.game, game_dir);
        if !is_install {
            return Err(Error::Refused("Invalid game installation".to_string()));
        }
        let build_id = build_id.as_deref();
        if !definition.supports_detected_build_or_hashes(build_id) {
            let message = if build_id.is_some() {
                "Build ID not supported"
            } else {
                "Steam build ID unavailable and this fix has no exact source-file hash anchors"
            };
            return Err(Error::Refused(message.to_string()));
        }

        let old_paths: HashSet<String> = old_manifest
            .files
            .iter()
            .map(|f| f.relative_path.to_ascii_lowercase())
            .collect();
        let new_paths: HashSet<String> = Self::managed_paths(definition)
            .into_iter()
            .map(|p| p.to_ascii_lowercase())
            .collect();
        if old_paths != new_paths {
            return Err(Error::Refused(
                "Updates that change the managed file set are not supported".to_string(),
            ));
        }

        let mut old_files = Vec::with_capacity(old_manifest.files.len());
        for file in &old_manifest.files {
            let path = resolve_game_path(game_dir, &file.relative_path)?;
            if !path.is_file() || !matches_file_hash(&path, &file.after_sha256) {
                return Err(Error::Refused(
                    "A managed game file changed after installation; refusing to update".to_string(),
                ));
            }
            let bytes =
                fs::read(&path).map_err(|error| Error::System(format!("Failed to snapshot installed fix: {error}")))?;
            old_files.push((file.relative_path.clone(), path, bytes));
        }

        let update_result = (|| -> Result<GameFixInstallResult> {
            self.uninstall(&definition.id, game_dir)?;
            self.install_internal(definition, game_dir, true)
        })();

        if let Err(error) = update_result {
            let fix_dir = get_fix_directory(game_dir, &definition.id);
            let mut rollback_errors = Vec::new();
            for (relative_path, path, bytes) in &old_files {
                if let Err(restore_error) = AtomicFileWriter::write(path, bytes, true) {
                    rollback_errors.push(format!("{relative_path}: {restore_error}"));
                }
            }
            if let Err(restore_error) = AtomicFileWriter::write(&manifest_path, &old_manifest_bytes, true) {
                rollback_errors.push(format!("manifest: {restore_error}"));
            }
            if rollback_errors.is_empty() {
                cleanup_update_journal_after_rollback(&fix_dir, &rollback_errors);
                return Err(error);
            }
            cleanup_update_journal_after_rollback(&fix_dir, &rollback_errors);
            return Err(Error::System(format!(
                "Game Fix update failed and rollback to the previous version was incomplete: {}",
                rollback_errors.join("; ")
            )));
        }

        update_result
    }

    /// Uninstalls an installed fix, restoring the original game files.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] or [`Error::System`] on failure.
    pub fn uninstall(&self, fix_id: &str, game_dir: &Path) -> Result<GameFixInstallResult> {
        let manifest_path = get_manifest_path(game_dir, fix_id);
        check_no_links(game_dir, &manifest_path)?;
        let _ = self.recover_interrupted(game_dir)?;

        if !manifest_path.is_file() {
            return Ok(GameFixInstallResult {
                changed: false,
                state: GameFixState::NotInstalled,
                files: Vec::new(),
            });
        }

        let manifest = read_manifest(&manifest_path, fix_id)?;
        if !manifest.installed {
            return Ok(GameFixInstallResult {
                changed: false,
                state: GameFixState::Removed,
                files: manifest.files.into_iter().map(|f| f.relative_path).collect(),
            });
        }

        let active = read_active_manifests(game_dir, None)?;
        let dependents: Vec<_> = active
            .iter()
            .filter(|m| m.fix_id != fix_id && m.depends_on.iter().any(|d| d == fix_id))
            .map(|m| m.fix_id.clone())
            .collect();
        if !dependents.is_empty() {
            return Err(Error::Refused(format!(
                "Remove dependent fixes first: {}",
                dependents.join(", ")
            )));
        }

        let fix_dir = get_fix_directory(game_dir, fix_id);
        let mut changes = Vec::with_capacity(manifest.files.len());

        for file in &manifest.files {
            let path = resolve_game_path(game_dir, &file.relative_path)?;
            let current = fs::read(&path).map_err(|e| Error::System(format!("Failed to read game file: {e}")))?;
            if sha256_hex(&current) != file.after_sha256 {
                return Err(Error::Refused(format!(
                    "A managed game file changed after installation: {}",
                    file.relative_path
                )));
            }
            let backup_path = resolve_state_path(&fix_dir, &file.backup_path)?;
            let before =
                fs::read(&backup_path).map_err(|e| Error::System(format!("Failed to read backup file: {e}")))?;
            if sha256_hex(&before) != file.before_sha256 {
                return Err(Error::damaged(format!(
                    "A recovery file failed hash check: {}",
                    file.relative_path
                )));
            }

            changes.push(PreparedFileChange {
                relative_path: file.relative_path.clone(),
                absolute_path: path,
                before_bytes: current,
                after_bytes: before,
                before_sha256: file.after_sha256.clone(),
                after_sha256: file.before_sha256.clone(),
                target_existed_before: file.target_existed_before,
                source_fingerprint: None,
            });
        }

        let journal = crate::models::GameFixJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            kind: JOURNAL_UNINSTALL.to_string(),
            fresh_state: false,
            files: manifest.files.clone(),
        };
        write_journal(&fix_dir, &journal)?;

        let mut applied: Vec<PreparedFileChange> = Vec::new();
        let uninstall_result = (|| -> Result<GameFixInstallResult> {
            for change in &changes {
                check_no_links(game_dir, &change.absolute_path)?;
                if !matches_file_hash(&change.absolute_path, &change.before_sha256) {
                    return Err(Error::Refused(format!(
                        "A managed game file changed after removal preflight: {}",
                        change.relative_path
                    )));
                }
                if change.target_existed_before {
                    AtomicFileWriter::write(&change.absolute_path, &change.after_bytes, true)?;
                } else {
                    fs::remove_file(&change.absolute_path)
                        .map_err(|error| Error::System(format!("Failed to remove managed game file: {error}")))?;
                }
                applied.push(change.clone());
            }

            let mut removed_manifest = manifest.clone();
            removed_manifest.installed = false;
            let removed_bytes = serialize_manifest(&removed_manifest);
            AtomicFileWriter::write(&manifest_path, &removed_bytes, true)?;
            delete_journal(&fix_dir);

            Ok(GameFixInstallResult {
                changed: true,
                state: GameFixState::Removed,
                files: manifest.files.into_iter().map(|f| f.relative_path).collect(),
            })
        })();

        match uninstall_result {
            Ok(result) => Ok(result),
            Err(error) => {
                let rollback_errors = rollback_uninstallation(game_dir, &applied);
                if rollback_errors.is_empty() {
                    match delete_journal_checked(game_dir, &fix_dir) {
                        Ok(()) => Err(error),
                        Err(journal_error) => Err(Error::System(format!(
                            "Game Fix uninstall failed; files were restored, but the recovery journal could not be cleared: {error}; {journal_error}"
                        ))),
                    }
                } else {
                    Err(Error::System(format!(
                        "Game Fix uninstall failed and rollback to the installed state was incomplete: {error}; {}",
                        rollback_errors.join("; ")
                    )))
                }
            }
        }
    }

    /// Verifies if a fix can be uninstalled safely without modifying files.
    #[must_use]
    pub fn check_uninstall(&self, fix_id: &str, game_dir: &Path) -> GameFixUninstallCheck {
        let manifest_path = get_manifest_path(game_dir, fix_id);
        if !manifest_path.is_file() {
            return GameFixUninstallCheck {
                can_uninstall: false,
                reason: Some("The provider manifest is missing.".to_string()),
                files: Vec::new(),
            };
        }

        let manifest = match read_manifest(&manifest_path, fix_id) {
            Ok(m) => m,
            Err(e) => {
                return GameFixUninstallCheck {
                    can_uninstall: false,
                    reason: Some(format!("Invalid manifest: {e:?}")),
                    files: Vec::new(),
                };
            }
        };

        if !manifest.installed {
            return GameFixUninstallCheck {
                can_uninstall: false,
                reason: Some("The manifest does not record an active fix.".to_string()),
                files: Vec::new(),
            };
        }

        let active = match read_active_manifests(game_dir, None) {
            Ok(a) => a,
            Err(e) => {
                return GameFixUninstallCheck {
                    can_uninstall: false,
                    reason: Some(format!("Cannot read active manifests: {e:?}")),
                    files: Vec::new(),
                };
            }
        };

        let dependents: Vec<_> = active
            .iter()
            .filter(|m| m.fix_id != fix_id && m.depends_on.iter().any(|d| d == fix_id))
            .map(|m| m.fix_id.clone())
            .collect();
        if !dependents.is_empty() {
            return GameFixUninstallCheck {
                can_uninstall: false,
                reason: Some(format!("Remove dependent fixes first: {}", dependents.join(", "))),
                files: Vec::new(),
            };
        }

        let fix_dir = get_fix_directory(game_dir, fix_id);
        for file in &manifest.files {
            let path = match resolve_game_path(game_dir, &file.relative_path) {
                Ok(p) => p,
                Err(e) => {
                    return GameFixUninstallCheck {
                        can_uninstall: false,
                        reason: Some(format!("{}: {e:?}", file.relative_path)),
                        files: Vec::new(),
                    };
                }
            };
            if !path.is_file() || !matches_file_hash(&path, &file.after_sha256) {
                return GameFixUninstallCheck {
                    can_uninstall: false,
                    reason: Some(format!(
                        "A managed game file changed after installation: {}",
                        file.relative_path
                    )),
                    files: Vec::new(),
                };
            }
            let backup_path = match resolve_state_path(&fix_dir, &file.backup_path) {
                Ok(p) => p,
                Err(e) => {
                    return GameFixUninstallCheck {
                        can_uninstall: false,
                        reason: Some(format!("{}: {e:?}", file.relative_path)),
                        files: Vec::new(),
                    };
                }
            };
            if !backup_path.is_file() || !matches_file_hash(&backup_path, &file.before_sha256) {
                return GameFixUninstallCheck {
                    can_uninstall: false,
                    reason: Some(format!("A recovery file failed its hash check: {}", file.relative_path)),
                    files: Vec::new(),
                };
            }
        }

        GameFixUninstallCheck {
            can_uninstall: true,
            reason: None,
            files: manifest.files.into_iter().map(|f| f.relative_path).collect(),
        }
    }

    /// Applies a preset to a game directory.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] or [`Error::System`] on failure.
    pub fn apply_preset(
        &self,
        game: GameTarget,
        preset: GameFixPreset,
        game_dir: &Path,
    ) -> Result<GameFixPresetResult> {
        let fixes = GameFixCatalog::for_preset(game, preset);
        self.apply_fixes(game, preset, &fixes, game_dir)
    }

    /// Applies a specific list of fixes as a transactional preset.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] or [`Error::System`] on failure.
    pub fn apply_fixes(
        &self,
        game: GameTarget,
        preset: GameFixPreset,
        fixes: &[&GameFixDefinition],
        game_dir: &Path,
    ) -> Result<GameFixPresetResult> {
        if preset == GameFixPreset::Custom {
            return Err(Error::Refused(
                "Custom selections must be installed explicitly".to_string(),
            ));
        }

        for f in fixes {
            if f.game != game || !GameFixCatalog::is_included_in_preset(f, preset) {
                return Err(Error::Refused(
                    "A preset can contain only safe fixes for the selected game".to_string(),
                ));
            }
            if !self.allow_synthetic_definitions
                && matches!(
                    f.verification_state,
                    GameFixVerificationState::Research | GameFixVerificationState::SyntheticTests
                )
            {
                return Err(Error::Refused(
                    "Preset application requires fixes validated against retail files".to_string(),
                ));
            }
        }

        let (is_install, build_id) = identify_game(game, game_dir);
        if !is_install {
            return Err(Error::Refused(
                "The selected directory does not pass the structural game check".to_string(),
            ));
        }
        if !fixes.is_empty()
            && fixes
                .iter()
                .any(|fix| !fix.supports_detected_build_or_hashes(build_id.as_deref()))
        {
            let message = if build_id.is_some() {
                "The detected Steam build is not supported by every selected Game Fix"
            } else {
                "Steam build ID unavailable and one or more selected fixes lack exact source-file hash anchors"
            };
            return Err(Error::Refused(message.to_string()));
        }

        let mut pending = Vec::new();
        let mut already_installed = Vec::new();

        for fix in fixes {
            let state = self.get_status(fix, game_dir)?;
            match state {
                GameFixState::Installed => already_installed.push(fix.id.clone()),
                GameFixState::Modified => {
                    return Err(Error::Refused(format!(
                        "A preset fix has an externally modified file: {}",
                        fix.id
                    )))
                }
                _ => pending.push(*fix),
            }
        }

        let mut newly_installed = Vec::new();
        for fix in pending {
            match self.install(fix, game_dir) {
                Ok(res) => {
                    if res.changed {
                        newly_installed.push(fix);
                    } else {
                        already_installed.push(fix.id.clone());
                    }
                }
                Err(err) => {
                    // Rollback all newly installed fixes in reverse order
                    for rollback_fix in newly_installed.iter().rev() {
                        let _ = self.uninstall(&rollback_fix.id, game_dir);
                    }
                    return Err(err);
                }
            }
        }

        Ok(GameFixPresetResult {
            preset,
            selected_fix_count: fixes.len(),
            installed_fix_ids: newly_installed.into_iter().map(|f| f.id.clone()).collect(),
            already_installed_fix_ids: already_installed,
        })
    }

    /// Recovers all interrupted transactions in the game directory.
    ///
    /// # Errors
    /// Returns [`Error::System`] or [`Error::Damaged`] on corrupted journal.
    pub fn recover_interrupted(&self, game_dir: &Path) -> Result<Vec<String>> {
        let state_dir = game_dir.join(STATE_DIRECTORY_NAME);
        if !state_dir.is_dir() {
            return Ok(Vec::new());
        }
        check_no_links(game_dir, &state_dir)?;

        let mut recovered = Vec::new();
        let entries = fs::read_dir(&state_dir)
            .map_err(|error| Error::System(format!("Failed to read Game Fix state directory: {error}")))?;

        for entry in entries {
            let entry =
                entry.map_err(|error| Error::System(format!("Failed to read Game Fix state entry: {error}")))?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(id) = path.file_name().and_then(|n| n.to_str()) {
                    if is_valid_id(id) {
                        check_no_links(game_dir, &path)?;
                        if finish_interrupted_transaction(game_dir, id)? {
                            recovered.push(id.to_string());
                        }
                    }
                }
            }
        }

        recovered.sort();
        Ok(recovered)
    }

    /// Returns every normalized path managed by a definition.
    #[must_use]
    pub fn managed_paths(definition: &GameFixDefinition) -> Vec<String> {
        let mut set = HashSet::new();
        let mut list = Vec::new();

        for p in &definition.text_patches {
            if let Ok(norm) = normalize_relative_path(&p.relative_path) {
                if set.insert(norm.to_ascii_lowercase()) {
                    list.push(norm);
                }
            }
        }
        for o in &definition.overlays {
            if let Ok(norm) = normalize_relative_path(&o.relative_path) {
                if set.insert(norm.to_ascii_lowercase()) {
                    list.push(norm);
                }
            }
        }
        for e in &definition.spawn_edits {
            if let Ok(norm) = normalize_relative_path(&e.relative_path) {
                if set.insert(norm.to_ascii_lowercase()) {
                    list.push(norm);
                }
            }
        }

        list
    }

    /// Returns active paths managed across all installed fixes.
    ///
    /// # Errors
    /// Returns [`Error::System`] on failure.
    pub fn get_active_managed_paths(game_dir: &Path) -> Result<HashSet<String>> {
        let active = read_active_manifests(game_dir, None)?;
        let mut set = HashSet::new();
        for m in active {
            for f in m.files {
                if let Ok(norm) = normalize_relative_path(&f.relative_path) {
                    set.insert(norm.to_ascii_lowercase());
                }
            }
        }
        Ok(set)
    }

    fn prepare_text_changes(&self, definition: &GameFixDefinition, game_dir: &Path) -> Result<Vec<PreparedFileChange>> {
        let mut grouped: HashMap<String, TextPatchTarget> = HashMap::new();

        for patch in &definition.text_patches {
            let rel = normalize_relative_path(&patch.relative_path)?;
            let abs = resolve_game_path(game_dir, &rel)?;

            if !grouped.contains_key(&rel) {
                let (bytes, existed, fp) = self.read_target_source(definition.game, game_dir, &rel, &abs)?;
                let text = decode_patch_text(&bytes, patch.code_page)?;
                grouped.insert(
                    rel.clone(),
                    TextPatchTarget {
                        abs_path: abs,
                        bytes,
                        text,
                        code_page: patch.code_page,
                        existed,
                        fingerprint: fp,
                    },
                );
            }

            let entry = grouped
                .get_mut(&rel)
                .ok_or_else(|| Error::damaged("Missing grouped entry"))?;
            if entry.code_page != patch.code_page {
                return Err(Error::Refused(
                    "All text patches for one file must use the same code page".to_string(),
                ));
            }

            if entry.existed && !self.allow_synthetic_definitions && patch.expected_file_sha256.is_none() {
                return Err(Error::damaged(format!(
                    "An existing loose game-data file has no verified source hash: {rel}"
                )));
            }

            if let Some(ref exp_sha) = patch.expected_file_sha256 {
                if sha256_hex(&entry.bytes) != *exp_sha {
                    return Err(Error::damaged(format!(
                        "The source file hash does not match verified build for {rel}"
                    )));
                }
            }

            let count = entry.text.matches(&patch.expected_text).count();
            if count != 1 {
                return Err(Error::damaged(format!(
                    "Expected exactly one text anchor in {rel}; found {count}"
                )));
            }

            entry.text = entry.text.replacen(&patch.expected_text, &patch.replacement_text, 1);
        }

        let mut changes = Vec::with_capacity(grouped.len());
        for (rel, target) in grouped {
            let after_bytes = encode_patch_text(&target.text, target.code_page)?;
            if after_bytes == target.bytes {
                return Err(Error::damaged(format!("Text patch produced no change: {rel}")));
            }

            changes.push(PreparedFileChange {
                relative_path: rel,
                absolute_path: target.abs_path,
                before_bytes: target.bytes.clone(),
                after_bytes: after_bytes.clone(),
                before_sha256: sha256_hex(&target.bytes),
                after_sha256: sha256_hex(&after_bytes),
                target_existed_before: target.existed,
                source_fingerprint: target.fingerprint,
            });
        }

        Ok(changes)
    }

    fn prepare_overlay_change(
        &self,
        game: GameTarget,
        game_dir: &Path,
        overlay: &FileOverlayOperation,
    ) -> Result<PreparedFileChange> {
        let rel = normalize_relative_path(&overlay.relative_path)?;
        let abs = resolve_game_path(game_dir, &rel)?;

        let content = (self.overlay_reader)(&overlay.content_sha256).ok_or_else(|| {
            Error::damaged(format!(
                "Overlay content missing from store: {}",
                overlay.content_sha256
            ))
        })?;
        if sha256_hex(&content) != overlay.content_sha256 {
            return Err(Error::damaged(format!(
                "Overlay content corrupted: {}",
                overlay.content_sha256
            )));
        }

        let (before, existed, fp) = if let Some(ref exp_sha) = overlay.expected_file_sha256 {
            let (b, ex, f) = self.read_target_source(game, game_dir, &rel, &abs)?;
            if sha256_hex(&b) != *exp_sha {
                return Err(Error::damaged(format!(
                    "Source file hash does not match overlay expectation for {rel}"
                )));
            }
            (b, ex, f)
        } else {
            if abs.is_file() || self.source_exists_in_archives(game, game_dir, &rel, &abs) {
                return Err(Error::Refused(format!(
                    "Overlay adds a file that already exists in the game: {rel}"
                )));
            }
            (Vec::new(), false, Some(ABSENT_SOURCE_FINGERPRINT.to_string()))
        };

        if content == before {
            return Err(Error::damaged(format!(
                "Fix pack file is identical to game file: {rel}"
            )));
        }

        Ok(PreparedFileChange {
            relative_path: rel,
            absolute_path: abs,
            before_bytes: before.clone(),
            after_bytes: content.clone(),
            before_sha256: sha256_hex(&before),
            after_sha256: sha256_hex(&content),
            target_existed_before: existed,
            source_fingerprint: fp,
        })
    }

    fn prepare_spawn_change(
        &self,
        game: GameTarget,
        game_dir: &Path,
        relative_path: &str,
        edits: &[SpawnEditOperation],
    ) -> Result<PreparedFileChange> {
        let abs = resolve_game_path(game_dir, relative_path)?;
        let (before, existed, fp) = self.read_target_source(game, game_dir, relative_path, &abs)?;

        for edit in edits {
            if let Some(ref exp) = edit.expected_file_sha256 {
                if sha256_hex(&before) != *exp {
                    return Err(Error::damaged(format!(
                        "Source file hash does not match for {relative_path}"
                    )));
                }
            } else if !self.allow_synthetic_definitions {
                return Err(Error::damaged(format!(
                    "All.spawn edit has no verified source hash: {relative_path}"
                )));
            }
        }

        let after = AllSpawnEditor::apply(&before, edits)?;

        Ok(PreparedFileChange {
            relative_path: relative_path.to_string(),
            absolute_path: abs,
            before_bytes: before.clone(),
            after_bytes: after.clone(),
            before_sha256: sha256_hex(&before),
            after_sha256: sha256_hex(&after),
            target_existed_before: existed,
            source_fingerprint: fp,
        })
    }

    fn read_target_source(
        &self,
        game: GameTarget,
        game_dir: &Path,
        relative_path: &str,
        absolute_path: &Path,
    ) -> Result<(Vec<u8>, bool, Option<String>)> {
        if absolute_path.is_file() {
            let bytes =
                fs::read(absolute_path).map_err(|e| Error::System(format!("Failed to read target source: {e}")))?;
            return Ok((bytes, true, None));
        }

        let content_rel = if relative_path.to_ascii_lowercase().starts_with("gamedata/") {
            &relative_path[9..]
        } else {
            relative_path
        };

        let (companion_game, fsgame) = match game {
            GameTarget::ShadowOfChernobyl => (CompanionGame::ShadowOfChernobyl, "fsgame.ltx"),
            GameTarget::ClearSky => (CompanionGame::ClearSky, "fsgame.ltx"),
            GameTarget::CallOfPripyat => (CompanionGame::CallOfPripyat, "fsgame.ltx"),
            GameTarget::ShadowOfChernobylEnhancedEdition => (CompanionGame::ShadowOfChernobyl, "fsgame_soc.ltx"),
            GameTarget::ClearSkyEnhancedEdition => (CompanionGame::ClearSky, "fsgame_cs.ltx"),
            GameTarget::CallOfPripyatEnhancedEdition => (CompanionGame::CallOfPripyat, "fsgame_cop.ltx"),
            GameTarget::Stalker2 => return Err(Error::Refused("S2 has no X-Ray archives".to_string())),
        };

        let tree = GameFileTree::load(
            companion_game,
            game_dir,
            |p| p.eq_ignore_ascii_case(content_rel),
            Some(&[fsgame]),
            true,
            false,
            false,
            None,
            None,
        )?;

        let file = tree
            .files
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(content_rel))
            .map(|(_, f)| f)
            .ok_or_else(|| {
                Error::damaged(format!(
                    "Target file absent from archives and loose data: {relative_path}"
                ))
            })?;

        let bytes = file.read()?;
        Ok((bytes, false, Some(tree.fingerprint)))
    }

    fn source_exists_in_archives(
        &self,
        game: GameTarget,
        game_dir: &Path,
        relative_path: &str,
        absolute_path: &Path,
    ) -> bool {
        self.read_target_source(game, game_dir, relative_path, absolute_path)
            .is_ok()
    }
}

#[derive(Debug, Clone)]
struct PreparedFileChange {
    relative_path: String,
    absolute_path: PathBuf,
    before_bytes: Vec<u8>,
    after_bytes: Vec<u8>,
    before_sha256: String,
    after_sha256: String,
    target_existed_before: bool,
    source_fingerprint: Option<String>,
}

fn get_fix_directory(root: &Path, id: &str) -> PathBuf {
    root.join(STATE_DIRECTORY_NAME).join(id)
}

fn get_manifest_path(root: &Path, id: &str) -> PathBuf {
    get_fix_directory(root, id).join(MANIFEST_FILE_NAME)
}

fn matches_file_hash(path: &Path, expected_sha: &str) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    sha256_hex(&bytes).eq_ignore_ascii_case(expected_sha)
}

fn matches_preflight_source(root: &Path, game: GameTarget, change: &PreparedFileChange) -> bool {
    if change.target_existed_before {
        return matches_file_hash(&change.absolute_path, &change.before_sha256);
    }

    if change.absolute_path.is_file() {
        return false;
    }

    if change.source_fingerprint.as_deref() == Some(ABSENT_SOURCE_FINGERPRINT) {
        return true;
    }

    let engine = GameFixEngine::new();
    if let Ok((bytes, existed, fp)) =
        engine.read_target_source(game, root, &change.relative_path, &change.absolute_path)
    {
        !existed && fp == change.source_fingerprint && sha256_hex(&bytes).eq_ignore_ascii_case(&change.before_sha256)
    } else {
        false
    }
}

fn ensure_no_companion_overlap(root: &Path, definition: &GameFixDefinition) -> Result<()> {
    let manifest_path = root.join(".save-editor-companion").join("manifest.json");
    let marker_path = root
        .join("gamedata")
        .join("scripts")
        .join("save_editor_companion.script");

    if !manifest_path.is_file() {
        if marker_path.is_file() {
            return Err(Error::Refused(
                "Manual companion installation detected without manifest".to_string(),
            ));
        }
        return Ok(());
    }

    let Ok(bytes) = fs::read(&manifest_path) else {
        return Err(Error::damaged("Companion manifest unreadable"));
    };
    let Ok(parsed) = parse_json(&String::from_utf8_lossy(&bytes)) else {
        return Err(Error::damaged("Companion manifest malformed"));
    };
    let Some(obj) = parsed.as_object() else {
        return Err(Error::damaged("Companion manifest not an object"));
    };
    let Some(files_arr) = obj
        .iter()
        .find(|(k, _)| k.as_str() == "files")
        .and_then(|(_, v)| v.as_array())
    else {
        return Err(Error::damaged("Companion manifest missing files list"));
    };

    let mut companion_paths = HashSet::new();
    for file in files_arr {
        if let Some(f_obj) = file.as_object() {
            if let Some(path_str) = f_obj
                .iter()
                .find(|(k, _)| k.as_str() == "path")
                .and_then(|(_, v)| v.as_str())
            {
                if let Ok(norm) = normalize_relative_path(path_str) {
                    companion_paths.insert(norm.to_ascii_lowercase());
                }
            }
        }
    }

    for path in GameFixEngine::managed_paths(definition) {
        if companion_paths.contains(&path.to_ascii_lowercase()) {
            return Err(Error::Refused(format!(
                "Game Fix overlaps a Companion-managed file: {path}"
            )));
        }
    }

    Ok(())
}

fn read_active_manifests(root: &Path, mut issues: Option<&mut Vec<String>>) -> Result<Vec<GameFixManifest>> {
    let state_dir = root.join(STATE_DIRECTORY_NAME);
    if !state_dir.is_dir() {
        return Ok(Vec::new());
    }
    check_no_links(root, &state_dir)?;

    let mut active = Vec::new();
    let Ok(entries) = fs::read_dir(&state_dir) else {
        return Ok(Vec::new());
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let Some(id) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let manifest_path = path.join(MANIFEST_FILE_NAME);
            let journal_path = path.join(JOURNAL_FILE_NAME);

            if !manifest_path.is_file() {
                if journal_path.is_file() {
                    continue; // killed before manifest: ignored until recovered
                }
                if let Some(ref mut iss) = issues {
                    iss.push(format!("missing manifest: {id}"));
                    continue;
                }
                return Err(Error::damaged(format!("State directory is missing a manifest: {id}")));
            }

            match read_manifest(&manifest_path, id) {
                Ok(m) => {
                    if m.installed {
                        active.push(m);
                    }
                }
                Err(e) => {
                    if let Some(ref mut iss) = issues {
                        iss.push(format!("{id}: {e:?}"));
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    Ok(active)
}

fn read_manifest(manifest_path: &Path, expected_id: &str) -> Result<GameFixManifest> {
    let bytes = fs::read(manifest_path).map_err(|e| Error::System(format!("Failed to read manifest: {e}")))?;
    let text = String::from_utf8(bytes).map_err(|_| Error::damaged("Manifest not UTF-8"))?;
    let parsed = parse_json(&text).map_err(|_| Error::damaged("Manifest malformed JSON"))?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| Error::damaged("Manifest root not an object"))?;

    let get_str = |key: &str| -> Result<String> {
        obj.iter()
            .find(|(k, _)| k.as_str() == key)
            .and_then(|(_, v)| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::damaged(format!("Missing manifest field '{key}'")))
    };

    let raw_schema_version = obj
        .iter()
        .find(|(k, _)| k.as_str() == "schemaVersion")
        .and_then(|(_, v)| v.as_u64())
        .ok_or_else(|| Error::damaged("Missing schemaVersion"))?;
    let schema_version = u32::try_from(raw_schema_version).map_err(|_| Error::damaged("Invalid schemaVersion"))?;

    if schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(Error::damaged(format!(
            "Unsupported manifest schema version {schema_version}"
        )));
    }

    let fix_id = get_str("fixId")?;
    if fix_id != expected_id || !is_valid_id(&fix_id) {
        return Err(Error::damaged("Invalid manifest fix ID"));
    }

    let game_str = get_str("game")?;
    let game =
        GameTarget::parse(&game_str).ok_or_else(|| Error::damaged(format!("Unknown game in manifest '{game_str}'")))?;
    let steam_build_id = get_str("steamBuildId")?;
    let version = get_str("version")?;
    let title = get_str("title")?;
    let problem = get_str("problem")?;
    let description = get_str("description")?;

    let implementation = obj
        .iter()
        .find(|(k, _)| k.as_str() == "implementation")
        .and_then(|(_, v)| v.as_str())
        .and_then(GameFixImplementationType::parse)
        .ok_or_else(|| Error::damaged("Invalid implementation in manifest"))?;

    let requires_new_game = obj
        .iter()
        .find(|(k, _)| k.as_str() == "requiresNewGame")
        .and_then(|(_, v)| v.as_bool())
        .ok_or_else(|| Error::damaged("Missing requiresNewGame"))?;

    let save_compatibility = obj
        .iter()
        .find(|(k, _)| k.as_str() == "saveCompatibility")
        .and_then(|(_, v)| v.as_str())
        .and_then(GameFixSaveCompatibility::parse)
        .ok_or_else(|| Error::damaged("Invalid saveCompatibility in manifest"))?;

    let verification_state = obj
        .iter()
        .find(|(k, _)| k.as_str() == "verificationState")
        .and_then(|(_, v)| v.as_str())
        .and_then(GameFixVerificationState::parse)
        .ok_or_else(|| Error::damaged("Invalid verificationState in manifest"))?;

    let detection_method = get_str("detectionMethod")?;

    let references = obj
        .iter()
        .find(|(k, _)| k.as_str() == "references")
        .and_then(|(_, v)| v.as_array())
        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|x| x.to_string())).collect())
        .ok_or_else(|| Error::damaged("Missing references list"))?;

    let category = obj
        .iter()
        .find(|(k, _)| k.as_str() == "category")
        .and_then(|(_, v)| v.as_str())
        .and_then(GameFixCategory::parse)
        .ok_or_else(|| Error::damaged("Invalid category in manifest"))?;

    let maturity = obj
        .iter()
        .find(|(k, _)| k.as_str() == "maturity")
        .and_then(|(_, v)| v.as_str())
        .and_then(GameFixMaturity::parse)
        .ok_or_else(|| Error::damaged("Invalid maturity in manifest"))?;

    let depends_on = obj
        .iter()
        .find(|(k, _)| k.as_str() == "dependsOn")
        .and_then(|(_, v)| v.as_array())
        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|x| x.to_string())).collect())
        .ok_or_else(|| Error::damaged("Missing dependsOn list"))?;

    let conflicts_with = obj
        .iter()
        .find(|(k, _)| k.as_str() == "conflictsWith")
        .and_then(|(_, v)| v.as_array())
        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|x| x.to_string())).collect())
        .ok_or_else(|| Error::damaged("Missing conflictsWith list"))?;

    let source = get_str("source")?;

    let installed = obj
        .iter()
        .find(|(k, _)| k.as_str() == "installed")
        .and_then(|(_, v)| v.as_bool())
        .ok_or_else(|| Error::damaged("Missing installed field"))?;

    let files_json = obj
        .iter()
        .find(|(k, _)| k.as_str() == "files")
        .and_then(|(_, v)| v.as_array())
        .ok_or_else(|| Error::damaged("Missing files list"))?;

    let mut files = Vec::with_capacity(files_json.len());
    for f in files_json {
        let f_obj = f
            .as_object()
            .ok_or_else(|| Error::damaged("File entry not an object"))?;
        let rel = f_obj
            .iter()
            .find(|(k, _)| k.as_str() == "relativePath")
            .and_then(|(_, v)| v.as_str())
            .ok_or_else(|| Error::damaged("Missing relativePath"))?
            .to_string();
        let before_sha = f_obj
            .iter()
            .find(|(k, _)| k.as_str() == "beforeSha256")
            .and_then(|(_, v)| v.as_str())
            .ok_or_else(|| Error::damaged("Missing beforeSha256"))?
            .to_string();
        let after_sha = f_obj
            .iter()
            .find(|(k, _)| k.as_str() == "afterSha256")
            .and_then(|(_, v)| v.as_str())
            .ok_or_else(|| Error::damaged("Missing afterSha256"))?
            .to_string();
        let backup = f_obj
            .iter()
            .find(|(k, _)| k.as_str() == "backupPath")
            .and_then(|(_, v)| v.as_str())
            .ok_or_else(|| Error::damaged("Missing backupPath"))?
            .to_string();
        let existed = f_obj
            .iter()
            .find(|(k, _)| k.as_str() == "targetExistedBefore")
            .and_then(|(_, v)| v.as_bool())
            .ok_or_else(|| Error::damaged("Missing targetExistedBefore"))?;

        files.push(ManagedGameFile {
            relative_path: rel,
            before_sha256: before_sha,
            after_sha256: after_sha,
            backup_path: backup,
            target_existed_before: existed,
        });
    }

    Ok(GameFixManifest {
        schema_version,
        fix_id,
        game,
        steam_build_id,
        version,
        title,
        problem,
        description,
        implementation,
        requires_new_game,
        save_compatibility,
        verification_state,
        detection_method,
        references,
        category,
        maturity,
        depends_on,
        conflicts_with,
        source,
        installed,
        files,
    })
}

fn serialize_manifest(manifest: &GameFixManifest) -> Vec<u8> {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str(&format!("  \"schemaVersion\": {},\n", manifest.schema_version));
    json.push_str(&format!("  \"fixId\": \"{}\",\n", manifest.fix_id));
    json.push_str(&format!("  \"game\": \"{}\",\n", manifest.game.json_name()));
    json.push_str(&format!("  \"steamBuildId\": \"{}\",\n", manifest.steam_build_id));
    json.push_str(&format!("  \"version\": \"{}\",\n", manifest.version));
    json.push_str(&format!("  \"title\": {},\n", escape_json_str(&manifest.title)));
    json.push_str(&format!("  \"problem\": {},\n", escape_json_str(&manifest.problem)));
    json.push_str(&format!(
        "  \"description\": {},\n",
        escape_json_str(&manifest.description)
    ));
    json.push_str(&format!(
        "  \"implementation\": \"{}\",\n",
        manifest.implementation.as_str()
    ));
    json.push_str(&format!("  \"requiresNewGame\": {},\n", manifest.requires_new_game));
    json.push_str(&format!(
        "  \"saveCompatibility\": \"{}\",\n",
        manifest.save_compatibility.as_str()
    ));
    json.push_str(&format!(
        "  \"verificationState\": \"{}\",\n",
        manifest.verification_state.as_str()
    ));
    json.push_str(&format!(
        "  \"detectionMethod\": {},\n",
        escape_json_str(&manifest.detection_method)
    ));

    json.push_str("  \"references\": [\n");
    for (i, r) in manifest.references.iter().enumerate() {
        json.push_str(&format!(
            "    {}{}",
            escape_json_str(r),
            if i.saturating_add(1) < manifest.references.len() {
                ",\n"
            } else {
                "\n"
            }
        ));
    }
    json.push_str("  ],\n");

    json.push_str(&format!("  \"category\": \"{}\",\n", manifest.category.as_str()));
    json.push_str(&format!("  \"maturity\": \"{}\",\n", manifest.maturity.as_str()));

    json.push_str("  \"dependsOn\": [\n");
    for (i, d) in manifest.depends_on.iter().enumerate() {
        json.push_str(&format!(
            "    \"{}\"{}",
            d,
            if i.saturating_add(1) < manifest.depends_on.len() {
                ",\n"
            } else {
                "\n"
            }
        ));
    }
    json.push_str("  ],\n");

    json.push_str("  \"conflictsWith\": [\n");
    for (i, c) in manifest.conflicts_with.iter().enumerate() {
        json.push_str(&format!(
            "    \"{}\"{}",
            c,
            if i.saturating_add(1) < manifest.conflicts_with.len() {
                ",\n"
            } else {
                "\n"
            }
        ));
    }
    json.push_str("  ],\n");

    json.push_str(&format!("  \"source\": {},\n", escape_json_str(&manifest.source)));
    json.push_str(&format!("  \"installed\": {},\n", manifest.installed));

    json.push_str("  \"files\": [\n");
    for (i, f) in manifest.files.iter().enumerate() {
        json.push_str("    {\n");
        json.push_str(&format!("      \"relativePath\": \"{}\",\n", f.relative_path));
        json.push_str(&format!("      \"beforeSha256\": \"{}\",\n", f.before_sha256));
        json.push_str(&format!("      \"afterSha256\": \"{}\",\n", f.after_sha256));
        json.push_str(&format!("      \"backupPath\": \"{}\",\n", f.backup_path));
        json.push_str(&format!("      \"targetExistedBefore\": {}\n", f.target_existed_before));
        json.push_str(&format!(
            "    }}{}",
            if i.saturating_add(1) < manifest.files.len() {
                ",\n"
            } else {
                "\n"
            }
        ));
    }
    json.push_str("  ]\n");
    json.push_str("}\n");

    json.into_bytes()
}

fn write_journal(fix_dir: &Path, journal: &crate::models::GameFixJournal) -> Result<()> {
    let path = fix_dir.join(JOURNAL_FILE_NAME);
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str(&format!("  \"schemaVersion\": {},\n", journal.schema_version));
    json.push_str(&format!("  \"kind\": \"{}\",\n", journal.kind));
    json.push_str(&format!("  \"freshState\": {},\n", journal.fresh_state));
    json.push_str("  \"files\": [\n");
    for (i, f) in journal.files.iter().enumerate() {
        json.push_str("    {\n");
        json.push_str(&format!("      \"relativePath\": \"{}\",\n", f.relative_path));
        json.push_str(&format!("      \"beforeSha256\": \"{}\",\n", f.before_sha256));
        json.push_str(&format!("      \"afterSha256\": \"{}\",\n", f.after_sha256));
        json.push_str(&format!("      \"backupPath\": \"{}\",\n", f.backup_path));
        json.push_str(&format!("      \"targetExistedBefore\": {}\n", f.target_existed_before));
        json.push_str(&format!(
            "    }}{}",
            if i.saturating_add(1) < journal.files.len() {
                ",\n"
            } else {
                "\n"
            }
        ));
    }
    json.push_str("  ]\n");
    json.push_str("}\n");

    AtomicFileWriter::write(&path, json.as_bytes(), true)
}

fn delete_journal(fix_dir: &Path) {
    let path = fix_dir.join(JOURNAL_FILE_NAME);
    if path.is_file() {
        let _ = fs::remove_file(path);
    }
}

fn delete_journal_checked(game_dir: &Path, fix_dir: &Path) -> Result<()> {
    let path = fix_dir.join(JOURNAL_FILE_NAME);
    check_no_links(game_dir, &path)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::System(format!("Failed to clear Game Fix journal: {error}"))),
    }
}

fn finish_interrupted_transaction(game_dir: &Path, fix_id: &str) -> Result<bool> {
    let fix_dir = get_fix_directory(game_dir, fix_id);
    let journal_path = fix_dir.join(JOURNAL_FILE_NAME);
    if !journal_path.is_file() {
        return Ok(false);
    }

    let bytes = fs::read(&journal_path).map_err(|e| Error::System(format!("Failed to read journal: {e}")))?;
    let text = String::from_utf8(bytes).map_err(|_| Error::damaged("Journal not UTF-8"))?;
    let parsed = parse_json(&text).map_err(|_| Error::damaged("Journal malformed JSON"))?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| Error::damaged("Journal not an object"))?;

    let kind = obj
        .iter()
        .find(|(k, _)| k.as_str() == "kind")
        .and_then(|(_, v)| v.as_str())
        .ok_or_else(|| Error::damaged("Missing journal kind"))?;

    let fresh_state = obj
        .iter()
        .find(|(k, _)| k.as_str() == "freshState")
        .and_then(|(_, v)| v.as_bool())
        .unwrap_or(false);

    let manifest_path = get_manifest_path(game_dir, fix_id);
    let manifest = if manifest_path.is_file() {
        read_manifest(&manifest_path, fix_id).ok()
    } else {
        None
    };

    let committed = if kind == JOURNAL_INSTALL {
        manifest.as_ref().is_some_and(|m| m.installed)
    } else {
        manifest.as_ref().is_some_and(|m| !m.installed)
    };

    if committed {
        delete_journal(&fix_dir);
        return Ok(false);
    }

    let files_json = obj
        .iter()
        .find(|(k, _)| k.as_str() == "files")
        .and_then(|(_, v)| v.as_array())
        .ok_or_else(|| Error::damaged("Missing journal files list"))?;

    for f in files_json {
        let f_obj = f
            .as_object()
            .ok_or_else(|| Error::damaged("Journal file entry not an object"))?;
        let rel = f_obj
            .iter()
            .find(|(k, _)| k.as_str() == "relativePath")
            .and_then(|(_, v)| v.as_str())
            .ok_or_else(|| Error::damaged("Missing relativePath in journal"))?;
        let before_sha = f_obj
            .iter()
            .find(|(k, _)| k.as_str() == "beforeSha256")
            .and_then(|(_, v)| v.as_str())
            .ok_or_else(|| Error::damaged("Missing beforeSha256 in journal"))?;
        let after_sha = f_obj
            .iter()
            .find(|(k, _)| k.as_str() == "afterSha256")
            .and_then(|(_, v)| v.as_str())
            .ok_or_else(|| Error::damaged("Missing afterSha256 in journal"))?;
        let backup_path = f_obj
            .iter()
            .find(|(k, _)| k.as_str() == "backupPath")
            .and_then(|(_, v)| v.as_str())
            .ok_or_else(|| Error::damaged("Missing backupPath in journal"))?;
        let existed = f_obj
            .iter()
            .find(|(k, _)| k.as_str() == "targetExistedBefore")
            .and_then(|(_, v)| v.as_bool())
            .ok_or_else(|| Error::damaged("Missing targetExistedBefore in journal"))?;

        let full_path = resolve_game_path(game_dir, rel)?;
        if !full_path.is_file() {
            if !existed {
                continue;
            }
            return Err(Error::damaged(format!("File is missing during recovery: {rel}")));
        }

        let current_sha = sha256_hex(
            &fs::read(&full_path)
                .map_err(|e| Error::System(format!("Failed to read game file during recovery: {e}")))?,
        );
        if existed && current_sha.eq_ignore_ascii_case(before_sha) {
            continue;
        }
        if !current_sha.eq_ignore_ascii_case(after_sha) {
            return Err(Error::Refused(format!("File was changed by something else: {rel}")));
        }

        if !existed {
            let _ = fs::remove_file(&full_path);
            continue;
        }

        let full_backup = resolve_state_path(&fix_dir, backup_path)?;
        let before_bytes =
            fs::read(&full_backup).map_err(|e| Error::System(format!("Failed to read recovery backup: {e}")))?;
        if !sha256_hex(&before_bytes).eq_ignore_ascii_case(before_sha) {
            return Err(Error::damaged(format!("Recovery copy failed hash check: {rel}")));
        }

        AtomicFileWriter::write(&full_path, &before_bytes, true)?;
    }

    if kind == JOURNAL_UNINSTALL {
        if let Some(mut m) = manifest {
            m.installed = false;
            let bytes = serialize_manifest(&m);
            AtomicFileWriter::write(&manifest_path, &bytes, true)?;
        }
        delete_journal(&fix_dir);
        return Ok(true);
    }

    if fresh_state {
        let note = fix_dir.join(RECOVERY_NOTE_FILE_NAME);
        if note.is_file() {
            let _ = fs::remove_file(note);
        }
        delete_empty_state_tree(&fix_dir);
    }

    delete_journal(&fix_dir);
    Ok(true)
}

fn rollback_installation(applied: &[PreparedFileChange]) -> Vec<String> {
    let mut errors = Vec::new();
    for change in applied.iter().rev() {
        if !change.absolute_path.is_file() {
            continue;
        }
        if let Ok(bytes) = fs::read(&change.absolute_path) {
            if sha256_hex(&bytes) != change.after_sha256 {
                errors.push(format!("file changed during rollback: {}", change.relative_path));
                continue;
            }
        }
        if change.target_existed_before {
            if AtomicFileWriter::write(&change.absolute_path, &change.before_bytes, true).is_err() {
                errors.push(format!("failed to restore: {}", change.relative_path));
            }
        } else {
            let _ = fs::remove_file(&change.absolute_path);
        }
    }
    errors
}

fn rollback_uninstallation(game_dir: &Path, applied: &[PreparedFileChange]) -> Vec<String> {
    let mut errors = Vec::new();
    for change in applied.iter().rev() {
        if let Err(error) = check_no_links(game_dir, &change.absolute_path) {
            errors.push(format!("{}: {error}", change.relative_path));
            continue;
        }

        let current = match fs::read(&change.absolute_path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !change.target_existed_before => None,
            Err(error) => {
                errors.push(format!(
                    "{}: failed to read during rollback: {error}",
                    change.relative_path
                ));
                continue;
            }
        };

        if let Some(ref bytes) = current {
            let current_sha = sha256_hex(bytes);
            if current_sha == change.before_sha256 {
                continue;
            }
            if current_sha != change.after_sha256 {
                errors.push(format!(
                    "{}: file changed during uninstall rollback",
                    change.relative_path
                ));
                continue;
            }
        }

        if let Err(error) = AtomicFileWriter::write(&change.absolute_path, &change.before_bytes, true) {
            errors.push(format!(
                "{}: failed to restore installed file: {error}",
                change.relative_path
            ));
            continue;
        }
        if !matches_file_hash(&change.absolute_path, &change.before_sha256) {
            errors.push(format!("{}: restored file failed its hash check", change.relative_path));
        }
    }
    errors
}

fn write_recovery_note(
    fix_dir: &Path,
    fix_id: &str,
    changes: &[PreparedFileChange],
    applied: &[PreparedFileChange],
    errors: &[String],
) {
    let mut note = format!(
        "Game Fix {fix_id}: the installation failed and the game files could not all be restored.\n\
         Copy each original file back over the game file, then delete this folder.\n\n"
    );

    for (index, change) in changes.iter().enumerate() {
        let state = if applied.iter().any(|c| c.relative_path == change.relative_path) {
            "may be changed"
        } else {
            "not touched"
        };
        if change.target_existed_before {
            note.push_str(&format!(
                "backups/file-{index:04}.before -> {} ({state}, original sha256 {})\n",
                change.relative_path, change.before_sha256
            ));
        } else {
            note.push_str(&format!(
                "{} ({state}): did not exist before; delete it if present\n",
                change.relative_path
            ));
        }
    }

    note.push('\n');
    for err in errors {
        note.push_str(&format!("problem: {err}\n"));
    }

    let note_path = fix_dir.join(RECOVERY_NOTE_FILE_NAME);
    let _ = fs::write(note_path, note.as_bytes());
}

fn delete_empty_state_tree(fix_dir: &Path) {
    if !fix_dir.is_dir() {
        return;
    }
    let _ = fs::remove_dir_all(fix_dir);
}

fn is_valid_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 128 {
        return false;
    }
    let Some(first) = id.chars().next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

fn cleanup_update_journal_after_rollback(fix_dir: &Path, rollback_errors: &[String]) {
    if rollback_errors.is_empty() {
        delete_journal(fix_dir);
    }
}

fn parse_numeric_version(value: &str) -> Result<Vec<u64>> {
    value
        .split('.')
        .map(|part| {
            if part.is_empty() {
                return Err(Error::Refused("Fix version contains an empty numeric part".to_owned()));
            }
            part.parse::<u64>()
                .map_err(|_| Error::Refused(format!("Fix version is not numeric: {value}")))
        })
        .collect()
}

fn validate_definition(def: &GameFixDefinition) -> Result<()> {
    if !is_valid_id(&def.id) {
        return Err(Error::damaged(format!("Invalid fix ID '{}'", def.id)));
    }
    if def.version.trim().is_empty()
        || def.title.trim().is_empty()
        || def.source.trim().is_empty()
        || def.supported_steam_build_ids.is_empty()
    {
        return Err(Error::damaged(
            "Fix version, title, source and supported builds are required",
        ));
    }
    if def.maturity == GameFixMaturity::ResearchOnly {
        return Err(Error::Refused("Research-only fixes cannot be installed".to_string()));
    }
    Ok(())
}

/// Decodes patch text using specified code page (28591 = Latin-1, 1251 = Windows-1251).
pub fn decode_patch_text(bytes: &[u8], code_page: u32) -> Result<String> {
    match code_page {
        28591 => Ok(bytes.iter().map(|&b| char::from(b)).collect()),
        1251 => Ok(sse_content::encoding::decode_windows_1251(bytes)),
        _ => Err(Error::Refused(format!("Unsupported text patch code page {code_page}"))),
    }
}

/// Encodes text to bytes using the specified code page.
///
/// # Errors
/// Returns [`Error::Damaged`] on unencodable characters or [`Error::Refused`] on unsupported code pages.
pub fn encode_patch_text(text: &str, code_page: u32) -> Result<Vec<u8>> {
    match code_page {
        28591 => {
            let mut out = Vec::with_capacity(text.len());
            for c in text.chars() {
                let u = u32::from(c);
                let byte = u8::try_from(u)
                    .map_err(|_| Error::damaged(format!("Character '{c}' cannot be encoded in Latin-1")))?;
                out.push(byte);
            }
            Ok(out)
        }
        1251 => {
            let mut out = Vec::with_capacity(text.len());
            for c in text.chars() {
                let u = u32::from(c);
                if u < 0x80 {
                    let byte = u8::try_from(u).map_err(|_| Error::damaged("Invalid ascii byte"))?;
                    out.push(byte);
                } else if let Some(idx) = sse_content::encoding::CP1251_TABLE
                    .iter()
                    .position(|&cp| u32::from(cp) == u)
                {
                    let byte_val = 0x80usize
                        .checked_add(idx)
                        .and_then(|val| u8::try_from(val).ok())
                        .ok_or_else(|| Error::damaged("CP1251 index overflow"))?;
                    out.push(byte_val);
                } else {
                    return Err(Error::damaged(format!(
                        "Character '{c}' cannot be encoded in Windows-1251"
                    )));
                }
            }
            Ok(out)
        }
        _ => Err(Error::Refused(format!("Unsupported text patch code page {code_page}"))),
    }
}

fn escape_json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod g13_tests {
    use super::*;
    use crate::fs_util::fail_atomic_write_number_for_test;
    use crate::models::TextPatchOperation;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_game_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        // macOS clocks tick in microseconds, so parallel tests need a counter as well.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("sse-g13-update-{}-{nonce:x}-{sequence}", std::process::id()))
    }

    fn definition(version: &str, a: &str, b: &str, a_sha: &str, b_sha: &str) -> GameFixDefinition {
        GameFixDefinition {
            id: "test.g13.transaction".to_owned(),
            game: GameTarget::ShadowOfChernobyl,
            version: version.to_owned(),
            title: "G13 transaction test".to_owned(),
            supported_steam_build_ids: vec!["100".to_owned()],
            category: GameFixCategory::Experimental,
            maturity: GameFixMaturity::Experimental,
            depends_on: Vec::new(),
            conflicts_with: Vec::new(),
            text_patches: vec![
                TextPatchOperation {
                    relative_path: "gamedata/configs/a.ltx".to_owned(),
                    expected_text: "old-a".to_owned(),
                    replacement_text: a.to_owned(),
                    expected_file_sha256: Some(a_sha.to_owned()),
                    code_page: 28_591,
                    retail_only: false,
                },
                TextPatchOperation {
                    relative_path: "gamedata/configs/b.ltx".to_owned(),
                    expected_text: "old-b".to_owned(),
                    replacement_text: b.to_owned(),
                    expected_file_sha256: Some(b_sha.to_owned()),
                    code_page: 28_591,
                    retail_only: false,
                },
            ],
            source: "test".to_owned(),
            problem: "test".to_owned(),
            description: "test".to_owned(),
            implementation: GameFixImplementationType::ExactTextReplacement,
            requires_new_game: false,
            save_compatibility: GameFixSaveCompatibility::ExistingSaves,
            verification_state: GameFixVerificationState::SyntheticTests,
            detection_method: "test".to_owned(),
            references: Vec::new(),
            overlays: Vec::new(),
            spawn_edits: Vec::new(),
        }
    }

    #[test]
    fn hash_anchored_fix_installs_and_uninstalls_without_a_steam_manifest() -> Result<()> {
        let root = temp_game_root();
        fs::create_dir_all(root.join("gamedata/configs"))?;
        fs::write(root.join("fsgame.ltx"), b"$game_data$=true|true|$fs_root$|gamedata\\")?;

        let original_a = b"value=old-a\n";
        let original_b = b"value=old-b\n";
        let path_a = root.join("gamedata/configs/a.ltx");
        let path_b = root.join("gamedata/configs/b.ltx");
        fs::write(&path_a, original_a)?;
        fs::write(&path_b, original_b)?;

        let fix = definition("1.0", "v1-a", "v1-b", &sha256_hex(original_a), &sha256_hex(original_b));
        let engine = GameFixEngine::with_synthetic(true);

        let installed = engine.install(&fix, &root)?;
        assert!(installed.changed);
        assert_eq!(fs::read(&path_a)?, b"value=v1-a\n");
        assert_eq!(fs::read(&path_b)?, b"value=v1-b\n");
        assert_eq!(engine.get_manifest(&fix.id, &root)?.steam_build_id, "");

        engine.uninstall(&fix.id, &root)?;
        assert_eq!(fs::read(&path_a)?, original_a);
        assert_eq!(fs::read(&path_b)?, original_b);

        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn steamless_fix_requires_exact_source_hashes() -> Result<()> {
        let root = temp_game_root();
        fs::create_dir_all(root.join("gamedata/configs"))?;
        fs::write(root.join("fsgame.ltx"), b"$game_data$=true|true|$fs_root$|gamedata\\")?;
        fs::write(root.join("gamedata/configs/a.ltx"), b"value=old-a\n")?;
        fs::write(root.join("gamedata/configs/b.ltx"), b"value=old-b\n")?;

        let mut fix = definition(
            "1.0",
            "v1-a",
            "v1-b",
            &sha256_hex(b"value=old-a\n"),
            &sha256_hex(b"value=old-b\n"),
        );
        let patch = fix
            .text_patches
            .get_mut(0)
            .ok_or_else(|| Error::damaged("Test definition is missing its first text patch"))?;
        patch.expected_file_sha256 = None;
        let error = match GameFixEngine::with_synthetic(true).install(&fix, &root) {
            Err(error) => error,
            Ok(_) => {
                return Err(Error::damaged(
                    "Unanchored fix unexpectedly used the non-Steam fallback",
                ))
            }
        };

        assert!(error.to_string().contains("exact source-file hash anchors"));
        assert_eq!(fs::read(root.join("gamedata/configs/a.ltx"))?, b"value=old-a\n");
        assert!(!get_manifest_path(&root, &fix.id).exists());
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn steamless_fix_checks_source_hashes_before_writing() -> Result<()> {
        let root = temp_game_root();
        fs::create_dir_all(root.join("gamedata/configs"))?;
        fs::write(root.join("fsgame.ltx"), b"$game_data$=true|true|$fs_root$|gamedata\\")?;
        fs::write(root.join("gamedata/configs/a.ltx"), b"value=unexpected\n")?;
        fs::write(root.join("gamedata/configs/b.ltx"), b"value=old-b\n")?;

        let fix = definition(
            "1.0",
            "v1-a",
            "v1-b",
            &sha256_hex(b"value=old-a\n"),
            &sha256_hex(b"value=old-b\n"),
        );
        let error = match GameFixEngine::with_synthetic(true).install(&fix, &root) {
            Err(error) => error,
            Ok(_) => return Err(Error::damaged("A mismatched source hash unexpectedly installed")),
        };

        assert!(error
            .to_string()
            .contains("source file hash does not match verified build"));
        assert_eq!(fs::read(root.join("gamedata/configs/a.ltx"))?, b"value=unexpected\n");
        assert!(!get_manifest_path(&root, &fix.id).exists());
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn unlisted_steam_build_can_use_exact_source_hashes() -> Result<()> {
        let root = temp_game_root();
        fs::create_dir_all(root.join("gamedata/configs"))?;
        fs::write(root.join("fsgame.ltx"), b"$game_data$=true|true|$fs_root$|gamedata\\")?;
        fs::write(
            root.join("appmanifest_4500.acf"),
            b"\"AppState\" { \"appid\" \"4500\" \"buildid\" \"101\" }",
        )?;

        let original_a = b"value=old-a\n";
        let original_b = b"value=old-b\n";
        let path_a = root.join("gamedata/configs/a.ltx");
        let path_b = root.join("gamedata/configs/b.ltx");
        fs::write(&path_a, original_a)?;
        fs::write(&path_b, original_b)?;

        let fix = definition("1.0", "v1-a", "v1-b", &sha256_hex(original_a), &sha256_hex(original_b));
        let engine = GameFixEngine::with_synthetic(true);

        let installed = engine.install(&fix, &root)?;
        assert!(installed.changed);
        assert_eq!(engine.get_manifest(&fix.id, &root)?.steam_build_id, "101");
        engine.uninstall(&fix.id, &root)?;
        assert_eq!(fs::read(&path_a)?, original_a);
        assert_eq!(fs::read(&path_b)?, original_b);

        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn failed_uninstall_restores_files_already_removed_by_the_transaction() -> Result<()> {
        let root = temp_game_root();
        fs::create_dir_all(root.join("gamedata/configs"))?;
        fs::write(root.join("fsgame.ltx"), b"$game_data$=true|true|$fs_root$|gamedata\\")?;
        fs::write(
            root.join("appmanifest_4500.acf"),
            b"\"AppState\" { \"appid\" \"4500\" \"buildid\" \"100\" }",
        )?;

        let original_a = b"value=old-a\n";
        let original_b = b"value=old-b\n";
        let path_a = root.join("gamedata/configs/a.ltx");
        let path_b = root.join("gamedata/configs/b.ltx");
        fs::write(&path_a, original_a)?;
        fs::write(&path_b, original_b)?;
        let fix = definition("1.0", "v1-a", "v1-b", &sha256_hex(original_a), &sha256_hex(original_b));
        let engine = GameFixEngine::with_synthetic(true);
        engine.install(&fix, &root)?;
        let installed_a = fs::read(&path_a)?;
        let installed_b = fs::read(&path_b)?;

        // Journal, first restore, then fail while restoring the second managed file.
        fail_atomic_write_number_for_test(3);
        assert!(engine.uninstall(&fix.id, &root).is_err());

        assert_eq!(fs::read(&path_a)?, installed_a);
        assert_eq!(fs::read(&path_b)?, installed_b);
        assert_eq!(engine.get_status(&fix, &root)?, GameFixState::Installed);
        assert!(!get_fix_directory(&root, &fix.id).join(JOURNAL_FILE_NAME).exists());
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn uninstall_rollback_recreates_a_removed_overlay() -> Result<()> {
        let root = temp_game_root();
        let target = root.join("gamedata/configs/new-overlay.ltx");
        fs::create_dir_all(
            target
                .parent()
                .ok_or_else(|| Error::damaged("Missing fixture parent"))?,
        )?;
        let installed_bytes = b"overlay=installed\n";
        let change = PreparedFileChange {
            relative_path: "gamedata/configs/new-overlay.ltx".to_owned(),
            absolute_path: target.clone(),
            before_bytes: installed_bytes.to_vec(),
            after_bytes: Vec::new(),
            before_sha256: sha256_hex(installed_bytes),
            after_sha256: sha256_hex(&[]),
            target_existed_before: false,
            source_fingerprint: None,
        };

        assert!(rollback_uninstallation(&root, &[change]).is_empty());
        assert_eq!(fs::read(&target)?, installed_bytes);

        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn incomplete_update_rollback_preserves_recovery_journal() -> Result<()> {
        let root = temp_game_root();
        let fix_dir = get_fix_directory(&root, "test.g13.transaction");
        fs::create_dir_all(&fix_dir)?;
        let journal = fix_dir.join(JOURNAL_FILE_NAME);
        fs::write(&journal, b"recovery")?;

        cleanup_update_journal_after_rollback(&fix_dir, &["restore failed".to_owned()]);
        assert!(journal.is_file());

        cleanup_update_journal_after_rollback(&fix_dir, &[]);
        assert!(!journal.exists());
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn update_versions_must_be_fully_numeric() {
        assert_eq!(parse_numeric_version("1.2.3").ok(), Some(vec![1, 2, 3]));
        assert!(parse_numeric_version("1.beta.3").is_err());
        assert!(parse_numeric_version("1..3").is_err());
    }

    #[test]
    fn update_failure_on_second_new_file_restores_previous_version_and_manifest() -> Result<()> {
        let root = temp_game_root();
        fs::create_dir_all(root.join("gamedata/configs"))?;
        fs::write(root.join("fsgame.ltx"), b"$game_data$=true|true|$fs_root$|gamedata\\")?;
        fs::write(
            root.join("appmanifest_4500.acf"),
            b"\"AppState\" { \"appid\" \"4500\" \"buildid\" \"100\" }",
        )?;

        let original_a = b"value=old-a\n";
        let original_b = b"value=old-b\n";
        let path_a = root.join("gamedata/configs/a.ltx");
        let path_b = root.join("gamedata/configs/b.ltx");
        fs::write(&path_a, original_a)?;
        fs::write(&path_b, original_b)?;

        let a_sha = sha256_hex(original_a);
        let b_sha = sha256_hex(original_b);
        let v1 = definition("1.0", "v1-a", "v1-b", &a_sha, &b_sha);
        let v2 = definition("2.0", "v2-a", "v2-b", &a_sha, &b_sha);
        let engine = GameFixEngine::with_synthetic(true);

        engine.install(&v1, &root)?;
        let installed_a = fs::read(&path_a)?;
        let installed_b = fs::read(&path_b)?;
        let manifest_path = get_manifest_path(&root, &v1.id);
        let installed_manifest = fs::read(&manifest_path)?;

        // update writes: uninstall journal, two old-file restores, removed manifest,
        // install journal, first v2 file, second v2 file. Fail exactly on that second file.
        fail_atomic_write_number_for_test(7);
        let update = engine.update(&v2, &root);
        assert!(update.is_err());

        assert_eq!(fs::read(&path_a)?, installed_a);
        assert_eq!(fs::read(&path_b)?, installed_b);
        assert_eq!(fs::read(&manifest_path)?, installed_manifest);
        assert_eq!(engine.get_status(&v1, &root)?, GameFixState::Installed);
        assert!(!get_fix_directory(&root, &v1.id).join(JOURNAL_FILE_NAME).exists());

        let _ = fs::remove_dir_all(&root);
        Ok(())
    }
}
