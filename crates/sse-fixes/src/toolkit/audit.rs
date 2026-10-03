//! Installation file audit classifying game files into vanilla, managed, orphaned, and custom.
//!
//! Reference: `Core/Diagnostics/ToolkitInstallAudit.cs`.
//! All operations take the game installation path explicitly.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::GameFixEngine;
use crate::fs_util::check_no_links;
use crate::models::GameTarget;
use sse_core::{Error, Result};

/// Classification category of a file in the game directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileClassification {
    /// File is tracked by an active Game Fix manifest or journal.
    ToolkitManaged,
    /// File was created by the toolkit in a previous session or backup, but is no longer managed.
    OrphanedToolkitOwned,
    /// An orphaned file that indicates an interrupted or conflicted state needing review.
    OrphanedStateNeedsReview,
    /// Loose user modification not installed or owned by the toolkit.
    CustomUserMod,
}

/// An individual audited file entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditItem {
    /// Path relative to the game directory.
    pub relative_path: String,
    /// Classification.
    pub classification: FileClassification,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Explanatory note or owning fix identifier.
    pub details: String,
}

/// Comprehensive audit report of the game directory environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolkitAuditReport {
    /// Targeted game.
    pub game: GameTarget,
    /// Total count of inspected loose files.
    pub total_scanned: usize,
    /// Count of actively managed files.
    pub managed_count: usize,
    /// Count of orphaned toolkit files.
    pub orphaned_count: usize,
    /// Count of custom user mod files.
    pub custom_mod_count: usize,
    /// Count of files requiring review.
    pub needs_review_count: usize,
    /// Detailed list of audited items.
    pub items: Vec<AuditItem>,
}

impl ToolkitAuditReport {
    /// Returns true if there are no orphaned or review-needed files.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.orphaned_count == 0 && self.needs_review_count == 0
    }
}

/// Service auditing game directory contents.
pub struct ToolkitInstallAudit;

impl ToolkitInstallAudit {
    /// Audits all loose game files in the given game installation.
    ///
    /// # Errors
    /// Returns an error if directory traversal fails or symlinks are encountered.
    pub fn audit_installation(
        game_directory: &Path,
        game: GameTarget,
        engine: &GameFixEngine,
    ) -> Result<ToolkitAuditReport> {
        check_no_links(game_directory, game_directory)?;

        // 1. Gather all currently managed relative file paths
        let installed = engine.list_installed(game_directory, None)?;
        let mut managed_paths = BTreeSet::new();
        let mut active_fix_ids = BTreeSet::new();

        for fix_info in installed {
            active_fix_ids.insert(fix_info.id.clone());
            if let Ok(manifest) = engine.get_manifest(&fix_info.id, game_directory) {
                for file_entry in manifest.files {
                    managed_paths.insert(file_entry.relative_path);
                }
            }
        }

        let mut items = Vec::new();
        let mut total_scanned: usize = 0;
        let mut managed_count: usize = 0;
        let mut orphaned_count: usize = 0;
        let mut custom_mod_count: usize = 0;
        let mut needs_review_count: usize = 0;

        // 2. Scan gamedata/ directory if it exists
        let gamedata_dir = game_directory.join("gamedata");
        if gamedata_dir.is_dir() {
            let files = collect_files_recursive(&gamedata_dir)?;
            for abs_path in files {
                total_scanned = total_scanned.saturating_add(1);
                let rel_path = match abs_path.strip_prefix(game_directory) {
                    Ok(p) => normalize_path_str(p),
                    Err(_) => continue,
                };
                let meta = fs::metadata(&abs_path).map_err(Error::from)?;
                let size_bytes = meta.len();

                if managed_paths.contains(&rel_path) {
                    managed_count = managed_count.saturating_add(1);
                    items.push(AuditItem {
                        relative_path: rel_path,
                        classification: FileClassification::ToolkitManaged,
                        size_bytes,
                        details: "Actively managed by an installed Game Fix".to_string(),
                    });
                } else if rel_path.ends_with(".sse-backup") || rel_path.ends_with(".sse-orig") {
                    orphaned_count = orphaned_count.saturating_add(1);
                    items.push(AuditItem {
                        relative_path: rel_path,
                        classification: FileClassification::OrphanedToolkitOwned,
                        size_bytes,
                        details: "Toolkit backup file not referenced by current active fixes".to_string(),
                    });
                } else {
                    custom_mod_count = custom_mod_count.saturating_add(1);
                    items.push(AuditItem {
                        relative_path: rel_path,
                        classification: FileClassification::CustomUserMod,
                        size_bytes,
                        details: "Custom loose mod file placed in gamedata".to_string(),
                    });
                }
            }
        }

        // 3. Scan .sse/ internal state directory
        let sse_dir = game_directory.join(".sse");
        if sse_dir.is_dir() {
            let sse_files = collect_files_recursive(&sse_dir)?;
            for abs_path in sse_files {
                total_scanned = total_scanned.saturating_add(1);
                let rel_path = match abs_path.strip_prefix(game_directory) {
                    Ok(p) => normalize_path_str(p),
                    Err(_) => continue,
                };
                let meta = fs::metadata(&abs_path).map_err(Error::from)?;
                let size_bytes = meta.len();

                // Check if inside .sse/fixes/<id>
                if let Some(fix_id) = extract_fix_id_from_sse_path(&rel_path) {
                    if active_fix_ids.contains(&fix_id) {
                        managed_count = managed_count.saturating_add(1);
                        items.push(AuditItem {
                            relative_path: rel_path,
                            classification: FileClassification::ToolkitManaged,
                            size_bytes,
                            details: format!("Active fix state for '{fix_id}'"),
                        });
                    } else {
                        orphaned_count = orphaned_count.saturating_add(1);
                        items.push(AuditItem {
                            relative_path: rel_path,
                            classification: FileClassification::OrphanedToolkitOwned,
                            size_bytes,
                            details: format!("Orphaned state for uninstalled fix '{fix_id}'"),
                        });
                    }
                } else if rel_path.ends_with(".lock") || rel_path.ends_with(".tmp") {
                    needs_review_count = needs_review_count.saturating_add(1);
                    items.push(AuditItem {
                        relative_path: rel_path,
                        classification: FileClassification::OrphanedStateNeedsReview,
                        size_bytes,
                        details: "Interrupted temporary or lock file".to_string(),
                    });
                } else {
                    // Other toolkit files (snapshots, profiles, etc.)
                    items.push(AuditItem {
                        relative_path: rel_path,
                        classification: FileClassification::ToolkitManaged,
                        size_bytes,
                        details: "Toolkit environment record".to_string(),
                    });
                }
            }
        }

        items.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        Ok(ToolkitAuditReport {
            game,
            total_scanned,
            managed_count,
            orphaned_count,
            custom_mod_count,
            needs_review_count,
            items,
        })
    }

    /// Safely cleans up orphaned toolkit-owned backup and temporary files.
    ///
    /// Never touches custom user mods or actively managed files.
    ///
    /// # Errors
    /// Returns an error if removing an orphaned file fails.
    pub fn cleanup_orphans(game_directory: &Path, audit_report: &ToolkitAuditReport) -> Result<usize> {
        let mut removed_count: usize = 0;
        for item in &audit_report.items {
            if matches!(
                item.classification,
                FileClassification::OrphanedToolkitOwned | FileClassification::OrphanedStateNeedsReview
            ) {
                let full_path = game_directory.join(&item.relative_path);
                if full_path.is_file() {
                    fs::remove_file(&full_path).map_err(Error::from)?;
                    removed_count = removed_count.saturating_add(1);
                }
            }
        }
        Ok(removed_count)
    }
}

fn collect_files_recursive(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir).map_err(Error::from)?;
    for entry in entries {
        let entry = entry.map_err(Error::from)?;
        let path = entry.path();
        if path.is_dir() {
            let mut sub = collect_files_recursive(&path)?;
            files.append(&mut sub);
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(files)
}

fn normalize_path_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn extract_fix_id_from_sse_path(rel_path: &str) -> Option<String> {
    let parts: Vec<&str> = rel_path.split('/').collect();
    if parts.len() >= 3 && parts.first() == Some(&".sse") && parts.get(1) == Some(&"fixes") {
        return parts.get(2).map(|s| (*s).to_string());
    }
    None
}
