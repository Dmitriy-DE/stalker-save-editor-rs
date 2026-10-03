//! Models and enumerations for the game fixes catalogue and engine.

use std::fmt;

/// Target game release or edition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GameTarget {
    /// S.T.A.L.K.E.R.: Shadow of Chernobyl (retail)
    ShadowOfChernobyl,
    /// S.T.A.L.K.E.R.: Clear Sky (retail)
    ClearSky,
    /// S.T.A.L.K.E.R.: Call of Pripyat (retail)
    CallOfPripyat,
    /// S.T.A.L.K.E.R.: Shadow of Chornobyl Enhanced Edition
    ShadowOfChernobylEnhancedEdition,
    /// S.T.A.L.K.E.R.: Clear Sky Enhanced Edition
    ClearSkyEnhancedEdition,
    /// S.T.A.L.K.E.R.: Call of Prypiat Enhanced Edition
    CallOfPripyatEnhancedEdition,
    /// S.T.A.L.K.E.R. 2: Heart of Chornobyl
    Stalker2,
}

impl GameTarget {
    /// All game targets in canonical order.
    pub const ALL: [Self; 7] = [
        Self::ShadowOfChernobyl,
        Self::ClearSky,
        Self::CallOfPripyat,
        Self::ShadowOfChernobylEnhancedEdition,
        Self::ClearSkyEnhancedEdition,
        Self::CallOfPripyatEnhancedEdition,
        Self::Stalker2,
    ];

    /// Short identifier for the game target (e.g. `"soc"`, `"cs-ee"`).
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::ShadowOfChernobyl => "soc",
            Self::ClearSky => "cs",
            Self::CallOfPripyat => "cop",
            Self::ShadowOfChernobylEnhancedEdition => "soc-ee",
            Self::ClearSkyEnhancedEdition => "cs-ee",
            Self::CallOfPripyatEnhancedEdition => "cop-ee",
            Self::Stalker2 => "s2",
        }
    }

    /// Full human-readable title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::ShadowOfChernobyl => "Shadow of Chernobyl",
            Self::ClearSky => "Clear Sky",
            Self::CallOfPripyat => "Call of Pripyat",
            Self::ShadowOfChernobylEnhancedEdition => "Shadow of Chornobyl Enhanced Edition",
            Self::ClearSkyEnhancedEdition => "Clear Sky Enhanced Edition",
            Self::CallOfPripyatEnhancedEdition => "Call of Prypiat Enhanced Edition",
            Self::Stalker2 => "S.T.A.L.K.E.R. 2",
        }
    }

    /// Steam application ID if distributed on Steam.
    #[must_use]
    pub const fn steam_app_id(self) -> Option<u32> {
        match self {
            Self::ShadowOfChernobyl => Some(4_500),
            Self::ClearSky => Some(20_510),
            Self::CallOfPripyat => Some(41_700),
            Self::ShadowOfChernobylEnhancedEdition => Some(2_427_410),
            Self::ClearSkyEnhancedEdition => Some(2_427_420),
            Self::CallOfPripyatEnhancedEdition => Some(2_427_430),
            Self::Stalker2 => Some(1_643_320),
        }
    }

    /// Returns true if this game is based on the X-Ray engine.
    #[must_use]
    pub const fn is_xray(self) -> bool {
        !matches!(self, Self::Stalker2)
    }

    /// Returns true if Companion mod is supported.
    #[must_use]
    pub const fn companion_supported(self) -> bool {
        matches!(self, Self::ShadowOfChernobyl | Self::ClearSky | Self::CallOfPripyat)
    }

    /// Parses a game target from its short id or JSON name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.id().eq_ignore_ascii_case(s) || t.json_name().eq_ignore_ascii_case(s))
    }

    /// JSON serialized name matching the C# enum.
    #[must_use]
    pub const fn json_name(self) -> &'static str {
        match self {
            Self::ShadowOfChernobyl => "ShadowOfChernobyl",
            Self::ClearSky => "ClearSky",
            Self::CallOfPripyat => "CallOfPripyat",
            Self::ShadowOfChernobylEnhancedEdition => "ShadowOfChernobylEnhancedEdition",
            Self::ClearSkyEnhancedEdition => "ClearSkyEnhancedEdition",
            Self::CallOfPripyatEnhancedEdition => "CallOfPripyatEnhancedEdition",
            Self::Stalker2 => "Stalker2",
        }
    }
}

impl fmt::Display for GameTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.id())
    }
}

/// Category of a game fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GameFixCategory {
    /// Essential fix for crashes or progression blockers.
    Essential,
    /// Recommended bug fix safe for general play.
    Recommended,
    /// Optional fix for minor visual or behavioral issues.
    Optional,
    /// Community restored or rebalanced content.
    Community,
    /// Experimental fix undergoing testing.
    Experimental,
}

impl GameFixCategory {
    /// All categories.
    pub const ALL: [Self; 5] = [
        Self::Essential,
        Self::Recommended,
        Self::Optional,
        Self::Community,
        Self::Experimental,
    ];

    /// Name as serialized in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Essential => "Essential",
            Self::Recommended => "Recommended",
            Self::Optional => "Optional",
            Self::Community => "Community",
            Self::Experimental => "Experimental",
        }
    }

    /// Parses a category from string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Essential" | "essential" => Some(Self::Essential),
            "Recommended" | "recommended" => Some(Self::Recommended),
            "Optional" | "optional" => Some(Self::Optional),
            "Community" | "community" => Some(Self::Community),
            "Experimental" | "experimental" => Some(Self::Experimental),
            _ => None,
        }
    }
}

impl fmt::Display for GameFixCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Maturity level of a game fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GameFixMaturity {
    /// Validated against retail files.
    Validated,
    /// Experimental fix.
    Experimental,
    /// Research-only inspection stub.
    ResearchOnly,
}

impl GameFixMaturity {
    /// Name as serialized in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Validated => "Validated",
            Self::Experimental => "Experimental",
            Self::ResearchOnly => "ResearchOnly",
        }
    }

    /// Parses a maturity level from string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Validated" | "validated" => Some(Self::Validated),
            "Experimental" | "experimental" => Some(Self::Experimental),
            "ResearchOnly" | "researchOnly" | "research-only" => Some(Self::ResearchOnly),
            _ => None,
        }
    }
}

impl fmt::Display for GameFixMaturity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// State of a fix within a game installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameFixState {
    /// Not currently installed.
    NotInstalled,
    /// Installed and matching expected hashes.
    Installed,
    /// Uninstalled with manifest preserved.
    Removed,
    /// Managed files were modified externally.
    Modified,
}

impl GameFixState {
    /// Name as serialized in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotInstalled => "NotInstalled",
            Self::Installed => "Installed",
            Self::Removed => "Removed",
            Self::Modified => "Modified",
        }
    }

    /// Parses a state from string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "NotInstalled" | "notInstalled" | "not_installed" => Some(Self::NotInstalled),
            "Installed" | "installed" => Some(Self::Installed),
            "Removed" | "removed" => Some(Self::Removed),
            "Modified" | "modified" => Some(Self::Modified),
            _ => None,
        }
    }
}

impl fmt::Display for GameFixState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Type of patch implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameFixImplementationType {
    /// Exact text anchor replacement.
    ExactTextReplacement,
    /// Structured binary edit (all.spawn).
    Structured,
    /// Direct byte patch.
    BinaryPatch,
    /// Whole-file overlay.
    Overlay,
}

impl GameFixImplementationType {
    /// Name as serialized in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExactTextReplacement => "ExactTextReplacement",
            Self::Structured => "Structured",
            Self::BinaryPatch => "BinaryPatch",
            Self::Overlay => "Overlay",
        }
    }

    /// Parses an implementation type from string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ExactTextReplacement" => Some(Self::ExactTextReplacement),
            "Structured" => Some(Self::Structured),
            "BinaryPatch" => Some(Self::BinaryPatch),
            "Overlay" => Some(Self::Overlay),
            _ => None,
        }
    }
}

/// Verification state of a fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameFixVerificationState {
    /// Research only.
    Research,
    /// Verified with synthetic test fixtures.
    SyntheticTests,
    /// Verified against official retail game files.
    RetailFilesVerified,
    /// Verified in-game.
    InGameVerified,
    /// Issue successfully reproduced.
    IssueReproduced,
}

impl GameFixVerificationState {
    /// Name as serialized in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Research => "Research",
            Self::SyntheticTests => "SyntheticTests",
            Self::RetailFilesVerified => "RetailFilesVerified",
            Self::InGameVerified => "InGameVerified",
            Self::IssueReproduced => "IssueReproduced",
        }
    }

    /// Parses verification state from string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Research" => Some(Self::Research),
            "SyntheticTests" => Some(Self::SyntheticTests),
            "RetailFilesVerified" => Some(Self::RetailFilesVerified),
            "InGameVerified" => Some(Self::InGameVerified),
            "IssueReproduced" => Some(Self::IssueReproduced),
            _ => None,
        }
    }
}

/// Save file compatibility classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameFixSaveCompatibility {
    /// Compatibility unknown.
    Unknown,
    /// Compatible with existing save files.
    ExistingSaves,
    /// Requires starting a new game.
    NewGameRequired,
    /// Incompatible with existing saves.
    Incompatible,
}

impl GameFixSaveCompatibility {
    /// Name as serialized in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::ExistingSaves => "ExistingSaves",
            Self::NewGameRequired => "NewGameRequired",
            Self::Incompatible => "Incompatible",
        }
    }

    /// Parses save compatibility from string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Unknown" => Some(Self::Unknown),
            "ExistingSaves" => Some(Self::ExistingSaves),
            "NewGameRequired" => Some(Self::NewGameRequired),
            "Incompatible" => Some(Self::Incompatible),
            _ => None,
        }
    }
}

/// Preset selection of fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameFixPreset {
    /// Essential fixes only.
    EssentialOnly,
    /// Recommended fixes (essential + recommended).
    Recommended,
    /// All safe fixes.
    AllSafeFixes,
    /// Custom selection.
    Custom,
}

impl GameFixPreset {
    /// Parses a preset from CLI argument.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "essential" | "essential-only" => Some(Self::EssentialOnly),
            "recommended" => Some(Self::Recommended),
            "all-safe" | "all-safe-fixes" => Some(Self::AllSafeFixes),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// Name as serialized in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EssentialOnly => "EssentialOnly",
            Self::Recommended => "Recommended",
            Self::AllSafeFixes => "AllSafeFixes",
            Self::Custom => "Custom",
        }
    }
}

impl fmt::Display for GameFixPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// One text replacement operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPatchOperation {
    /// Relative path from game root (e.g. `"gamedata/configs/creatures/spawn_sections_garbage.ltx"`).
    pub relative_path: String,
    /// Exact text anchor to find in the source file.
    pub expected_text: String,
    /// Replacement text to substitute.
    pub replacement_text: String,
    /// Expected SHA-256 of the source file before patching (lowercase hex).
    pub expected_file_sha256: Option<String>,
    /// Single-byte code page (28591 = Latin-1, 1251 = Windows-1251).
    pub code_page: u32,
    /// True if this patch applies only to the retail build and is already fixed in Enhanced Edition.
    pub retail_only: bool,
}

/// Whole-file overlay operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOverlayOperation {
    /// Relative path from game root.
    pub relative_path: String,
    /// Content SHA-256 hash in content store.
    pub content_sha256: String,
    /// Expected SHA-256 of the existing file (or None if the file must not exist before).
    pub expected_file_sha256: Option<String>,
}

/// Kind of structural all.spawn edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpawnEditKind {
    /// Patrol waypoint edit.
    PatrolPoint,
    /// Spawn object custom data edit.
    CustomData,
}

impl SpawnEditKind {
    /// Name as serialized in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PatrolPoint => "PatrolPoint",
            Self::CustomData => "CustomData",
        }
    }

    /// Parses from string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "PatrolPoint" => Some(Self::PatrolPoint),
            "CustomData" => Some(Self::CustomData),
            _ => None,
        }
    }
}

/// Structural edit of an X-Ray all.spawn file.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnEditOperation {
    /// Path to all.spawn (e.g. `"gamedata/spawns/all.spawn"`).
    pub relative_path: String,
    /// Edit kind (patrol waypoint or custom data).
    pub kind: SpawnEditKind,
    /// Target path or object name.
    pub target: String,
    /// Expected SHA-256 of the all.spawn file.
    pub expected_file_sha256: Option<String>,
    /// Waypoint index for patrol points.
    pub point: usize,
    /// Current expected text.
    pub expected: String,
    /// Replacement text (or None if keeping current name).
    pub replacement: Option<String>,
    /// Optional updated 3D coordinates.
    pub position: Option<[f32; 3]>,
    /// Optional level vertex ID.
    pub level_vertex_id: Option<u32>,
    /// Optional game vertex ID.
    pub game_vertex_id: Option<u16>,
}

/// Full definition of a game fix from the catalogue.
#[derive(Debug, Clone, PartialEq)]
pub struct GameFixDefinition {
    /// Unique identifier (e.g. `"cs.quest.dead-wild-napr"`).
    pub id: String,
    /// Target game release.
    pub game: GameTarget,
    /// Version string.
    pub version: String,
    /// Display title.
    pub title: String,
    /// Supported Steam build IDs.
    pub supported_steam_build_ids: Vec<String>,
    /// Category.
    pub category: GameFixCategory,
    /// Maturity level.
    pub maturity: GameFixMaturity,
    /// Fix IDs that must be installed before this fix.
    pub depends_on: Vec<String>,
    /// Conflicting fix IDs.
    pub conflicts_with: Vec<String>,
    /// Text replacement operations.
    pub text_patches: Vec<TextPatchOperation>,
    /// Provenance / author credits.
    pub source: String,
    /// Problem explanation.
    pub problem: String,
    /// Description of the fix.
    pub description: String,
    /// Implementation type.
    pub implementation: GameFixImplementationType,
    /// True if starting a new game is required.
    pub requires_new_game: bool,
    /// Save file compatibility.
    pub save_compatibility: GameFixSaveCompatibility,
    /// Verification state.
    pub verification_state: GameFixVerificationState,
    /// Detection method description.
    pub detection_method: String,
    /// External documentation and provenance URLs.
    pub references: Vec<String>,
    /// Whole-file overlays.
    pub overlays: Vec<FileOverlayOperation>,
    /// Structural all.spawn edits.
    pub spawn_edits: Vec<SpawnEditOperation>,
}

/// Reason a game fix operation failed or was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameFixFailure {
    /// Precondition failed or refused.
    Refused,
    /// I/O error while reading or writing.
    Io,
    /// Operation failed and rollback was incomplete.
    RollbackIncomplete,
}

/// Result of installing, updating, or removing a fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFixInstallResult {
    /// True if files were changed on disk.
    pub changed: bool,
    /// Resulting state.
    pub state: GameFixState,
    /// Managed relative file paths.
    pub files: Vec<String>,
}

/// Result of pre-flight check before uninstalling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFixUninstallCheck {
    /// True if uninstall can proceed safely.
    pub can_uninstall: bool,
    /// Reason if cannot uninstall.
    pub reason: Option<String>,
    /// Managed file paths.
    pub files: Vec<String>,
}

/// Result of applying a preset of fixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFixPresetResult {
    /// The applied preset.
    pub preset: GameFixPreset,
    /// Number of fixes selected by the preset.
    pub selected_fix_count: usize,
    /// IDs of fixes newly installed.
    pub installed_fix_ids: Vec<String>,
    /// IDs of fixes already current before the operation.
    pub already_installed_fix_ids: Vec<String>,
}

impl GameFixPresetResult {
    /// True if any files were changed.
    #[must_use]
    pub fn changed(&self) -> bool {
        !self.installed_fix_ids.is_empty()
    }
}

/// Information about an installed fix in a game directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFixInstalledInfo {
    /// Fix identifier.
    pub id: String,
    /// Target game.
    pub game: GameTarget,
    /// Installed version.
    pub version: String,
    /// Display title.
    pub title: String,
    /// Category.
    pub category: GameFixCategory,
    /// Maturity.
    pub maturity: GameFixMaturity,
    /// Current state.
    pub state: GameFixState,
    /// Managed file relative paths.
    pub files: Vec<String>,
}

/// Status of a file managed by an installed fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFixManagedFileStatus {
    /// Owning fix ID.
    pub fix_id: String,
    /// Managed relative path.
    pub relative_path: String,
    /// True if the file exists on disk.
    pub exists: bool,
    /// True if the file matches its recorded after-patch hash.
    pub matches_expected_hash: bool,
}

/// Managed file record inside a manifest or journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedGameFile {
    /// Relative path.
    pub relative_path: String,
    /// SHA-256 before modification.
    pub before_sha256: String,
    /// SHA-256 after modification.
    pub after_sha256: String,
    /// Relative path to stored backup.
    pub backup_path: String,
    /// True if the file existed loose before the fix.
    pub target_existed_before: bool,
}

/// Manifest saved in `.save-editor-game-fixes/<id>/manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFixManifest {
    /// Schema version (2).
    pub schema_version: u32,
    /// Fix identifier.
    pub fix_id: String,
    /// Target game.
    pub game: GameTarget,
    /// Detected Steam build ID.
    pub steam_build_id: String,
    /// Fix version.
    pub version: String,
    /// Title.
    pub title: String,
    /// Problem description.
    pub problem: String,
    /// Solution description.
    pub description: String,
    /// Implementation type.
    pub implementation: GameFixImplementationType,
    /// Requires new game.
    pub requires_new_game: bool,
    /// Save compatibility.
    pub save_compatibility: GameFixSaveCompatibility,
    /// Verification state.
    pub verification_state: GameFixVerificationState,
    /// Detection method.
    pub detection_method: String,
    /// References list.
    pub references: Vec<String>,
    /// Category.
    pub category: GameFixCategory,
    /// Maturity.
    pub maturity: GameFixMaturity,
    /// Dependencies.
    pub depends_on: Vec<String>,
    /// Conflicts.
    pub conflicts_with: Vec<String>,
    /// Provenance source.
    pub source: String,
    /// True if active.
    pub installed: bool,
    /// Managed files.
    pub files: Vec<ManagedGameFile>,
}

/// Journal saved in `.save-editor-game-fixes/<id>/transaction.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFixJournal {
    /// Schema version (1).
    pub schema_version: u32,
    /// Operation kind (`"install"` or `"uninstall"`).
    pub kind: String,
    /// True if the fix had no previous state folder.
    pub fresh_state: bool,
    /// Managed files.
    pub files: Vec<ManagedGameFile>,
}
