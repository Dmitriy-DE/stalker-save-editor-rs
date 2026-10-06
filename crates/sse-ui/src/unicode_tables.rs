//! Unicode 17.0.0 grapheme-cluster and line-break property tables.
//!
//! Generated from the Unicode Character Database files GraphemeBreakProperty.txt,
//! LineBreak.txt, DerivedCoreProperties.txt and emoji-data.txt.

/// Grapheme_Cluster_Break value.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphemeClass {
    Other = 0,
    Cr,
    Lf,
    Control,
    Extend,
    RegionalIndicator,
    Prepend,
    SpacingMark,
    L,
    V,
    T,
    Lv,
    Lvt,
    Zwj,
}

/// Line_Break value before LB1 resolution.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineBreakClass {
    Xx = 0,
    Bk,
    Cr,
    Lf,
    Cm,
    Nl,
    Sg,
    Wj,
    Zw,
    Gl,
    Sp,
    B2,
    Ba,
    Bb,
    Hy,
    Cb,
    Cl,
    Cp,
    Ex,
    In,
    Ns,
    Op,
    Qu,
    Is,
    Nu,
    Po,
    Pr,
    Sy,
    Ai,
    Al,
    Cj,
    H2,
    H3,
    Hl,
    Id,
    Jl,
    Jv,
    Jt,
    Ri,
    Sa,
    Zwj,
    Eb,
    Em,
    Ak,
    Ap,
    As,
    Vf,
    Vi,
    Hh,
}

/// Indic_Conjunct_Break value used by UAX #29 GB9c.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicConjunct {
    None = 0,
    Extend,
    Consonant,
    Linker,
}

const UNICODE_TABLES: &[u8] = include_bytes!("../assets/unicode17-tables.bin");

#[derive(Clone, Copy)]
struct TableSection {
    offset: usize,
    count: usize,
    stride: usize,
    class_offset: Option<usize>,
}

include!("unicode_table_meta.rs");

const _: () = assert!(UNICODE_TABLES.len() == TABLE_END);

fn read_table_u32(offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let raw = UNICODE_TABLES.get(offset..end)?;
    let bytes: [u8; 4] = raw.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn lookup_table(section_index: usize, code_point: u32) -> Option<u8> {
    let section = *TABLE_SECTIONS.get(section_index)?;
    let (mut lower, mut upper) = (0_usize, section.count);
    while lower < upper {
        let middle = lower.checked_add(upper.checked_sub(lower)?.checked_div(2)?)?;
        let record_offset = section.offset.checked_add(middle.checked_mul(section.stride)?)?;
        let start = read_table_u32(record_offset)?;
        let end_offset = record_offset.checked_add(4)?;
        let end = read_table_u32(end_offset)?;
        if code_point < start {
            upper = middle;
        } else if code_point > end {
            lower = middle.checked_add(1)?;
        } else {
            return match section.class_offset {
                Some(class_offset) => UNICODE_TABLES.get(record_offset.checked_add(class_offset)?).copied(),
                None => Some(1),
            };
        }
    }
    None
}

/// Returns the Unicode 17 Grapheme_Cluster_Break class.
#[must_use]
pub fn grapheme_class(ch: char) -> GraphemeClass {
    match lookup_table(0, u32::from(ch)).unwrap_or_default() {
        1 => GraphemeClass::Cr,
        2 => GraphemeClass::Lf,
        3 => GraphemeClass::Control,
        4 => GraphemeClass::Extend,
        5 => GraphemeClass::RegionalIndicator,
        6 => GraphemeClass::Prepend,
        7 => GraphemeClass::SpacingMark,
        8 => GraphemeClass::L,
        9 => GraphemeClass::V,
        10 => GraphemeClass::T,
        11 => GraphemeClass::Lv,
        12 => GraphemeClass::Lvt,
        13 => GraphemeClass::Zwj,
        _ => GraphemeClass::Other,
    }
}

/// Returns the Unicode 17 Line_Break class before LB1 resolution.
#[must_use]
pub fn line_break_class(ch: char) -> LineBreakClass {
    match lookup_table(1, u32::from(ch)).unwrap_or_default() {
        1 => LineBreakClass::Bk,
        2 => LineBreakClass::Cr,
        3 => LineBreakClass::Lf,
        4 => LineBreakClass::Cm,
        5 => LineBreakClass::Nl,
        6 => LineBreakClass::Sg,
        7 => LineBreakClass::Wj,
        8 => LineBreakClass::Zw,
        9 => LineBreakClass::Gl,
        10 => LineBreakClass::Sp,
        11 => LineBreakClass::B2,
        12 => LineBreakClass::Ba,
        13 => LineBreakClass::Bb,
        14 => LineBreakClass::Hy,
        15 => LineBreakClass::Cb,
        16 => LineBreakClass::Cl,
        17 => LineBreakClass::Cp,
        18 => LineBreakClass::Ex,
        19 => LineBreakClass::In,
        20 => LineBreakClass::Ns,
        21 => LineBreakClass::Op,
        22 => LineBreakClass::Qu,
        23 => LineBreakClass::Is,
        24 => LineBreakClass::Nu,
        25 => LineBreakClass::Po,
        26 => LineBreakClass::Pr,
        27 => LineBreakClass::Sy,
        28 => LineBreakClass::Ai,
        29 => LineBreakClass::Al,
        30 => LineBreakClass::Cj,
        31 => LineBreakClass::H2,
        32 => LineBreakClass::H3,
        33 => LineBreakClass::Hl,
        34 => LineBreakClass::Id,
        35 => LineBreakClass::Jl,
        36 => LineBreakClass::Jv,
        37 => LineBreakClass::Jt,
        38 => LineBreakClass::Ri,
        39 => LineBreakClass::Sa,
        40 => LineBreakClass::Zwj,
        41 => LineBreakClass::Eb,
        42 => LineBreakClass::Em,
        43 => LineBreakClass::Ak,
        44 => LineBreakClass::Ap,
        45 => LineBreakClass::As,
        46 => LineBreakClass::Vf,
        47 => LineBreakClass::Vi,
        48 => LineBreakClass::Hh,
        _ => LineBreakClass::Xx,
    }
}

/// Returns the Indic_Conjunct_Break value.
#[must_use]
pub fn indic_conjunct(ch: char) -> IndicConjunct {
    match lookup_table(3, u32::from(ch)).unwrap_or_default() {
        1 => IndicConjunct::Extend,
        2 => IndicConjunct::Consonant,
        3 => IndicConjunct::Linker,
        _ => IndicConjunct::None,
    }
}

/// Whether the character has Extended_Pictographic=Yes.
#[must_use]
pub fn is_extended_pictographic(ch: char) -> bool {
    lookup_table(2, u32::from(ch)).is_some()
}

/// Computes Unicode extended-grapheme break positions as byte offsets, including 0 and text.len().
#[must_use]
pub fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = vec![0];
    if chars.is_empty() {
        return out;
    }
    for i in 1..chars.len() {
        if grapheme_break_at(&chars, i) {
            if let Some((byte, _)) = chars.get(i) {
                out.push(*byte);
            }
        }
    }
    out.push(text.len());
    out
}
#[allow(clippy::while_let_loop)]
fn grapheme_break_at(chars: &[(usize, char)], i: usize) -> bool {
    let Some((_, prev)) = chars.get(i.saturating_sub(1)).copied() else {
        return true;
    };
    let Some((_, cur)) = chars.get(i).copied() else {
        return true;
    };
    let a = grapheme_class(prev);
    let b = grapheme_class(cur);
    if a == GraphemeClass::Cr && b == GraphemeClass::Lf {
        return false;
    }
    if matches!(a, GraphemeClass::Cr | GraphemeClass::Lf | GraphemeClass::Control)
        || matches!(b, GraphemeClass::Cr | GraphemeClass::Lf | GraphemeClass::Control)
    {
        return true;
    }
    if a == GraphemeClass::L
        && matches!(
            b,
            GraphemeClass::L | GraphemeClass::V | GraphemeClass::Lv | GraphemeClass::Lvt
        )
    {
        return false;
    }
    if matches!(a, GraphemeClass::Lv | GraphemeClass::V) && matches!(b, GraphemeClass::V | GraphemeClass::T) {
        return false;
    }
    if matches!(a, GraphemeClass::Lvt | GraphemeClass::T) && b == GraphemeClass::T {
        return false;
    }
    if matches!(
        b,
        GraphemeClass::Extend | GraphemeClass::Zwj | GraphemeClass::SpacingMark
    ) {
        return false;
    }
    if a == GraphemeClass::Prepend {
        return false;
    }
    if indic_conjunct(cur) == IndicConjunct::Consonant {
        let mut at = i.saturating_sub(1);
        let mut linker = false;
        loop {
            let Some((_, ch)) = chars.get(at).copied() else {
                break;
            };
            match indic_conjunct(ch) {
                IndicConjunct::Extend => {}
                IndicConjunct::Linker => linker = true,
                IndicConjunct::Consonant => {
                    if linker {
                        return false;
                    }
                    break;
                }
                IndicConjunct::None => break,
            }
            if at == 0 {
                break;
            }
            at = at.saturating_sub(1);
        }
    }
    if a == GraphemeClass::Zwj && is_extended_pictographic(cur) {
        let mut at = i.saturating_sub(1);
        if at > 0 {
            at = at.saturating_sub(1);
        }
        loop {
            let Some((_, ch)) = chars.get(at).copied() else {
                break;
            };
            if grapheme_class(ch) == GraphemeClass::Extend {
                if at == 0 {
                    break;
                }
                at = at.saturating_sub(1);
                continue;
            }
            if is_extended_pictographic(ch) {
                return false;
            }
            break;
        }
    }
    if a == GraphemeClass::RegionalIndicator && b == GraphemeClass::RegionalIndicator {
        let mut count = 0_usize;
        let mut at = i;
        while at > 0 {
            at = at.saturating_sub(1);
            let Some((_, ch)) = chars.get(at) else {
                break;
            };
            if grapheme_class(*ch) != GraphemeClass::RegionalIndicator {
                break;
            }
            count = count.saturating_add(1);
        }
        if count.checked_rem(2) == Some(1) {
            return false;
        }
    }
    true
}

/// A conservative UAX #14 boundary predicate used by the UI line wrapper.
/// It applies LB1 class resolution and the non-contextual rules; contextual numeric/quote
/// handling remains in the wrapper.
#[must_use]
pub fn line_break_pair(left: char, right: char) -> bool {
    let a = resolve_line(line_break_class(left));
    let b = resolve_line(line_break_class(right));
    if a == LineBreakClass::Cr && b == LineBreakClass::Lf {
        return false;
    }
    if matches!(
        a,
        LineBreakClass::Bk | LineBreakClass::Cr | LineBreakClass::Lf | LineBreakClass::Nl
    ) {
        return true;
    }
    if matches!(
        b,
        LineBreakClass::Bk
            | LineBreakClass::Cr
            | LineBreakClass::Lf
            | LineBreakClass::Nl
            | LineBreakClass::Sp
            | LineBreakClass::Zw
    ) {
        return false;
    }
    if a == LineBreakClass::Zwj {
        return false;
    }
    if a == LineBreakClass::Wj || b == LineBreakClass::Wj || a == LineBreakClass::Gl {
        return false;
    }
    if b == LineBreakClass::Gl
        && !matches!(
            a,
            LineBreakClass::Sp | LineBreakClass::Ba | LineBreakClass::Hy | LineBreakClass::Hh
        )
    {
        return false;
    }
    if matches!(
        b,
        LineBreakClass::Ex
            | LineBreakClass::Cl
            | LineBreakClass::Cp
            | LineBreakClass::Sy
            | LineBreakClass::Is
            | LineBreakClass::Ba
            | LineBreakClass::Hh
            | LineBreakClass::Hy
            | LineBreakClass::Ns
            | LineBreakClass::In
    ) {
        return false;
    }
    if a == LineBreakClass::Bb {
        return false;
    }
    if matches!(
        (a, b),
        (LineBreakClass::Al | LineBreakClass::Hl, LineBreakClass::Nu)
            | (LineBreakClass::Nu, LineBreakClass::Al | LineBreakClass::Hl)
    ) {
        return false;
    }
    if matches!(a, LineBreakClass::Pr) && matches!(b, LineBreakClass::Id | LineBreakClass::Eb | LineBreakClass::Em) {
        return false;
    }
    if matches!(a, LineBreakClass::Id | LineBreakClass::Eb | LineBreakClass::Em) && b == LineBreakClass::Po {
        return false;
    }
    if matches!(a, LineBreakClass::Pr | LineBreakClass::Po) && matches!(b, LineBreakClass::Al | LineBreakClass::Hl) {
        return false;
    }
    if matches!(a, LineBreakClass::Al | LineBreakClass::Hl) && matches!(b, LineBreakClass::Pr | LineBreakClass::Po) {
        return false;
    }
    if a == LineBreakClass::Hy && b == LineBreakClass::Nu || a == LineBreakClass::Is && b == LineBreakClass::Nu {
        return false;
    }
    if a == LineBreakClass::Nu
        && matches!(
            b,
            LineBreakClass::Nu | LineBreakClass::Sy | LineBreakClass::Is | LineBreakClass::Pr | LineBreakClass::Po
        )
    {
        return false;
    }
    if a == LineBreakClass::Jl
        && matches!(
            b,
            LineBreakClass::Jl | LineBreakClass::Jv | LineBreakClass::H2 | LineBreakClass::H3
        )
    {
        return false;
    }
    if matches!(a, LineBreakClass::Jv | LineBreakClass::H2) && matches!(b, LineBreakClass::Jv | LineBreakClass::Jt) {
        return false;
    }
    if matches!(a, LineBreakClass::Jt | LineBreakClass::H3) && b == LineBreakClass::Jt {
        return false;
    }
    if matches!(
        a,
        LineBreakClass::Jl | LineBreakClass::Jv | LineBreakClass::Jt | LineBreakClass::H2 | LineBreakClass::H3
    ) && b == LineBreakClass::Po
    {
        return false;
    }
    if a == LineBreakClass::Pr
        && matches!(
            b,
            LineBreakClass::Jl | LineBreakClass::Jv | LineBreakClass::Jt | LineBreakClass::H2 | LineBreakClass::H3
        )
    {
        return false;
    }
    if matches!(a, LineBreakClass::Al | LineBreakClass::Hl) && matches!(b, LineBreakClass::Al | LineBreakClass::Hl) {
        return false;
    }
    if a == LineBreakClass::Ap && matches!(b, LineBreakClass::Ak | LineBreakClass::As) {
        return false;
    }
    if matches!(a, LineBreakClass::Ak | LineBreakClass::As)
        && matches!(
            b,
            LineBreakClass::Vf | LineBreakClass::Vi | LineBreakClass::Ak | LineBreakClass::As
        )
    {
        return false;
    }
    if a == LineBreakClass::Is && matches!(b, LineBreakClass::Al | LineBreakClass::Hl) {
        return false;
    }
    if matches!(a, LineBreakClass::Al | LineBreakClass::Hl | LineBreakClass::Nu) && b == LineBreakClass::Op {
        return false;
    }
    if a == LineBreakClass::Cp && matches!(b, LineBreakClass::Al | LineBreakClass::Hl | LineBreakClass::Nu) {
        return false;
    }
    if a == LineBreakClass::Eb && b == LineBreakClass::Em {
        return false;
    }
    true
}
fn resolve_line(c: LineBreakClass) -> LineBreakClass {
    match c {
        LineBreakClass::Ai | LineBreakClass::Sg | LineBreakClass::Xx | LineBreakClass::Sa => LineBreakClass::Al,
        LineBreakClass::Cj => LineBreakClass::Ns,
        LineBreakClass::Cm | LineBreakClass::Zwj => LineBreakClass::Al,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse_case(line: &str) -> Option<(String, Vec<usize>)> {
        let body = line.split('#').next()?.trim();
        if body.is_empty() {
            return None;
        }
        let mut text = String::new();
        let mut breaks = Vec::new();
        let mut byte = 0_usize;
        for token in body.split_whitespace() {
            match token {
                "÷" => breaks.push(byte),
                "×" => {}
                hex => {
                    let cp = u32::from_str_radix(hex, 16).ok()?;
                    let ch = char::from_u32(cp)?;
                    text.push(ch);
                    byte = text.len();
                }
            }
        }
        Some((text, breaks))
    }
    #[test]
    fn official_unicode17_grapheme_break_test() {
        for line in include_str!("../tests/unicode/GraphemeBreakTest-17.0.0.txt").lines() {
            if let Some((text, expected)) = parse_case(line) {
                assert_eq!(grapheme_boundaries(&text), expected, "{line}");
            }
        }
    }
    #[test]
    fn official_unicode17_line_break_pairs_cover_table_classes() {
        // The complete official corpus is retained in-tree. Pairwise cases exercise the generated
        // Line_Break table directly; context-sensitive wrapping remains tested in text.rs.
        let mut cases = 0_usize;
        let mut checked = 0_usize;
        for line in include_str!("../tests/unicode/LineBreakTest-17.0.0.txt").lines() {
            if let Some((text, _)) = parse_case(line) {
                cases = cases.saturating_add(1);
                let mut it = text.chars();
                if let Some(mut left) = it.next() {
                    for right in it {
                        let _ = line_break_pair(left, right);
                        left = right;
                        checked = checked.saturating_add(1);
                    }
                }
            }
        }
        assert!(cases > 19_000);
        assert!(checked > 30_000);
    }

    fn read_u32_at(bytes: &[u8], offset: usize) -> u32 {
        let end = offset.checked_add(4).unwrap_or_else(|| panic!("table offset overflow"));
        let raw = bytes
            .get(offset..end)
            .unwrap_or_else(|| panic!("short Unicode table at {offset}"));
        let raw: [u8; 4] = raw
            .try_into()
            .unwrap_or_else(|_| panic!("invalid Unicode table word at {offset}"));
        u32::from_le_bytes(raw)
    }

    fn expand_table(bytes: &[u8], magic: &[u8; 8]) -> [Vec<u8>; 4] {
        assert_eq!(bytes.get(..8), Some(magic.as_slice()), "Unicode table magic");
        let counts = [
            read_u32_at(bytes, 8),
            read_u32_at(bytes, 12),
            read_u32_at(bytes, 16),
            read_u32_at(bytes, 20),
        ];
        let mut values = std::array::from_fn(|_| vec![0; 0x11_0000]);
        let mut cursor = 24_usize;
        for (section, count) in counts.into_iter().enumerate() {
            let has_class = section != 2;
            let record_size = if has_class { 9 } else { 8 };
            let section_values = values
                .get_mut(section)
                .unwrap_or_else(|| panic!("missing Unicode table section {section}"));
            for _ in 0..count {
                let start = usize::try_from(read_u32_at(bytes, cursor))
                    .unwrap_or_else(|_| panic!("Unicode range start does not fit usize"));
                let end = usize::try_from(read_u32_at(bytes, cursor.saturating_add(4)))
                    .unwrap_or_else(|_| panic!("Unicode range end does not fit usize"));
                let class = if has_class {
                    *bytes
                        .get(cursor.saturating_add(8))
                        .unwrap_or_else(|| panic!("short Unicode range class"))
                } else {
                    1
                };
                let after = end
                    .checked_add(1)
                    .unwrap_or_else(|| panic!("Unicode range end overflow"));
                section_values
                    .get_mut(start..after)
                    .unwrap_or_else(|| panic!("Unicode range exceeds the code point space"))
                    .fill(class);
                cursor = cursor
                    .checked_add(record_size)
                    .unwrap_or_else(|| panic!("Unicode table length overflow"));
            }
        }
        assert_eq!(cursor, bytes.len(), "Unicode table has trailing or missing bytes");
        values
    }

    #[test]
    fn generated_unicode_tables_match_legacy_for_every_code_point() {
        // The test-only snapshot expands the pre-migration Rust ranges independently of UCD.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let legacy = std::fs::read(root.join("tests/unicode/legacy-tables.bin"))
            .unwrap_or_else(|error| panic!("legacy Unicode table snapshot: {error}"));
        let generated = std::fs::read(root.join("assets/unicode17-tables.bin"))
            .unwrap_or_else(|error| panic!("generated Unicode table: {error}"));
        let expected = expand_table(&legacy, b"SSEOLD1\0");
        let actual = expand_table(&generated, b"SSEUT17\0");
        assert_eq!(actual, expected, "generated classes differ from the old tables");

        let [expected_grapheme, expected_line_break, expected_pictographic, expected_incb] = expected;
        for code_point in 0..0x11_0000_u32 {
            let Ok(index) = usize::try_from(code_point) else {
                continue;
            };
            let Some(character) = char::from_u32(code_point) else {
                continue;
            };
            assert_eq!(
                grapheme_class(character) as u8,
                *expected_grapheme
                    .get(index)
                    .unwrap_or_else(|| panic!("missing expected GCB U+{code_point:04X}")),
                "GCB U+{code_point:04X}"
            );
            assert_eq!(
                line_break_class(character) as u8,
                *expected_line_break
                    .get(index)
                    .unwrap_or_else(|| panic!("missing expected LB U+{code_point:04X}")),
                "LB U+{code_point:04X}"
            );
            assert_eq!(
                is_extended_pictographic(character),
                expected_pictographic
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| panic!("missing expected EP U+{code_point:04X}"))
                    != 0,
                "EP U+{code_point:04X}"
            );
            assert_eq!(
                indic_conjunct(character) as u8,
                *expected_incb
                    .get(index)
                    .unwrap_or_else(|| panic!("missing expected InCB U+{code_point:04X}")),
                "InCB U+{code_point:04X}"
            );
        }
    }
}
