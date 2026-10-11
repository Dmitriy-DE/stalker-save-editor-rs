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

/// The UAX #14 (Unicode 17) boundary verdict for two adjacent code points: `true` when a break is allowed
/// between them.
///
/// Every rule that the two code points decide on their own is applied in the order of the standard, after LB1
/// class resolution. Rules that look further than the pair are answered the way they resolve with no further
/// context:
///
/// - LB15b, LB15c: a space before a quotation mark or a decimal separator breaks or not by what follows it;
/// - LB19a: a quotation mark between two East Asian characters may break, everywhere else it does not;
/// - LB20a, LB21a: a hyphen starting a word, or following a Hebrew letter, holds on to the next letter;
/// - LB25: closing punctuation and solidus inside a number;
/// - LB28a: a virama between two aksaras, and an aksara before a final consonant;
/// - LB30a: regional indicators pair up from the start of their run, and the wrapper only asks at grapheme
///   boundaries, where every pair is already complete.
#[must_use]
pub fn line_break_pair(left: char, right: char) -> bool {
    use LineBreakClass as C;
    let a = resolve_line(left);
    let b = resolve_line(right);
    // LB4, LB5: hard breaks.
    if a == C::Cr && b == C::Lf {
        return false;
    }
    if matches!(a, C::Bk | C::Cr | C::Lf | C::Nl) {
        return true;
    }
    // LB6, LB7.
    if matches!(b, C::Bk | C::Cr | C::Lf | C::Nl | C::Sp | C::Zw) {
        return false;
    }
    // LB8, LB8a.
    if a == C::Zw {
        return true;
    }
    if a == C::Zwj {
        return false;
    }
    // LB9: a combining mark or joiner attaches to whatever precedes it, except a space.
    if matches!(b, C::Cm | C::Zwj) && a != C::Sp {
        return false;
    }
    // LB10: a mark with no base behaves as a letter.
    let a = if a == C::Cm { C::Al } else { a };
    let b = if matches!(b, C::Cm | C::Zwj) { C::Al } else { b };
    // LB11, LB12, LB12a.
    if a == C::Wj || b == C::Wj || a == C::Gl {
        return false;
    }
    if b == C::Gl && !matches!(a, C::Sp | C::Ba | C::Hy | C::Hh) {
        return false;
    }
    // LB13, LB14, LB15d.
    if matches!(b, C::Cl | C::Cp | C::Ex | C::Sy) || a == C::Op || b == C::Is {
        return false;
    }
    // LB17.
    if a == C::B2 && b == C::B2 {
        return false;
    }
    // LB18.
    if a == C::Sp {
        return true;
    }
    // LB19, and LB19a outside an East Asian context.
    if a == C::Qu || b == C::Qu {
        return false;
    }
    // LB20.
    if a == C::Cb || b == C::Cb {
        return true;
    }
    // LB21, LB21b, LB22.
    if matches!(b, C::Ba | C::Hh | C::Hy | C::Ns | C::In) || a == C::Bb {
        return false;
    }
    if a == C::Sy && b == C::Hl {
        return false;
    }
    let letter = |class| matches!(class, C::Al | C::Hl);
    // LB23.
    if letter(a) && b == C::Nu || a == C::Nu && letter(b) {
        return false;
    }
    // LB23a.
    let ideograph = |class| matches!(class, C::Id | C::Eb | C::Em);
    if a == C::Pr && ideograph(b) || ideograph(a) && b == C::Po {
        return false;
    }
    // LB24.
    let affix = |class| matches!(class, C::Pr | C::Po);
    if affix(a) && letter(b) || letter(a) && affix(b) {
        return false;
    }
    // LB25.
    if matches!(a, C::Pr | C::Po | C::Hy | C::Is | C::Nu) && b == C::Nu || a == C::Nu && affix(b) {
        return false;
    }
    // LB26.
    if a == C::Jl && matches!(b, C::Jl | C::Jv | C::H2 | C::H3) {
        return false;
    }
    if matches!(a, C::Jv | C::H2) && matches!(b, C::Jv | C::Jt) {
        return false;
    }
    if matches!(a, C::Jt | C::H3) && b == C::Jt {
        return false;
    }
    // LB27.
    let hangul = |class| matches!(class, C::Jl | C::Jv | C::Jt | C::H2 | C::H3);
    if hangul(a) && b == C::Po || a == C::Pr && hangul(b) {
        return false;
    }
    // LB28.
    if letter(a) && letter(b) {
        return false;
    }
    // LB28a.
    let aksara = |class, character| matches!(class, C::Ak | C::As) || character == DOTTED_CIRCLE;
    if a == C::Ap && aksara(b, right) || aksara(a, left) && matches!(b, C::Vf | C::Vi) {
        return false;
    }
    // LB29.
    if a == C::Is && letter(b) {
        return false;
    }
    // LB30.
    let word = |class| matches!(class, C::Al | C::Hl | C::Nu);
    if word(a) && b == C::Op && !is_east_asian_bracket(right) || a == C::Cp && !is_east_asian_bracket(left) && word(b) {
        return false;
    }
    // LB30b.
    if b == C::Em && (a == C::Eb || is_unassigned_pictographic(left)) {
        return false;
    }
    // LB31.
    true
}

/// The character whose class decides a break after `cluster`: LB9 makes trailing combining marks take the
/// class of their base, while a trailing joiner keeps its own rule (LB8a).
#[must_use]
pub fn line_break_base(cluster: &str) -> Option<char> {
    let last = cluster.chars().next_back()?;
    if resolve_line(last) != LineBreakClass::Cm {
        return Some(last);
    }
    let base = cluster
        .chars()
        .rev()
        .find(|character| !matches!(resolve_line(*character), LineBreakClass::Cm | LineBreakClass::Zwj));
    match base {
        Some(base)
            if !matches!(
                resolve_line(base),
                LineBreakClass::Bk
                    | LineBreakClass::Cr
                    | LineBreakClass::Lf
                    | LineBreakClass::Nl
                    | LineBreakClass::Sp
                    | LineBreakClass::Zw
            ) =>
        {
            Some(base)
        }
        _ => Some(last),
    }
}

const DOTTED_CIRCLE: char = '\u{25cc}';

/// LB1 resolution of a code point's class.
fn resolve_line(character: char) -> LineBreakClass {
    match line_break_class(character) {
        LineBreakClass::Ai | LineBreakClass::Sg | LineBreakClass::Xx => LineBreakClass::Al,
        LineBreakClass::Sa if is_complex_context_mark(character) => LineBreakClass::Cm,
        LineBreakClass::Sa => LineBreakClass::Al,
        LineBreakClass::Cj => LineBreakClass::Ns,
        other => other,
    }
}

/// Whether a Line_Break=SA code point has General_Category Mn or Mc, which LB1 resolves to CM.
///
/// The table carries no General_Category, so this is derived from Grapheme_Cluster_Break: Extend and
/// SpacingMark cover the marks, apart from the two SARA AM letters that are SpacingMark without being marks and
/// the spacing marks that UAX #29 leaves out of SpacingMark.
fn is_complex_context_mark(character: char) -> bool {
    match u32::from(character) {
        0x0e33 | 0x0eb3 => false,
        0x102b..=0x102c
        | 0x1038
        | 0x1062..=0x1064
        | 0x1067..=0x106d
        | 0x1083
        | 0x1087..=0x108c
        | 0x108f
        | 0x109a..=0x109c
        | 0x1a61
        | 0x1a63..=0x1a64
        | 0xaa7b
        | 0xaa7d
        | 0x11720..=0x11721 => true,
        _ => matches!(
            grapheme_class(character),
            GraphemeClass::Extend | GraphemeClass::SpacingMark
        ),
    }
}

/// Whether an OP or CP code point has East_Asian_Width F, W or H, which exempts it from LB30. Every such
/// bracket lives in one of these blocks, and every OP or CP code point in them is wide or halfwidth.
fn is_east_asian_bracket(character: char) -> bool {
    matches!(
        u32::from(character),
        0x2329..=0x232a | 0x3000..=0x303f | 0xfe10..=0xfe6f | 0xff00..=0xffef
    )
}

/// Extended_Pictographic code points that are unassigned (LB30b). Line_Break=XX is the default of unassigned
/// code points outside the ideographic ranges, and no assigned XX code point is pictographic.
fn is_unassigned_pictographic(character: char) -> bool {
    line_break_class(character) == LineBreakClass::Xx && is_extended_pictographic(character)
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
    /// Class pairs where [`line_break_pair`] disagrees with the official two-code-point cases, grouped by the
    /// rule that decides them from more than the pair itself.
    ///
    /// Each entry is `Left>Right` with the corpus verdict: `÷` break allowed, `×` break forbidden. The comparison
    /// below is exact: a new disagreement or a silently fixed one fails the test.
    const KNOWN_PAIR_DIVERGENCES: &[(&str, &str)] = &[
        (
            "LB20a: a hyphen holds on to the letter after it only at the start of a word; the corpus pair starts \
             the text, a pair inside a line follows a word",
            "Hh>Ai× Hh>Al× Hh>Hl× Hh>Sa× Hh>Xx× Hy>Ai× Hy>Al× Hy>Hl× Hy>Sa× Hy>Xx×",
        ),
        (
            "LB30b: an unassigned pictographic code point of an ideographic block needs General_Category=Cn, \
             which the table does not carry; assigned ID code points break before a modifier",
            "Id>Em×",
        ),
        (
            "LB30a: regional indicators pair up counted from the start of their run",
            "Ri>Ri×",
        ),
        (
            "LB15b: a closing quotation mark stays with the space before it only when the text ends or closing \
             punctuation follows",
            "Sp>Qu×",
        ),
    ];

    #[test]
    fn line_break_base_skips_marks_but_keeps_a_trailing_joiner() {
        assert_eq!(line_break_base("e\u{301}"), Some('e'));
        assert_eq!(line_break_base("-\u{301}\u{308}"), Some('-'));
        assert_eq!(line_break_base("\u{301}"), Some('\u{301}'));
        assert_eq!(line_break_base("\u{1f468}\u{200d}"), Some('\u{200d}'));
        assert_eq!(line_break_base(""), None);
        // LB9 through the base: a hyphen with a mark still breaks before a letter, a letter with a mark does not.
        assert!(line_break_pair('-', 'a'));
        assert!(!line_break_pair('e', 'a'));
    }

    #[test]
    fn line_break_pair_applies_the_pairwise_rules_in_order() {
        // LB8 before LB13, LB20 before LB21, LB18 before LB21.
        assert!(line_break_pair('\u{200b}', ')'));
        assert!(line_break_pair('\u{fffc}', '-'));
        assert!(line_break_pair(' ', '-'));
        // LB9, LB14, LB19, LB25.
        assert!(!line_break_pair(')', '\u{308}'));
        assert!(!line_break_pair('(', '\u{4e00}'));
        assert!(!line_break_pair('\u{4e00}', '"'));
        assert!(!line_break_pair('$', '1'));
        assert!(!line_break_pair('1', '%'));
        // LB30 does not hold a letter to a wide bracket.
        assert!(!line_break_pair('a', '('));
        assert!(line_break_pair('a', '\u{3008}'));
        // LB28a: aksaras break apart unless a virama or the dotted circle joins them.
        assert!(line_break_pair('\u{1b05}', '\u{1b05}'));
        assert!(!line_break_pair('\u{1b05}', '\u{1b44}'));
        assert!(!line_break_pair('\u{25cc}', '\u{1b44}'));
    }

    #[test]
    fn official_unicode17_line_break_pairs_match_the_corpus_verdicts() {
        // The complete official corpus is retained in-tree. Cases of exactly two code points are the ones a
        // pairwise predicate can decide alone; longer cases need context and are wrapped in text.rs.
        let mut cases = 0_usize;
        let mut checked = 0_usize;
        let mut agreed = 0_usize;
        let mut diverged = std::collections::BTreeSet::new();
        for line in include_str!("../tests/unicode/LineBreakTest-17.0.0.txt").lines() {
            let Some((text, expected)) = parse_case(line) else {
                continue;
            };
            cases = cases.saturating_add(1);
            assert_eq!(expected.last().copied(), Some(text.len()), "{line}");
            let mut characters = text.char_indices();
            let (Some((_, left)), Some((offset, right)), None) =
                (characters.next(), characters.next(), characters.next())
            else {
                continue;
            };
            checked = checked.saturating_add(1);
            let corpus_allows_break = expected.contains(&offset);
            if line_break_pair(left, right) == corpus_allows_break {
                agreed = agreed.saturating_add(1);
            } else {
                let verdict = if corpus_allows_break { '÷' } else { '×' };
                diverged.insert(format!(
                    "{:?}>{:?}{verdict}",
                    line_break_class(left),
                    line_break_class(right)
                ));
            }
        }
        assert!(cases > 19_000);
        assert_eq!((agreed, checked), (4_783, 4_806), "agreeing two-code-point cases");
        let mut known = KNOWN_PAIR_DIVERGENCES
            .iter()
            .flat_map(|(_, pairs)| pairs.split_whitespace())
            .collect::<Vec<_>>();
        known.sort_unstable();
        assert_eq!(
            diverged.iter().map(String::as_str).collect::<Vec<_>>(),
            known,
            "pairwise verdicts changed against the official corpus"
        );
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
