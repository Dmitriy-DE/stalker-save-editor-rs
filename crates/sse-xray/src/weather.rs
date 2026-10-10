//! Dynamic weather saved by the level scripts (Clear Sky and Call of Pripyat).
//!
//! Port of `XRayWeatherReader` from the C# 1.3.1 core. The value is stored as `<graph>=<current>,<next>` followed
//! by a zero byte; the C# regex is matched here by scanning the zero-terminated segments.

/// Weather states the level scripts use.
const KNOWN_STATES: [&str; 8] = [
    "clear", "cloudy", "rain", "thunder", "pasmurno", "groza", "foggy", "storm",
];

/// The dynamic weather: graph (for example `dynamic_default`), current and next state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weather {
    /// Weather graph name.
    pub graph: String,
    /// Current state.
    pub current: String,
    /// Next state.
    pub next: String,
}

/// Finds the weather in the actor's record text first, then in the whole image (only when unique there).
pub(crate) fn find_weather(actor_text: &str, image_text: &str) -> Option<Weather> {
    find(actor_text).or_else(|| find_unique(image_text))
}

/// The first value whose two states are known.
fn find(text: &str) -> Option<Weather> {
    matches(text)
        .into_iter()
        .find(|value| is_known(&value.current) && is_known(&value.next))
}

/// Accepts a `dynamic_*` value only when the whole image holds exactly one distinct such value.
fn find_unique(text: &str) -> Option<Weather> {
    let mut found: Vec<Weather> = matches(text)
        .into_iter()
        .filter(|value| value.graph.starts_with("dynamic_") && is_known(&value.current) && is_known(&value.next))
        .collect();
    found.sort_by(|a, b| (&a.graph, &a.current, &a.next).cmp(&(&b.graph, &b.current, &b.next)));
    found.dedup();
    match found.len() {
        1 => found.pop(),
        _ => None,
    }
}

fn is_known(state: &str) -> bool {
    KNOWN_STATES.contains(&state)
}

/// Every `graph=current,next` value, in order. Each value is followed by a zero byte, so the segments before
/// each zero byte are the only candidates.
fn matches(text: &str) -> Vec<Weather> {
    let parts: Vec<&str> = text.split('\0').collect();
    let terminated = parts.len().saturating_sub(1);
    parts
        .iter()
        .take(terminated)
        .filter_map(|segment| parse_segment(segment))
        .collect()
}

/// Parses one segment that ends at a zero byte. Each name is the maximal run of `[a-z_]` before its separator,
/// which is where the C# regex's leftmost match starts.
fn parse_segment(segment: &str) -> Option<Weather> {
    let (head, next) = split_name_suffix(segment)?;
    let head = head.strip_suffix(',')?;
    let (head, current) = split_name_suffix(head)?;
    let head = head.strip_suffix('=')?;
    let (_, graph) = split_name_suffix(head)?;
    Some(Weather {
        graph: graph.to_owned(),
        current: current.to_owned(),
        next: next.to_owned(),
    })
}

/// Splits `text` into the part before its maximal trailing `[a-z_]` run and that run; `None` when the run is empty.
fn split_name_suffix(text: &str) -> Option<(&str, &str)> {
    let start = text
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_ascii_lowercase() || *c == '_'))
        .map_or(0, |(index, c)| index.saturating_add(c.len_utf8()));
    let name = text.get(start..)?;
    (!name.is_empty()).then(|| (text.get(..start).unwrap_or_default(), name))
}

#[cfg(test)]
mod tests {
    use super::{find_weather, Weather};

    fn value(graph: &str, current: &str, next: &str) -> Weather {
        Weather {
            graph: graph.to_owned(),
            current: current.to_owned(),
            next: next.to_owned(),
        }
    }

    #[test]
    fn a_zero_terminated_value_in_the_actor_text_is_found() {
        let text = "xx dynamic_default=clear,rain\0more";
        assert_eq!(
            find_weather(text, text),
            Some(value("dynamic_default", "clear", "rain"))
        );
    }

    #[test]
    fn a_value_without_the_terminator_is_not_a_weather() {
        let text = "dynamic_default=clear,rain";
        assert_eq!(find_weather(text, text), None);
    }

    #[test]
    fn an_unknown_state_is_not_a_weather() {
        let text = "dynamic_default=clear,snow\0";
        assert_eq!(find_weather(text, text), None);
    }

    #[test]
    fn outside_the_actor_text_a_unique_dynamic_value_is_accepted_but_an_ambiguous_one_is_not() {
        let unique = "x\0dynamic_default=cloudy,clear\0";
        assert_eq!(
            find_weather("", unique),
            Some(value("dynamic_default", "cloudy", "clear"))
        );
        let ambiguous = "dynamic_default=cloudy,clear\0dynamic_default=rain,clear\0";
        assert_eq!(find_weather("", ambiguous), None);
    }
}
