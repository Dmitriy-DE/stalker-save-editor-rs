//! Ownership of backup-named files in the game folder: a file is the toolkit's only when a fix manifest records it.

// Tests use unwrap/expect to fail fast and index into known-length collections.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_fixes::toolkit::{FileClassification, ToolkitInstallAudit};
use sse_fixes::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(1);
const BUILD_ID: &str = "11450472";
const BACKUP: &str = "gamedata/user-backup.sse-orig";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let counter = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sse-audit-ownership-{}-{}-{}",
            std::process::id(),
            counter,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("fsgame.ltx"),
            "; fsgame marker\n$game_data$ = false| false| $fs_root$| gamedata\\\n",
        )
        .unwrap();
        let app = GameTarget::ClearSky.steam_app_id().unwrap();
        fs::write(
            root.join(format!("appmanifest_{app}.acf")),
            format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{app}\"\n\t\"buildid\"\t\t\"{BUILD_ID}\"\n}}\n"),
        )
        .unwrap();
        Self { root }
    }

    fn write(&self, relative: &str, content: &[u8]) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn exists(&self, relative: &str) -> bool {
        self.root.join(relative).exists()
    }

    fn quarantine_has_files(&self) -> bool {
        let dir = self.root.join(".save-editor-quarantine");
        dir.exists()
            && fs::read_dir(dir)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false)
    }

    /// Installs a synthetic fix that edits `BACKUP`, then marks the fix uninstalled: its manifest stays on disk.
    fn install_then_uninstall_fix(&self, engine: &GameFixEngine) {
        self.write(BACKUP, b"local state = 1\n");
        let definition = GameFixDefinition {
            id: "cs.test.backup-owner".to_string(),
            game: GameTarget::ClearSky,
            version: "1.0.0".to_string(),
            title: "Test Fix".to_string(),
            supported_steam_build_ids: vec![BUILD_ID.to_string()],
            category: GameFixCategory::Essential,
            maturity: GameFixMaturity::Validated,
            depends_on: vec![],
            conflicts_with: vec![],
            text_patches: vec![TextPatchOperation {
                relative_path: BACKUP.to_string(),
                expected_text: "local state = 1\n".to_string(),
                replacement_text: "local state = 2\n".to_string(),
                code_page: 28591,
                expected_file_sha256: None,
                retail_only: false,
            }],
            source: "unit test".to_string(),
            problem: "problem".to_string(),
            description: "description".to_string(),
            implementation: GameFixImplementationType::ExactTextReplacement,
            requires_new_game: false,
            save_compatibility: GameFixSaveCompatibility::ExistingSaves,
            verification_state: GameFixVerificationState::RetailFilesVerified,
            detection_method: "method".to_string(),
            references: vec![],
            overlays: vec![],
            spawn_edits: vec![],
        };
        engine.install(&definition, &self.root).unwrap();
        let manifest = self
            .root
            .join(".save-editor-game-fixes")
            .join("cs.test.backup-owner")
            .join("manifest.json");
        let text = fs::read_to_string(&manifest).unwrap();
        let uninstalled = text
            .replace("\"installed\": true", "\"installed\": false")
            .replace("\"installed\":true", "\"installed\":false");
        assert_ne!(
            text, uninstalled,
            "the manifest must record the fix as installed before the test changes it"
        );
        fs::write(manifest, uninstalled).unwrap();
    }

    fn classification(&self, engine: &GameFixEngine, relative: &str) -> FileClassification {
        let report = ToolkitInstallAudit::audit_installation(&self.root, GameTarget::ClearSky, engine).unwrap();
        report
            .items
            .iter()
            .find(|item| item.relative_path == relative)
            .map(|item| item.classification)
            .expect("the backup-named file must be audited")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// (a) A player's own `*.sse-orig` file that no fix records stays in the game folder and is not quarantined.
#[test]
fn an_unrecorded_backup_named_file_is_left_in_place() {
    let fixture = Fixture::new();
    let engine = GameFixEngine::with_synthetic(true);
    fixture.write(BACKUP, b"the player's own file\n");

    let report = ToolkitInstallAudit::audit_installation(&fixture.root, GameTarget::ClearSky, &engine).unwrap();
    let moved = ToolkitInstallAudit::cleanup_orphans(&fixture.root, &report).unwrap();

    assert_eq!(moved, 0, "an unrecorded file must not be moved");
    assert!(fixture.exists(BACKUP), "the player's file stays in the game folder");
    assert!(!fixture.quarantine_has_files(), "the quarantine stays empty");
    assert_eq!(
        fixture.classification(&engine, BACKUP),
        FileClassification::UnknownSimilarName
    );
}

/// (b) A file the toolkit wrote, recorded in an uninstalled fix's manifest and unchanged since, is still moved, as before.
#[test]
fn a_recorded_orphan_unchanged_since_the_fix_is_moved() {
    let fixture = Fixture::new();
    let engine = GameFixEngine::with_synthetic(true);
    fixture.install_then_uninstall_fix(&engine);

    assert_eq!(
        fixture.classification(&engine, BACKUP),
        FileClassification::OrphanedToolkitOwned
    );
    let report = ToolkitInstallAudit::audit_installation(&fixture.root, GameTarget::ClearSky, &engine).unwrap();
    let moved = ToolkitInstallAudit::cleanup_orphans(&fixture.root, &report).unwrap();

    assert_eq!(moved, 1);
    assert!(!fixture.exists(BACKUP));
    assert!(fixture.quarantine_has_files());
}

/// (c) A file a manifest records, whose contents the player has since replaced, is shown as changed and not moved.
#[test]
fn a_recorded_file_whose_contents_changed_is_not_moved() {
    let fixture = Fixture::new();
    let engine = GameFixEngine::with_synthetic(true);
    fixture.install_then_uninstall_fix(&engine);
    fixture.write(BACKUP, b"the player replaced this\n");

    assert_eq!(
        fixture.classification(&engine, BACKUP),
        FileClassification::ModifiedToolkitBackup
    );
    let report = ToolkitInstallAudit::audit_installation(&fixture.root, GameTarget::ClearSky, &engine).unwrap();
    let moved = ToolkitInstallAudit::cleanup_orphans(&fixture.root, &report).unwrap();

    assert_eq!(moved, 0);
    assert!(fixture.exists(BACKUP));
    assert_eq!(
        fs::read(fixture.root.join(BACKUP)).unwrap(),
        b"the player replaced this\n"
    );
}
