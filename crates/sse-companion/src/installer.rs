//! Explicit, manifest-backed companion install and exact removal.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sse_content::{CompanionGame, EntryDecoder, GameFileTree, HeaderDecoder};

use sse_codecs::{
    json::{Event, Reader, Text},
    sha256,
};

const STATE_DIRECTORY: &str = ".save-editor-companion";
const MANIFEST_FILE: &str = "manifest.json";
const JOURNAL_FILE: &str = "transaction.log";
const TRANSACTION_DIRECTORY: &str = "transaction";
const MANIFEST_LIMIT: usize = 8 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// One relative game file in the bundled companion payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadFile {
    /// Path relative to the game root, usually `gamedata/...`.
    pub relative_path: String,
    /// Exact bytes to install.
    pub bytes: Vec<u8>,
}

impl PayloadFile {
    /// Creates one payload file.
    #[must_use]
    pub fn new(relative_path: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            relative_path: relative_path.into(),
            bytes,
        }
    }
}

/// Installation or state validation error.
#[derive(Debug)]
pub struct InstallError {
    message: String,
}

impl InstallError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for InstallError {}
impl From<std::io::Error> for InstallError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

#[derive(Debug, Clone)]
struct Manifest {
    game: String,
    version: String,
    game_data_created: bool,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Clone)]
struct ManifestFile {
    path: String,
    kind: String,
    before_sha256: Option<String>,
    after_sha256: String,
    backup_path: Option<String>,
    was_present: bool,
}

#[derive(Debug)]
struct TransactionJournal {
    state_existed: bool,
    game_data_existed: bool,
    remove_state_on_commit: bool,
    remove_game_data_on_commit: bool,
    manifest: TransactionTarget,
    files: Vec<TransactionTarget>,
    new_backups: Vec<(String, String)>,
    obsolete_backups: Vec<(String, String)>,
}

#[derive(Debug)]
struct TransactionTarget {
    path: String,
    before_sha256: Option<String>,
    after_sha256: Option<String>,
    preimage: Option<u32>,
}

#[derive(Debug)]
struct PlannedFileChange {
    path: String,
    after: Option<Vec<u8>>,
}

fn refuse_active_game_fix_overlap(root: &Path, payloads: &[PayloadFile]) -> Result<(), InstallError> {
    let managed = sse_fixes::GameFixEngine::get_active_managed_paths(root)
        .map_err(|error| InstallError::new(format!("cannot verify active Game Fix files: {error}")))?;
    for payload in payloads {
        let relative = normalize_relative(&payload.relative_path)?;
        if managed.contains(&relative.to_ascii_lowercase()) {
            return Err(InstallError::new(format!(
                "Companion refuses to overwrite a file managed by an active Game Fix: {relative}"
            )));
        }
    }
    Ok(())
}

fn make_archive_decoders() -> (HeaderDecoder, EntryDecoder) {
    let header_decoder: HeaderDecoder = Arc::new(|data: &[u8]| {
        let mut candidates = Vec::new();
        if let Ok(decoded) = sse_codecs::lzhuf::decode(data) {
            candidates.push(decoded);
        }
        for world_wide in [true, false] {
            let descrambled = sse_codecs::lzhuf::descramble(data, world_wide);
            if let Ok(decoded) = sse_codecs::lzhuf::decode(&descrambled) {
                candidates.push(decoded);
            }
        }
        if candidates.is_empty() {
            return Err(sse_core::Error::damaged("X-Ray archive header could not be decoded"));
        }
        candidates.dedup();
        if candidates.len() != 1 {
            return Err(sse_core::Error::damaged(
                "X-Ray archive header decryption is ambiguous: candidates differ",
            ));
        }
        Ok(candidates)
    });
    let entry_decoder: EntryDecoder =
        Arc::new(|data: &[u8], expected_size: usize| sse_codecs::lzo1x::decompress(data, expected_size));
    (header_decoder, entry_decoder)
}

fn read_xray_hook_source(
    root: &Path,
    game: CompanionGame,
    fsgame_names: &[&str],
    tree_path: &str,
    game_path: &str,
) -> Result<Vec<u8>, InstallError> {
    let loose_path = safe_game_path(root, game_path)?;
    if loose_path.is_file() {
        return read_companion_file(&loose_path);
    }
    if loose_path.exists() {
        return Err(InstallError::new("hook source path exists as a non-file"));
    }

    let (header_decoder, entry_decoder) = make_archive_decoders();
    let wanted_path = tree_path.to_ascii_lowercase();
    let tree = GameFileTree::load(
        game,
        root,
        |path| path.eq_ignore_ascii_case(&wanted_path),
        Some(fsgame_names),
        false,
        true,
        true,
        Some(header_decoder),
        Some(entry_decoder),
    )
    .map_err(|error| InstallError::new(format!("could not read X-Ray archives: {error}")))?;

    let source = tree
        .files
        .iter()
        .find(|(path, _)| path.eq_ignore_ascii_case(tree_path))
        .map(|(_, file)| file)
        .ok_or_else(|| {
            let issues = if tree.issues.is_empty() {
                String::new()
            } else {
                format!(" ({})", tree.issues.join("; "))
            };
            InstallError::new(format!(
                "required hook source {tree_path} was not found in loose gamedata or game archives{issues}"
            ))
        })?;
    source
        .read()
        .map_err(|error| InstallError::new(format!("could not read archived {tree_path}: {error}")))
}

/// Installs the requested payload only when called. Source originals are kept in the C#-compatible state directory.
pub fn install_files(root: &Path, game: &str, version: &str, payloads: &[PayloadFile]) -> Result<(), InstallError> {
    validate_game(game)?;
    if version.is_empty() || payloads.is_empty() {
        return Err(InstallError::new("version and at least one payload are required"));
    }
    let root = fs::canonicalize(root)?;
    recover_install_transaction(&root)?;
    refuse_active_game_fix_overlap(&root, payloads)?;
    let state = root.join(STATE_DIRECTORY);
    let manifest_path = safe_game_path(&root, &format!("{STATE_DIRECTORY}/{MANIFEST_FILE}"))?;
    let state_preexisted = state.exists();
    let game_data_preexisted = root.join("gamedata").exists();
    let previous_manifest_bytes = if manifest_path.exists() {
        Some(read_limited(&manifest_path)?)
    } else {
        None
    };
    let previous = if let Some(bytes) = &previous_manifest_bytes {
        Some(parse_manifest(bytes, game)?)
    } else if state.exists() {
        return Err(InstallError::new(
            "companion state directory exists without a readable manifest",
        ));
    } else {
        None
    };
    if let Some(manifest) = &previous {
        verify_managed_files(&root, manifest)?;
    }

    let old_by_path = previous
        .as_ref()
        .map(|manifest| {
            manifest
                .files
                .iter()
                .map(|file| (file.path.clone(), file))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    let mut new_files = Vec::with_capacity(payloads.len());
    let mut changes = Vec::with_capacity(payloads.len());
    let mut backup_writes = Vec::new();
    let mut obsolete_backups = Vec::new();
    let game_data_created = previous
        .as_ref()
        .map_or(!game_data_preexisted, |manifest| manifest.game_data_created);
    for payload in payloads {
        let relative = normalize_relative(&payload.relative_path)?;
        if !seen.insert(relative.clone()) {
            return Err(InstallError::new("payload contains a duplicate path"));
        }
        if payload.bytes.len() > crate::MAX_COMPANION_FILE_BYTES {
            return Err(InstallError::new("payload exceeds the configured file-size bound"));
        }
        let target = safe_game_path(&root, &relative)?;
        let after_sha256 = sha256::sha256_hex(&payload.bytes);
        changes.push(PlannedFileChange {
            path: relative.clone(),
            after: Some(payload.bytes.clone()),
        });
        if let Some(old) = old_by_path.get(&relative) {
            new_files.push(ManifestFile {
                path: relative,
                kind: old.kind.clone(),
                before_sha256: old.before_sha256.clone(),
                after_sha256,
                backup_path: old.backup_path.clone(),
                was_present: old.was_present,
            });
        } else if target.is_file() {
            let before = read_companion_file(&target)?;
            let backup_path = format!("backups/{relative}.original");
            backup_writes.push((backup_path.clone(), before.clone()));
            new_files.push(ManifestFile {
                path: relative,
                kind: "modified".to_owned(),
                before_sha256: Some(sha256::sha256_hex(&before)),
                after_sha256,
                backup_path: Some(backup_path),
                was_present: true,
            });
        } else if target.exists() {
            return Err(InstallError::new("payload path already exists as a non-file"));
        } else {
            new_files.push(ManifestFile {
                path: relative,
                kind: "copy".to_owned(),
                before_sha256: None,
                after_sha256,
                backup_path: None,
                was_present: false,
            });
        }
    }
    if let Some(previous) = &previous {
        for entry in &previous.files {
            if seen.contains(&entry.path) {
                continue;
            }
            let original = match (&entry.before_sha256, &entry.backup_path) {
                (Some(expected), Some(relative)) => {
                    let backup = safe_state_path(&root, relative)?;
                    let bytes = read_companion_file(&backup)?;
                    if sha256::sha256_hex(&bytes) != *expected {
                        return Err(InstallError::new("companion backup hash does not match the manifest"));
                    }
                    obsolete_backups.push((relative.clone(), expected.clone()));
                    Some(bytes)
                }
                (None, None) => None,
                _ => return Err(InstallError::new("companion manifest has inconsistent backup fields")),
            };
            changes.push(PlannedFileChange {
                path: normalize_relative(&entry.path)?,
                after: original,
            });
        }
    }
    new_files.sort_by(|left, right| left.path.cmp(&right.path));
    fs::create_dir_all(&state)?;
    let manifest = Manifest {
        game: game.to_owned(),
        version: version.to_owned(),
        game_data_created,
        files: new_files,
    };
    let manifest_bytes = serialize_manifest(&manifest).into_bytes();
    if let Err(error) = begin_install_transaction(
        &root,
        state_preexisted,
        game_data_preexisted,
        previous_manifest_bytes.as_deref(),
        &manifest_bytes,
        &changes,
        &backup_writes,
        &obsolete_backups,
    ) {
        let _ = recover_install_transaction(&root);
        return Err(error);
    }
    let apply = (|| {
        for (relative, bytes) in &backup_writes {
            let destination = safe_state_path(&root, relative)?;
            if !destination.exists() {
                write_atomic(&destination, bytes)?;
            }
        }
        for change in &changes {
            let target = safe_game_path(&root, &change.path)?;
            if let Some(bytes) = &change.after {
                ensure_parent(&root, &target)?;
                write_atomic(&target, bytes)?;
            } else if target.exists() {
                fs::remove_file(target)?;
            }
        }
        write_atomic(&manifest_path, &manifest_bytes)?;
        cleanup_obsolete_backups(&root, &obsolete_backups)?;
        Ok::<(), InstallError>(())
    })();
    if let Err(error) = apply {
        let rollback = recover_install_transaction(&root);
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(InstallError::new(format!(
                "install failed ({error}); rollback also failed ({rollback_error})"
            ))),
        };
    }
    finish_install_transaction(&root)?;
    Ok(())
}

/// Installs the statically bundled X-Ray mod for one retail game.
pub fn install_bundled(root: &Path, game: crate::bundled::Game) -> Result<(), InstallError> {
    let root = fs::canonicalize(root)?;
    recover_install_transaction(&root)?;
    let (game_id, content_game, fsgame_names) = match game {
        crate::bundled::Game::ShadowOfChernobyl => (
            "soc",
            CompanionGame::ShadowOfChernobyl,
            &["fsgame.ltx", "fsgame_soc.ltx"][..],
        ),
        crate::bundled::Game::ClearSky => ("cs", CompanionGame::ClearSky, &["fsgame.ltx", "fsgame_cs.ltx"][..]),
        crate::bundled::Game::CallOfPripyat => (
            "cop",
            CompanionGame::CallOfPripyat,
            &["fsgame.ltx", "fsgame_cop.ltx"][..],
        ),
    };
    let mut payloads = crate::bundled::payloads(game)?;

    let bind_path = "gamedata/scripts/bind_stalker.script";
    let bind_bytes = read_xray_hook_source(
        &root,
        content_game,
        fsgame_names,
        "scripts/bind_stalker.script",
        bind_path,
    )?;
    let hooked_bind =
        crate::hook::patch_bind_stalker(&bind_bytes, game).map_err(|error| InstallError::new(error.to_string()))?;
    payloads.push(PayloadFile::new(bind_path, hooked_bind));

    let menu_path = "gamedata/scripts/ui_main_menu.script";
    let menu_bytes = read_xray_hook_source(
        &root,
        content_game,
        fsgame_names,
        "scripts/ui_main_menu.script",
        menu_path,
    )?;
    let hooked_menu =
        crate::hook::patch_main_menu(&menu_bytes).map_err(|error| InstallError::new(error.to_string()))?;
    payloads.push(PayloadFile::new(menu_path, hooked_menu));
    install_files(&root, game_id, "v1", &payloads)
}

/// Installs the S.T.A.L.K.E.R. 2 mod into a UE4SS `Mods` directory.
///
/// The target mod directory must be absent or already owned by this install manifest.
pub fn install_stalker2(mods_directory: &Path) -> Result<(), InstallError> {
    let target = mods_directory.join("SaveEditorCompanion");
    let manifest = mods_directory.join(STATE_DIRECTORY).join(MANIFEST_FILE);
    if target.exists() && !manifest.is_file() {
        return Err(InstallError::new(
            "SaveEditorCompanion exists without an editor-owned install manifest",
        ));
    }
    let payloads = crate::bundled::stalker2_payloads()?;
    let build = crate::bundled::stalker2_build();
    install_files(mods_directory, "s2", &build, &payloads)
}

/// Removes only unchanged files owned by this manifest, restoring every original byte-for-byte.
pub fn uninstall(root: &Path, game: &str) -> Result<bool, InstallError> {
    validate_game(game)?;
    let root = fs::canonicalize(root)?;
    recover_install_transaction(&root)?;
    let state = root.join(STATE_DIRECTORY);
    let manifest_path = safe_game_path(&root, &format!("{STATE_DIRECTORY}/{MANIFEST_FILE}"))?;
    if !manifest_path.exists() {
        return Ok(false);
    }
    let manifest_bytes = read_limited(&manifest_path)?;
    let manifest = parse_manifest(&manifest_bytes, game)?;
    verify_managed_files(&root, &manifest)?;
    // Complete all reads and path checks before the first write, so conflicts never cause a partial uninstall.
    let mut restores = Vec::with_capacity(manifest.files.len());
    for entry in &manifest.files {
        let target = safe_game_path(&root, &entry.path)?;
        let original = match (&entry.before_sha256, &entry.backup_path) {
            (Some(expected), Some(relative)) => {
                let backup = safe_state_path(&root, relative)?;
                let bytes = read_companion_file(&backup)?;
                if sha256::sha256_hex(&bytes) != *expected {
                    return Err(InstallError::new("companion backup hash does not match the manifest"));
                }
                Some(bytes)
            }
            (None, None) => None,
            _ => return Err(InstallError::new("companion manifest has inconsistent backup fields")),
        };
        restores.push((entry.path.clone(), target, original));
    }
    begin_uninstall_transaction(&root, &manifest_bytes, &restores, manifest.game_data_created)?;
    let apply = (|| {
        for (_, target, original) in &restores {
            if let Some(bytes) = original {
                write_atomic(target, bytes)?;
            } else if target.exists() {
                fs::remove_file(target)?;
            }
        }
        if manifest.game_data_created {
            remove_empty_tree(&root.join("gamedata"))?;
        }
        if manifest.game == "s2" {
            remove_empty_tree(&root.join("SaveEditorCompanion"))?;
        }
        fs::remove_file(&manifest_path)?;
        fs::remove_dir_all(&state)?;
        Ok::<(), InstallError>(())
    })();
    if let Err(error) = apply {
        let rollback = recover_install_transaction(&root);
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(InstallError::new(format!(
                "uninstall failed ({error}); rollback also failed ({rollback_error})"
            ))),
        };
    }
    Ok(true)
}

fn begin_uninstall_transaction(
    root: &Path,
    manifest_bytes: &[u8],
    restores: &[(String, PathBuf, Option<Vec<u8>>)],
    remove_game_data_on_commit: bool,
) -> Result<(), InstallError> {
    let transaction_dir = safe_state_path(root, TRANSACTION_DIRECTORY)?;
    fs::create_dir_all(transaction_dir.join("preimages"))?;
    let mut next_index = 0_u32;
    let manifest_preimage = next_index;
    next_index = next_index
        .checked_add(1)
        .ok_or_else(|| InstallError::new("uninstall transaction has too many files"))?;
    write_atomic(&preimage_path(root, manifest_preimage)?, manifest_bytes)?;
    let manifest = TransactionTarget {
        path: format!("{STATE_DIRECTORY}/{MANIFEST_FILE}"),
        before_sha256: Some(sha256::sha256_hex(manifest_bytes)),
        after_sha256: None,
        preimage: Some(manifest_preimage),
    };
    let mut files = Vec::with_capacity(restores.len());
    for (relative, target, after) in restores {
        let current = read_companion_file(target)?;
        let index = next_index;
        next_index = next_index
            .checked_add(1)
            .ok_or_else(|| InstallError::new("uninstall transaction has too many files"))?;
        write_atomic(&preimage_path(root, index)?, &current)?;
        files.push(TransactionTarget {
            path: normalize_relative(relative)?,
            before_sha256: Some(sha256::sha256_hex(&current)),
            after_sha256: after.as_deref().map(sha256::sha256_hex),
            preimage: Some(index),
        });
    }
    let journal = TransactionJournal {
        state_existed: true,
        game_data_existed: true,
        remove_state_on_commit: true,
        remove_game_data_on_commit,
        manifest,
        files,
        new_backups: Vec::new(),
        obsolete_backups: Vec::new(),
    };
    let journal_path = safe_state_path(root, JOURNAL_FILE)?;
    write_atomic(&journal_path, serialize_transaction(&journal).as_bytes())
}

#[allow(clippy::too_many_arguments)] // The journal records each independent rollback input explicitly.
fn begin_install_transaction(
    root: &Path,
    state_existed: bool,
    game_data_existed: bool,
    old_manifest: Option<&[u8]>,
    new_manifest: &[u8],
    changes: &[PlannedFileChange],
    new_backups: &[(String, Vec<u8>)],
    obsolete_backups: &[(String, String)],
) -> Result<(), InstallError> {
    let transaction_dir = safe_state_path(root, TRANSACTION_DIRECTORY)?;
    fs::create_dir_all(&transaction_dir)?;
    let preimage_dir = transaction_dir.join("preimages");
    fs::create_dir_all(&preimage_dir)?;
    let mut next_index = 0_u32;
    let manifest_before_sha256 = old_manifest.map(sha256::sha256_hex);
    let manifest_preimage = if let Some(bytes) = old_manifest {
        let index = next_index;
        next_index = next_index.saturating_add(1);
        write_atomic(&preimage_path(root, index)?, bytes)?;
        Some(index)
    } else {
        None
    };
    let manifest_target = TransactionTarget {
        path: format!("{STATE_DIRECTORY}/{MANIFEST_FILE}"),
        before_sha256: manifest_before_sha256,
        after_sha256: Some(sha256::sha256_hex(new_manifest)),
        preimage: manifest_preimage,
    };
    let mut files = Vec::with_capacity(changes.len());
    for change in changes {
        let relative = normalize_relative(&change.path)?;
        let target = safe_game_path(root, &relative)?;
        let before = if target.is_file() {
            Some(read_companion_file(&target)?)
        } else if target.exists() {
            return Err(InstallError::new("payload path already exists as a non-file"));
        } else {
            None
        };
        let before_sha256 = before.as_deref().map(sha256::sha256_hex);
        let after_sha256 = change.after.as_deref().map(sha256::sha256_hex);
        if before_sha256 == after_sha256 {
            continue;
        }
        let preimage = if let Some(bytes) = before {
            let index = next_index;
            next_index = next_index
                .checked_add(1)
                .ok_or_else(|| InstallError::new("install transaction has too many files"))?;
            write_atomic(&preimage_path(root, index)?, &bytes)?;
            Some(index)
        } else {
            None
        };
        files.push(TransactionTarget {
            path: relative,
            before_sha256,
            after_sha256,
            preimage,
        });
    }
    let journal = TransactionJournal {
        state_existed,
        game_data_existed,
        remove_state_on_commit: false,
        remove_game_data_on_commit: false,
        manifest: manifest_target,
        files,
        new_backups: new_backups
            .iter()
            .map(|(relative, bytes)| (relative.clone(), sha256::sha256_hex(bytes)))
            .collect(),
        obsolete_backups: obsolete_backups.to_vec(),
    };
    let bytes = serialize_transaction(&journal);
    let journal_path = safe_state_path(root, JOURNAL_FILE)?;
    write_atomic(&journal_path, bytes.as_bytes())
}

fn preimage_path(root: &Path, index: u32) -> Result<PathBuf, InstallError> {
    safe_state_path(root, &format!("{TRANSACTION_DIRECTORY}/preimages/{index:08}.pre"))
}

fn serialize_transaction(journal: &TransactionJournal) -> String {
    let mut output = format!(
        "SSEC-C6-J1\nS\t{}\t{}\t{}\t{}\nM\t{}\t{}\t{}\n",
        u8::from(journal.state_existed),
        u8::from(journal.game_data_existed),
        u8::from(journal.remove_state_on_commit),
        u8::from(journal.remove_game_data_on_commit),
        journal.manifest.before_sha256.as_deref().unwrap_or("-"),
        journal.manifest.after_sha256.as_deref().unwrap_or("-"),
        journal
            .manifest
            .preimage
            .map_or_else(|| "-".to_owned(), |index| index.to_string()),
    );
    for file in &journal.files {
        output.push_str(&format!(
            "F\t{}\t{}\t{}\t{}\n",
            hex_encode(file.path.as_bytes()),
            file.before_sha256.as_deref().unwrap_or("-"),
            file.after_sha256.as_deref().unwrap_or("-"),
            file.preimage.map_or_else(|| "-".to_owned(), |index| index.to_string()),
        ));
    }
    for (relative, hash) in &journal.new_backups {
        output.push_str(&format!("B\t{}\t{hash}\n", hex_encode(relative.as_bytes())));
    }
    for (relative, hash) in &journal.obsolete_backups {
        output.push_str(&format!("O\t{}\t{hash}\n", hex_encode(relative.as_bytes())));
    }
    output
}

fn parse_transaction(bytes: &[u8]) -> Result<TransactionJournal, InstallError> {
    let text = std::str::from_utf8(bytes).map_err(|_| InstallError::new("install transaction is not UTF-8"))?;
    let mut lines = text.lines();
    if lines.next() != Some("SSEC-C6-J1") {
        return Err(InstallError::new("install transaction format is unknown"));
    }
    let state = lines
        .next()
        .ok_or_else(|| InstallError::new("install transaction has no state flags"))?;
    let state = state.split('\t').collect::<Vec<_>>();
    let [tag, state_existed, game_data_existed, remove_state_on_commit, remove_game_data_on_commit] = state.as_slice()
    else {
        return Err(InstallError::new("install transaction state flags are invalid"));
    };
    if *tag != "S" {
        return Err(InstallError::new("install transaction state flags are invalid"));
    }
    let state_existed = parse_flag(state_existed)?;
    let game_data_existed = parse_flag(game_data_existed)?;
    let remove_state_on_commit = parse_flag(remove_state_on_commit)?;
    let remove_game_data_on_commit = parse_flag(remove_game_data_on_commit)?;
    let manifest_line = lines
        .next()
        .ok_or_else(|| InstallError::new("install transaction has no manifest entry"))?;
    let manifest_fields = manifest_line.split('\t').collect::<Vec<_>>();
    let [tag, before, after, preimage] = manifest_fields.as_slice() else {
        return Err(InstallError::new("install transaction manifest entry is invalid"));
    };
    if *tag != "M" {
        return Err(InstallError::new("install transaction manifest entry is invalid"));
    }
    let manifest = parse_transaction_target(format!("{STATE_DIRECTORY}/{MANIFEST_FILE}"), before, after, preimage)?;
    let mut files = Vec::new();
    let mut new_backups = Vec::new();
    let mut obsolete_backups = Vec::new();
    let mut paths = BTreeSet::new();
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.as_slice() {
            ["F", encoded_path, before, after, preimage] => {
                let path = String::from_utf8(hex_decode(encoded_path)?)
                    .map_err(|_| InstallError::new("transaction target path is not UTF-8"))?;
                let path = normalize_relative(&path)?;
                if !paths.insert(path.clone()) {
                    return Err(InstallError::new("install transaction has duplicate target paths"));
                }
                files.push(parse_transaction_target(path, before, after, preimage)?);
            }
            ["B", encoded_path, hash] => {
                let path = String::from_utf8(hex_decode(encoded_path)?)
                    .map_err(|_| InstallError::new("transaction backup path is not UTF-8"))?;
                let path = normalize_relative(&path)?;
                if !is_hash(hash) {
                    return Err(InstallError::new("transaction backup hash is invalid"));
                }
                new_backups.push((path, (*hash).to_owned()));
            }
            ["O", encoded_path, hash] => {
                let path = String::from_utf8(hex_decode(encoded_path)?)
                    .map_err(|_| InstallError::new("transaction backup path is not UTF-8"))?;
                let path = normalize_relative(&path)?;
                if !is_hash(hash) {
                    return Err(InstallError::new("transaction backup hash is invalid"));
                }
                obsolete_backups.push((path, (*hash).to_owned()));
            }
            _ => return Err(InstallError::new("install transaction record is invalid")),
        }
        if files.len() > 10_000 || new_backups.len() > 10_000 || obsolete_backups.len() > 10_000 {
            return Err(InstallError::new("install transaction has too many records"));
        }
    }
    Ok(TransactionJournal {
        state_existed,
        game_data_existed,
        manifest,
        files,
        new_backups,
        obsolete_backups,
        remove_state_on_commit,
        remove_game_data_on_commit,
    })
}

fn parse_transaction_target(
    path: String,
    before: &str,
    after: &str,
    preimage: &str,
) -> Result<TransactionTarget, InstallError> {
    let before_sha256 = (before != "-").then(|| before.to_owned());
    let after_sha256 = (after != "-").then(|| after.to_owned());
    if after_sha256.as_ref().is_some_and(|value| !is_hash(value))
        || before_sha256.as_ref().is_some_and(|value| !is_hash(value))
    {
        return Err(InstallError::new("install transaction contains an invalid hash"));
    }
    let preimage = if preimage == "-" {
        None
    } else {
        Some(
            preimage
                .parse::<u32>()
                .map_err(|_| InstallError::new("install transaction preimage index is invalid"))?,
        )
    };
    if before_sha256.is_some() != preimage.is_some() {
        return Err(InstallError::new(
            "install transaction preimage metadata is inconsistent",
        ));
    }
    Ok(TransactionTarget {
        path,
        before_sha256,
        after_sha256,
        preimage,
    })
}

fn recover_install_transaction(root: &Path) -> Result<(), InstallError> {
    let state = root.join(STATE_DIRECTORY);
    if !state.exists() {
        return Ok(());
    }
    let journal_path = safe_state_path(root, JOURNAL_FILE)?;
    let transaction_dir = safe_state_path(root, TRANSACTION_DIRECTORY)?;
    if !journal_path.exists() {
        if transaction_dir.exists() {
            fs::remove_dir_all(transaction_dir)?;
        }
        let manifest = safe_state_path(root, MANIFEST_FILE)?;
        if !manifest.exists() && fs::read_dir(&state)?.next().is_none() {
            fs::remove_dir(state)?;
        }
        return Ok(());
    }
    let journal = parse_transaction(&read_limited(&journal_path)?)?;
    let manifest_path = safe_game_path(root, &journal.manifest.path)?;
    let current_manifest = if manifest_path.is_file() {
        Some(read_limited(&manifest_path)?)
    } else if manifest_path.exists() {
        return Err(InstallError::new("transaction manifest path is not a regular file"));
    } else {
        None
    };
    let current_manifest_hash = current_manifest.as_deref().map(sha256::sha256_hex);
    if current_manifest_hash == journal.manifest.after_sha256 {
        if journal.remove_state_on_commit {
            if state.exists() {
                fs::remove_dir_all(&state)?;
            }
            if journal.remove_game_data_on_commit {
                remove_empty_tree(&root.join("gamedata"))?;
            }
        } else {
            cleanup_obsolete_backups(root, &journal.obsolete_backups)?;
            finish_install_transaction(root)?;
        }
        return Ok(());
    }
    for entry in journal.files.iter().rev() {
        restore_transaction_target(root, entry)?;
    }
    restore_transaction_target(root, &journal.manifest)?;
    for (relative, expected_hash) in &journal.new_backups {
        let path = safe_state_path(root, relative)?;
        if path.is_file() {
            if sha256::sha256_hex(&read_companion_file(&path)?) != *expected_hash {
                return Err(InstallError::new(
                    "new companion backup changed during interrupted install",
                ));
            }
            fs::remove_file(path)?;
        }
    }
    finish_install_transaction(root)?;
    if !journal.state_existed && state.is_dir() && fs::read_dir(&state)?.next().is_none() {
        fs::remove_dir(state)?;
    }
    if !journal.game_data_existed {
        remove_empty_tree(&root.join("gamedata"))?;
    }
    Ok(())
}

fn restore_transaction_target(root: &Path, entry: &TransactionTarget) -> Result<(), InstallError> {
    let target = safe_game_path(root, &entry.path)?;
    let actual = if target.is_file() {
        Some(sha256::sha256_hex(&read_companion_file(&target)?))
    } else if target.exists() {
        return Err(InstallError::new("transaction target is not a regular file"));
    } else {
        None
    };
    if actual.is_none() && entry.after_sha256.is_some() {
        if let Some(expected) = entry.before_sha256.as_deref() {
            if let Some(previous) = matching_previous_sibling(&target, expected)? {
                if target.exists() {
                    return Err(InstallError::new(
                        "atomic replacement target appeared during transaction recovery",
                    ));
                }
                fs::rename(previous, &target)?;
                if sha256::sha256_hex(&read_companion_file(&target)?) == expected {
                    return Ok(());
                }
                return Err(InstallError::new("atomic replacement sibling changed during recovery"));
            }
        }
    }
    if actual == entry.before_sha256 {
        return Ok(());
    }
    if actual != entry.after_sha256 {
        return Err(InstallError::new(format!(
            "cannot roll back interrupted companion install because {} changed",
            entry.path
        )));
    }
    if let Some(index) = entry.preimage {
        let saved = read_limited(&preimage_path(root, index)?)?;
        if Some(sha256::sha256_hex(&saved)) != entry.before_sha256 {
            return Err(InstallError::new("install transaction preimage hash does not match"));
        }
        write_atomic(&target, &saved)?;
    } else {
        fs::remove_file(target)?;
    }
    Ok(())
}

fn matching_previous_sibling(target: &Path, expected_sha256: &str) -> Result<Option<PathBuf>, InstallError> {
    let Some(parent) = target.parent() else {
        return Ok(None);
    };
    if !parent.is_dir() {
        return Ok(None);
    }
    let Some(file_name) = target.file_name() else {
        return Ok(None);
    };
    let prefix = format!(".{}.previous-", file_name.to_string_lossy());
    let mut matching = None;
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().starts_with(&prefix) || !entry.file_type()?.is_file() {
            continue;
        }
        let candidate = entry.path();
        if sha256::sha256_hex(&read_companion_file(&candidate)?) != expected_sha256 {
            continue;
        }
        if matching.is_some() {
            return Err(InstallError::new(
                "multiple atomic replacement siblings match the transaction preimage",
            ));
        }
        matching = Some(candidate);
    }
    Ok(matching)
}

fn finish_install_transaction(root: &Path) -> Result<(), InstallError> {
    let journal = safe_state_path(root, JOURNAL_FILE)?;
    let transaction_dir = safe_state_path(root, TRANSACTION_DIRECTORY)?;
    if journal.exists() {
        fs::remove_file(journal)?;
    }
    if transaction_dir.exists() {
        fs::remove_dir_all(transaction_dir)?;
    }
    Ok(())
}

fn cleanup_obsolete_backups(root: &Path, backups: &[(String, String)]) -> Result<(), InstallError> {
    for (relative, expected_hash) in backups {
        let Ok(path) = safe_state_path(root, relative) else {
            continue;
        };
        if path.is_file() && read_companion_file(&path).is_ok_and(|bytes| sha256::sha256_hex(&bytes) == *expected_hash)
        {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn parse_flag(value: &str) -> Result<bool, InstallError> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(InstallError::new("install transaction state flag is invalid")),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        output.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    output
}

fn hex_decode(value: &str) -> Result<Vec<u8>, InstallError> {
    if value.len() % 2 != 0 {
        return Err(InstallError::new("transaction hex record has odd length"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair.first().copied()).and_then(hex_value);
            let low = pair.get(1).copied().and_then(hex_value);
            match (high, low) {
                (Some(high), Some(low)) => Ok((high << 4) | low),
                _ => Err(InstallError::new("transaction hex record is invalid")),
            }
        })
        .collect()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => byte.checked_sub(b'0'),
        b'a'..=b'f' => byte.checked_sub(b'a').and_then(|offset| offset.checked_add(10)),
        _ => None,
    }
}

fn verify_managed_files(root: &Path, manifest: &Manifest) -> Result<(), InstallError> {
    for entry in &manifest.files {
        let path = safe_game_path(root, &entry.path)?;
        let bytes = read_companion_file(&path)
            .map_err(|_| InstallError::new("a manifest-owned companion file is missing or exceeds the size limit"))?;
        if sha256::sha256_hex(&bytes) != entry.after_sha256 {
            return Err(InstallError::new(
                "a manifest-owned companion file changed after installation",
            ));
        }
    }
    Ok(())
}

fn safe_game_path(root: &Path, relative: &str) -> Result<PathBuf, InstallError> {
    let normalized = normalize_relative(relative)?;
    let mut path = root.to_path_buf();
    for component in Path::new(&normalized).components() {
        let Component::Normal(part) = component else {
            return Err(InstallError::new("unsafe relative game path"));
        };
        path.push(part);
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(InstallError::new("symlink in a companion game path is not allowed"));
        }
    }
    if !path.starts_with(root) {
        return Err(InstallError::new("companion path escapes the game root"));
    }
    Ok(path)
}

fn safe_state_path(root: &Path, relative: &str) -> Result<PathBuf, InstallError> {
    let normalized = normalize_relative(relative)?;
    safe_game_path(root, &format!("{STATE_DIRECTORY}/{normalized}"))
}

fn normalize_relative(path: &str) -> Result<String, InstallError> {
    let normalized = path.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.chars().any(char::is_control))
    {
        return Err(InstallError::new(
            "companion path is empty, rooted, or traverses outside its root",
        ));
    }
    Ok(normalized)
}

fn validate_game(game: &str) -> Result<(), InstallError> {
    if matches!(game, "soc" | "cs" | "cop" | "s2") {
        Ok(())
    } else {
        Err(InstallError::new("unknown companion game id"))
    }
}

fn ensure_parent(root: &Path, target: &Path) -> Result<(), InstallError> {
    let parent = target
        .parent()
        .ok_or_else(|| InstallError::new("companion target has no parent"))?;
    fs::create_dir_all(parent)?;
    if !parent.starts_with(root) {
        return Err(InstallError::new("companion parent escapes the game root"));
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), InstallError> {
    let parent = path
        .parent()
        .ok_or_else(|| InstallError::new("atomic write path has no parent"))?;
    fs::create_dir_all(parent)?;
    let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .ok_or_else(|| InstallError::new("atomic write path has no file name"))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.pending-{}-{sequence}", std::process::id()));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    let previous = parent.join(format!(".{name}.previous-{}-{sequence}", std::process::id()));
    let had_target = path.exists();
    if had_target {
        fs::rename(path, &previous)?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_target {
            let _ = fs::rename(&previous, path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(InstallError::from(error));
    }
    if had_target {
        fs::remove_file(previous)?;
    }
    Ok(())
}

fn read_limited(path: &Path) -> Result<Vec<u8>, InstallError> {
    read_bounded_file(path, MANIFEST_LIMIT, "companion state file")
}

fn read_companion_file(path: &Path) -> Result<Vec<u8>, InstallError> {
    read_bounded_file(path, crate::MAX_COMPANION_FILE_BYTES, "companion game file")
}

fn read_bounded_file(path: &Path, limit: usize, description: &str) -> Result<Vec<u8>, InstallError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(InstallError::new(format!("{description} is not a regular file")));
    }
    let limit_u64 = u64::try_from(limit).unwrap_or(u64::MAX);
    if metadata.len() > limit_u64 {
        return Err(InstallError::new(format!("{description} exceeds the size limit")));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit_u64.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(InstallError::new(format!("{description} exceeds the size limit")));
    }
    Ok(bytes)
}

fn remove_empty_tree(path: &Path) -> Result<(), InstallError> {
    if !path.is_dir() {
        return Ok(());
    }
    let children = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    for child in children {
        if child.file_type()?.is_dir() {
            remove_empty_tree(&child.path())?;
        }
    }
    if fs::read_dir(path)?.next().is_none() {
        fs::remove_dir(path)?;
    }
    Ok(())
}

fn serialize_manifest(manifest: &Manifest) -> String {
    let mut output = format!(
        "{{\"schemaVersion\":1,\"game\":{},\"version\":{},\"gameDataCreatedFromScratch\":{},\"files\":[",
        quote(&manifest.game),
        quote(&manifest.version),
        manifest.game_data_created
    );
    for (index, entry) in manifest.files.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(&format!("{{\"path\":{},\"kind\":{},\"beforeSha256\":{},\"afterSha256\":{},\"backupPath\":{},\"wasPresentBeforeInstall\":{}}}", quote(&entry.path), quote(&entry.kind), option_string(&entry.before_sha256), quote(&entry.after_sha256), option_string(&entry.backup_path), entry.was_present));
    }
    output.push_str("]}");
    output
}

fn quote(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control <= '\u{1f}' => output.push_str(&format!("\\u{:04x}", u32::from(control))),
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

fn option_string(value: &Option<String>) -> String {
    value.as_deref().map_or_else(|| "null".to_owned(), quote)
}

#[derive(Debug)]
enum JsonValue {
    Object(BTreeMap<String, JsonValue>),
    Array(Vec<JsonValue>),
    String(String),
    Number(String),
    Bool(bool),
    Null,
}

fn parse_manifest(bytes: &[u8], expected_game: &str) -> Result<Manifest, InstallError> {
    let mut reader = Reader::new(bytes);
    let first = next_event(&mut reader)?;
    let root = parse_json_value(&mut reader, first)?;
    if reader
        .next_event()
        .map_err(|error| InstallError::new(error.to_string()))?
        .is_some()
    {
        return Err(InstallError::new("trailing JSON after companion manifest"));
    }
    let JsonValue::Object(mut object) = root else {
        return Err(InstallError::new("companion manifest must be an object"));
    };
    let schema = take_number(&mut object, "schemaVersion")?;
    let game = take_string(&mut object, "game")?;
    let version = take_string(&mut object, "version")?;
    let game_data_created = take_bool(&mut object, "gameDataCreatedFromScratch")?;
    let files_value = object
        .remove("files")
        .ok_or_else(|| InstallError::new("manifest files field is missing"))?;
    if schema != 1 || game != expected_game {
        return Err(InstallError::new("companion manifest schema or game id is unsupported"));
    }
    let JsonValue::Array(values) = files_value else {
        return Err(InstallError::new("manifest files field must be an array"));
    };
    let mut files = Vec::with_capacity(values.len());
    let mut seen = BTreeSet::new();
    for value in values {
        let JsonValue::Object(mut entry) = value else {
            return Err(InstallError::new("manifest file entry must be an object"));
        };
        let path = normalize_relative(&take_string(&mut entry, "path")?)?;
        let kind = take_string(&mut entry, "kind")?;
        let before_sha256 = take_optional_string(&mut entry, "beforeSha256")?;
        let after_sha256 = take_string(&mut entry, "afterSha256")?;
        let backup_path = take_optional_string(&mut entry, "backupPath")?
            .map(|value| normalize_relative(&value))
            .transpose()?;
        let was_present = take_bool(&mut entry, "wasPresentBeforeInstall")?;
        if !seen.insert(path.clone())
            || !is_hash(&after_sha256)
            || before_sha256.as_ref().is_some_and(|value| !is_hash(value))
            || !matches!(kind.as_str(), "copy" | "modified")
        {
            return Err(InstallError::new(
                "companion manifest contains an invalid or duplicate file entry",
            ));
        }
        if before_sha256.is_some() != backup_path.is_some() || (was_present && before_sha256.is_none()) {
            return Err(InstallError::new("companion manifest backup metadata is inconsistent"));
        }
        files.push(ManifestFile {
            path,
            kind,
            before_sha256,
            after_sha256,
            backup_path,
            was_present,
        });
    }
    Ok(Manifest {
        game,
        version,
        game_data_created,
        files,
    })
}

fn next_event<'a>(reader: &mut Reader<'a>) -> Result<Event<'a>, InstallError> {
    reader
        .next_event()
        .map_err(|error| InstallError::new(error.to_string()))?
        .ok_or_else(|| InstallError::new("companion manifest is truncated"))
}

fn parse_json_value<'a>(reader: &mut Reader<'a>, event: Event<'a>) -> Result<JsonValue, InstallError> {
    match event {
        Event::ObjectStart => {
            let mut values = BTreeMap::new();
            loop {
                match next_event(reader)? {
                    Event::ObjectEnd => break,
                    Event::Key(key) => {
                        let key = key.into_owned();
                        let value_event = next_event(reader)?;
                        let value = parse_json_value(reader, value_event)?;
                        if values.insert(key, value).is_some() {
                            return Err(InstallError::new("duplicate JSON property in companion state"));
                        }
                    }
                    _ => return Err(InstallError::new("invalid companion JSON object")),
                }
            }
            Ok(JsonValue::Object(values))
        }
        Event::ArrayStart => {
            let mut values = Vec::new();
            loop {
                let event = next_event(reader)?;
                if event == Event::ArrayEnd {
                    break;
                }
                values.push(parse_json_value(reader, event)?);
            }
            Ok(JsonValue::Array(values))
        }
        Event::String(value) => Ok(JsonValue::String(text_owned(value))),
        Event::Number(number) => Ok(JsonValue::Number(number.to_owned())),
        Event::Bool(value) => Ok(JsonValue::Bool(value)),
        Event::Null => Ok(JsonValue::Null),
        _ => Err(InstallError::new("unexpected JSON token in companion state")),
    }
}

fn text_owned(value: Text<'_>) -> String {
    value.into_owned()
}
fn take_string(object: &mut BTreeMap<String, JsonValue>, key: &str) -> Result<String, InstallError> {
    match object.remove(key) {
        Some(JsonValue::String(value)) => Ok(value),
        _ => Err(InstallError::new(format!("manifest {key} field must be a string"))),
    }
}
fn take_optional_string(object: &mut BTreeMap<String, JsonValue>, key: &str) -> Result<Option<String>, InstallError> {
    match object.remove(key) {
        Some(JsonValue::String(value)) => Ok(Some(value)),
        Some(JsonValue::Null) => Ok(None),
        _ => Err(InstallError::new(format!(
            "manifest {key} field must be a string or null"
        ))),
    }
}
fn take_bool(object: &mut BTreeMap<String, JsonValue>, key: &str) -> Result<bool, InstallError> {
    match object.remove(key) {
        Some(JsonValue::Bool(value)) => Ok(value),
        _ => Err(InstallError::new(format!("manifest {key} field must be boolean"))),
    }
}
fn take_number(object: &mut BTreeMap<String, JsonValue>, key: &str) -> Result<u32, InstallError> {
    match object.remove(key) {
        Some(JsonValue::Number(value)) => value
            .parse()
            .map_err(|_| InstallError::new(format!("manifest {key} field is invalid"))),
        _ => Err(InstallError::new(format!("manifest {key} field must be a number"))),
    }
}
fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[allow(clippy::expect_used)] // These tests use expect only to fail fast on temporary-fixture setup errors.
mod transaction_tests {
    use super::{
        begin_install_transaction, begin_uninstall_transaction, install_bundled, read_limited,
        recover_install_transaction, PayloadFile, PlannedFileChange, JOURNAL_FILE, STATE_DIRECTORY,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn root() -> PathBuf {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |value| value.as_nanos());
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sse-c6-journal-{}-{time}-{sequence}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    #[test]
    fn interrupted_install_restores_changed_bytes_and_removes_new_files() {
        let root = root();
        fs::create_dir_all(root.join("gamedata/scripts")).expect("create payload directory");
        fs::write(root.join("gamedata/scripts/user.script"), b"original").expect("write original");
        let payloads = [
            PayloadFile::new("gamedata/scripts/user.script", b"installed".to_vec()),
            PayloadFile::new("gamedata/scripts/new.script", b"new".to_vec()),
        ];
        let changes = payloads
            .iter()
            .map(|file| PlannedFileChange {
                path: file.relative_path.clone(),
                after: Some(file.bytes.clone()),
            })
            .collect::<Vec<_>>();
        begin_install_transaction(&root, false, true, None, b"new manifest", &changes, &[], &[])
            .expect("write transaction journal");
        fs::write(root.join("gamedata/scripts/user.script"), b"installed").expect("simulate replacement");
        fs::write(root.join("gamedata/scripts/new.script"), b"new").expect("simulate new file");

        recover_install_transaction(&root).expect("roll back partial install");

        assert_eq!(
            fs::read(root.join("gamedata/scripts/user.script")).expect("read restored bytes"),
            b"original"
        );
        assert!(!root.join("gamedata/scripts/new.script").exists());
        assert!(!root.join(STATE_DIRECTORY).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interrupted_atomic_replace_restores_matching_previous_sibling() {
        let root = root();
        let target = root.join("gamedata/scripts/user.script");
        fs::create_dir_all(target.parent().expect("target parent")).expect("create target directory");
        fs::write(&target, b"original").expect("write original");
        let change = PlannedFileChange {
            path: "gamedata/scripts/user.script".to_owned(),
            after: Some(b"installed".to_vec()),
        };
        begin_install_transaction(&root, false, true, None, b"new manifest", &[change], &[], &[])
            .expect("write transaction journal");
        let previous = target
            .parent()
            .expect("target parent")
            .join(".user.script.previous-simulated");
        fs::rename(&target, &previous).expect("simulate crash after moving old target");

        recover_install_transaction(&root).expect("recover old target from its atomic sibling");

        assert_eq!(fs::read(&target).expect("read restored file"), b"original");
        assert!(!previous.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_install_recovers_before_reading_a_target_moved_to_previous() {
        let root = root();
        let bind_path = root.join("gamedata/scripts/bind_stalker.script");
        let menu_path = root.join("gamedata/scripts/ui_main_menu.script");
        fs::create_dir_all(bind_path.parent().expect("bind parent")).expect("create game scripts directory");
        fs::write(
            &bind_path,
            b"function bind:update()\n\tobject_binder.update(self, delta)\nend\nself.object:set_callback(callback.on_item_drop, self.on_item_drop, self)\n",
        )
        .expect("write bind source");
        fs::write(
            &menu_path,
            b"function main_menu:OnKeyboard(dik, keyboard_action)\n\tif keyboard_action == ui_events.WINDOW_KEY_PRESSED then\n\t\treturn true\n\tend\n\treturn false\nend\n",
        )
        .expect("write menu source");
        let game = crate::bundled::Game::ClearSky;
        install_bundled(&root, game).expect("initial bundled install");
        let manifest_path = root.join(STATE_DIRECTORY).join("manifest.json");
        let manifest = read_limited(&manifest_path).expect("read existing manifest");
        let change = PlannedFileChange {
            path: "gamedata/scripts/bind_stalker.script".to_owned(),
            after: Some(b"replacement".to_vec()),
        };
        begin_install_transaction(
            &root,
            true,
            true,
            Some(&manifest),
            b"next manifest",
            &[change],
            &[],
            &[],
        )
        .expect("begin interrupted update");
        let previous = bind_path
            .parent()
            .expect("bind parent")
            .join(".bind_stalker.script.previous-simulated");
        fs::rename(&bind_path, &previous).expect("simulate interruption during replace");

        install_bundled(&root, game).expect("recover and retry bundle install");

        assert!(fs::read(&bind_path)
            .expect("read installed script")
            .windows(b"save_editor_companion then save_editor_companion.update()".len())
            .any(|window| window == b"save_editor_companion then save_editor_companion.update()"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interrupted_install_refuses_to_overwrite_a_third_party_edit() {
        let root = root();
        fs::create_dir_all(root.join("gamedata/scripts")).expect("create payload directory");
        fs::write(root.join("gamedata/scripts/user.script"), b"original").expect("write original");
        let payloads = [PayloadFile::new("gamedata/scripts/user.script", b"installed".to_vec())];
        let changes = payloads
            .iter()
            .map(|file| PlannedFileChange {
                path: file.relative_path.clone(),
                after: Some(file.bytes.clone()),
            })
            .collect::<Vec<_>>();
        begin_install_transaction(&root, false, true, None, b"new manifest", &changes, &[], &[])
            .expect("write transaction journal");
        fs::write(root.join("gamedata/scripts/user.script"), b"third party").expect("simulate external edit");

        assert!(recover_install_transaction(&root).is_err());
        assert_eq!(
            fs::read(root.join("gamedata/scripts/user.script")).expect("read external edit"),
            b"third party"
        );
        assert!(root.join(STATE_DIRECTORY).join(JOURNAL_FILE).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interrupted_uninstall_rolls_back_before_manifest_removal_and_finishes_after_it() {
        let root = root();
        let target = root.join("gamedata/scripts/user.script");
        fs::create_dir_all(target.parent().expect("target parent")).expect("create target directory");
        fs::write(&target, b"installed").expect("write installed file");
        let state = root.join(STATE_DIRECTORY);
        fs::create_dir_all(&state).expect("create state directory");
        let manifest = b"manifest bytes";
        fs::write(state.join("manifest.json"), manifest).expect("write manifest");
        let restore_plan = [(
            "gamedata/scripts/user.script".to_owned(),
            target.clone(),
            Some(b"original".to_vec()),
        )];
        begin_uninstall_transaction(&root, manifest, &restore_plan, false).expect("write uninstall journal");
        fs::write(&target, b"original").expect("simulate restored game file");

        recover_install_transaction(&root).expect("undo interrupted uninstall");

        assert_eq!(fs::read(&target).expect("read rolled back file"), b"installed");
        assert_eq!(
            fs::read(state.join("manifest.json")).expect("read restored manifest"),
            manifest
        );

        begin_uninstall_transaction(&root, manifest, &restore_plan, false).expect("write second journal");
        fs::write(&target, b"original").expect("simulate restored game file");
        fs::remove_file(state.join("manifest.json")).expect("simulate uninstall commit point");

        recover_install_transaction(&root).expect("finish committed uninstall");

        assert_eq!(fs::read(&target).expect("read final original file"), b"original");
        assert!(!state.exists());
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod g14_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn archived_hook_file_can_be_read_from_content_tree() {
        let mut files = HashMap::new();
        files.insert(
            "scripts/bind_stalker.script".to_owned(),
            sse_content::GameFile::from_bytes(
                "scripts/bind_stalker.script",
                "fixture.db",
                b"function actor_binder:update(delta) end".to_vec(),
            ),
        );
        let tree = GameFileTree {
            files,
            fingerprint: "fixture".to_owned(),
            has_loose_overlay: false,
            config_prefix: "config/".to_owned(),
            data_directory: None,
            issues: Vec::new(),
        };
        let bytes = tree
            .files
            .get("scripts/bind_stalker.script")
            .and_then(|file| file.read().ok());
        assert_eq!(
            bytes.as_deref(),
            Some(b"function actor_binder:update(delta) end".as_slice())
        );
    }

    #[test]
    fn overlap_check_is_case_insensitive_after_normalization() -> Result<(), InstallError> {
        let payload = PayloadFile::new("GameData/Scripts/bind_stalker.script", vec![1]);
        let normalized = normalize_relative(&payload.relative_path)?;
        assert_eq!(normalized.to_ascii_lowercase(), "gamedata/scripts/bind_stalker.script");
        Ok(())
    }
}
