//! X-Ray XML string tables reader (`text/<lang>/*.xml`).
//!
//! Extracts localized strings `<string id="..."> <text>...</text> </string>`.
//! Selects language directories by priority order to avoid mixing languages.

use std::collections::HashMap;

use crate::encoding::decode_text;

/// Reader for X-Ray XML string tables.
pub struct XRayStringTables;

impl XRayStringTables {
    /// Returns the list of language folder codes in preference order for the given UI language.
    #[must_use]
    pub fn preference_order(ui_language: &str) -> Vec<&'static [&'static str]> {
        let mut order: Vec<&'static [&'static str]> = Vec::new();
        if let Some(own) = folder_codes(ui_language) {
            order.push(own);
        }
        if ui_language == "ru" {
            if let Some(en) = folder_codes("en") {
                order.push(en);
            }
        } else {
            if let Some(en) = folder_codes("en") {
                order.push(en);
            }
            if let Some(ru) = folder_codes("ru") {
                order.push(ru);
            }
        }

        // Deduplicate
        let mut deduped = Vec::new();
        for group in order {
            if !deduped
                .iter()
                .any(|&g: &&'static [&'static str]| std::ptr::eq(g, group))
            {
                deduped.push(group);
            }
        }
        deduped
    }

    /// Reads all string table entries from files matching the preferred language.
    #[must_use]
    pub fn read<'a, T, F>(
        files: &'a [T],
        get_path: impl Fn(&'a T) -> &'a str,
        read_bytes: F,
        ui_language: &str,
    ) -> HashMap<String, String>
    where
        F: Fn(&'a T) -> Option<Vec<u8>>,
    {
        let candidates: Vec<&'a T> = files
            .iter()
            .filter(|f| {
                let path = get_path(f);
                if !path.to_ascii_lowercase().ends_with(".xml") {
                    return false;
                }
                let padded = format!("/{}/", path.replace('\\', "/").to_ascii_lowercase());
                padded.contains("/text/") || padded.contains("/localization/")
            })
            .collect();

        let mut chosen = candidates.clone();
        for codes in Self::preference_order(ui_language) {
            let matched: Vec<&'a T> = candidates
                .iter()
                .copied()
                .filter(|f| {
                    let path = get_path(f);
                    let padded = format!("/{}/", path.replace('\\', "/").to_ascii_lowercase());
                    codes.iter().any(|&code| {
                        let text_pattern = format!("/text/{code}/");
                        let loc_pattern = format!("/localization/{code}/");
                        padded.contains(&text_pattern) || padded.contains(&loc_pattern)
                    })
                })
                .collect();

            if !matched.is_empty() {
                chosen = matched;
                break;
            }
        }

        let mut values = HashMap::new();
        for file in chosen {
            let Some(bytes) = read_bytes(file) else {
                continue;
            };
            parse_xml_string_table(&bytes, &mut values);
        }

        values
    }
}

fn folder_codes(lang: &str) -> Option<&'static [&'static str]> {
    match lang {
        "ru" => Some(&["rus", "ru"]),
        "uk" => Some(&["ukr", "uk", "ua"]),
        "en" => Some(&["eng", "en"]),
        "de" => Some(&["ger", "de", "deu"]),
        "fr" => Some(&["fra", "fr", "fre"]),
        "it" => Some(&["ita", "it"]),
        "es" => Some(&["spa", "es", "esp"]),
        "pl" => Some(&["pol", "pl"]),
        "cs" => Some(&["cze", "cs", "ces"]),
        _ => None,
    }
}

fn parse_xml_string_table(raw_bytes: &[u8], values: &mut HashMap<String, String>) {
    let mut text = decode_text(raw_bytes);

    // Strip <?xml ... ?> if present
    let trimmed = text.trim_start();
    if trimmed.to_ascii_lowercase().starts_with("<?xml") {
        if let Some(end_idx) = text.find("?>") {
            let after = text.get(end_idx.saturating_add(2)..).unwrap_or("");
            text = after.to_string();
        }
    }

    // Parse <string id="..."> <text>...</text> </string>
    let mut search_from = 0usize;
    while let Some(open_string_rel) = text.get(search_from..).and_then(|s| find_tag_open(s, "string")) {
        let string_start = search_from.saturating_add(open_string_rel);
        let tag_close_rel = match text.get(string_start..).and_then(|s| s.find('>')) {
            Some(idx) => idx,
            None => break,
        };
        let tag_header = text
            .get(string_start..string_start.saturating_add(tag_close_rel))
            .unwrap_or("");
        let id_val = extract_attribute(tag_header, "id");

        let body_start = string_start.saturating_add(tag_close_rel).saturating_add(1);
        let end_string_rel = match text.get(body_start..).and_then(|s| find_tag_close(s, "string")) {
            Some(idx) => idx,
            None => break,
        };
        let string_body = text
            .get(body_start..body_start.saturating_add(end_string_rel))
            .unwrap_or("");
        search_from = body_start.saturating_add(end_string_rel);

        if let Some(id) = id_val {
            if !id.is_empty() {
                if let Some(text_content) = extract_tag_content(string_body, "text") {
                    let decoded_text = decode_xml_entities(text_content.trim());
                    if !decoded_text.is_empty() {
                        values.insert(id, decoded_text);
                    }
                }
            }
        }
    }
}

/// Same result as `text.to_ascii_lowercase().starts_with(prefix)` without copying the rest of `text`.
fn starts_with_ascii_lowercase(text: &str, prefix: &str) -> bool {
    text.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.iter().map(u8::to_ascii_lowercase).eq(prefix.bytes()))
}

fn find_tag_open(s: &str, tag_name: &str) -> Option<usize> {
    let mut search_idx = 0usize;
    while let Some(idx) = s.get(search_idx..).and_then(|sub| sub.find('<')) {
        let abs_idx = search_idx.saturating_add(idx);
        let after = s.get(abs_idx.saturating_add(1)..)?;
        if starts_with_ascii_lowercase(after, tag_name) {
            let next_char = after.chars().nth(tag_name.len());
            if next_char.is_none()
                || next_char == Some(' ')
                || next_char == Some('>')
                || next_char == Some('\t')
                || next_char == Some('\n')
                || next_char == Some('\r')
            {
                return Some(abs_idx);
            }
        }
        search_idx = abs_idx.saturating_add(1);
    }
    None
}

fn find_tag_close(s: &str, tag_name: &str) -> Option<usize> {
    let mut search_idx = 0usize;
    while let Some(idx) = s.get(search_idx..).and_then(|sub| sub.find("</")) {
        let abs_idx = search_idx.saturating_add(idx);
        let after = s.get(abs_idx.saturating_add(2)..)?;
        if starts_with_ascii_lowercase(after, tag_name) {
            let next_char = after.chars().nth(tag_name.len());
            if next_char.is_none()
                || next_char == Some(' ')
                || next_char == Some('>')
                || next_char == Some('\t')
                || next_char == Some('\n')
                || next_char == Some('\r')
            {
                return Some(abs_idx);
            }
        }
        search_idx = abs_idx.saturating_add(2);
    }
    None
}

fn extract_attribute(tag_header: &str, attr_name: &str) -> Option<String> {
    let lower_header = tag_header.to_ascii_lowercase();
    let attr_pattern = format!("{attr_name}=");
    let attr_pos = lower_header.find(&attr_pattern)?;
    let after_eq = tag_header
        .get(attr_pos.saturating_add(attr_pattern.len())..)?
        .trim_start();
    let quote = after_eq.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let after_quote = after_eq.get(1..)?;
    let close_quote = after_quote.find(quote)?;
    Some(after_quote.get(..close_quote)?.to_string())
}

fn extract_tag_content<'a>(body: &'a str, tag_name: &str) -> Option<&'a str> {
    let open_pos = find_tag_open(body, tag_name)?;
    let tag_close_pos = body.get(open_pos..)?.find('>')?;
    let content_start = open_pos.saturating_add(tag_close_pos).saturating_add(1);

    let close_tag_rel = find_tag_close(body.get(content_start..)?, tag_name)?;
    body.get(content_start..content_start.saturating_add(close_tag_rel))
}

fn decode_xml_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            let mut entity = String::new();
            let mut found_semicolon = false;
            while let Some(&next) = chars.peek() {
                chars.next();
                if next == ';' {
                    found_semicolon = true;
                    break;
                }
                entity.push(next);
                if entity.len() > 10 {
                    break;
                }
            }
            if found_semicolon {
                match entity.as_str() {
                    "quot" => out.push('"'),
                    "amp" => out.push('&'),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "apos" => out.push('\''),
                    _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                        if let Ok(val) = u32::from_str_radix(entity.get(2..).unwrap_or(""), 16) {
                            if let Some(c) = char::from_u32(val) {
                                out.push(c);
                            }
                        }
                    }
                    _ if entity.starts_with('#') => {
                        if let Ok(val) = entity.get(1..).unwrap_or("").parse::<u32>() {
                            if let Some(c) = char::from_u32(val) {
                                out.push(c);
                            }
                        }
                    }
                    _ => {
                        out.push('&');
                        out.push_str(&entity);
                        out.push(';');
                    }
                }
            } else {
                out.push('&');
                out.push_str(&entity);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tag_scan_tests {
    use super::{find_tag_close, find_tag_open};

    #[test]
    fn tag_names_match_case_insensitively_but_not_as_prefixes() {
        // `<stringx` is a different tag; `<String ` is the same tag in another case.
        assert_eq!(find_tag_open("<stringx><String id=\"a\">", "string"), Some(9));
        assert_eq!(find_tag_close("</stringx></STRING>", "string"), Some(10));
    }

    #[test]
    fn multibyte_text_before_a_tag_does_not_panic_or_match() {
        assert_eq!(find_tag_open("<ü<string>", "string"), Some(3));
        assert_eq!(find_tag_open("<ü>", "string"), None);
    }
}
