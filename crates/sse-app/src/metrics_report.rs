//! Developer report over downloaded performance summaries (`metrics/*.json`, schema 1).
//!
//! The input is a folder of files exactly as the upload path stores them (see `metrics::upload_preview`).
//! Summaries are read one by one; a file that is not a valid schema-1 summary is skipped with a warning.
//! Percentiles cannot be merged exactly from per-upload summaries, so the report says how each one is combined.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use sse_codecs::json::{Event, Reader};

/// The only summary schema this report understands.
pub const SUMMARY_SCHEMA: u64 = 1;

/// Group label for summaries that do not carry an operating-system field.
pub const UNKNOWN_OS: &str = "не указана";

/// One summary file after parsing.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// Operating-system category, when the summary carries one.
    pub os: Option<String>,
    /// Sessions covered by this upload.
    pub sessions: u64,
    /// Average first-frame time in milliseconds.
    pub first_frame_avg_ms: Option<f64>,
    /// Screen-switch timing.
    pub screen_switch: Triplet,
    /// Scroll-frame timing.
    pub scroll: Triplet,
    /// Average save discovery time in milliseconds.
    pub discovery_avg_ms: Option<f64>,
    /// High-water memory in MiB.
    pub peak_memory_mib: Option<f64>,
    /// Save reads and writes grouped by format and size.
    pub save_operations: Vec<SaveOperation>,
}

/// Percentile and maximum timing for one measurement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Triplet {
    /// Median in milliseconds.
    pub p50: Option<f64>,
    /// 95th percentile in milliseconds.
    pub p95: Option<f64>,
    /// Largest sample in milliseconds.
    pub max: Option<f64>,
}

/// One save operation group from a summary.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveOperation {
    /// Operation name, for example `read` or `write`.
    pub operation: String,
    /// Save format identifier.
    pub format: String,
    /// Size bucket label.
    pub size_bucket: String,
    /// Operations counted in this group.
    pub count: u64,
    /// Average duration in milliseconds.
    pub average_ms: f64,
    /// Largest duration in milliseconds.
    pub max_ms: f64,
}

/// Parsed JSON value, enough for the summary shape.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Null,
    Bool(bool),
    /// The number as a float, and as an integer when the source text is one.
    Number(f64, Option<u64>),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Self::Object(fields) => fields.iter().find(|(key, _)| key == name).map(|(_, value)| value),
            _ => None,
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Self::Number(value, _) => Some(*value),
            _ => None,
        }
    }

    fn integer(&self) -> Option<u64> {
        match self {
            Self::Number(_, integer) => *integer,
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    fn array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }
}

const MAX_DEPTH: usize = 32;

fn read_value(reader: &mut Reader<'_>, depth: usize) -> Result<Value, String> {
    let event = reader
        .next_event()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "unexpected end of input".to_owned())?;
    value_from_event(reader, event, depth)
}

fn value_from_event(reader: &mut Reader<'_>, event: Event<'_>, depth: usize) -> Result<Value, String> {
    if depth > MAX_DEPTH {
        return Err("summary nests too deeply".to_owned());
    }
    match event {
        Event::Null => Ok(Value::Null),
        Event::Bool(value) => Ok(Value::Bool(value)),
        Event::Number(text) => {
            let value = text
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| "number is not finite".to_owned())?;
            Ok(Value::Number(value, text.parse::<u64>().ok()))
        }
        Event::String(text) => Ok(Value::String(text.into_owned())),
        Event::ArrayStart => {
            let mut items = Vec::new();
            loop {
                match reader.next_event().map_err(|error| error.to_string())? {
                    Some(Event::ArrayEnd) => return Ok(Value::Array(items)),
                    Some(event) => items.push(value_from_event(reader, event, depth.saturating_add(1))?),
                    None => return Err("array is not closed".to_owned()),
                }
            }
        }
        Event::ObjectStart => {
            let mut fields = Vec::new();
            loop {
                match reader.next_event().map_err(|error| error.to_string())? {
                    Some(Event::ObjectEnd) => return Ok(Value::Object(fields)),
                    Some(Event::Key(key)) => {
                        let key = key.into_owned();
                        let value = read_value(reader, depth.saturating_add(1))?;
                        fields.push((key, value));
                    }
                    _ => return Err("object is malformed".to_owned()),
                }
            }
        }
        Event::ObjectEnd | Event::ArrayEnd | Event::Key(_) => Err("unexpected token".to_owned()),
    }
}

fn triplet(value: Option<&Value>) -> Triplet {
    let field = |name: &str| value.and_then(|value| value.field(name)).and_then(Value::number);
    Triplet {
        p50: field("p50"),
        p95: field("p95"),
        max: field("max"),
    }
}

/// Parses one schema-1 summary.
///
/// # Errors
/// Returns a message when the bytes are not JSON, are not an object, or have another schema.
pub fn parse_summary(bytes: &[u8]) -> Result<Summary, String> {
    let mut reader = Reader::new(bytes);
    let root = read_value(&mut reader, 0)?;
    if reader.next_event().map_err(|error| error.to_string())?.is_some() {
        return Err("trailing data after the summary".to_owned());
    }
    let schema = root.field("schema").and_then(Value::integer);
    if schema != Some(SUMMARY_SCHEMA) {
        return Err(format!("unsupported schema {schema:?}"));
    }
    let sessions = root
        .field("sessions")
        .and_then(Value::integer)
        .ok_or_else(|| "sessions is missing or not a count".to_owned())?;
    let save_operations = root
        .field("save_operations")
        .and_then(Value::array)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            Some(SaveOperation {
                operation: item.field("operation")?.text()?.to_owned(),
                format: item.field("format")?.text()?.to_owned(),
                size_bucket: item.field("size_bucket")?.text()?.to_owned(),
                count: item.field("count")?.integer()?,
                average_ms: item.field("average_ms")?.number()?,
                max_ms: item.field("max_ms")?.number()?,
            })
        })
        .collect();
    Ok(Summary {
        os: root.field("os").and_then(Value::text).map(str::to_owned),
        sessions,
        first_frame_avg_ms: root.field("first_frame_avg_ms").and_then(Value::number),
        screen_switch: triplet(root.field("screen_switch_ms")),
        scroll: triplet(root.field("scroll_frame_ms")),
        discovery_avg_ms: root.field("save_discovery_avg_ms").and_then(Value::number),
        peak_memory_mib: root.field("peak_memory_mib").and_then(Value::number),
        save_operations,
    })
}

/// One operating-system group in the report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OsGroup {
    /// Number of uploads read for this group.
    pub uploads: u64,
    /// Sessions summed over the uploads.
    pub sessions: u64,
    /// Session-weighted average first-frame time.
    pub first_frame_avg_ms: Option<f64>,
    /// Median of the per-upload medians.
    pub screen_switch_p50: Option<f64>,
    /// Largest per-upload 95th percentile.
    pub screen_switch_p95: Option<f64>,
    /// Largest sample.
    pub screen_switch_max: Option<f64>,
    /// Median of the per-upload scroll medians.
    pub scroll_p50: Option<f64>,
    /// Largest per-upload scroll 95th percentile.
    pub scroll_p95: Option<f64>,
    /// Largest scroll sample.
    pub scroll_max: Option<f64>,
    /// Highest memory value.
    pub peak_memory_mib: Option<f64>,
    /// Save operations grouped by (operation, format, size bucket): count, average, max.
    pub save_operations: BTreeMap<(String, String, String), (u64, f64, f64)>,
}

/// Report over every readable summary, keyed by operating system.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Groups in operating-system order.
    pub groups: BTreeMap<String, OsGroup>,
    /// Files that were skipped, with the reason.
    pub skipped: Vec<(String, String)>,
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied()
}

fn largest(values: impl Iterator<Item = f64>) -> Option<f64> {
    values.reduce(f64::max)
}

/// Builds the report from `(file name, bytes)` pairs.
#[must_use]
pub fn build_report<'a>(files: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> Report {
    let mut by_os: BTreeMap<String, Vec<Summary>> = BTreeMap::new();
    let mut skipped = Vec::new();
    for (name, bytes) in files {
        match parse_summary(bytes) {
            Ok(summary) => {
                let os = summary.os.clone().unwrap_or_else(|| UNKNOWN_OS.to_owned());
                by_os.entry(os).or_default().push(summary);
            }
            Err(reason) => skipped.push((name.to_owned(), reason)),
        }
    }
    let groups = by_os
        .into_iter()
        .map(|(os, summaries)| (os, group(&summaries)))
        .collect();
    Report { groups, skipped }
}

fn group(summaries: &[Summary]) -> OsGroup {
    let mut out = OsGroup {
        uploads: summaries.len() as u64,
        ..OsGroup::default()
    };
    let mut frame_total = 0.0;
    let mut frame_sessions = 0.0;
    for summary in summaries {
        out.sessions = out.sessions.saturating_add(summary.sessions);
        if let Some(average) = summary.first_frame_avg_ms {
            frame_total += average * summary.sessions as f64;
            frame_sessions += summary.sessions as f64;
        }
        out.peak_memory_mib = largest(out.peak_memory_mib.into_iter().chain(summary.peak_memory_mib));
        for operation in &summary.save_operations {
            let entry = out
                .save_operations
                .entry((
                    operation.operation.clone(),
                    operation.format.clone(),
                    operation.size_bucket.clone(),
                ))
                .or_insert((0, 0.0, 0.0));
            entry.1 += operation.average_ms * operation.count as f64;
            entry.0 = entry.0.saturating_add(operation.count);
            entry.2 = entry.2.max(operation.max_ms);
        }
    }
    out.first_frame_avg_ms = (frame_sessions > 0.0).then(|| frame_total / frame_sessions);
    out.screen_switch_p50 = median(summaries.iter().filter_map(|s| s.screen_switch.p50).collect());
    out.screen_switch_p95 = largest(summaries.iter().filter_map(|s| s.screen_switch.p95));
    out.screen_switch_max = largest(summaries.iter().filter_map(|s| s.screen_switch.max));
    out.scroll_p50 = median(summaries.iter().filter_map(|s| s.scroll.p50).collect());
    out.scroll_p95 = largest(summaries.iter().filter_map(|s| s.scroll.p95));
    out.scroll_max = largest(summaries.iter().filter_map(|s| s.scroll.max));
    for value in out.save_operations.values_mut() {
        value.1 = if value.0 == 0 { 0.0 } else { value.1 / value.0 as f64 };
    }
    out
}

fn cell(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("{value:.1}"))
}

/// Renders the report as plain text.
#[must_use]
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("Сводка метрик (schema 1). p50 — медиана медиан отправок; p95 и макс — наибольшее по отправкам.\n");
    out.push_str("Время в миллисекундах; пик памяти в MiB. Группа «не указана» — отправки без поля ОС.\n");
    for (os, group) in &report.groups {
        let _ = writeln!(
            out,
            "\n== ОС: {os} · отправок: {} · сессий: {}",
            group.uploads, group.sessions
        );
        let _ = writeln!(out, "первый кадр (среднее): {}", cell(group.first_frame_avg_ms));
        let _ = writeln!(
            out,
            "смена экрана p50 / p95 / макс: {} / {} / {}",
            cell(group.screen_switch_p50),
            cell(group.screen_switch_p95),
            cell(group.screen_switch_max)
        );
        let _ = writeln!(
            out,
            "прокрутка p50 / p95 / макс: {} / {} / {}",
            cell(group.scroll_p50),
            cell(group.scroll_p95),
            cell(group.scroll_max)
        );
        let _ = writeln!(out, "пик памяти: {}", cell(group.peak_memory_mib));
        if group.save_operations.is_empty() {
            out.push_str("сейвы: нет данных\n");
        }
        for ((operation, format, bucket), (count, average, maximum)) in &group.save_operations {
            let _ = writeln!(
                out,
                "сейв {operation} · {format} · {bucket}: операций {count}, среднее {average:.1}, макс {maximum:.1}"
            );
        }
    }
    if report.groups.is_empty() {
        out.push_str("\nЧитаемых сводок нет.\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{build_report, parse_summary, render, UNKNOWN_OS};

    fn summary(os: Option<&str>, sessions: u64, first: f64, switch: (f64, f64, f64), memory: u64) -> String {
        let os_field = os.map_or_else(String::new, |os| format!("\"os\":\"{os}\","));
        format!(
            "{{{os_field}\"schema\":1,\"sessions\":{sessions},\"first_frame_avg_ms\":{first},\
             \"screen_switch_ms\":{{\"p50\":{},\"p95\":{},\"max\":{}}},\
             \"scroll_frame_ms\":{{\"p50\":1.0,\"p95\":2.0,\"max\":3.0}},\
             \"save_discovery_avg_ms\":5.0,\"peak_memory_mib\":{memory},\
             \"save_operations\":[{{\"operation\":\"read\",\"format\":\"soc\",\"size_bucket\":\"small\",\
             \"count\":4,\"average_ms\":10.0,\"max_ms\":20.0}}]}}",
            switch.0, switch.1, switch.2
        )
    }

    #[test]
    fn a_valid_summary_parses_every_section() {
        let text = summary(Some("windows"), 2, 120.0, (8.0, 40.0, 90.0), 300);
        let parsed = match parse_summary(text.as_bytes()) {
            Ok(parsed) => parsed,
            Err(reason) => panic!("valid summary refused: {reason}"),
        };
        assert_eq!(parsed.os.as_deref(), Some("windows"));
        assert_eq!(parsed.sessions, 2);
        assert_eq!(parsed.screen_switch.p95, Some(40.0));
        assert_eq!(parsed.save_operations.len(), 1);
        assert_eq!(parsed.peak_memory_mib, Some(300.0));
    }

    #[test]
    fn other_schemas_and_broken_json_are_refused() {
        assert!(parse_summary(b"{\"schema\":2,\"sessions\":1}").is_err());
        assert!(parse_summary(b"{\"schema\":1,\"sessions\":1").is_err());
        assert!(parse_summary(b"[1,2]").is_err());
        assert!(parse_summary(b"{\"schema\":1,\"sessions\":-1}").is_err());
    }

    #[test]
    fn a_broken_file_is_skipped_with_a_reason_and_the_rest_still_counts() {
        let good = summary(Some("linux"), 1, 100.0, (5.0, 9.0, 12.0), 200);
        let report = build_report([
            ("a.json", good.as_bytes()),
            ("broken.json", b"{\"schema\":1,\"sessions\":".as_slice()),
        ]);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(
            report.skipped.first().map(|(name, _)| name.as_str()),
            Some("broken.json")
        );
        assert_eq!(report.groups.get("linux").map(|group| group.uploads), Some(1));
    }

    #[test]
    fn groups_by_os_and_combines_as_the_report_header_says() {
        let windows_a = summary(Some("windows"), 2, 100.0, (4.0, 30.0, 50.0), 200);
        let windows_b = summary(Some("windows"), 6, 200.0, (10.0, 60.0, 80.0), 500);
        let unknown = summary(None, 1, 300.0, (6.0, 20.0, 25.0), 100);
        let report = build_report([
            ("1.json", windows_a.as_bytes()),
            ("2.json", windows_b.as_bytes()),
            ("3.json", unknown.as_bytes()),
        ]);
        let Some(windows) = report.groups.get("windows") else {
            panic!("windows group missing");
        };
        assert_eq!(windows.uploads, 2);
        assert_eq!(windows.sessions, 8);
        // Session-weighted: (100*2 + 200*6) / 8 = 175.
        assert_eq!(windows.first_frame_avg_ms, Some(175.0));
        // The largest per-upload p95 is kept, not an average of percentiles.
        assert_eq!(windows.screen_switch_p95, Some(60.0));
        assert_eq!(windows.peak_memory_mib, Some(500.0));
        assert!(report.groups.contains_key(UNKNOWN_OS));
        let text = render(&report);
        assert!(text.contains("== ОС: windows"));
        assert!(text.contains("сейв read · soc · small: операций 8"));
    }

    #[test]
    fn an_empty_folder_says_so() {
        let report = build_report(std::iter::empty());
        assert!(render(&report).contains("Читаемых сводок нет."));
    }
}
