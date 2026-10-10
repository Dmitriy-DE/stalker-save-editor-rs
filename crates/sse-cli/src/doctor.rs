use std::path::Path;

use sse_core::{Error, SaveBuffer};
use sse_xray::writer::{Change, Placement};

pub(super) fn is_json(arguments: &[String]) -> bool {
    arguments.last().map(String::as_str) == Some("--json")
}

pub(super) fn parse_crash_options(arguments: &[String]) -> Option<(Option<&String>, bool)> {
    let mut game = None;
    let mut json = false;
    let mut cursor = 3_usize;
    while cursor < arguments.len() {
        match arguments.get(cursor)?.as_str() {
            "--json" => {
                json = true;
                cursor = cursor.checked_add(1)?;
            }
            "--game" => {
                let value_at = cursor.checked_add(1)?;
                let value = arguments.get(value_at)?;
                if value.starts_with("--") {
                    return None;
                }
                game = Some(value);
                cursor = cursor.checked_add(2)?;
            }
            _ => return None,
        }
    }
    Some((game, json))
}

pub(super) fn doctor_save(path: Option<&String>, json: bool) -> sse_core::Result<()> {
    let path = path.ok_or_else(|| Error::damaged("missing save path"))?;
    let packed = SaveBuffer::read(Path::new(path))?;
    let report = sse_doctor::analyze_save(packed.as_slice());
    if json {
        println!("{}", json_save_report(&report)?);
        if report.status == sse_doctor::SaveDoctorStatus::Error {
            return Err(Error::damaged("save structure could not be validated"));
        }
        return Ok(());
    }
    println!("Save Doctor: {:?}", report.status);
    println!("Format: {}", report.format_id.unwrap_or("unknown"));
    println!(
        "Objects: {}",
        report
            .object_count
            .map_or_else(|| "unknown".to_owned(), |count| count.to_string())
    );
    println!(
        "Inventory objects: {}",
        report
            .inventory_count
            .map_or_else(|| "unknown".to_owned(), |count| count.to_string())
    );
    for finding in &report.findings {
        println!(
            "{:?} [{}] {}: {}",
            finding.severity, finding.id, finding.looked_at, finding.found
        );
    }
    if report.status == sse_doctor::SaveDoctorStatus::Error {
        let detail = report.findings.first().map_or_else(
            || "save structure could not be validated".to_owned(),
            |finding| finding.found.clone(),
        );
        return Err(Error::damaged(detail));
    }
    Ok(())
}

pub(super) fn doctor_crash(path: Option<&String>, game: Option<&String>, json: bool) -> sse_core::Result<()> {
    let path = path.ok_or_else(|| Error::damaged("missing crash-log path"))?;
    let game = game.map(String::as_str);
    if let Some(name) = game {
        if !sse_doctor::is_known_crash_game(name) {
            return Err(Error::Refused(format!(
                "unknown game for crash signatures: {name}; use cs, cs-ee, soc or soc-ee"
            )));
        }
    }
    let analysis = sse_doctor::analyze_crash_file(Path::new(path), game)?;
    if json {
        println!("{}", json_crash_analysis(&analysis)?);
        return Ok(());
    }
    println!("Crash kind: {:?}", analysis.kind);
    println!("Summary: {}", analysis.summary);
    if let Some(issue) = analysis.known_issue {
        println!("Known issue: {} — {}", issue.id, issue.title);
        println!("Advice: {:?}", issue.advice);
    } else {
        println!("Known issue: none");
    }
    Ok(())
}

pub(super) fn doctor_game(target: Option<&String>, directory: Option<&String>, json: bool) -> sse_core::Result<()> {
    let target = target
        .and_then(|id| sse_doctor::GameTarget::parse(id))
        .ok_or_else(|| Error::Refused("unknown game target".to_owned()))?;
    let directory = directory.ok_or_else(|| Error::damaged("missing game directory"))?;
    let report = sse_doctor::analyze_game_install_from_steam(target, Path::new(directory));
    if json {
        println!("{}", json_game_report(&report)?);
        if report.status == sse_doctor::SaveDoctorStatus::Error {
            return Err(Error::damaged(
                "selected directory does not match the requested game target",
            ));
        }
        return Ok(());
    }
    println!("Game Doctor: {:?}", report.status);
    println!("Target: {}", target.id());
    println!("Directory: {}", report.directory.display());
    println!("Game marker: {}", report.marker_found);
    println!("Build fingerprint: {:?}", report.build.status);
    for finding in &report.findings {
        println!(
            "{:?} [{}] {}: {}",
            finding.severity, finding.id, finding.looked_at, finding.found
        );
    }
    if report.status == sse_doctor::SaveDoctorStatus::Error {
        return Err(Error::damaged(
            "selected directory does not match the requested game target",
        ));
    }
    Ok(())
}

pub(super) fn doctor_quests(path: Option<&String>, json: bool) -> sse_core::Result<()> {
    let path = path.ok_or_else(|| Error::damaged("missing save path"))?;
    let packed = SaveBuffer::read(Path::new(path))?;
    let report = sse_doctor::analyze_quests(packed.as_slice());
    if json {
        println!("{}", json_quest_report(&report)?);
        if report.status == sse_doctor::SaveDoctorStatus::Error {
            return Err(Error::damaged(report.summary));
        }
        return Ok(());
    }
    println!("Quest Doctor: {:?}", report.status);
    println!("Format: {}", report.format_id.unwrap_or("unknown"));
    println!("Summary: {}", report.summary);
    for state in &report.states {
        println!("{:?} [{}] {}: {}", state.status, state.id, state.title, state.detail);
    }
    if report.status == sse_doctor::SaveDoctorStatus::Error {
        return Err(Error::damaged(report.summary));
    }
    Ok(())
}

#[cfg(test)]
fn json_string(value: &str) -> sse_core::Result<String> {
    let mut writer = sse_codecs::json::Writer::compact();
    writer.string(value)?;
    finish_json(writer)
}

fn finish_json(writer: sse_codecs::json::Writer) -> sse_core::Result<String> {
    String::from_utf8(writer.finish()?).map_err(|_| Error::damaged("JSON writer returned invalid UTF-8"))
}

fn json_write_string(writer: &mut sse_codecs::json::Writer, value: &str) -> sse_core::Result<()> {
    writer.string(value)
}

/// Writes a durability value; NaN and infinities have no JSON form, so they are written as `null`.
fn json_write_f32(writer: &mut sse_codecs::json::Writer, value: f32) -> sse_core::Result<()> {
    if value.is_finite() {
        writer.number(&value.to_string())
    } else {
        writer.null()
    }
}

fn json_write_optional_string(writer: &mut sse_codecs::json::Writer, value: Option<&str>) -> sse_core::Result<()> {
    match value {
        Some(value) => json_write_string(writer, value),
        None => writer.null(),
    }
}

fn json_write_optional_count(writer: &mut sse_codecs::json::Writer, value: Option<usize>) -> sse_core::Result<()> {
    match value {
        Some(value) => writer.u64(u64::try_from(value).map_err(|_| Error::damaged("JSON count does not fit u64"))?),
        None => writer.null(),
    }
}

fn json_write_optional_i32(writer: &mut sse_codecs::json::Writer, value: Option<i32>) -> sse_core::Result<()> {
    match value {
        Some(value) => writer.i64(i64::from(value)),
        None => writer.null(),
    }
}

fn json_write_change(writer: &mut sse_codecs::json::Writer, change: &Change) -> sse_core::Result<()> {
    writer.object_start()?;
    match change {
        Change::SetMoney {
            target_object,
            old_value,
            new_value,
        } => {
            writer.key("kind")?;
            writer.string("setMoney")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldValue")?;
            writer.u64(u64::from(*old_value))?;
            writer.key("newValue")?;
            writer.u64(u64::from(*new_value))?;
        }
        Change::SetStack {
            target_object,
            old_value,
            new_value,
        } => {
            writer.key("kind")?;
            writer.string("setStack")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldValue")?;
            writer.u64(u64::from(*old_value))?;
            writer.key("newValue")?;
            writer.u64(u64::from(*new_value))?;
        }
        Change::SetDurability {
            target_object,
            old_value,
            new_value,
        } => {
            writer.key("kind")?;
            writer.string("setDurability")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldValue")?;
            json_write_f32(writer, *old_value)?;
            writer.key("newValue")?;
            json_write_f32(writer, *new_value)?;
        }
        Change::SetPlacement {
            target_object,
            destination,
        } => {
            writer.key("kind")?;
            writer.string("setPlacement")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("destination")?;
            match destination {
                Placement::Ruck => writer.string("ruck")?,
                Placement::Belt => writer.string("belt")?,
                Placement::Slot(slot) => writer.string(&format!("slot:{slot}"))?,
            }
        }
        Change::MoveItem {
            target_object,
            old_parent,
            new_parent,
        } => {
            writer.key("kind")?;
            writer.string("moveItem")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldParent")?;
            writer.u64(u64::from(*old_parent))?;
            writer.key("newParent")?;
            writer.u64(u64::from(*new_parent))?;
        }
        Change::RemoveItem { target_object } => {
            writer.key("kind")?;
            writer.string("removeItem")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
        }
        Change::AddItem {
            template_object,
            item_key,
            object_id,
            quantity,
        } => {
            writer.key("kind")?;
            writer.string("addItem")?;
            writer.key("templateObject")?;
            writer.u64(u64::from(*template_object))?;
            writer.key("itemKey")?;
            json_write_string(writer, item_key)?;
            writer.key("objectId")?;
            writer.u64(u64::from(*object_id))?;
            writer.key("quantity")?;
            writer.u64(u64::from(*quantity))?;
        }
        Change::SetPlayerFaction {
            target_object,
            old_value,
            faction_key,
        } => {
            writer.key("kind")?;
            writer.string("setPlayerFaction")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldValue")?;
            writer.i64(i64::from(*old_value))?;
            writer.key("factionKey")?;
            json_write_string(writer, faction_key)?;
        }
        Change::SetFactionRelation {
            target_object,
            faction_key,
            old_value,
            new_value,
        } => {
            writer.key("kind")?;
            writer.string("setFactionRelation")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("factionKey")?;
            json_write_string(writer, faction_key)?;
            writer.key("oldValue")?;
            json_write_optional_i32(writer, *old_value)?;
            writer.key("newValue")?;
            writer.i64(i64::from(*new_value))?;
        }
        Change::SetUpgrades {
            target_object,
            old_value,
            new_value,
        } => {
            writer.key("kind")?;
            writer.string("setUpgrades")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldValue")?;
            writer.array_start()?;
            for value in old_value {
                json_write_string(writer, value)?;
            }
            writer.array_end()?;
            writer.key("newValue")?;
            writer.array_start()?;
            for value in new_value {
                json_write_string(writer, value)?;
            }
            writer.array_end()?;
        }
        Change::AddInfoPortions {
            target_object,
            info_portions,
        } => {
            writer.key("kind")?;
            writer.string("addInfoPortions")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("infoPortions")?;
            writer.array_start()?;
            for value in info_portions {
                json_write_string(writer, value)?;
            }
            writer.array_end()?;
        }
        Change::RelocateActor { destination_changer } => {
            writer.key("kind")?;
            writer.string("relocateActor")?;
            writer.key("destinationChanger")?;
            writer.u64(u64::from(*destination_changer))?;
        }
    }
    writer.object_end()
}

fn json_write_findings(
    writer: &mut sse_codecs::json::Writer,
    findings: &[sse_doctor::RuleFinding],
) -> sse_core::Result<()> {
    writer.array_start()?;
    for finding in findings {
        writer.object_start()?;
        writer.key("id")?;
        json_write_string(writer, finding.id)?;
        writer.key("severity")?;
        json_write_string(writer, &format!("{:?}", finding.severity).to_lowercase())?;
        writer.key("lookedAt")?;
        json_write_string(writer, finding.looked_at)?;
        writer.key("found")?;
        json_write_string(writer, &finding.found)?;
        writer.key("repair")?;
        if let Some(change_set) = &finding.repair {
            writer.array_start()?;
            for change in change_set.changes() {
                json_write_change(writer, change)?;
            }
            writer.array_end()?;
        } else {
            writer.null()?;
        }
        writer.object_end()?;
    }
    writer.array_end()
}

fn json_save_report(report: &sse_doctor::SaveDoctorReport) -> sse_core::Result<String> {
    let mut writer = sse_codecs::json::Writer::compact();
    writer.object_start()?;
    writer.key("status")?;
    json_write_string(&mut writer, &format!("{:?}", report.status).to_lowercase())?;
    writer.key("formatId")?;
    json_write_optional_string(&mut writer, report.format_id)?;
    writer.key("objectCount")?;
    json_write_optional_count(&mut writer, report.object_count)?;
    writer.key("inventoryCount")?;
    json_write_optional_count(&mut writer, report.inventory_count)?;
    writer.key("findings")?;
    json_write_findings(&mut writer, &report.findings)?;
    writer.object_end()?;
    finish_json(writer)
}

fn json_quest_report(report: &sse_doctor::QuestDoctorReport) -> sse_core::Result<String> {
    let mut writer = sse_codecs::json::Writer::compact();
    writer.object_start()?;
    writer.key("status")?;
    json_write_string(&mut writer, &format!("{:?}", report.status).to_lowercase())?;
    writer.key("formatId")?;
    json_write_optional_string(&mut writer, report.format_id)?;
    writer.key("questStatesAvailable")?;
    writer.bool(report.quest_states_available)?;
    writer.key("summary")?;
    json_write_string(&mut writer, &report.summary)?;
    writer.key("states")?;
    writer.array_start()?;
    for state in &report.states {
        writer.object_start()?;
        writer.key("id")?;
        json_write_string(&mut writer, state.id)?;
        writer.key("title")?;
        json_write_string(&mut writer, state.title)?;
        writer.key("status")?;
        json_write_string(&mut writer, &format!("{:?}", state.status).to_lowercase())?;
        writer.key("reason")?;
        json_write_string(&mut writer, state.reason)?;
        writer.key("missingInfo")?;
        json_write_optional_string(&mut writer, state.missing_info)?;
        writer.key("preventingFixId")?;
        json_write_optional_string(&mut writer, state.preventing_fix_id)?;
        writer.key("needsPreventingFix")?;
        writer.bool(state.needs_preventing_fix)?;
        writer.key("detail")?;
        json_write_string(&mut writer, state.detail)?;
        writer.key("references")?;
        writer.array_start()?;
        for reference in state.references {
            json_write_string(&mut writer, reference)?;
        }
        writer.array_end()?;
        writer.object_end()?;
    }
    writer.array_end()?;
    writer.object_end()?;
    finish_json(writer)
}

fn json_game_report(report: &sse_doctor::GameDoctorReport) -> sse_core::Result<String> {
    let mut writer = sse_codecs::json::Writer::compact();
    writer.object_start()?;
    writer.key("status")?;
    json_write_string(&mut writer, &format!("{:?}", report.status).to_lowercase())?;
    writer.key("target")?;
    json_write_string(&mut writer, report.target.id())?;
    writer.key("directory")?;
    json_write_string(&mut writer, &report.directory.display().to_string())?;
    writer.key("markerFound")?;
    writer.bool(report.marker_found)?;
    writer.key("build")?;
    writer.object_start()?;
    writer.key("buildId")?;
    json_write_optional_string(&mut writer, report.build.build_id.as_deref())?;
    writer.key("status")?;
    json_write_string(&mut writer, &format!("{:?}", report.build.status).to_lowercase())?;
    writer.object_end()?;
    writer.key("findings")?;
    json_write_findings(&mut writer, &report.findings)?;
    writer.object_end()?;
    finish_json(writer)
}

fn json_crash_analysis(analysis: &sse_doctor::CrashLogAnalysis) -> sse_core::Result<String> {
    let mut writer = sse_codecs::json::Writer::compact();
    writer.object_start()?;
    writer.key("kind")?;
    json_write_string(&mut writer, &format!("{:?}", analysis.kind).to_lowercase())?;
    writer.key("summary")?;
    json_write_string(&mut writer, &analysis.summary)?;
    writer.key("knownIssue")?;
    if let Some(issue) = analysis.known_issue {
        writer.object_start()?;
        writer.key("id")?;
        json_write_string(&mut writer, issue.id)?;
        writer.key("title")?;
        json_write_string(&mut writer, issue.title)?;
        writer.key("game")?;
        json_write_string(&mut writer, issue.game)?;
        writer.key("advice")?;
        json_write_string(&mut writer, &format!("{:?}", issue.advice).to_lowercase())?;
        writer.object_end()?;
    } else {
        writer.null()?;
    }
    writer.key("faultingModuleOffset")?;
    json_write_optional_string(&mut writer, analysis.faulting_module_offset.as_deref())?;
    writer.object_end()?;
    finish_json(writer)
}

#[cfg(test)]
mod doctor_tests {
    use super::{json_string, json_write_f32};

    #[test]
    fn non_finite_durability_is_written_as_null_not_an_error() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut writer = sse_codecs::json::Writer::compact();
            assert!(json_write_f32(&mut writer, value).is_ok());
            assert_eq!(super::finish_json(writer).ok().as_deref(), Some("null"));
        }
        let mut writer = sse_codecs::json::Writer::compact();
        assert!(json_write_f32(&mut writer, 0.5).is_ok());
        assert_eq!(super::finish_json(writer).ok().as_deref(), Some("0.5"));
    }
    use crate::run;

    const SYNTHETIC_XRAY_SAVE: &[u8] = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");

    #[test]
    fn doctor_save_accepts_a_supported_synthetic_save() {
        let path = std::env::temp_dir().join(format!("sse-cli-doctor-{}.sav", std::process::id()));
        let write = std::fs::write(&path, SYNTHETIC_XRAY_SAVE);
        assert!(write.is_ok());
        if write.is_err() {
            return;
        }
        let arguments = vec![
            "doctor".to_owned(),
            "save".to_owned(),
            path.to_string_lossy().into_owned(),
        ];

        let exit_code = run(&arguments);

        let mut json_arguments = arguments.clone();
        json_arguments.push("--json".to_owned());
        let json_exit_code = run(&json_arguments);

        let _ = std::fs::remove_file(path);
        assert_eq!(exit_code, sse_core::ExitCode::Done as u8);
        assert_eq!(json_exit_code, sse_core::ExitCode::Done as u8);
    }

    #[test]
    fn doctor_crash_accepts_an_explicit_synthetic_log() {
        let path = std::env::temp_dir().join(format!("sse-cli-doctor-{}.log", std::process::id()));
        let write = std::fs::write(
            &path,
            "! [LUA][ERROR] ERROR: wrong target for storyline quest: logic@work5,gar_smart_terrain_6_3",
        );
        assert!(write.is_ok());
        if write.is_err() {
            return;
        }
        let arguments = vec![
            "doctor".to_owned(),
            "crash".to_owned(),
            path.to_string_lossy().into_owned(),
            "--game".to_owned(),
            "cs".to_owned(),
        ];

        let exit_code = run(&arguments);

        let mut json_arguments = arguments.clone();
        json_arguments.push("--json".to_owned());
        let json_exit_code = run(&json_arguments);

        let _ = std::fs::remove_file(path);
        assert_eq!(exit_code, sse_core::ExitCode::Done as u8);
        assert_eq!(json_exit_code, sse_core::ExitCode::Done as u8);
    }

    #[test]
    fn doctor_crash_refuses_an_unknown_game_instead_of_matching_only_general_signatures() {
        let path = std::env::temp_dir().join(format!("sse-cli-doctor-game-{}.log", std::process::id()));
        assert!(std::fs::write(&path, "fatal error").is_ok());
        let arguments = vec![
            "doctor".to_owned(),
            "crash".to_owned(),
            path.to_string_lossy().into_owned(),
            "--game".to_owned(),
            "s2".to_owned(),
        ];

        let exit_code = run(&arguments);

        let _ = std::fs::remove_file(path);
        assert_eq!(exit_code, sse_core::ExitCode::Refused as u8);
    }

    #[test]
    fn doctor_crash_rejects_unknown_options() {
        let arguments = vec![
            "doctor".to_owned(),
            "crash".to_owned(),
            "synthetic.log".to_owned(),
            "--unknown".to_owned(),
        ];

        assert_eq!(run(&arguments), sse_core::ExitCode::Usage as u8);
    }

    #[test]
    fn doctor_game_checks_the_explicit_target_and_directory() {
        let directory = std::env::temp_dir().join(format!("sse-cli-doctor-game-{}", std::process::id()));
        let setup = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&directory)?;
            std::fs::write(directory.join("fsgame_cs.ltx"), b"$game_data$ = true")
        })();
        assert!(setup.is_ok());
        if setup.is_err() {
            let _ = std::fs::remove_dir_all(&directory);
            return;
        }
        let arguments = vec![
            "doctor".to_owned(),
            "game".to_owned(),
            "cs".to_owned(),
            directory.to_string_lossy().into_owned(),
        ];

        let exit_code = run(&arguments);

        let _ = std::fs::remove_dir_all(&directory);
        assert_eq!(exit_code, sse_core::ExitCode::Done as u8);
    }

    #[test]
    fn doctor_quests_keeps_unreadable_quest_state_unknown() {
        let path = std::env::temp_dir().join(format!("sse-cli-doctor-quests-{}.sav", std::process::id()));
        let write = std::fs::write(&path, SYNTHETIC_XRAY_SAVE);
        assert!(write.is_ok());
        if write.is_err() {
            return;
        }
        let arguments = vec![
            "doctor".to_owned(),
            "quests".to_owned(),
            path.to_string_lossy().into_owned(),
        ];

        let exit_code = run(&arguments);

        let _ = std::fs::remove_file(path);
        assert_eq!(exit_code, sse_core::ExitCode::Done as u8);
    }

    #[test]
    fn json_string_escapes_quotes_slashes_and_controls() {
        assert_eq!(
            json_string("a\n\"b\\c\u{0001}").unwrap_or_else(|error| format!("JSON encoding error: {error}")),
            "\"a\\n\\\"b\\\\c\\u0001\""
        );
    }

    #[test]
    fn save_doctor_json_contains_status_counts_and_rule_evidence() {
        let report = sse_doctor::analyze_save(SYNTHETIC_XRAY_SAVE);
        let output = super::json_save_report(&report).unwrap_or_else(|error| format!("JSON report error: {error}"));

        assert!(output.starts_with('{'));
        assert!(output.ends_with('}'));
        assert!(output.contains("\"status\":\"ok\""));
        assert!(output.contains("\"formatId\":\"stalker-soc\""));
        assert!(output.contains("\"id\":\"semantic-state\""));
        assert!(output.contains("\"repair\":null"));
    }
}
