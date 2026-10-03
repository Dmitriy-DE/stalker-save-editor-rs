//! Read-only save and crash diagnostics.

use sse_core::{Error, Result};
use sse_xray::writer::ChangeSet;
use sse_xray::Save;
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

/// Overall state reported by Save Doctor and each diagnostic rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveDoctorStatus {
    /// Supported structure parsed; no validated issue was detected.
    Ok,
    /// At least one validated issue needs attention.
    Warning,
    /// Input was empty, unsupported, or structurally damaged.
    Error,
    /// Available evidence cannot establish this check's state.
    Unknown,
}

/// One rule's evidence and optional evidence-backed repair.
#[derive(Debug, Clone)]
pub struct RuleFinding {
    /// Stable rule identifier.
    pub id: &'static str,
    /// Severity for this result.
    pub severity: SaveDoctorStatus,
    /// Field or structure inspected by the rule.
    pub looked_at: &'static str,
    /// Evidence found by the rule.
    pub found: String,
    /// Repair offered only when the evidence proves a safe mutation.
    pub repair: Option<ChangeSet>,
}

/// Read-only result for one selected save.
#[derive(Debug, Clone)]
pub struct SaveDoctorReport {
    /// Combined structural status.
    pub status: SaveDoctorStatus,
    /// Detected format, when the reader accepted the input.
    pub format_id: Option<&'static str>,
    /// Number of indexed registry records, when available.
    pub object_count: Option<usize>,
    /// Actor-owned inventory entry count, when available.
    pub inventory_count: Option<usize>,
    /// Findings in stable rule order.
    pub findings: Vec<RuleFinding>,
}

struct SaveIndex<'a> {
    save: &'a Save,
    inventory_count: usize,
}

struct Rule {
    id: &'static str,
    severity: SaveDoctorStatus,
    looked_at: &'static str,
    evaluate: fn(&SaveIndex<'_>) -> RuleEvaluation,
}

struct RuleEvaluation {
    found: String,
    repair: Option<ChangeSet>,
}

const SAVE_RULES: [Rule; 4] = [
    Rule {
        id: "structure",
        severity: SaveDoctorStatus::Ok,
        looked_at: "container, frame, and indexed registry records",
        evaluate: structure_rule,
    },
    Rule {
        id: "semantic-state",
        severity: SaveDoctorStatus::Unknown,
        looked_at: "validated quest, object-reference, and progression signatures",
        evaluate: semantic_rule,
    },
    Rule {
        id: "quest-state",
        severity: SaveDoctorStatus::Unknown,
        looked_at: "quest and task state fields",
        evaluate: quest_rule,
    },
    Rule {
        id: "repair",
        severity: SaveDoctorStatus::Unknown,
        looked_at: "evidence-backed repair capability",
        evaluate: repair_rule,
    },
];

/// Analyzes one in-memory save without changing it.
#[must_use]
pub fn analyze_save(data: &[u8]) -> SaveDoctorReport {
    if data.is_empty() {
        return failed_save_report("The selected file is empty.");
    }
    let save = match Save::read(data) {
        Ok(save) => save,
        Err(error) => return failed_save_report(&error.to_string()),
    };
    let inventory_count = match save.inventory() {
        Ok(items) => items.len(),
        Err(error) => return failed_save_report(&error.to_string()),
    };
    let index = SaveIndex {
        save: &save,
        inventory_count,
    };
    let findings = run_rules(&SAVE_RULES, &index);
    SaveDoctorReport {
        status: SaveDoctorStatus::Ok,
        format_id: Some(save.format().id()),
        object_count: Some(save.registry_objects().len()),
        inventory_count: Some(inventory_count),
        findings,
    }
}

fn failed_save_report(detail: &str) -> SaveDoctorReport {
    SaveDoctorReport {
        status: SaveDoctorStatus::Error,
        format_id: None,
        object_count: None,
        inventory_count: None,
        findings: vec![
            RuleFinding {
                id: "structure",
                severity: SaveDoctorStatus::Error,
                looked_at: "supported save structure",
                found: format!("Save structure could not be validated: {detail}"),
                repair: None,
            },
            RuleFinding {
                id: "semantic-state",
                severity: SaveDoctorStatus::Unknown,
                looked_at: "semantic save state",
                found: "The save must parse before semantic checks can run.".to_owned(),
                repair: None,
            },
            RuleFinding {
                id: "quest-state",
                severity: SaveDoctorStatus::Unknown,
                looked_at: "quest and task state fields",
                found: "The save must parse before quest state can be inspected.".to_owned(),
                repair: None,
            },
            RuleFinding {
                id: "repair",
                severity: SaveDoctorStatus::Unknown,
                looked_at: "repair capability",
                found: "No save was changed.".to_owned(),
                repair: None,
            },
        ],
    }
}

fn structure_rule(index: &SaveIndex<'_>) -> RuleEvaluation {
    RuleEvaluation {
        found: format!(
            "Supported structure parsed: format {}; {} inventory record(s).",
            index.save.format().id(),
            index.inventory_count
        ),
        repair: None,
    }
}

fn semantic_rule(_index: &SaveIndex<'_>) -> RuleEvaluation {
    RuleEvaluation {
        found: "Semantic health is not classified; no validated signature is registered.".to_owned(),
        repair: None,
    }
}

fn quest_rule(_index: &SaveIndex<'_>) -> RuleEvaluation {
    RuleEvaluation {
        found: "The supported X-Ray reader does not expose validated quest state fields.".to_owned(),
        repair: None,
    }
}

fn repair_rule(_index: &SaveIndex<'_>) -> RuleEvaluation {
    RuleEvaluation {
        found: "No evidence-backed repair is available for this save.".to_owned(),
        repair: None,
    }
}

fn run_rules(rules: &[Rule], index: &SaveIndex<'_>) -> Vec<RuleFinding> {
    if rules.is_empty() {
        return Vec::new();
    }
    let worker_count = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(rules.len());
    let chunk_size = rules.len().div_ceil(worker_count);
    let mut findings = (0..rules.len()).map(|_| None).collect::<Vec<Option<RuleFinding>>>();
    std::thread::scope(|scope| {
        let (sender, receiver) = std::sync::mpsc::channel();
        for (chunk_number, chunk) in rules.chunks(chunk_size).enumerate() {
            let sender = sender.clone();
            scope.spawn(move || {
                for (offset, rule) in chunk.iter().enumerate() {
                    let Some(position) = chunk_number
                        .checked_mul(chunk_size)
                        .and_then(|start| start.checked_add(offset))
                    else {
                        continue;
                    };
                    let evaluation = (rule.evaluate)(index);
                    let finding = RuleFinding {
                        id: rule.id,
                        severity: rule.severity,
                        looked_at: rule.looked_at,
                        found: evaluation.found,
                        repair: evaluation.repair,
                    };
                    let _ = sender.send((position, finding));
                }
            });
        }
        drop(sender);
        for (position, finding) in receiver {
            if let Some(slot) = findings.get_mut(position) {
                *slot = Some(finding);
            }
        }
    });
    findings.into_iter().flatten().collect()
}

/// Quest-state classification for one known task rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestTaskStatus {
    /// The cancellation flag is set or the NPC remains alive.
    Ok,
    /// The NPC is dead and the expected cancellation flag is absent.
    Broken,
    /// The save does not expose enough evidence to classify the task.
    Unknown,
}

/// Read-only NPC state needed by an evidence-backed quest rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestNpcVitals {
    /// NPC section identifier from the game's spawn data.
    pub section: String,
    /// Whether the validated creature state says the NPC is dead.
    pub is_dead: bool,
}

/// One quest rule's state and, when proven, the missing cancellation flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestTaskState {
    /// Stable quest rule identifier.
    pub id: &'static str,
    /// Game-facing task description.
    pub title: &'static str,
    /// Evidence-based rule result.
    pub status: QuestTaskStatus,
    /// Why the state was selected.
    pub reason: &'static str,
    /// Missing flag that would prevent the known broken state.
    pub missing_info: Option<&'static str>,
    /// Game Fix needed to make this flag affect the task, where applicable.
    pub preventing_fix_id: Option<&'static str>,
    /// Whether the repair depends on a separate game fix.
    pub needs_preventing_fix: bool,
    /// Short explanation for this result.
    pub detail: &'static str,
    /// Provenance for the rule.
    pub references: &'static [&'static str],
}

struct QuestRule {
    id: &'static str,
    format_id: &'static str,
    title: &'static str,
    npc_section: &'static str,
    info_portion: &'static str,
    preventing_fix_id: Option<&'static str>,
    too_late_info: Option<&'static str>,
    needs_preventing_fix: bool,
    detail: &'static str,
    references: &'static [&'static str],
}

const QUEST_RULES: [QuestRule; 7] = [
    QuestRule {
        id: "cs.wild-napr-dead",
        format_id: "stalker-cs",
        title: "Wild Napr's tasks after his death",
        npc_section: "gar_digger_quester",
        info_portion: "gar_flea_market_stop_quest_line",
        preventing_fix_id: Some("cs.quest.dead-wild-napr"),
        too_late_info: None,
        needs_preventing_fix: false,
        detail: "Wild Napr is dead, but the Flea Market task cancellation flag is missing.",
        references: &["SRP v1.1.5 Version History"],
    },
    QuestRule {
        id: "cs.wolf-dead",
        format_id: "stalker-cs",
        title: "Wolf's tasks after his death",
        npc_section: "esc_wolf",
        info_portion: "esc_wolf_dead",
        preventing_fix_id: Some("cs.quest.wolf-offline-cancellation"),
        too_late_info: None,
        needs_preventing_fix: true,
        detail: "Wolf is dead, but his offline-death cancellation flag is missing.",
        references: &["SRP v1.1.5 Version History"],
    },
    QuestRule {
        id: "cs.hog-dead",
        format_id: "stalker-cs",
        title: "Hog's storyline task after his death",
        npc_section: "mil_hog",
        info_portion: "mil_hog_death",
        preventing_fix_id: None,
        too_late_info: Some("forester_talked_2"),
        needs_preventing_fix: false,
        detail: "Hog is dead, but the Army Warehouses cancellation flag is missing.",
        references: &["SRP v1.1.5 Version History"],
    },
    QuestRule {
        id: "soc.mole-dead",
        format_id: "stalker-soc",
        title: "Mole's Agroprom task after his death",
        npc_section: "agr_krot",
        info_portion: "agr_krot_dead",
        preventing_fix_id: None,
        too_late_info: None,
        needs_preventing_fix: false,
        detail: "Mole is dead, but the Agroprom task cancellation flag is missing.",
        references: &["retail all.spawn", "tasks_agroprom.xml"],
    },
    QuestRule {
        id: "soc.prisoner-dead",
        format_id: "stalker-soc",
        title: "Dark Valley prisoner task after his death",
        npc_section: "val_prisoner_captive",
        info_portion: "val_prisoner_dead",
        preventing_fix_id: None,
        too_late_info: None,
        needs_preventing_fix: false,
        detail: "The captive Duty soldier is dead, but the task cancellation flag is missing.",
        references: &["retail all.spawn", "tasks_darkvalley.xml"],
    },
    QuestRule {
        id: "soc.courier-dead",
        format_id: "stalker-soc",
        title: "Freedom courier task after his death",
        npc_section: "mil_freedom_member0001",
        info_portion: "mil_courier_dead",
        preventing_fix_id: None,
        too_late_info: None,
        needs_preventing_fix: false,
        detail: "The Freedom courier is dead, but the task completion flag is missing.",
        references: &["retail all.spawn", "tasks_military.xml"],
    },
    QuestRule {
        id: "soc.informer-dead",
        format_id: "stalker-soc",
        title: "Freedom informer task after his death",
        npc_section: "mil_ara",
        info_portion: "mil_ara_dead",
        preventing_fix_id: None,
        too_late_info: None,
        needs_preventing_fix: false,
        detail: "The informer is dead, but the task completion flag is missing.",
        references: &["retail all.spawn", "tasks_military.xml"],
    },
];

/// Evaluates the checked-in SoC and Clear Sky quest rules against validated save facts.
///
/// This accepts already-read facts because the current X-Ray API does not yet expose actor info portions or
/// creature health. It only reports repairs as missing flag names; it does not produce or apply a write.
#[must_use]
pub fn evaluate_quest_facts(
    format_id: &str,
    known_info: Option<&[String]>,
    npc_vitals: &[QuestNpcVitals],
) -> Vec<QuestTaskState> {
    QUEST_RULES
        .iter()
        .filter(|rule| rule.format_id == format_id)
        .map(|rule| evaluate_quest_rule(rule, known_info, npc_vitals))
        .collect()
}

fn evaluate_quest_rule(
    rule: &QuestRule,
    known_info: Option<&[String]>,
    npc_vitals: &[QuestNpcVitals],
) -> QuestTaskState {
    let state = if known_info.is_none() {
        (
            QuestTaskStatus::Unknown,
            "no-info-list",
            "The save has no readable actor info list.",
        )
    } else if known_info.is_some_and(|portions| portions.iter().any(|portion| portion == rule.info_portion)) {
        (
            QuestTaskStatus::Ok,
            "flag-set",
            "The task cancellation flag is already set.",
        )
    } else {
        let mut npc_found = false;
        let mut npc_alive = false;
        for vitals in npc_vitals
            .iter()
            .filter(|vitals| vitals.section.eq_ignore_ascii_case(rule.npc_section))
        {
            npc_found = true;
            npc_alive |= !vitals.is_dead;
        }
        if !npc_found {
            (
                QuestTaskStatus::Unknown,
                "npc-missing",
                "The NPC is absent or unreadable; absence does not prove death.",
            )
        } else if npc_alive {
            (QuestTaskStatus::Ok, "alive", "The NPC is alive in the save.")
        } else if rule
            .too_late_info
            .is_some_and(|flag| known_info.is_some_and(|portions| portions.iter().any(|portion| portion == flag)))
        {
            (
                QuestTaskStatus::Unknown,
                "too-late",
                "The quest line has already branched; adding the flag is not a proven repair.",
            )
        } else {
            (QuestTaskStatus::Broken, "dead-without-flag", rule.detail)
        }
    };
    QuestTaskState {
        id: rule.id,
        title: rule.title,
        status: state.0,
        reason: state.1,
        missing_info: (state.0 == QuestTaskStatus::Broken).then_some(rule.info_portion),
        preventing_fix_id: rule.preventing_fix_id,
        needs_preventing_fix: rule.needs_preventing_fix,
        detail: state.2,
        references: rule.references,
    }
}

/// Quest Doctor report for one selected save.
#[derive(Debug, Clone)]
pub struct QuestDoctorReport {
    /// Combined result; unknown save fields are never reported as broken quests.
    pub status: SaveDoctorStatus,
    /// Detected save format, when parsing succeeded.
    pub format_id: Option<&'static str>,
    /// Whether this format has one or more evidence-backed rules.
    pub quest_states_available: bool,
    /// Short explanation for the report's certainty level.
    pub summary: String,
    /// Evaluated rule states.
    pub states: Vec<QuestTaskState>,
}

/// Parses one save and reports quest rules only when the evidence is exposed by the current reader.
#[must_use]
pub fn analyze_quests(data: &[u8]) -> QuestDoctorReport {
    let save = match Save::read(data) {
        Ok(save) => save,
        Err(error) => {
            return QuestDoctorReport {
                status: SaveDoctorStatus::Error,
                format_id: None,
                quest_states_available: false,
                summary: format!("The save could not be parsed: {error}"),
                states: Vec::new(),
            };
        }
    };
    let format_id = save.format().id();
    let states = evaluate_quest_facts(format_id, None, &[]);
    let quest_states_available = !states.is_empty();
    let summary = if quest_states_available {
        "The supported save reader does not expose validated actor info portions or NPC health; all listed rules remain unknown.".to_owned()
    } else {
        "Quest Doctor has no evidence-backed rules for this save format.".to_owned()
    };
    QuestDoctorReport {
        status: SaveDoctorStatus::Unknown,
        format_id: Some(format_id),
        quest_states_available,
        summary,
        states,
    }
}

/// Supported original-trilogy game releases with independent build histories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameTarget {
    /// Shadow of Chernobyl.
    Soc,
    /// Clear Sky.
    Cs,
    /// Call of Pripyat.
    Cop,
    /// Shadow of Chernobyl Enhanced Edition.
    SocEe,
    /// Clear Sky Enhanced Edition.
    CsEe,
    /// Call of Pripyat Enhanced Edition.
    CopEe,
}

impl GameTarget {
    /// Stable command-line identifier.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Soc => "soc",
            Self::Cs => "cs",
            Self::Cop => "cop",
            Self::SocEe => "soc-ee",
            Self::CsEe => "cs-ee",
            Self::CopEe => "cop-ee",
        }
    }

    /// Parses a stable target identifier case-insensitively.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        [Self::Soc, Self::Cs, Self::Cop, Self::SocEe, Self::CsEe, Self::CopEe]
            .into_iter()
            .find(|target| target.id().eq_ignore_ascii_case(id))
    }
}

/// Classification of one target's Steam build identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameBuildStatus {
    /// Build id matches the value used to verify the reader and fix catalog.
    Verified,
    /// Build id is absent or is not in the verified list.
    Unknown,
    /// The selected game directory is not installed.
    NotInstalled,
}

/// One target and its verified build identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameBuildFingerprint {
    /// Game release this build id belongs to.
    pub target: GameTarget,
    /// Steam build id, if one was read from a matching manifest.
    pub build_id: Option<String>,
    /// Verification classification.
    pub status: GameBuildStatus,
}

const VERIFIED_BUILDS: [(GameTarget, &str); 6] = [
    (GameTarget::Soc, "11567845"),
    (GameTarget::Cs, "11450472"),
    (GameTarget::Cop, "11450453"),
    (GameTarget::SocEe, "24067120"),
    (GameTarget::CsEe, "24067129"),
    (GameTarget::CopEe, "24067133"),
];

/// Classifies a build id only against the selected game target.
#[must_use]
pub fn classify_game_build(target: GameTarget, build_id: Option<&str>) -> GameBuildFingerprint {
    let status = match build_id {
        None => GameBuildStatus::NotInstalled,
        Some(build_id)
            if VERIFIED_BUILDS
                .iter()
                .any(|(known_target, known_build)| *known_target == target && *known_build == build_id) =>
        {
            GameBuildStatus::Verified
        }
        Some(_) => GameBuildStatus::Unknown,
    };
    GameBuildFingerprint {
        target,
        build_id: build_id.map(str::to_owned),
        status,
    }
}

/// Read-only game-directory check for the original trilogy and Enhanced Editions.
#[derive(Debug, Clone)]
pub struct GameDoctorReport {
    /// Target inferred from the caller's explicit choice.
    pub target: GameTarget,
    /// Selected path.
    pub directory: PathBuf,
    /// Overall structural status.
    pub status: SaveDoctorStatus,
    /// Whether a game-specific root marker exists.
    pub marker_found: bool,
    /// Build fingerprint supplied by the Steam manifest reader, if available.
    pub build: GameBuildFingerprint,
    /// Installation and build findings.
    pub findings: Vec<RuleFinding>,
}

/// Checks an explicitly chosen game folder for its root marker and classifies a known build id.
#[must_use]
pub fn analyze_game_install(target: GameTarget, directory: &Path, build_id: Option<&str>) -> GameDoctorReport {
    let exists = directory.is_dir();
    let markers: &[&str] = match target {
        GameTarget::Soc | GameTarget::SocEe => &["fsgame.ltx", "fsgame_soc.ltx"],
        GameTarget::Cs | GameTarget::CsEe => &["fsgame.ltx", "fsgame_cs.ltx"],
        GameTarget::Cop | GameTarget::CopEe => &["fsgame.ltx", "fsgame_cop.ltx"],
    };
    let marker_found = exists && markers.iter().any(|marker| directory.join(marker).is_file());
    let status = if !exists || !marker_found {
        SaveDoctorStatus::Error
    } else {
        SaveDoctorStatus::Ok
    };
    let build = classify_game_build(target, if exists { build_id } else { None });
    let install_detail = if !exists {
        "The selected installation directory does not exist.".to_owned()
    } else if marker_found {
        "A target-specific root fsgame marker was found; retail files were not verified.".to_owned()
    } else {
        "No expected root fsgame marker was found.".to_owned()
    };
    let build_detail = match build.status {
        GameBuildStatus::Verified => format!(
            "Steam build {} is in the verified fingerprint list.",
            build.build_id.as_deref().unwrap_or("")
        ),
        GameBuildStatus::Unknown => format!(
            "Steam build {} is not verified for this target.",
            build.build_id.as_deref().unwrap_or("")
        ),
        GameBuildStatus::NotInstalled => "A matching Steam build id was not provided.".to_owned(),
    };
    let findings = vec![
        RuleFinding {
            id: "installation",
            severity: status,
            looked_at: "selected directory and target-specific root marker",
            found: install_detail,
            repair: None,
        },
        RuleFinding {
            id: "build-fingerprint",
            severity: match build.status {
                GameBuildStatus::Verified => SaveDoctorStatus::Ok,
                GameBuildStatus::Unknown | GameBuildStatus::NotInstalled => SaveDoctorStatus::Unknown,
            },
            looked_at: "Steam appmanifest build id",
            found: build_detail,
            repair: None,
        },
    ];
    GameDoctorReport {
        target,
        directory: directory.to_path_buf(),
        status,
        marker_found,
        build,
        findings,
    }
}

/// Reads the matching Steam app manifest before checking an explicitly chosen installation.
#[must_use]
pub fn analyze_game_install_from_steam(target: GameTarget, directory: &Path) -> GameDoctorReport {
    let build_id = read_steam_build_id(target, directory);
    analyze_game_install(target, directory, build_id.as_deref())
}

fn read_steam_build_id(target: GameTarget, game_directory: &Path) -> Option<String> {
    const MAXIMUM_STEAM_MANIFEST_BYTES: u64 = 1024 * 1024;

    let app_id = target.steam_app_id();
    let canonical_game = fs::canonicalize(game_directory).ok()?;
    let library_root = game_directory.parent()?.parent()?.parent()?;
    let steamapps = library_root.join("steamapps");
    let manifest_path = steamapps.join(format!("appmanifest_{app_id}.acf"));
    let manifest_file = File::open(&manifest_path).ok()?;
    if manifest_file.metadata().ok()?.len() > MAXIMUM_STEAM_MANIFEST_BYTES {
        return None;
    }
    let mut manifest = String::new();
    manifest_file
        .take(MAXIMUM_STEAM_MANIFEST_BYTES.saturating_add(1))
        .read_to_string(&mut manifest)
        .ok()?;
    if u64::try_from(manifest.len()).ok()? > MAXIMUM_STEAM_MANIFEST_BYTES {
        return None;
    }
    let fields = parse_app_state(&manifest)?;
    if fields.app_id.as_deref() != Some(app_id) {
        return None;
    }
    let install_name = fields.install_dir.as_deref()?;
    let expected_directory = fs::canonicalize(steamapps.join("common").join(install_name)).ok()?;
    if !same_path(&canonical_game, &expected_directory) {
        return None;
    }
    fields.build_id
}

fn same_path(left: &Path, right: &Path) -> bool {
    if cfg!(windows) {
        left.to_string_lossy().eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

impl GameTarget {
    const fn steam_app_id(self) -> &'static str {
        match self {
            Self::Soc => "4500",
            Self::Cs => "20510",
            Self::Cop => "41700",
            Self::SocEe => "2427410",
            Self::CsEe => "2427420",
            Self::CopEe => "2427430",
        }
    }
}

struct AppStateFields {
    app_id: Option<String>,
    install_dir: Option<String>,
    build_id: Option<String>,
}

enum VdfToken {
    Value(String),
    Open,
    Close,
}

// Temporary local VDF tokenizer; replace with sse_codecs::vdf when that codec lands.
fn parse_app_state(text: &str) -> Option<AppStateFields> {
    let tokens = tokenize_vdf(text)?;
    let mut depth = 0_usize;
    let mut app_state_depth = None;
    let mut app_state_count = 0_usize;
    let mut fields = AppStateFields {
        app_id: None,
        install_dir: None,
        build_id: None,
    };
    let mut cursor = 0_usize;
    while cursor < tokens.len() {
        match tokens.get(cursor)? {
            VdfToken::Open => {
                depth = depth.checked_add(1)?;
                cursor = cursor.checked_add(1)?;
            }
            VdfToken::Close => {
                depth = depth.checked_sub(1)?;
                if app_state_depth == depth.checked_add(1) {
                    app_state_depth = None;
                }
                cursor = cursor.checked_add(1)?;
            }
            VdfToken::Value(key) => {
                let value_at = cursor.checked_add(1)?;
                match tokens.get(value_at)? {
                    VdfToken::Open => {
                        depth = depth.checked_add(1)?;
                        if key == "AppState" {
                            app_state_count = app_state_count.checked_add(1)?;
                            if app_state_count > 1 || app_state_depth.is_some() {
                                return None;
                            }
                            app_state_depth = Some(depth);
                        }
                        cursor = cursor.checked_add(2)?;
                    }
                    VdfToken::Value(value) => {
                        if app_state_depth == Some(depth) {
                            let slot = match key.as_str() {
                                "appid" => &mut fields.app_id,
                                "installdir" => &mut fields.install_dir,
                                "buildid" => &mut fields.build_id,
                                _ => {
                                    cursor = cursor.checked_add(2)?;
                                    continue;
                                }
                            };
                            if slot.replace(value.clone()).is_some() {
                                return None;
                            }
                        }
                        cursor = cursor.checked_add(2)?;
                    }
                    VdfToken::Close => return None,
                }
            }
        }
    }
    if depth != 0 || app_state_count != 1 || app_state_depth.is_some() {
        return None;
    }
    Some(fields)
}

fn tokenize_vdf(text: &str) -> Option<Vec<VdfToken>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        let byte = *bytes.get(cursor)?;
        if byte.is_ascii_whitespace() {
            cursor = cursor.checked_add(1)?;
            continue;
        }
        if byte == b'/' && bytes.get(cursor.checked_add(1)?) == Some(&b'/') {
            while bytes.get(cursor).is_some_and(|value| *value != b'\n') {
                cursor = cursor.checked_add(1)?;
            }
            continue;
        }
        match byte {
            b'{' => {
                tokens.push(VdfToken::Open);
                cursor = cursor.checked_add(1)?;
            }
            b'}' => {
                tokens.push(VdfToken::Close);
                cursor = cursor.checked_add(1)?;
            }
            b'"' => {
                cursor = cursor.checked_add(1)?;
                let mut value = String::new();
                let mut segment_start = cursor;
                loop {
                    match bytes.get(cursor).copied()? {
                        b'"' => {
                            value.push_str(text.get(segment_start..cursor)?);
                            cursor = cursor.checked_add(1)?;
                            break;
                        }
                        b'\\' => {
                            value.push_str(text.get(segment_start..cursor)?);
                            cursor = cursor.checked_add(1)?;
                            let escaped = *bytes.get(cursor)?;
                            value.push(match escaped {
                                b'"' => '"',
                                b'\\' => '\\',
                                b'n' => '\n',
                                b't' => '\t',
                                other => char::from(other),
                            });
                            cursor = cursor.checked_add(1)?;
                            segment_start = cursor;
                        }
                        _ => cursor = cursor.checked_add(1)?,
                    }
                }
                tokens.push(VdfToken::Value(value));
            }
            _ => {
                let start = cursor;
                while bytes
                    .get(cursor)
                    .is_some_and(|value| !value.is_ascii_whitespace() && !matches!(*value, b'{' | b'}'))
                {
                    cursor = cursor.checked_add(1)?;
                }
                tokens.push(VdfToken::Value(text.get(start..cursor)?.to_owned()));
            }
        }
        if tokens.len() > 100_000 {
            return None;
        }
    }
    Some(tokens)
}

/// Advice associated with a documented crash message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashAdvice {
    /// Install the validated game fix.
    InstallFix,
    /// Check the selected save with Quest Doctor.
    RepairSave,
    /// Load an earlier save.
    ReloadEarlierSave,
    /// Install the cited community patch.
    CommunityPatch,
    /// The save itself appears damaged.
    CorruptSave,
    /// Repair or verify the game installation.
    RepairInstallation,
}

/// One literal signature from the checked-in diagnostic subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashSignature {
    /// Stable signature identifier.
    pub id: &'static str,
    /// Game id, or `any` for installation-wide failures.
    pub game: &'static str,
    /// User-facing signature name.
    pub title: &'static str,
    /// Recommended next action.
    pub advice: CrashAdvice,
    /// Literal text searched for, case-insensitively.
    pub pattern: &'static str,
    /// Evidence note or reference.
    pub source: &'static str,
}

const SIGNATURES: [CrashSignature; 11] = [
    CrashSignature {
        id: "cs.wrong-target-wild-napr",
        game: "cs",
        title: "Task targets Wild Napr after his death",
        advice: CrashAdvice::RepairSave,
        pattern: "wrong target for storyline quest: logic@work5,gar_smart_terrain_6_3",
        source: "SRP v1.1.5 Version History",
    },
    CrashSignature {
        id: "cs.insufficient-smart-jobs",
        game: "cs",
        title: "Too many stalkers for one camp",
        advice: CrashAdvice::InstallFix,
        pattern: "insufficient smart_terrain jobs",
        source: "SRP v1.1.5 Version History",
    },
    CrashSignature {
        id: "cs.hospital-jump-down-animation",
        game: "cs",
        title: "Hospital jump animation is missing",
        advice: CrashAdvice::InstallFix,
        pattern: "cant find animation for slot",
        source: "Clear Sky crash report reference",
    },
    CrashSignature {
        id: "cs.sim-combat-actor-nil",
        game: "cs",
        title: "Loading during a squad fight",
        advice: CrashAdvice::InstallFix,
        pattern: "attempt to index field 'actor' (a nil value)",
        source: "SRP v1.1.5 Version History",
    },
    CrashSignature {
        id: "cs.pstor-unknown-type",
        game: "cs",
        title: "Saved object data has an unknown type",
        advice: CrashAdvice::CorruptSave,
        pattern: "pstor_load_all: not registered type n",
        source: "Clear Sky save-load error reports",
    },
    CrashSignature {
        id: "soc.entity-not-found",
        game: "soc",
        title: "A referenced entity does not exist",
        advice: CrashAdvice::InstallFix,
        pattern: "entity not found. id_parent=",
        source: "ZRP 1.09 CrashesStillInTheGame",
    },
    CrashSignature {
        id: "soc.map-location-dead-object",
        game: "soc",
        title: "Map location refers to a missing object",
        advice: CrashAdvice::InstallFix,
        pattern: "smaplocation binded to non-existent object id=",
        source: "ZRP 1.09 CrashesStillInTheGame",
    },
    CrashSignature {
        id: "any.missing-model",
        game: "any",
        title: "A model file is missing",
        advice: CrashAdvice::RepairInstallation,
        pattern: "can't find model file",
        source: "Game installation diagnostic signature",
    },
    CrashSignature {
        id: "any.missing-config-value",
        game: "any",
        title: "A configuration value is missing",
        advice: CrashAdvice::RepairInstallation,
        pattern: "can't find variable ",
        source: "Game installation diagnostic signature",
    },
    CrashSignature {
        id: "any.missing-string-table",
        game: "any",
        title: "A string-table file is missing",
        advice: CrashAdvice::RepairInstallation,
        pattern: "string table xml file not found",
        source: "Game installation diagnostic signature",
    },
    CrashSignature {
        id: "cs.missing-backpack-model",
        game: "cs",
        title: "Clear Sky references a missing backpack model",
        advice: CrashAdvice::InstallFix,
        pattern: "can't find model file 'dynamics\\equipments\\item_rukzak.ogf'",
        source: "SRP v1.1.5 Version History",
    },
];

/// Compiled crash catalog; patterns are matched in one byte pass.
pub struct CrashSignatureCatalog;

impl CrashSignatureCatalog {
    /// Returns the signatures currently ported from the C# catalog.
    #[must_use]
    pub const fn all() -> &'static [CrashSignature] {
        &SIGNATURES
    }

    /// Finds the most specific known literal, optionally restricting matches to one game.
    #[must_use]
    pub fn match_log(text: &str, game: Option<&str>) -> Option<&'static CrashSignature> {
        AUTOMATON.get_or_init(build_automaton).find(text.as_bytes(), game)
    }
}

/// Crash kind derived from explicit log markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashKind {
    /// No known marker was present.
    Unknown,
    /// A Lua script raised an error.
    LuaError,
    /// The engine emitted its fatal-error marker.
    FatalError,
    /// A generic engine error marker was present.
    EngineError,
}

/// Read-only result for one crash-log string.
#[derive(Debug, Clone)]
pub struct CrashLogAnalysis {
    /// Detected marker class.
    pub kind: CrashKind,
    /// Compact summary from the source text.
    pub summary: String,
    /// Exact known issue, when the literal catalog matched.
    pub known_issue: Option<&'static CrashSignature>,
    /// Faulting module and offset recovered from a minidump, when supplied by a reader.
    pub faulting_module_offset: Option<String>,
}

/// One recent log or dump found in an explicitly selected game directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredCrashLog {
    /// Full path of the discovered file.
    pub path: PathBuf,
    /// File name only.
    pub name: String,
    /// Last modification time when the directory was scanned.
    pub modified: SystemTime,
    /// File length in bytes.
    pub length: u64,
}

/// Facts returned by a checked minidump parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashDumpFacts {
    /// Engine error text recovered from the dump.
    pub message: String,
    /// Faulting frame in `module+offset` form; symbols are not resolved.
    pub faulting_module_offset: Option<String>,
}

/// Boundary for the minidump reader supplied by `sse-codecs` when it lands.
pub trait CrashDumpReader: Sync {
    /// Reads a bounded minidump and returns only checked textual crash facts.
    ///
    /// # Errors
    /// Returns an error for malformed or unsupported dump streams.
    fn read_minidump(&self, bytes: &[u8]) -> Result<CrashDumpFacts>;
}

const CRASH_LOG_TAIL_BYTES: u64 = 256 * 1024;
const MAXIMUM_DISCOVERED_CRASH_LOGS: usize = 500;

/// Reads at most the last 256 KiB of one explicit text log and analyzes that tail.
///
/// Minidump files are detected by their content and refused until a checked dump reader is available.
///
/// # Errors
/// Returns an I/O error for an unreadable file, or `Refused` for a minidump that this crate cannot parse.
pub fn analyze_crash_file(path: &Path, game: Option<&str>) -> Result<CrashLogAnalysis> {
    analyze_crash_file_with_dump_reader(path, game, None)
}

/// Reads a crash log or delegates a recognized dump to an injected checked parser.
///
/// Text logs are limited to their last 256 KiB. Minidumps are limited to 64 MiB before allocation.
///
/// # Errors
/// Returns `Refused` for a minidump when no reader is supplied or the file exceeds the size bound.
pub fn analyze_crash_file_with_dump_reader(
    path: &Path,
    game: Option<&str>,
    dump_reader: Option<&dyn CrashDumpReader>,
) -> Result<CrashLogAnalysis> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut signature = [0_u8; 4];
    let signature_bytes = file.read(&mut signature)?;
    if signature_bytes == signature.len() && signature == *b"MDMP" {
        const MAXIMUM_MINIDUMP_BYTES: u64 = 64 * 1024 * 1024;
        if length > MAXIMUM_MINIDUMP_BYTES {
            return Err(Error::Refused(
                "minidump exceeds the 64 MiB inspection limit".to_owned(),
            ));
        }
        let dump_reader = dump_reader
            .ok_or_else(|| Error::Refused("minidump inspection needs a checked CrashDumpReader".to_owned()))?;
        file.seek(SeekFrom::Start(0))?;
        let capacity = usize::try_from(length)
            .map_err(|_| Error::Refused("minidump length does not fit this platform".to_owned()))?;
        let mut dump = Vec::with_capacity(capacity);
        file.take(length).read_to_end(&mut dump)?;
        if u64::try_from(dump.len()).ok() != Some(length) {
            return Err(Error::damaged("minidump changed while it was being read"));
        }
        let facts = dump_reader.read_minidump(&dump)?;
        let mut analysis = analyze_crash_log(&facts.message, game);
        analysis.faulting_module_offset = facts.faulting_module_offset;
        return Ok(analysis);
    }
    let tail_length = length.min(CRASH_LOG_TAIL_BYTES);
    let tail_start = length.saturating_sub(CRASH_LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(tail_start))?;
    let capacity = usize::try_from(tail_length)
        .map_err(|_| Error::Refused("crash-log tail does not fit this platform".to_owned()))?;
    let mut tail = Vec::with_capacity(capacity);
    file.take(CRASH_LOG_TAIL_BYTES).read_to_end(&mut tail)?;
    let text = String::from_utf8_lossy(&tail);
    Ok(analyze_crash_log(&text, game))
}

/// Finds recent `.log` and `.mdmp` files in the known log directories of one chosen install.
///
/// The walk is limited to the three direct log folders used by the games and to `max_results` entries.
///
/// # Errors
/// Returns `Refused` when the result bound is outside `1..=500`.
pub fn discover_crash_logs(game_directory: &Path, max_results: usize) -> Result<Vec<DiscoveredCrashLog>> {
    if !(1..=MAXIMUM_DISCOVERED_CRASH_LOGS).contains(&max_results) {
        return Err(Error::Refused(
            "crash-log result limit must be between 1 and 500".to_owned(),
        ));
    }
    let directories = [
        game_directory.join("logs"),
        game_directory.join("_appdata_").join("logs"),
        game_directory.join("_appdata_").join("log"),
    ];
    let mut files = Vec::new();
    for directory in directories {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_crash_file(&path) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let name = entry.file_name().to_string_lossy().into_owned();
            let discovered = DiscoveredCrashLog {
                path,
                name,
                modified,
                length: metadata.len(),
            };
            if files.len() < max_results {
                files.push(discovered);
            } else if let Some(oldest_index) = files
                .iter()
                .enumerate()
                .max_by(|(_, left), (_, right)| compare_recent(left, right))
                .map(|(index, _)| index)
            {
                if files
                    .get(oldest_index)
                    .is_some_and(|oldest| compare_recent(&discovered, oldest).is_lt())
                {
                    if let Some(slot) = files.get_mut(oldest_index) {
                        *slot = discovered;
                    }
                }
            }
        }
    }
    files.sort_by(compare_recent);
    Ok(files)
}

fn compare_recent(left: &DiscoveredCrashLog, right: &DiscoveredCrashLog) -> std::cmp::Ordering {
    right
        .modified
        .cmp(&left.modified)
        .then_with(|| left.path.cmp(&right.path))
}

fn is_crash_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("log") || extension.eq_ignore_ascii_case("mdmp"))
}

/// Classifies a crash log and attaches only an exact catalogued signature.
#[must_use]
pub fn analyze_crash_log(text: &str, game: Option<&str>) -> CrashLogAnalysis {
    let kind = if contains_ascii_case_insensitive(text, "fatal error") {
        CrashKind::FatalError
    } else if contains_ascii_case_insensitive(text, "[lua][error]")
        || contains_ascii_case_insensitive(text, "lua error")
    {
        CrashKind::LuaError
    } else if contains_ascii_case_insensitive(text, "[error]") || contains_ascii_case_insensitive(text, "error:") {
        CrashKind::EngineError
    } else {
        CrashKind::Unknown
    };
    let summary = text
        .lines()
        .find(|line| {
            contains_ascii_case_insensitive(line, "error")
                || contains_ascii_case_insensitive(line, "expression")
                || contains_ascii_case_insensitive(line, "description")
        })
        .map_or_else(
            || "No recognized crash marker was found.".to_owned(),
            |line| line.trim().to_owned(),
        );
    CrashLogAnalysis {
        kind,
        summary,
        known_issue: CrashSignatureCatalog::match_log(text, game),
        faulting_module_offset: None,
    }
}

fn contains_ascii_case_insensitive(text: &str, needle: &str) -> bool {
    let needle = needle.as_bytes();
    !needle.is_empty()
        && text
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
}

const ALPHABET_SIZE: usize = 128;
const MISSING_STATE: u32 = u32::MAX;

struct AutomatonNode {
    transitions: [u32; ALPHABET_SIZE],
    failure: u32,
    outputs: Vec<usize>,
}

impl AutomatonNode {
    fn new() -> Self {
        Self {
            transitions: [MISSING_STATE; ALPHABET_SIZE],
            failure: 0,
            outputs: Vec::new(),
        }
    }
}

struct Automaton {
    nodes: Vec<AutomatonNode>,
}

static AUTOMATON: OnceLock<Automaton> = OnceLock::new();

impl Automaton {
    fn find(&self, text: &[u8], game: Option<&str>) -> Option<&'static CrashSignature> {
        let mut state = 0_u32;
        let mut best: Option<(usize, usize)> = None;
        for &byte in text {
            let symbol = usize::from(byte.to_ascii_lowercase());
            if symbol >= ALPHABET_SIZE {
                state = 0;
                continue;
            }
            state = self
                .nodes
                .get(state_index(state))
                .and_then(|node| node.transitions.get(symbol))
                .copied()
                .filter(|next| *next != MISSING_STATE)
                .unwrap_or(0);
            let Some(node) = self.nodes.get(state_index(state)) else {
                continue;
            };
            for signature_index in &node.outputs {
                let Some(signature) = SIGNATURES.get(*signature_index) else {
                    continue;
                };
                if !game_matches(signature.game, game) {
                    continue;
                }
                let length = signature.pattern.len();
                if best.is_none_or(|(_, best_length)| length > best_length) {
                    best = Some((*signature_index, length));
                }
            }
        }
        best.and_then(|(signature_index, _)| SIGNATURES.get(signature_index))
    }
}

fn state_index(state: u32) -> usize {
    usize::try_from(state).unwrap_or_default()
}

fn game_matches(signature_game: &str, requested_game: Option<&str>) -> bool {
    signature_game == "any"
        || match requested_game {
            None => true,
            Some(game) => normalize_crash_game(game) == Some(signature_game),
        }
}

fn normalize_crash_game(game: &str) -> Option<&'static str> {
    let game = game.trim();
    if ["cs", "cs-ee", "clear sky", "stalker-cs", "stalker-cs-ee"]
        .iter()
        .any(|alias| game.eq_ignore_ascii_case(alias))
    {
        Some("cs")
    } else if ["soc", "soc-ee", "shadow of chernobyl", "stalker-soc", "stalker-soc-ee"]
        .iter()
        .any(|alias| game.eq_ignore_ascii_case(alias))
    {
        Some("soc")
    } else {
        None
    }
}

fn build_automaton() -> Automaton {
    let mut nodes = vec![AutomatonNode::new()];
    for (signature_index, signature) in SIGNATURES.iter().enumerate() {
        let mut state = 0_u32;
        for byte in signature.pattern.bytes().map(|value| value.to_ascii_lowercase()) {
            let symbol = usize::from(byte);
            if symbol >= ALPHABET_SIZE {
                break;
            }
            let existing = nodes
                .get(state_index(state))
                .and_then(|node| node.transitions.get(symbol))
                .copied()
                .unwrap_or(MISSING_STATE);
            if existing != MISSING_STATE {
                state = existing;
                continue;
            }
            let Ok(new_state) = u32::try_from(nodes.len()) else {
                break;
            };
            if let Some(transition) = nodes
                .get_mut(state_index(state))
                .and_then(|node| node.transitions.get_mut(symbol))
            {
                *transition = new_state;
            }
            nodes.push(AutomatonNode::new());
            state = new_state;
        }
        if let Some(node) = nodes.get_mut(state_index(state)) {
            node.outputs.push(signature_index);
        }
    }

    let mut queue = VecDeque::new();
    let root_transitions = nodes.first().map(|node| node.transitions);
    if let Some(transitions) = root_transitions {
        for (symbol, child) in transitions.iter().copied().enumerate() {
            if child == MISSING_STATE {
                if let Some(root) = nodes.get_mut(0).and_then(|node| node.transitions.get_mut(symbol)) {
                    *root = 0;
                }
            } else {
                queue.push_back(child);
            }
        }
    }

    while let Some(state) = queue.pop_front() {
        let Some(state_node) = nodes.get(state_index(state)) else {
            continue;
        };
        let transitions = state_node.transitions;
        let failure = state_node.failure;
        for (symbol, child) in transitions.iter().copied().enumerate() {
            if child == MISSING_STATE {
                let fallback = nodes
                    .get(state_index(failure))
                    .and_then(|node| node.transitions.get(symbol))
                    .copied()
                    .unwrap_or(0);
                if let Some(transition) = nodes
                    .get_mut(state_index(state))
                    .and_then(|node| node.transitions.get_mut(symbol))
                {
                    *transition = fallback;
                }
                continue;
            }
            let fallback = nodes
                .get(state_index(failure))
                .and_then(|node| node.transitions.get(symbol))
                .copied()
                .unwrap_or(0);
            if let Some(child_node) = nodes.get_mut(state_index(child)) {
                child_node.failure = fallback;
            }
            let inherited = nodes
                .get(state_index(fallback))
                .map(|node| node.outputs.clone())
                .unwrap_or_default();
            if let Some(child_node) = nodes.get_mut(state_index(child)) {
                child_node.outputs.extend(inherited);
            }
            queue.push_back(child);
        }
    }
    Automaton { nodes }
}

#[cfg(test)]
mod tests;
