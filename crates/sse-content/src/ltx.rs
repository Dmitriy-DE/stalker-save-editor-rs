//! X-Ray LTX configuration document parser and inheritance resolver.
//!
//! Supports section inheritance `[name]:base1,base2`, `#include` directives,
//! wildcard includes, comments, and cycles handling.

use std::collections::{HashMap, HashSet};

use crate::encoding::decode_text;

/// One section in an LTX document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LtxSection {
    /// Section name as declared in `[name]`.
    pub name: String,
    /// Base section names this section inherits from.
    pub bases: Vec<String>,
    /// Source file path where this section was parsed from.
    pub source: String,
    /// Key-value properties defined in this section. Keys are lowercase.
    pub values: HashMap<String, String>,
    /// Bare entries (lines without `=`).
    pub entries: Vec<String>,
}

impl LtxSection {
    /// Creates a new section.
    #[must_use]
    pub fn new(name: impl Into<String>, bases: Vec<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            bases,
            source: source.into(),
            values: HashMap::new(),
            entries: Vec::new(),
        }
    }
}

/// LTX document parser.
pub struct LtxDocument;

impl LtxDocument {
    /// Decodes raw bytes of an LTX file, supporting UTF-8 (with BOM) and Windows-1251.
    #[must_use]
    pub fn decode(data: &[u8]) -> String {
        decode_text(data)
    }

    /// Parses an LTX document text.
    #[must_use]
    pub fn parse(text: &str, source: &str) -> HashMap<String, LtxSection> {
        let mut sections = HashMap::new();
        Self::parse_internal(text, source, &mut sections, &mut |_| {});
        sections
    }

    /// Parses the complete include graph starting from `root_path`.
    ///
    /// Follows `#include` directives relative to each including file.
    #[must_use]
    pub fn parse_include_graph<F>(
        root_path: &str,
        files: &HashMap<String, F>,
        read_file: impl Fn(&F) -> Option<Vec<u8>>,
    ) -> Option<HashMap<String, LtxSection>> {
        let root_key = find_key_case_insensitive(files, root_path)?;
        let mut sections = HashMap::new();
        let mut visited = HashSet::new();

        // Index candidate paths by folder for fast wildcard lookup
        let mut by_directory: HashMap<String, Vec<String>> = HashMap::new();
        for key in files.keys() {
            let folder = directory_of(key);
            by_directory.entry(folder).or_default().push(key.clone());
        }

        Self::visit_include(&root_key, files, &read_file, &by_directory, &mut visited, &mut sections);

        Some(sections)
    }

    fn visit_include<F>(
        path: &str,
        files: &HashMap<String, F>,
        read_file: &impl Fn(&F) -> Option<Vec<u8>>,
        by_directory: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        sections: &mut HashMap<String, LtxSection>,
    ) {
        let lower_path = path.to_ascii_lowercase();
        if !visited.insert(lower_path) {
            return;
        }

        let Some(file_entry) = files.get(path) else {
            return;
        };

        let Some(raw_bytes) = read_file(file_entry) else {
            return;
        };

        let text = Self::decode(&raw_bytes);
        let directory = directory_of(path);

        let mut includes = Vec::new();
        Self::parse_internal(&text, path, sections, &mut |inc| {
            includes.push(inc.to_string());
        });

        for inc in includes {
            let combined = format!("{}{}", directory, inc.replace('\\', "/"));
            let target = normalize_relative(&combined);

            if !target.contains('*') {
                if let Some(actual_key) = find_key_case_insensitive(files, &target) {
                    Self::visit_include(&actual_key, files, read_file, by_directory, visited, sections);
                }
                continue;
            }

            let folder = directory_of(&target);
            let mut candidates = Vec::new();
            if folder.contains('*') {
                for key in files.keys() {
                    if Self::matches_mask(key, &target) {
                        candidates.push(key.clone());
                    }
                }
            } else if let Some(in_folder) = by_directory.get(&folder) {
                let mask = target.get(folder.len()..).unwrap_or("");
                for cand in in_folder {
                    let sub = cand.get(folder.len()..).unwrap_or("");
                    if Self::matches_mask(sub, mask) {
                        candidates.push(cand.clone());
                    }
                }
            }

            candidates.sort_by_key(|a| a.to_ascii_lowercase());
            for match_path in candidates {
                Self::visit_include(&match_path, files, read_file, by_directory, visited, sections);
            }
        }
    }

    fn parse_internal(
        text: &str,
        source: &str,
        sections: &mut HashMap<String, LtxSection>,
        on_include: &mut impl FnMut(&str),
    ) {
        let mut current_section: Option<String> = None;

        for raw_line in text.lines() {
            let line = strip_comment(raw_line);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(inc) = parse_include_directive(trimmed) {
                on_include(inc);
                continue;
            }

            if let Some((name, bases)) = parse_section_header(trimmed) {
                let section = LtxSection::new(name.clone(), bases, source);
                sections.insert(name.clone(), section);
                current_section = Some(name);
                continue;
            }

            let Some(sec_name) = &current_section else {
                continue;
            };

            let Some(sec) = sections.get_mut(sec_name) else {
                continue;
            };

            if let Some(equals_pos) = trimmed.find('=') {
                let key = trimmed.get(..equals_pos).unwrap_or("").trim().to_ascii_lowercase();
                let raw_val = trimmed.get(equals_pos.saturating_add(1)..).unwrap_or("").trim();
                let clean_val = strip_quotes(raw_val).trim();
                sec.values.insert(key, clean_val.to_string());
            } else {
                sec.entries.push(trimmed.to_string());
            }
        }
    }

    /// Resolves inheritance for all sections, returning each section with its resolved key-values.
    #[must_use]
    pub fn resolve(sections: &HashMap<String, LtxSection>) -> Vec<(LtxSection, HashMap<String, String>)> {
        let mut cache: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut result = Vec::with_capacity(sections.len());

        for section in sections.values() {
            let mut cut = false;
            let mut stack = HashSet::new();
            let resolved = Self::resolve_one(&section.name, sections, &mut cache, &mut stack, &mut cut);
            result.push((section.clone(), resolved));
        }

        result
    }

    fn resolve_one(
        name: &str,
        sections: &HashMap<String, LtxSection>,
        cache: &mut HashMap<String, HashMap<String, String>>,
        stack: &mut HashSet<String>,
        cut: &mut bool,
    ) -> HashMap<String, String> {
        if let Some(cached) = cache.get(name) {
            return cached.clone();
        }

        if !stack.insert(name.to_string()) {
            *cut = true;
            return HashMap::new();
        }

        let Some(section) = sections.get(name) else {
            stack.remove(name);
            return HashMap::new();
        };

        // Track cycle cuts per subtree: only a subtree that cut no cycle is independent of the stack,
        // so only that one may be cached. A cut elsewhere must not stop unrelated nodes from caching.
        let outer_cut = std::mem::replace(cut, false);
        let mut values: Option<HashMap<String, String>> = None;

        for parent in &section.bases {
            if !sections.contains_key(parent) {
                continue;
            }
            let inherited = Self::resolve_one(parent, sections, cache, stack, cut);
            match &mut values {
                None => {
                    values = Some(inherited);
                }
                Some(current) => {
                    for (k, v) in inherited {
                        current.insert(k, v);
                    }
                }
            }
        }

        let final_values = match values {
            None => section.values.clone(),
            Some(mut merged) => {
                for (k, v) in &section.values {
                    merged.insert(k.clone(), v.clone());
                }
                merged
            }
        };

        stack.remove(name);
        let subtree_cut = *cut;
        *cut = outer_cut || subtree_cut;
        if !subtree_cut {
            cache.insert(name.to_string(), final_values.clone());
        }

        final_values
    }

    /// Simple wildcard mask matcher where `*` matches 0 or more characters (case-insensitive).
    #[must_use]
    pub fn matches_mask(name: &str, mask: &str) -> bool {
        let name_chars: Vec<char> = name.chars().collect();
        let mask_chars: Vec<char> = mask.chars().collect();

        let mut name_idx = 0usize;
        let mut mask_idx = 0usize;
        let mut star_idx: Option<usize> = None;
        let mut resume_idx = 0usize;

        while name_idx < name_chars.len() {
            if mask_idx < mask_chars.len() && mask_chars.get(mask_idx).copied() == Some('*') {
                star_idx = Some(mask_idx);
                mask_idx = mask_idx.saturating_add(1);
                resume_idx = name_idx;
            } else if mask_idx < mask_chars.len()
                && chars_equal_ignore_case(mask_chars.get(mask_idx).copied(), name_chars.get(name_idx).copied())
            {
                mask_idx = mask_idx.saturating_add(1);
                name_idx = name_idx.saturating_add(1);
            } else if let Some(star) = star_idx {
                mask_idx = star.saturating_add(1);
                resume_idx = resume_idx.saturating_add(1);
                name_idx = resume_idx;
            } else {
                return false;
            }
        }

        while mask_idx < mask_chars.len() && mask_chars.get(mask_idx).copied() == Some('*') {
            mask_idx = mask_idx.saturating_add(1);
        }

        mask_idx == mask_chars.len()
    }
}

fn chars_equal_ignore_case(a: Option<char>, b: Option<char>) -> bool {
    match (a, b) {
        (Some(c1), Some(c2)) => c1.to_uppercase().eq(c2.to_uppercase()),
        _ => false,
    }
}

fn directory_of(path: &str) -> String {
    if let Some(pos) = path.rfind('/') {
        path.get(..pos.saturating_add(1)).unwrap_or("").to_string()
    } else {
        String::new()
    }
}

fn normalize_relative(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if !parts.is_empty() {
                parts.pop();
            }
        } else {
            parts.push(part);
        }
    }
    parts.join("/")
}

fn strip_comment(line: &str) -> &str {
    let mut in_quote: Option<char> = None;
    for (idx, ch) in line.char_indices() {
        if ch == '"' || ch == '\'' {
            if in_quote == Some(ch) {
                in_quote = None;
            } else if in_quote.is_none() {
                in_quote = Some(ch);
            }
        } else if in_quote.is_none() && ch == ';' {
            return line.get(..idx).unwrap_or(line);
        }
    }
    line
}

fn strip_quotes(s: &str) -> &str {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        if s.len() >= 2 {
            s.get(1..s.len().saturating_sub(1)).unwrap_or("")
        } else {
            ""
        }
    } else {
        s
    }
}

fn parse_include_directive(line: &str) -> Option<&str> {
    if !line.to_ascii_lowercase().starts_with("#include") {
        return None;
    }
    let rest = line.get(8..)?.trim();
    let quote_char = rest.chars().next()?;
    if quote_char != '"' && quote_char != '\'' {
        return None;
    }
    let after_open = rest.get(1..)?;
    let close_idx = after_open.find(quote_char)?;
    after_open.get(..close_idx)
}

fn parse_section_header(line: &str) -> Option<(String, Vec<String>)> {
    if !line.starts_with('[') {
        return None;
    }
    let close_bracket = line.find(']')?;
    let name = line.get(1..close_bracket)?.trim().to_string();
    if name.is_empty() {
        return None;
    }

    let remainder = line.get(close_bracket.saturating_add(1)..)?.trim();
    let mut bases = Vec::new();
    if let Some(bases_str) = remainder.strip_prefix(':') {
        for b in bases_str.split(',') {
            let trimmed_base = b.trim();
            if !trimmed_base.is_empty() {
                bases.push(trimmed_base.to_string());
            }
        }
    }

    Some((name, bases))
}

fn find_key_case_insensitive<V>(map: &HashMap<String, V>, target: &str) -> Option<String> {
    if map.contains_key(target) {
        return Some(target.to_string());
    }
    let target_lower = target.to_ascii_lowercase();
    for key in map.keys() {
        if key.to_ascii_lowercase() == target_lower {
            return Some(key.clone());
        }
    }
    None
}

#[cfg(test)]
mod resolve_tests {
    use super::{HashSet, LtxDocument, LtxSection};
    use std::collections::HashMap;

    fn add(sections: &mut HashMap<String, LtxSection>, name: &str, bases: Vec<String>) {
        sections.insert(name.to_string(), LtxSection::new(name, bases, "test.ltx"));
    }

    #[test]
    fn a_cycle_does_not_make_later_diamond_inheritance_exponential() {
        // `top` cuts a cycle first, then reaches a 40-level diamond that no earlier resolution has cached.
        // Without per-subtree caching the diamond is walked once per path (2^40) after the cut.
        const DEPTH: usize = 40;
        let mut sections = HashMap::new();
        add(&mut sections, "cyc_x", vec!["cyc_y".to_string()]);
        add(&mut sections, "cyc_y", vec!["cyc_x".to_string()]);
        for level in 0..DEPTH {
            let next = level + 1;
            let bases = vec![format!("d{next}_a"), format!("d{next}_b")];
            add(&mut sections, &format!("d{level}_a"), bases.clone());
            add(&mut sections, &format!("d{level}_b"), bases);
        }
        add(&mut sections, &format!("d{DEPTH}_a"), Vec::new());
        add(&mut sections, &format!("d{DEPTH}_b"), Vec::new());
        if let Some(leaf) = sections.get_mut(&format!("d{DEPTH}_a")) {
            leaf.values.insert("leaf".to_string(), "1".to_string());
        }
        add(&mut sections, "top", vec!["cyc_x".to_string(), "d0_a".to_string()]);

        let mut cache = HashMap::new();
        let mut stack = HashSet::new();
        let mut cut = false;
        let resolved = LtxDocument::resolve_one("top", &sections, &mut cache, &mut stack, &mut cut);

        assert_eq!(resolved.get("leaf").map(String::as_str), Some("1"));
    }
}
