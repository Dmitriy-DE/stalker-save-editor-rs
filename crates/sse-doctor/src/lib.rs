//! Read-only save and crash diagnostics.

use sse_core::{Error, Result, SaveBuffer};
use sse_xray::writer::{self, Change, ChangeSet};
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
/// The result is unknown whenever the actor info list or the matching creature STATE cannot be read safely.
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
        let mut npc_found = 0_usize;
        let mut npc_alive = false;
        for vitals in npc_vitals
            .iter()
            .filter(|vitals| vitals.section.eq_ignore_ascii_case(rule.npc_section))
        {
            npc_found = npc_found.saturating_add(1);
            npc_alive |= !vitals.is_dead;
        }
        if npc_found > 1 {
            (
                QuestTaskStatus::Unknown,
                "npc-ambiguous",
                "More than one creature record matches the quest NPC; the save does not identify which one is the quest giver.",
            )
        } else if npc_found == 0 {
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
    analyze_quests_from_save(&save)
}

/// Evaluates quest rules against an already parsed save to avoid building a second index.
#[must_use]
pub fn analyze_quests_from_save(save: &Save) -> QuestDoctorReport {
    let format_id = save.format().id();
    let known_info = save.actor_known_info();
    let mut npc_vitals = Vec::new();
    for rule in QUEST_RULES.iter().filter(|rule| rule.format_id == format_id) {
        npc_vitals.extend(
            save.find_creature_vitals(rule.npc_section)
                .into_iter()
                .map(|vitals| QuestNpcVitals {
                    section: rule.npc_section.to_owned(),
                    is_dead: vitals.is_dead(),
                }),
        );
    }
    let states = evaluate_quest_facts(format_id, known_info, &npc_vitals);
    let quest_states_available = !states.is_empty();
    let broken = states
        .iter()
        .filter(|state| state.status == QuestTaskStatus::Broken)
        .count();
    let has_unknown = states.iter().any(|state| state.status == QuestTaskStatus::Unknown);
    let status = if broken > 0 {
        SaveDoctorStatus::Warning
    } else if has_unknown || !quest_states_available {
        SaveDoctorStatus::Unknown
    } else {
        SaveDoctorStatus::Ok
    };
    let summary = if !quest_states_available {
        "Quest Doctor has evidence-backed rules only for Shadow of Chernobyl and Clear Sky; no states or repairs can be inferred for this format.".to_owned()
    } else if broken > 0 {
        format!("{broken} known broken quest(s) found; each can be repaired by adding one info portion.")
    } else if has_unknown {
        "Quest state remains unknown where the save does not prove both the actor flag list and NPC state.".to_owned()
    } else {
        "No known broken quest was found. Only the listed rules are checked.".to_owned()
    };
    QuestDoctorReport {
        status,
        format_id: Some(format_id),
        quest_states_available,
        summary,
        states,
    }
}

/// Prepares the missing actor info portions only when the save proves a known broken quest.
pub fn prepare_quest_repair(data: &[u8]) -> Result<Option<SaveBuffer>> {
    let save = Save::read(data)?;
    let report = analyze_quests_from_save(&save);
    if report.status == SaveDoctorStatus::Error {
        return Err(Error::damaged(report.summary));
    }
    let info_portions = repairable_info_portions(&report.states);
    if info_portions.is_empty() {
        return Ok(None);
    }
    let prepared = writer::apply(
        &save,
        &ChangeSet::new(vec![Change::AddInfoPortions {
            target_object: save.actor_id(),
            info_portions,
        }]),
    )?;
    verify_quest_repair(prepared.as_slice())?;
    Ok(Some(prepared))
}

/// Info portions to add: broken states whose flag works without a separate game fix, each once.
///
/// A state that `needs_preventing_fix` stays broken after the flag is added, so it is never repaired here.
fn repairable_info_portions(states: &[QuestTaskState]) -> Vec<String> {
    states
        .iter()
        .filter(|state| state.status == QuestTaskStatus::Broken && !state.needs_preventing_fix)
        .filter_map(|state| state.missing_info.map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Verifies that a written save parses and no evidence-backed quest remains broken.
pub fn verify_quest_repair(written: &[u8]) -> Result<()> {
    let report = analyze_quests(written);
    if !report.quest_states_available
        || report.status == SaveDoctorStatus::Error
        || report
            .states
            .iter()
            .any(|state| state.status == QuestTaskStatus::Broken)
    {
        return Err(Error::damaged(
            "the repaired save still reports a broken or unreadable quest state",
        ));
    }
    Ok(())
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

fn parse_app_state(text: &str) -> Option<AppStateFields> {
    let document = sse_codecs::vdf::parse(text).ok()?;
    let app_state = document.get_object("AppState")?;
    Some(AppStateFields {
        app_id: app_state.get_string("appid").map(str::to_owned),
        install_dir: app_state.get_string("installdir").map(str::to_owned),
        build_id: app_state.get_string("buildid").map(str::to_owned),
    })
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

/// One crash signature from the C# diagnostic catalog.
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
    /// Evidence and cause description.
    pub explanation: &'static str,
    /// C# regular-expression pattern, matched case-insensitively by the bounded safe matcher.
    pub pattern: &'static str,
    /// Evidence note or reference from the C# catalog.
    pub source: &'static str,
    /// Linked fix identifier, when the crash has an applicable bundled fix.
    pub fix_id: Option<&'static str>,
    /// Linked Quest Doctor rule, when the crash has a validated save repair.
    pub quest_rule_id: Option<&'static str>,
}

const CRASH_SOURCE_SRP: &str = "https://github.com/Decane/SRP/blob/master/SRP%20v1.1.5%20-%20Version%20History.txt";
const CRASH_SOURCE_ZRP: &str = "ZRP 1.09 XR3a, gamedata/docs/CrashesStillInTheGame.txt (metacognix.com)";
const CRASH_SOURCE_PLAYERS: &str = "Players' crash logs, Steam discussions of the three games (2026-10)";

const SIGNATURES: [CrashSignature; 47] = [
    CrashSignature {
        id: "cs.wrong-target-wild-napr",
        game: "cs",
        title: "Task targets Wild Napr after his death",
        advice: CrashAdvice::RepairSave,
        explanation: "A Flea Market task was given with Wild Napr as its target after he died offline.",
        pattern: "wrong target for storyline quest:\\s*logic@work5,\\s*gar_smart_terrain_6_3",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.quest.dead-wild-napr"),
        quest_rule_id: Some("cs.wild-napr-dead"),
    },
    CrashSignature {
        id: "cs.insufficient-smart-jobs",
        game: "cs",
        title: "Too many stalkers for one camp",
        advice: CrashAdvice::InstallFix,
        explanation: "More squads were sent to a smart terrain than it has jobs (Dark Valley wagon, Army Warehouses rocks and others).",
        pattern: "Insufficient smart_terrain jobs",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.smart-terrain-no-free-job"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.hospital-jump-down-animation",
        game: "cs",
        title: "Hospital enemy jumps down with a weapon the animation is not listed for",
        advice: CrashAdvice::InstallFix,
        explanation: "The jump animation of one Limansk hospital enemy is listed for a single weapon type; with any other weapon the script stops the game.",
        pattern: "cant find animation for slot",
        source: "https://steamcommunity.com/app/20510/discussions/0/3132792921893743264/",
        fix_id: Some("cs.crash.hospital-jump-down-animation"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.squad-hint-unknown-target",
        game: "cs",
        title: "Map hint of an attacking squad whose target camp is unknown",
        advice: CrashAdvice::InstallFix,
        explanation: "The hint of a squad on its way to attack names the target camp; when the camp is not in the simulation table the script stops the game.",
        pattern: "sim_squad_generic\\.script:\\d+:\\s*attempt to index field '\\?' \\(a nil value\\)",
        source: "https://steamcommunity.com/app/20510/discussions/0/1471967529575318261/",
        fix_id: Some("cs.crash.squad-action-finished-twice"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.pstor-unknown-type",
        game: "cs",
        title: "Saved NPC data cannot be read back",
        advice: CrashAdvice::CorruptSave,
        explanation: "While loading, the stored variables of an object contain a value type the game does not know: the save holds another object's data at this place.",
        pattern: "pstor_load_all: not registered type N \\d+ encountered",
        source: "https://steamcommunity.com/app/20510/discussions/0/558747922713833080",
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.sim-combat-actor-nil",
        game: "cs",
        title: "Loading a save during a squad fight",
        advice: CrashAdvice::InstallFix,
        explanation: "sim_combat.script reads the actor before it exists right after a save is loaded; loading again usually works.",
        pattern: "sim_combat\\.script:\\d+:\\s*attempt to index field 'actor' \\(a nil value\\)",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.sim-combat"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.sim-combat-attack-squad-nil",
        game: "cs",
        title: "Help task for a squad that no longer exists",
        advice: CrashAdvice::InstallFix,
        explanation: "The game evaluated a 'help' task for an attacking squad that was already gone.",
        pattern: "sim_combat\\.script:\\d+:\\s*attempt to index local 'attack_squad_obj'",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.sim-combat"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.squad-current-action-nil",
        game: "cs",
        title: "Smart terrain captured by a squad without an action",
        advice: CrashAdvice::InstallFix,
        explanation: "A squad captured a smart terrain while it had no current action.",
        pattern: "sim_squad_generic\\.script:\\d+:\\s*attempt to index field 'current_action'",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.squad-action-finished-twice"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.squad-help-task-nil",
        game: "cs",
        title: "'Help' task with nothing to offer",
        advice: CrashAdvice::InstallFix,
        explanation: "The game tried to offer a delayed defence ('help') task, but no task fitted.",
        pattern: "sim_squad_generic\\.script:\\d+:\\s*attempt to index local 'task' \\(a nil value\\)",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.squad-action-finished-twice"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.monster-squad-missing",
        game: "cs",
        title: "Mutant whose squad no longer exists",
        advice: CrashAdvice::InstallFix,
        explanation: "A mutant went online, offline or died after its squad had been removed.",
        pattern: "se_monster\\.script:\\d+:\\s*attempt to index local 'squad' \\(a nil value\\)",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.monster-squad-missing"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.heli-save-search",
        game: "cs",
        title: "Saving while a helicopter searches for you",
        advice: CrashAdvice::InstallFix,
        explanation: "The helicopter's search timers were not set yet when the game was saved.",
        pattern: "heli_combat\\.script:\\d+:\\s*attempt to perform arithmetic on field 'change_(?:dir|pos)_time'",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.heli-save-search"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.marsh-creature-no-squad",
        game: "cs",
        title: "Marsh creature attacked a stalker without a squad",
        advice: CrashAdvice::InstallFix,
        explanation: "The marsh creature ambush tried to make the victim's squad react, but the victim had no squad.",
        pattern: "sr_bloodsucker\\.script:\\d+:\\s*attempt to index field 'npc_squad'",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.marsh-creature-no-squad"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.agroprom-orest-path",
        game: "cs",
        title: "Orest left his spot at the Agroprom loner base",
        advice: CrashAdvice::InstallFix,
        explanation: "Orest's movement restrictor does not contain his own patrol path.",
        pattern: "patrol path \\[agr_stalker_leader_walk\\] is inaccessible",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.agroprom-orest-path"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.all-spawn-cordon-waypoint",
        game: "cs",
        title: "Waypoint off the AI map at the Cordon bonfire",
        advice: CrashAdvice::InstallFix,
        explanation: "A waypoint of the 'Bonfire in forest' camp lies outside the AI map.",
        pattern: "esc_smart_terrain_3_7_walker_1_walk",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.all-spawn-errors"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.storyline-task-missing-npc",
        game: "cs",
        title: "Story task for an NPC who is not there (Wild Napr)",
        advice: CrashAdvice::InstallFix,
        explanation: "The game tried to give a story task whose target NPC is fighting or has died offline.",
        pattern: "wrong target for storyline quest",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.capture-task-missing-squad"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.all-spawn-jobs-mil-2-1",
        game: "cs",
        title: "Too many squads for the Army Warehouses 'Camp amidst rocks'",
        advice: CrashAdvice::InstallFix,
        explanation: "The camp accepts more squads than it has jobs. The fix applies in a new game.",
        pattern: "Insufficient smart_terrain jobs mil_smart_terrain_2_1",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.all-spawn-errors"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.all-spawn-mil-path",
        game: "cs",
        title: "Missing path between Army Warehouses camps",
        advice: CrashAdvice::InstallFix,
        explanation: "A camp's list of neighbours misses a link that mutant attacks use. The fix applies in a new game.",
        pattern: "Path between \\[mil_smart_terrain_7_11\\] and \\[mil_smart_terrain_7_10\\] doesnt exist",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.all-spawn-errors"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.missing-backpack-model",
        game: "cs",
        title: "Missing backpack model",
        advice: CrashAdvice::InstallFix,
        explanation: "The stalker corpse model points at a file Clear Sky does not ship.",
        pattern: "Can't find model file 'dynamics\\\\equipments\\\\item_rukzak\\.ogf'",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.missing-backpack-model"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.kamp-empty-interval",
        game: "cs",
        title: "Campfire with nobody to talk",
        advice: CrashAdvice::InstallFix,
        explanation: "The campfire story scheme picked a random speaker from an empty list.",
        pattern: "xr_kamp\\.script:\\d+:\\s*bad argument #1 to 'random' \\(interval is empty\\)",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.kamp-no-animation"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.robbery-squad-left",
        game: "cs",
        title: "Robbers left during a hold-up",
        advice: CrashAdvice::InstallFix,
        explanation: "A robber squad walked off to another camp in the middle of a hold-up.",
        pattern: "sr_robbery\\.script:\\d+:\\s*attempt to index field '\\?' \\(a nil value\\)",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.robbery-squad-left"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.robbery-manager-nil",
        game: "cs",
        title: "Robbery leader chosen from a squad that already left",
        advice: CrashAdvice::InstallFix,
        explanation: "The robbery scheme still counted a squad that had left the camp when it picked the leader.",
        pattern: "actor_reaction\\.script:\\d+:\\s*attempt to index local 'manager'",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.robbery-leader-offline"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.capture-task-missing-squad",
        game: "cs",
        title: "Capture task for a squad that does not exist",
        advice: CrashAdvice::InstallFix,
        explanation: "The game tried to give a 'capture' task to a squad that no longer exists.",
        pattern: "task_objects\\.script:\\d+:\\s*attempt to index field '\\?' \\(a nil value\\)",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.capture-task-missing-squad"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.anomaly-art-nil",
        game: "cs",
        title: "Artefact spawn in an anomaly field",
        advice: CrashAdvice::InstallFix,
        explanation: "An anomaly field referenced an artefact that was already gone.",
        pattern: "bind_anomaly_zone\\.script:\\d+:\\s*attempt to index local 'art'",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.anomaly-zone-missing-artefact"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.saving-too-much",
        game: "cs",
        title: "Save data too large",
        advice: CrashAdvice::CommunityPatch,
        explanation: "The scripts wrote more data into a save packet than the engine allows.",
        pattern: "You are saving too much",
        source: CRASH_SOURCE_SRP,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.patrol-point-cordon-bonfire",
        game: "cs",
        title: "Patrol point at the Cordon forest bonfire",
        advice: CrashAdvice::InstallFix,
        explanation: "A stalker patrolling the 'Bonfire in forest' reached a waypoint that is not on the level graph.",
        pattern: "patrol path\\s*\\[esc_smart_terrain_3_7_walker_1_walk\\]",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.all-spawn-errors"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.patrol-red-forest-trader",
        game: "cs",
        title: "Red Forest mine trader left his desk",
        advice: CrashAdvice::InstallFix,
        explanation: "The trader in the mine strayed from his spot and his patrol path became unreachable.",
        pattern: "patrol path\\s*\\[red_smart_terrain_3_2_patrol_1_walk\\] is inaccessible",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.red-forest-mine-trader-path"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.patrol-agroprom-orest",
        game: "cs",
        title: "Orest displaced in Agroprom",
        advice: CrashAdvice::InstallFix,
        explanation: "Orest was pushed out of his space restrictor and his walk path became unreachable.",
        pattern: "patrol path\\s*\\[agr_stalker_leader_walk\\] is inaccessible",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.agroprom-orest-path"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.missing-rukzak-model",
        game: "cs",
        title: "Missing backpack model",
        advice: CrashAdvice::InstallFix,
        explanation: "The game referenced a backpack mesh that is not shipped.",
        pattern: "Can't find model file 'dynamics\\\\equipments\\\\item_rukzak\\.ogf'",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.missing-backpack-model"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.treasure-box-in-use",
        game: "cs",
        title: "Stash refilled while Stringov is alive",
        advice: CrashAdvice::InstallFix,
        explanation: "Re-entering the Garbage tried to fill a stash that was already filled.",
        pattern: "Unable to give treasure \\[gar_treasure_quest_smuggler_weapons\\]",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.treasure-given-twice"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.red-forest-missing-squad",
        game: "cs",
        title: "Witch Circle ambush squad already dead",
        advice: CrashAdvice::InstallFix,
        explanation: "Following Strelok's helper into the ambush after the ambush squad was killed.",
        pattern: "There is no squad \\[red_pursuit_bounty_hunters_squad_\\d+\\] in sim_board",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.relation-to-missing-squad"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "cs.military-dogs-path",
        game: "cs",
        title: "Army Warehouses mutant attack path",
        advice: CrashAdvice::InstallFix,
        explanation: "A mutant squad attacking the military base had no path between two smart terrains (new game needed after the patch).",
        pattern: "Path between \\[mil_smart_terrain_7_11\\] and \\[mil_smart_terrain_7_10\\] doesnt exist",
        source: CRASH_SOURCE_SRP,
        fix_id: Some("cs.crash.all-spawn-errors"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.gulag-job-nil",
        game: "soc",
        title: "Camp member without a job",
        advice: CrashAdvice::InstallFix,
        explanation: "A camp checked a job swap for a member that had no job at that moment.",
        pattern: "xr_gulag\\.script:\\d+:\\s*attempt to index local 'job' \\(a nil value\\)",
        source: CRASH_SOURCE_ZRP,
        fix_id: Some("soc.crash.gulag-job-nil"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.heli-save-search",
        game: "soc",
        title: "Saving while a helicopter searches for you",
        advice: CrashAdvice::InstallFix,
        explanation: "The helicopter's search timers were not set yet when the game was saved.",
        pattern: "heli_combat\\.script:\\d+:\\s*attempt to perform arithmetic on field 'change_(?:dir|pos)_time'",
        source: CRASH_SOURCE_ZRP,
        fix_id: Some("soc.crash.heli-save-search"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.kamp-remove-unseated",
        game: "soc",
        title: "Campfire with more stalkers than places",
        advice: CrashAdvice::InstallFix,
        explanation: "A stalker joined a full campfire and got no place.",
        pattern: "xr_kamp\\.script:\\d+:\\s*attempt to index field '\\?' \\(a nil value\\)|get dest Vertex: nil",
        source: CRASH_SOURCE_ZRP,
        fix_id: Some("soc.crash.kamp-remove-unseated"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.danger-ignore-types",
        game: "soc",
        title: "NPC without danger settings",
        advice: CrashAdvice::InstallFix,
        explanation: "An NPC whose danger settings were never set up noticed a grenade, a body, a hit or a sound.",
        pattern: "xr_danger\\.script:\\d+:\\s*attempt to index field 'ignore_types' \\(a nil value\\)",
        source: CRASH_SOURCE_ZRP,
        fix_id: Some("soc.crash.danger-ignore-types"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.garbage-robbers-dead-bandit",
        game: "soc",
        title: "Garbage robbery with the first robber dead",
        advice: CrashAdvice::InstallFix,
        explanation: "The fight at the Garbage entrance used a robber who was already dead.",
        pattern: "xr_effects\\.script:\\d+:\\s*attempt to index local 'bandit1' \\(a nil value\\)",
        source: CRASH_SOURCE_ZRP,
        fix_id: Some("soc.crash.garbage-robbers-and-duty-raid"),
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.controller-body-state",
        game: "soc",
        title: "Controller animation crash",
        advice: CrashAdvice::ReloadEarlierSave,
        explanation: "A bad controller animation, usually while it is under attack. Kill controllers before they reach this state.",
        pattern: "dBodyStateValide\\(b\\)",
        source: CRASH_SOURCE_ZRP,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.entity-not-found",
        game: "soc",
        title: "Dropped weapon vanished while an NPC evaluated it",
        advice: CrashAdvice::ReloadEarlierSave,
        explanation: "A killed NPC's weapon was destroyed or fell through the ground while another NPC considered picking it up.",
        pattern: "entity not found\\.\\s*id_parent=\\d+\\s*id_entity=\\d+",
        source: CRASH_SOURCE_ZRP,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.map-location-dead-object",
        game: "soc",
        title: "Map spot bound to a destroyed body",
        advice: CrashAdvice::CorruptSave,
        explanation: "The game destroyed a body but kept its map spot; every later save carries the damage.",
        pattern: "(?:SMapLocation|CMapLocation::UpdateSpot) binded to non-existent object",
        source: CRASH_SOURCE_ZRP,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.no-level-in-graph",
        game: "soc",
        title: "Creature spawned outside the level",
        advice: CrashAdvice::ReloadEarlierSave,
        explanation: "A mutant or NPC was spawned outside the level or below it.",
        pattern: "there is no specified level in the game graph|There is no proper graph point neighbour",
        source: CRASH_SOURCE_ZRP,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.unknown-weapon-rank",
        game: "soc",
        title: "Weapon missing from the rank table",
        advice: CrashAdvice::CommunityPatch,
        explanation: "A weapon (usually from a mod) has no entry in the weapon rank table.",
        pattern: "cannot find rank for",
        source: CRASH_SOURCE_ZRP,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "soc.format-no-value",
        game: "soc",
        title: "Script string formatting error",
        advice: CrashAdvice::CommunityPatch,
        explanation: "A script passed nothing to string.format; usually an incompatible mod.",
        pattern: "bad argument #2 to 'format' \\(string expected, got no value\\)",
        source: CRASH_SOURCE_ZRP,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "any.missing-model",
        game: "any",
        title: "A model file is missing",
        advice: CrashAdvice::RepairInstallation,
        explanation: "The game asked for a model that is not on disk: files left by a removed mod name it, or game files are missing.",
        pattern: "Can't find model file '",
        source: CRASH_SOURCE_PLAYERS,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "any.missing-section",
        game: "any",
        title: "A config section is missing",
        advice: CrashAdvice::RepairInstallation,
        explanation: "A config names a section no file defines: loose configs of a mod do not match the rest of the game.",
        pattern: "Can't open section '",
        source: CRASH_SOURCE_PLAYERS,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "any.missing-config-value",
        game: "any",
        title: "A config value is missing",
        advice: CrashAdvice::RepairInstallation,
        explanation: "A section lacks a value the engine needs: the config comes from another version of the game or from a mod.",
        pattern: "Can't find variable \\S+ in \\[",
        source: CRASH_SOURCE_PLAYERS,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "any.missing-string-table",
        game: "any",
        title: "Text files are missing",
        advice: CrashAdvice::RepairInstallation,
        explanation: "The list of text files was not found: the language set in localization.ltx is not installed, or game files are missing.",
        pattern: "string table xml file not found",
        source: CRASH_SOURCE_PLAYERS,
        fix_id: None,
        quest_rule_id: None,
    },
    CrashSignature {
        id: "any.config-not-opened",
        game: "any",
        title: "A game file could not be opened",
        advice: CrashAdvice::RepairInstallation,
        explanation: "A file the game needs was not opened (hFile>0): it is missing, locked or damaged by an edit.",
        pattern: "Expression\\s*:\\s*hFile>0",
        source: CRASH_SOURCE_PLAYERS,
        fix_id: None,
        quest_rule_id: None,
    },
];

/// Compiled crash catalog; literal prefixes are scanned together, then candidates are checked by a bounded regex NFA.
pub struct CrashSignatureCatalog;

impl CrashSignatureCatalog {
    /// Returns the signatures in the C# 1.3.1 catalog.
    #[must_use]
    pub const fn all() -> &'static [CrashSignature] {
        &SIGNATURES
    }

    /// Finds the first C# catalog match, optionally restricting matches to one game.
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

/// Boundary for checked minidump readers used by crash analysis.
pub trait CrashDumpReader: Sync {
    /// Reads a bounded minidump and returns only checked textual crash facts.
    ///
    /// # Errors
    /// Returns an error for malformed or unsupported dump streams.
    fn read_minidump(&self, bytes: &[u8]) -> Result<CrashDumpFacts>;
}

/// Default adapter over the bounded minidump reader in `sse-codecs`.
#[derive(Debug, Default, Clone, Copy)]
pub struct CodecMinidumpReader;

impl CrashDumpReader for CodecMinidumpReader {
    fn read_minidump(&self, bytes: &[u8]) -> Result<CrashDumpFacts> {
        let dump = sse_codecs::minidump::Minidump::parse(bytes)?;
        let exception = dump
            .exception()?
            .ok_or_else(|| Error::Refused("minidump has no exception record".to_owned()))?;
        let faulting_module_offset = dump
            .faulting_stack(1)?
            .into_iter()
            .next()
            .map(|frame| format!("{}+0x{:X}", frame.module, frame.offset));
        Ok(CrashDumpFacts {
            message: format!("Windows exception 0x{:08X}", exception.code),
            faulting_module_offset,
        })
    }
}

const CRASH_LOG_TAIL_BYTES: u64 = 256 * 1024;
const MAXIMUM_DISCOVERED_CRASH_LOGS: usize = 500;

/// Reads at most the last 256 KiB of one explicit text log or parses a minidump with `sse-codecs`.
///
/// # Errors
/// Returns an I/O error for an unreadable file, or `Refused` for a minidump that this crate cannot parse.
pub fn analyze_crash_file(path: &Path, game: Option<&str>) -> Result<CrashLogAnalysis> {
    analyze_crash_file_with_dump_reader(path, game, Some(&CodecMinidumpReader))
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
        if analysis.summary == "No recognized crash marker was found." {
            analysis.summary = facts.message.clone();
        }
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

#[derive(Clone, Copy)]
enum RegexCharacter {
    Literal(u8),
    Any,
    Digit,
    Whitespace,
    NonWhitespace,
}

impl RegexCharacter {
    fn matches(self, byte: u8) -> bool {
        match self {
            Self::Literal(expected) => byte.eq_ignore_ascii_case(&expected),
            Self::Any => true,
            Self::Digit => byte.is_ascii_digit(),
            Self::Whitespace => matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c),
            Self::NonWhitespace => !matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegexQuantifier {
    One,
    Optional,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Clone, Copy)]
struct RegexToken {
    character: RegexCharacter,
    quantifier: RegexQuantifier,
}

struct NfaTransition {
    character: RegexCharacter,
    target: usize,
}

#[derive(Default)]
struct NfaState {
    epsilon: Vec<usize>,
    transitions: Vec<NfaTransition>,
}

struct RegexVariant {
    anchor: Vec<u8>,
    #[cfg(test)]
    example: Vec<u8>,
    states: Vec<NfaState>,
    epsilon_closures: Vec<Vec<usize>>,
    accept: usize,
}

impl RegexVariant {
    fn compile(tokens: &[RegexToken]) -> Option<Self> {
        let mut states = vec![NfaState::default()];
        let mut current = 0_usize;
        for token in tokens {
            match token.quantifier {
                RegexQuantifier::One => {
                    let next = append_nfa_state(&mut states);
                    states.get_mut(current)?.transitions.push(NfaTransition {
                        character: token.character,
                        target: next,
                    });
                    current = next;
                }
                RegexQuantifier::Optional => {
                    let next = append_nfa_state(&mut states);
                    let state = states.get_mut(current)?;
                    state.epsilon.push(next);
                    state.transitions.push(NfaTransition {
                        character: token.character,
                        target: next,
                    });
                    current = next;
                }
                RegexQuantifier::ZeroOrMore => {
                    let next = append_nfa_state(&mut states);
                    let state = states.get_mut(current)?;
                    state.epsilon.push(next);
                    state.transitions.push(NfaTransition {
                        character: token.character,
                        target: current,
                    });
                    current = next;
                }
                RegexQuantifier::OneOrMore => {
                    let repeated = append_nfa_state(&mut states);
                    let next = append_nfa_state(&mut states);
                    states.get_mut(current)?.transitions.push(NfaTransition {
                        character: token.character,
                        target: repeated,
                    });
                    let repeated_state = states.get_mut(repeated)?;
                    repeated_state.epsilon.push(next);
                    repeated_state.transitions.push(NfaTransition {
                        character: token.character,
                        target: repeated,
                    });
                    current = next;
                }
            }
        }

        let anchor = tokens
            .iter()
            .take_while(|token| token.quantifier == RegexQuantifier::One)
            .map(|token| match token.character {
                RegexCharacter::Literal(byte) => Some(byte.to_ascii_lowercase()),
                RegexCharacter::Any
                | RegexCharacter::Digit
                | RegexCharacter::Whitespace
                | RegexCharacter::NonWhitespace => None,
            })
            .collect::<Option<Vec<_>>>()?;
        if anchor.is_empty() {
            return None;
        }
        #[cfg(test)]
        let example = tokens
            .iter()
            .map(|token| match token.character {
                RegexCharacter::Literal(byte) => byte,
                RegexCharacter::Any | RegexCharacter::NonWhitespace => b'x',
                RegexCharacter::Digit => b'7',
                RegexCharacter::Whitespace => b' ',
            })
            .collect();

        let mut epsilon_closures = Vec::with_capacity(states.len());
        for start in 0..states.len() {
            let mut visited = vec![false; states.len()];
            let mut pending = vec![start];
            let mut closure = Vec::new();
            while let Some(state_index) = pending.pop() {
                if visited.get(state_index).copied().unwrap_or(true) {
                    continue;
                }
                if let Some(visited_state) = visited.get_mut(state_index) {
                    *visited_state = true;
                }
                closure.push(state_index);
                if let Some(state) = states.get(state_index) {
                    pending.extend(state.epsilon.iter().copied());
                }
            }
            epsilon_closures.push(closure);
        }

        Some(Self {
            anchor,
            #[cfg(test)]
            example,
            states,
            epsilon_closures,
            accept: current,
        })
    }

    fn matches_at(&self, text: &[u8], start: usize) -> bool {
        let mut active = vec![false; self.states.len()];
        let Some(start_closure) = self.epsilon_closures.first() else {
            return false;
        };
        for state in start_closure {
            if let Some(slot) = active.get_mut(*state) {
                *slot = true;
            }
        }
        if active.get(self.accept).copied().unwrap_or(false) {
            return true;
        }

        let Some(input) = text.get(start..) else {
            return false;
        };
        let mut next = vec![false; self.states.len()];
        for byte in input {
            next.fill(false);
            for (state_index, is_active) in active.iter().enumerate() {
                if !is_active {
                    continue;
                }
                let Some(state) = self.states.get(state_index) else {
                    continue;
                };
                for transition in &state.transitions {
                    if !transition.character.matches(*byte) {
                        continue;
                    }
                    if let Some(closure) = self.epsilon_closures.get(transition.target) {
                        for destination in closure {
                            if let Some(slot) = next.get_mut(*destination) {
                                *slot = true;
                            }
                        }
                    }
                }
            }
            std::mem::swap(&mut active, &mut next);
            if active.get(self.accept).copied().unwrap_or(false) {
                return true;
            }
            if !active.iter().any(|is_active| *is_active) {
                return false;
            }
        }
        false
    }
}

fn append_nfa_state(states: &mut Vec<NfaState>) -> usize {
    states.push(NfaState::default());
    states.len().saturating_sub(1)
}

struct RegexProgram {
    variants: Vec<RegexVariant>,
}

impl RegexProgram {
    fn compile(pattern: &str) -> Option<Self> {
        if pattern.is_empty() || pattern.len() > 4096 {
            return None;
        }
        let bytes = pattern.as_bytes();
        let mut cursor = 0_usize;
        let variants = parse_regex_expression(bytes, &mut cursor, false)?;
        if cursor != bytes.len() {
            return None;
        }
        let variants = variants
            .iter()
            .map(|tokens| RegexVariant::compile(tokens))
            .collect::<Option<Vec<_>>>()?;
        Some(Self { variants })
    }
}

fn parse_regex_expression(bytes: &[u8], cursor: &mut usize, grouped: bool) -> Option<Vec<Vec<RegexToken>>> {
    let mut branches = vec![Vec::new()];
    let mut active_branch_start = 0_usize;
    while let Some(byte) = bytes.get(*cursor).copied() {
        if byte == b'|' {
            *cursor = cursor.checked_add(1)?;
            active_branch_start = branches.len();
            branches.push(Vec::new());
            continue;
        }
        if byte == b')' {
            if !grouped {
                return None;
            }
            *cursor = cursor.checked_add(1)?;
            return Some(branches);
        }
        let alternatives = parse_regex_atom(bytes, cursor)?;
        let quantifier = match bytes.get(*cursor).copied() {
            Some(b'?') => RegexQuantifier::Optional,
            Some(b'*') => RegexQuantifier::ZeroOrMore,
            Some(b'+') => RegexQuantifier::OneOrMore,
            _ => RegexQuantifier::One,
        };
        if quantifier != RegexQuantifier::One {
            *cursor = cursor.checked_add(1)?;
        }
        let mut alternatives = alternatives;
        if quantifier != RegexQuantifier::One {
            if alternatives.iter().any(|variant| variant.len() != 1) {
                return None;
            }
            for variant in &mut alternatives {
                if let Some(token) = variant.first_mut() {
                    token.quantifier = quantifier;
                }
            }
        }
        let mut combined = branches.iter().take(active_branch_start).cloned().collect::<Vec<_>>();
        for prefix in branches.iter().skip(active_branch_start) {
            for suffix in &alternatives {
                let mut variant = prefix.clone();
                variant.extend_from_slice(suffix);
                combined.push(variant);
            }
        }
        branches = combined;
    }
    if grouped {
        None
    } else {
        Some(branches)
    }
}

fn parse_regex_atom(bytes: &[u8], cursor: &mut usize) -> Option<Vec<Vec<RegexToken>>> {
    let byte = bytes.get(*cursor).copied()?;
    if byte == b'(' {
        let marker_end = cursor.checked_add(3)?;
        if bytes.get(*cursor..marker_end) != Some(b"(?:") {
            return None;
        }
        *cursor = marker_end;
        return parse_regex_expression(bytes, cursor, true);
    }
    *cursor = cursor.checked_add(1)?;
    let character = if byte == b'\\' {
        let escaped = bytes.get(*cursor).copied()?;
        *cursor = cursor.checked_add(1)?;
        match escaped {
            b'd' => RegexCharacter::Digit,
            b's' => RegexCharacter::Whitespace,
            b'S' => RegexCharacter::NonWhitespace,
            other => RegexCharacter::Literal(other),
        }
    } else if byte == b'.' {
        RegexCharacter::Any
    } else {
        RegexCharacter::Literal(byte)
    };
    Some(vec![vec![RegexToken {
        character,
        quantifier: RegexQuantifier::One,
    }]])
}

struct AutomatonNode {
    transitions: [u32; ALPHABET_SIZE],
    failure: u32,
    outputs: Vec<AutomatonOutput>,
}

#[derive(Clone, Copy)]
struct AutomatonOutput {
    signature_index: usize,
    variant_index: usize,
    anchor_len: usize,
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
    programs: Vec<RegexProgram>,
}

static AUTOMATON: OnceLock<Automaton> = OnceLock::new();

impl Automaton {
    fn find(&self, text: &[u8], game: Option<&str>) -> Option<&'static CrashSignature> {
        let mut state = 0_u32;
        let mut best: Option<usize> = None;
        for (offset, &byte) in text.iter().enumerate() {
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
            for output in &node.outputs {
                let Some(signature) = SIGNATURES.get(output.signature_index) else {
                    continue;
                };
                if !game_matches(signature.game, game) {
                    continue;
                }
                let Some(program) = self.programs.get(output.signature_index) else {
                    continue;
                };
                let Some(variant) = program.variants.get(output.variant_index) else {
                    continue;
                };
                let anchor_end = offset.saturating_add(1);
                let start = anchor_end.saturating_sub(output.anchor_len);
                if variant.matches_at(text, start) && best.is_none_or(|best_index| output.signature_index < best_index)
                {
                    best = Some(output.signature_index);
                }
            }
        }
        best.and_then(|signature_index| SIGNATURES.get(signature_index))
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

/// Whether a `--game` value names a game whose crash signatures can be matched.
#[must_use]
pub fn is_known_crash_game(game: &str) -> bool {
    normalize_crash_game(game).is_some()
}

fn normalize_crash_game(game: &str) -> Option<&'static str> {
    let game = game.trim();
    if ["cs", "cs-ee", "clear sky", "stalker-cs", "stalker-cs-ee"]
        .iter()
        .any(|alias| game.eq_ignore_ascii_case(alias))
    {
        Some("cs")
    } else if ["cop", "cop-ee", "call of pripyat", "stalker-cop", "stalker-cop-ee"]
        .iter()
        .any(|alias| game.eq_ignore_ascii_case(alias))
    {
        // Call of Pripyat has no crash signatures in the catalog yet; it is a known name with an empty match set.
        Some("cop")
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
    let mut programs = Vec::with_capacity(SIGNATURES.len());
    for (signature_index, signature) in SIGNATURES.iter().enumerate() {
        let Some(program) = RegexProgram::compile(signature.pattern) else {
            programs.push(RegexProgram { variants: Vec::new() });
            continue;
        };
        for (variant_index, variant) in program.variants.iter().enumerate() {
            let mut state = 0_u32;
            for byte in &variant.anchor {
                let symbol = usize::from(*byte);
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
                node.outputs.push(AutomatonOutput {
                    signature_index,
                    variant_index,
                    anchor_len: variant.anchor.len(),
                });
            }
        }
        programs.push(program);
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
    Automaton { nodes, programs }
}

#[cfg(test)]
mod tests;
