//! Unicode-aware text layout helpers for the retained UI.
//!
//! This module intentionally does not shape text: its input contract exposes only per-scalar
//! advances and kerning. It does, however, keep caret stops on extended grapheme clusters for
//! the scripts used by the editor, applies practical CJK line-breaking constraints, and performs
//! locale-aware search folding without allocating a folded copy of the haystack.

const SOFT_HYPHEN: char = '\u{00ad}';
const ELLIPSIS: char = '…';

/// Width information supplied by the font layer.
pub trait Metrics {
    /// Horizontal advance of one Unicode scalar in UI pixels.
    fn advance(&self, character: char) -> f32;

    /// Pair kerning in UI pixels.
    fn kerning(&self, left: char, right: char) -> f32;
}

/// Locale rules that affect case folding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchLocale {
    /// Locale-independent folding suitable for Russian, Ukrainian, Polish, Czech and English UI.
    #[default]
    General,
    /// Turkish dotted/dotless-I rules (`I` ↔ `ı`, `İ` ↔ `i`).
    Turkish,
}

/// One laid-out source line. Byte offsets always point to UTF-8 boundaries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// First byte belonging to the line.
    pub start: usize,
    /// Byte just after the source content belonging to the line.
    pub end: usize,
    /// Measured line width. A chosen soft-hyphen break includes the visible hyphen advance.
    pub width: f32,
    /// `true` when this line ended because of CR, LF, or CRLF in the source.
    pub hard_break: bool,
    /// `true` when rendering must append a visible `-` at the end of this line.
    pub append_hyphen: bool,
}

/// A legal caret stop at a grapheme boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    /// UTF-8 byte offset in the source.
    pub byte: usize,
    /// Horizontal position from the beginning of the string.
    pub x: f32,
}

#[derive(Clone, Copy, Debug)]
struct Cluster {
    start: usize,
    end: usize,
    width: f32,
    first_visible: Option<char>,
    last_visible: Option<char>,
    first_char: char,
    last_char: char,
    hard_break: bool,
    soft_hyphen: bool,
    whitespace: bool,
    cjk: bool,
}

#[derive(Clone, Copy, Debug)]
struct BreakCandidate {
    next_cluster: usize,
    end_byte: usize,
    width: f32,
    append_hyphen: bool,
}

/// Breaks text greedily to `max_width` without splitting grapheme clusters.
///
/// Latin/Cyrillic/Korean text breaks at normal word opportunities. CJK text may break between
/// ideographs, while common closing punctuation is kept off the beginning of a line and opening
/// punctuation is kept off the end. U+00AD is zero-width unless it becomes the selected break.
#[must_use]
pub fn break_lines<M: Metrics>(text: &str, max_width: f32, metrics: &M) -> Vec<Line> {
    let clusters = collect_clusters(text, metrics);
    if clusters.is_empty() {
        return vec![Line {
            start: 0,
            end: 0,
            width: 0.0,
            hard_break: false,
            append_hyphen: false,
        }];
    }

    let limit = if max_width.is_finite() {
        max_width.max(0.0)
    } else {
        f32::MAX
    };
    let mut lines = Vec::new();
    let mut line_start = 0_usize;

    while line_start < clusters.len() {
        let Some(first) = clusters.get(line_start).copied() else {
            break;
        };
        if first.hard_break {
            lines.push(Line {
                start: first.start,
                end: first.start,
                width: 0.0,
                hard_break: true,
                append_hyphen: false,
            });
            line_start = line_start.saturating_add(1);
            continue;
        }

        let mut at = line_start;
        let mut width = 0.0_f32;
        let mut last_visible = None;
        let mut candidate = None;
        let mut emitted = false;

        while at < clusters.len() {
            let Some(cluster) = clusters.get(at).copied() else {
                break;
            };

            if cluster.hard_break {
                lines.push(Line {
                    start: first.start,
                    end: cluster.start,
                    width,
                    hard_break: true,
                    append_hyphen: false,
                });
                line_start = at.saturating_add(1);
                emitted = true;
                break;
            }

            if at > line_start {
                let previous_index = at.saturating_sub(1);
                if let Some(previous) = clusters.get(previous_index).copied() {
                    if line_break_pair(previous.last_char, cluster.first_char)
                        && !previous.whitespace
                        && !cluster.whitespace
                    {
                        candidate = Some(BreakCandidate {
                            next_cluster: at,
                            end_byte: cluster.start,
                            width,
                            append_hyphen: false,
                        });
                    }
                }
            }

            let before_width = width;
            let added = cluster_addition(last_visible, cluster, metrics);
            let next_width = width + added;

            if next_width > limit && at > line_start {
                let usable = candidate
                    .filter(|item| item.next_cluster > line_start && candidate_starts_legally(&clusters, *item));
                if let Some(item) = usable {
                    lines.push(Line {
                        start: first.start,
                        end: item.end_byte,
                        width: item.width,
                        hard_break: false,
                        append_hyphen: item.append_hyphen,
                    });
                    line_start = item.next_cluster;
                } else if is_kinsoku_start(cluster.first_char) {
                    width = next_width;
                    let next_cluster = at.saturating_add(1);
                    lines.push(Line {
                        start: first.start,
                        end: cluster.end,
                        width,
                        hard_break: false,
                        append_hyphen: false,
                    });
                    line_start = next_cluster;
                } else {
                    lines.push(Line {
                        start: first.start,
                        end: cluster.start,
                        width,
                        hard_break: false,
                        append_hyphen: false,
                    });
                    line_start = at;
                }
                emitted = true;
                break;
            }

            width = next_width;
            last_visible = cluster.last_visible.or(last_visible);

            if cluster.soft_hyphen {
                candidate = Some(BreakCandidate {
                    next_cluster: at.saturating_add(1),
                    end_byte: cluster.start,
                    width: before_width + metrics.advance('-'),
                    append_hyphen: true,
                });
            } else if cluster.whitespace || is_break_after(cluster.last_char) {
                candidate = Some(BreakCandidate {
                    next_cluster: at.saturating_add(1),
                    end_byte: cluster.end,
                    width,
                    append_hyphen: false,
                });
            }

            at = at.saturating_add(1);
        }

        if !emitted {
            let end = clusters.last().map_or(text.len(), |item| item.end);
            lines.push(Line {
                start: first.start,
                end,
                width,
                hard_break: false,
                append_hyphen: false,
            });
            break;
        }
    }

    if let Some(last) = clusters.last() {
        if last.hard_break {
            lines.push(Line {
                start: last.end,
                end: last.end,
                width: 0.0,
                hard_break: false,
                append_hyphen: false,
            });
        }
    }

    lines
}

/// Returns legal caret stops for a single logical line.
///
/// Combining marks and emoji joined with ZWJ do not create intermediate stops.
#[must_use]
pub fn caret_positions<M: Metrics>(text: &str, metrics: &M) -> Vec<Caret> {
    let mut carets = Vec::new();
    carets.push(Caret { byte: 0, x: 0.0 });
    let mut x = 0.0_f32;
    let mut previous = None;
    visit_grapheme_ranges(text, |start, end| {
        if let Some(slice) = text.get(start..end) {
            if let Some(cluster) = make_cluster(slice, start, end, metrics) {
                x += cluster_addition(previous, cluster, metrics);
                previous = cluster.last_visible.or(previous);
                carets.push(Caret { byte: end, x });
            }
        }
    });
    carets
}

/// Hit-tests a horizontal position and returns the nearest legal UTF-8 caret offset.
#[must_use]
pub fn hit_test<M: Metrics>(text: &str, metrics: &M, x: f32) -> usize {
    let carets = caret_positions(text, metrics);
    if carets.is_empty() {
        return 0;
    }
    if x <= 0.0 {
        return 0;
    }

    let mut previous = carets.first().copied().unwrap_or(Caret { byte: 0, x: 0.0 });
    for current in carets.iter().copied().skip(1) {
        let middle = previous.x + ((current.x - previous.x) * 0.5);
        if x < middle {
            return previous.byte;
        }
        previous = current;
    }
    previous.byte
}

/// Returns an end-ellipsised copy that does not exceed `max_width`.
#[must_use]
pub fn ellipsize_end<M: Metrics>(text: &str, max_width: f32, metrics: &M) -> String {
    if measure_text(text, metrics) <= max_width {
        return text.to_owned();
    }
    let ellipsis_width = metrics.advance(ELLIPSIS);
    if max_width < ellipsis_width || !max_width.is_finite() {
        return if max_width.is_infinite() && max_width.is_sign_positive() {
            text.to_owned()
        } else {
            String::new()
        };
    }

    let clusters = collect_clusters(text, metrics);
    let mut width = 0.0_f32;
    let mut previous = None;
    let mut end_byte = 0_usize;
    for cluster in clusters {
        let addition = cluster_addition(previous, cluster, metrics);
        let candidate_width = width
            + addition
            + boundary_kerning(cluster.last_visible.or(previous), Some(ELLIPSIS), metrics)
            + ellipsis_width;
        if candidate_width > max_width {
            break;
        }
        width += addition;
        previous = cluster.last_visible.or(previous);
        end_byte = cluster.end;
    }

    let mut output = String::with_capacity(end_byte.saturating_add(ELLIPSIS.len_utf8()));
    if let Some(prefix) = text.get(..end_byte) {
        output.push_str(prefix);
    }
    output.push(ELLIPSIS);
    output
}

/// Returns a middle-ellipsised copy, useful for paths, that does not exceed `max_width`.
#[must_use]
pub fn ellipsize_middle<M: Metrics>(text: &str, max_width: f32, metrics: &M) -> String {
    if measure_text(text, metrics) <= max_width {
        return text.to_owned();
    }
    let ellipsis_width = metrics.advance(ELLIPSIS);
    if max_width < ellipsis_width || !max_width.is_finite() {
        return if max_width.is_infinite() && max_width.is_sign_positive() {
            text.to_owned()
        } else {
            String::new()
        };
    }

    let clusters = collect_clusters(text, metrics);
    if clusters.is_empty() {
        return String::new();
    }

    let mut left_count = 0_usize;
    let mut right_start = clusters.len();
    let mut left_width = 0.0_f32;
    let mut right_width = 0.0_f32;
    let mut left_last = None;
    let mut right_first = None;
    let mut take_right = true;

    loop {
        if left_count >= right_start {
            break;
        }
        let try_right = take_right;
        let fit_primary = if try_right {
            try_add_right(
                &clusters,
                left_count,
                right_start,
                left_width,
                right_width,
                left_last,
                right_first,
                ellipsis_width,
                max_width,
                metrics,
            )
        } else {
            try_add_left(
                &clusters,
                left_count,
                right_start,
                left_width,
                right_width,
                left_last,
                right_first,
                ellipsis_width,
                max_width,
                metrics,
            )
        };

        if let Some(side) = fit_primary {
            match side {
                SideAddition::Left { width, last_visible } => {
                    left_width = width;
                    left_last = last_visible;
                    left_count = left_count.saturating_add(1);
                }
                SideAddition::Right { width, first_visible } => {
                    right_width = width;
                    right_first = first_visible;
                    right_start = right_start.saturating_sub(1);
                }
            }
            take_right = !take_right;
            continue;
        }

        let fit_secondary = if try_right {
            try_add_left(
                &clusters,
                left_count,
                right_start,
                left_width,
                right_width,
                left_last,
                right_first,
                ellipsis_width,
                max_width,
                metrics,
            )
        } else {
            try_add_right(
                &clusters,
                left_count,
                right_start,
                left_width,
                right_width,
                left_last,
                right_first,
                ellipsis_width,
                max_width,
                metrics,
            )
        };

        if let Some(side) = fit_secondary {
            match side {
                SideAddition::Left { width, last_visible } => {
                    left_width = width;
                    left_last = last_visible;
                    left_count = left_count.saturating_add(1);
                }
                SideAddition::Right { width, first_visible } => {
                    right_width = width;
                    right_first = first_visible;
                    right_start = right_start.saturating_sub(1);
                }
            }
            take_right = !take_right;
        } else {
            break;
        }
    }

    let prefix_end = if left_count == 0 {
        0
    } else {
        clusters.get(left_count.saturating_sub(1)).map_or(0, |item| item.end)
    };
    let suffix_start = clusters.get(right_start).map_or(text.len(), |item| item.start);
    let capacity = prefix_end
        .saturating_add(ELLIPSIS.len_utf8())
        .saturating_add(text.len().saturating_sub(suffix_start));
    let mut output = String::with_capacity(capacity);
    if let Some(prefix) = text.get(..prefix_end) {
        output.push_str(prefix);
    }
    output.push(ELLIPSIS);
    if let Some(suffix) = text.get(suffix_start..) {
        output.push_str(suffix);
    }
    output
}

/// Measures text with the same scalar-advance contract used by line breaking.
#[must_use]
pub fn measure_text<M: Metrics>(text: &str, metrics: &M) -> f32 {
    let mut width = 0.0_f32;
    let mut previous = None;
    visit_grapheme_ranges(text, |start, end| {
        if let Some(slice) = text.get(start..end) {
            if let Some(cluster) = make_cluster(slice, start, end, metrics) {
                width += cluster_addition(previous, cluster, metrics);
                previous = cluster.last_visible.or(previous);
            }
        }
    });
    width
}

/// Writes a folded representation suitable for indexing or diagnostics.
///
/// Search itself should normally use [`folded_contains`], which does not allocate a folded copy of
/// the haystack.
pub fn fold_into(text: &str, locale: SearchLocale, output: &mut String) {
    output.clear();
    output.extend(FoldIter::new(text, locale));
}

/// Case-, accent- and width-insensitive substring search.
///
/// The pattern and its KMP prefix table are the only temporary allocations; the haystack is folded
/// as a stream, so a multi-megabyte path/log string is never duplicated.
#[must_use]
pub fn folded_contains(haystack: &str, needle: &str, locale: SearchLocale) -> bool {
    let pattern: Vec<char> = FoldIter::new(needle, locale).collect();
    if pattern.is_empty() {
        return true;
    }
    let prefix = build_prefix_table(&pattern);
    let mut matched = 0_usize;

    for character in FoldIter::new(haystack, locale) {
        while matched > 0 {
            let same = pattern.get(matched).is_some_and(|value| *value == character);
            if same {
                break;
            }
            let previous = matched.saturating_sub(1);
            matched = prefix.get(previous).copied().unwrap_or(0);
        }
        if pattern.get(matched).is_some_and(|value| *value == character) {
            matched = matched.saturating_add(1);
            if matched == pattern.len() {
                return true;
            }
        }
    }
    false
}

#[derive(Clone, Copy, Debug)]
enum SideAddition {
    Left { width: f32, last_visible: Option<char> },
    Right { width: f32, first_visible: Option<char> },
}

#[allow(clippy::too_many_arguments)]
fn try_add_left<M: Metrics>(
    clusters: &[Cluster],
    left_count: usize,
    right_start: usize,
    left_width: f32,
    right_width: f32,
    left_last: Option<char>,
    right_first: Option<char>,
    ellipsis_width: f32,
    max_width: f32,
    metrics: &M,
) -> Option<SideAddition> {
    if left_count >= right_start {
        return None;
    }
    let cluster = clusters.get(left_count).copied()?;
    let new_left_width = left_width + cluster_addition(left_last, cluster, metrics);
    let new_left_last = cluster.last_visible.or(left_last);
    let total = joined_middle_width(
        new_left_width,
        right_width,
        new_left_last,
        right_first,
        ellipsis_width,
        metrics,
    );
    (total <= max_width).then_some(SideAddition::Left {
        width: new_left_width,
        last_visible: new_left_last,
    })
}

#[allow(clippy::too_many_arguments)]
fn try_add_right<M: Metrics>(
    clusters: &[Cluster],
    left_count: usize,
    right_start: usize,
    left_width: f32,
    right_width: f32,
    left_last: Option<char>,
    right_first: Option<char>,
    ellipsis_width: f32,
    max_width: f32,
    metrics: &M,
) -> Option<SideAddition> {
    if left_count >= right_start {
        return None;
    }
    let index = right_start.saturating_sub(1);
    let cluster = clusters.get(index).copied()?;
    let internal_kern = boundary_kerning(cluster.last_visible, right_first, metrics);
    let new_right_width = cluster.width + internal_kern + right_width;
    let new_right_first = cluster.first_visible.or(right_first);
    let total = joined_middle_width(
        left_width,
        new_right_width,
        left_last,
        new_right_first,
        ellipsis_width,
        metrics,
    );
    (total <= max_width).then_some(SideAddition::Right {
        width: new_right_width,
        first_visible: new_right_first,
    })
}

fn joined_middle_width<M: Metrics>(
    left_width: f32,
    right_width: f32,
    left_last: Option<char>,
    right_first: Option<char>,
    ellipsis_width: f32,
    metrics: &M,
) -> f32 {
    left_width
        + boundary_kerning(left_last, Some(ELLIPSIS), metrics)
        + ellipsis_width
        + boundary_kerning(Some(ELLIPSIS), right_first, metrics)
        + right_width
}

fn build_prefix_table(pattern: &[char]) -> Vec<usize> {
    let mut prefix = vec![0_usize; pattern.len()];
    let mut length = 0_usize;
    let mut at = 1_usize;
    while at < pattern.len() {
        let current = pattern.get(at).copied();
        let candidate = pattern.get(length).copied();
        if current.is_some() && current == candidate {
            length = length.saturating_add(1);
            if let Some(slot) = prefix.get_mut(at) {
                *slot = length;
            }
            at = at.saturating_add(1);
        } else if length > 0 {
            let previous = length.saturating_sub(1);
            length = prefix.get(previous).copied().unwrap_or(0);
        } else {
            at = at.saturating_add(1);
        }
    }
    prefix
}

struct FoldIter<'a> {
    chars: std::str::Chars<'a>,
    lowercase: Option<std::char::ToLowercase>,
    locale: SearchLocale,
}

impl<'a> FoldIter<'a> {
    fn new(text: &'a str, locale: SearchLocale) -> Self {
        Self {
            chars: text.chars(),
            lowercase: None,
            locale,
        }
    }
}

impl Iterator for FoldIter<'_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(lowercase) = self.lowercase.as_mut() {
                if let Some(character) = lowercase.next() {
                    if let Some(folded) = strip_accent(character, self.locale) {
                        return Some(folded);
                    }
                    continue;
                }
                self.lowercase = None;
            }

            let original = self.chars.next()?;
            let width_folded = fold_full_width(original);
            if self.locale == SearchLocale::Turkish {
                match width_folded {
                    'I' | 'ı' => return Some('ı'),
                    'İ' | 'i' => return Some('i'),
                    _ => {}
                }
            }
            self.lowercase = Some(width_folded.to_lowercase());
        }
    }
}

fn fold_full_width(character: char) -> char {
    let value = u32::from(character);
    if (0xff01..=0xff5e).contains(&value) {
        let Some(shifted) = value.checked_sub(0xfee0) else {
            return character;
        };
        return char::from_u32(shifted).unwrap_or(character);
    }
    if character == '\u{3000}' {
        return ' ';
    }
    character
}

#[allow(clippy::match_same_arms)]
fn strip_accent(character: char, locale: SearchLocale) -> Option<char> {
    if is_combining_mark(character) || is_variation_selector(character) {
        return None;
    }
    let mapped = match character {
        'ё' => 'е',
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' | 'ħ' => 'h',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        'ı' if locale == SearchLocale::General => 'i',
        other => other,
    };
    Some(mapped)
}

fn collect_clusters<M: Metrics>(text: &str, metrics: &M) -> Vec<Cluster> {
    let mut clusters = Vec::new();
    visit_grapheme_ranges(text, |start, end| {
        if let Some(slice) = text.get(start..end) {
            if let Some(cluster) = make_cluster(slice, start, end, metrics) {
                clusters.push(cluster);
            }
        }
    });
    clusters
}

fn make_cluster<M: Metrics>(slice: &str, start: usize, end: usize, metrics: &M) -> Option<Cluster> {
    let mut chars = slice.chars();
    let first_char = chars.next()?;
    let last_char = slice.chars().last().unwrap_or(first_char);
    let hard_break = slice.chars().all(is_hard_break_char);
    let soft_hyphen = slice == "\u{00ad}";
    let whitespace = !hard_break && slice.chars().all(char::is_whitespace);
    let cjk = slice.chars().any(is_cjk_character);
    let (width, first_visible, last_visible) = measure_cluster(slice, metrics);
    Some(Cluster {
        start,
        end,
        width,
        first_visible,
        last_visible,
        first_char,
        last_char,
        hard_break,
        soft_hyphen,
        whitespace,
        cjk,
    })
}

fn measure_cluster<M: Metrics>(slice: &str, metrics: &M) -> (f32, Option<char>, Option<char>) {
    let mut width = 0.0_f32;
    let mut first = None;
    let mut previous = None;
    for character in slice.chars() {
        if !is_visible_for_metrics(character) {
            continue;
        }
        if let Some(left) = previous {
            width += metrics.kerning(left, character);
        }
        width += metrics.advance(character);
        if first.is_none() {
            first = Some(character);
        }
        previous = Some(character);
    }
    (width, first, previous)
}

fn cluster_addition<M: Metrics>(previous: Option<char>, cluster: Cluster, metrics: &M) -> f32 {
    boundary_kerning(previous, cluster.first_visible, metrics) + cluster.width
}

fn boundary_kerning<M: Metrics>(left: Option<char>, right: Option<char>, metrics: &M) -> f32 {
    match (left, right) {
        (Some(a), Some(b)) => metrics.kerning(a, b),
        _ => 0.0,
    }
}

fn is_visible_for_metrics(character: char) -> bool {
    character != SOFT_HYPHEN
        && character != '\u{200d}'
        && !is_combining_mark(character)
        && !is_variation_selector(character)
        && !is_hard_break_char(character)
}

fn candidate_starts_legally(clusters: &[Cluster], candidate: BreakCandidate) -> bool {
    let mut at = candidate.next_cluster;
    while let Some(cluster) = clusters.get(at) {
        if cluster.hard_break {
            return true;
        }
        if cluster.whitespace {
            at = at.saturating_add(1);
            continue;
        }
        return !is_kinsoku_start(cluster.first_char);
    }
    true
}

fn is_break_after(character: char) -> bool {
    matches!(
        line_break_class(character),
        LineBreakClass::Ba | LineBreakClass::Hy | LineBreakClass::Hh | LineBreakClass::B2 | LineBreakClass::Sy
    )
}

fn is_kinsoku_start(character: char) -> bool {
    matches!(
        line_break_class(character),
        LineBreakClass::Cl
            | LineBreakClass::Cp
            | LineBreakClass::Ex
            | LineBreakClass::Is
            | LineBreakClass::Ns
            | LineBreakClass::Po
    )
}

fn is_kinsoku_end(character: char) -> bool {
    matches!(line_break_class(character), LineBreakClass::Op)
}

fn is_cjk_character(character: char) -> bool {
    matches!(
        line_break_class(character),
        LineBreakClass::Id
            | LineBreakClass::Cj
            | LineBreakClass::H2
            | LineBreakClass::H3
            | LineBreakClass::Jl
            | LineBreakClass::Jv
            | LineBreakClass::Jt
    )
}

fn visit_grapheme_ranges(text: &str, mut visit: impl FnMut(usize, usize)) {
    let boundaries = grapheme_boundaries(text);
    for pair in boundaries.windows(2) {
        if let (Some(start), Some(end)) = (pair.first(), pair.get(1)) {
            visit(*start, *end);
        }
    }
}

fn is_hard_break_char(character: char) -> bool {
    matches!(
        line_break_class(character),
        LineBreakClass::Bk | LineBreakClass::Cr | LineBreakClass::Lf | LineBreakClass::Nl
    )
}

fn is_combining_mark(character: char) -> bool {
    grapheme_class(character) == GraphemeClass::Extend
}

fn is_variation_selector(character: char) -> bool {
    matches!(u32::from(character), 0xfe00..=0xfe0f | 0xe0100..=0xe01ef)
}


#[cfg(test)]
mod tests {
    use super::*;

    struct Mono;

    impl Metrics for Mono {
        fn advance(&self, character: char) -> f32 {
            if is_combining_mark(character) || is_variation_selector(character) || character == '\u{200d}' {
                0.0
            } else if is_cjk_character(character) {
                2.0
            } else {
                1.0
            }
        }

        fn kerning(&self, left: char, right: char) -> f32 {
            if left == 'A' && right == 'V' {
                -0.25
            } else {
                0.0
            }
        }
    }

    #[test]
    fn folding_table_has_more_than_eighty_cases() {
        let cases = [
            ("АК-74", "ак-74"),
            ("МЁРТВ", "мертв"),
            ("СЕЙВ", "сейв"),
            ("Чернобыль", "чернобыль"),
            ("ПРИПЯТЬ", "припять"),
            ("ЁЖ", "еж"),
            ("ёлка", "елка"),
            ("ПОЛЁТ", "полет"),
            ("ТЁМНЫЙ", "темный"),
            ("КАЛИБР", "калибр"),
            ("Łódź", "lodz"),
            ("ŻÓŁĆ", "zolc"),
            ("Ścieżka", "sciezka"),
            ("Ćma", "cma"),
            ("Ń", "n"),
            ("Ą", "a"),
            ("Ę", "e"),
            ("Ź", "z"),
            ("RZĄD", "rzad"),
            ("PÓŁKA", "polka"),
            ("Český", "cesky"),
            ("Příliš", "prilis"),
            ("Žluťoučký", "zlutoucky"),
            ("kůň", "kun"),
            ("Řeka", "reka"),
            ("Ďábel", "dabel"),
            ("ťukat", "tukat"),
            ("město", "mesto"),
            ("ŠKOLA", "skola"),
            ("ČÁST", "cast"),
            ("İSTANBUL", "istanbul"),
            ("Çalışma", "calisma"),
            ("Şeker", "seker"),
            ("Ğ", "g"),
            ("Ölçü", "olcu"),
            ("Ürün", "urun"),
            ("I", "i"),
            ("İ", "i"),
            ("ı", "i"),
            ("i", "i"),
            ("ＡＫ－７４", "ak-74"),
            ("ＦＩＬＥ１２３", "file123"),
            ("１２３４５", "12345"),
            ("ａｂｃ", "abc"),
            ("Ｚ９", "z9"),
            ("ＡＢＣ", "abc"),
            ("Ｆｏｏ", "foo"),
            ("ＰＡＴＨ", "path"),
            ("９ｍｍ", "9mm"),
            ("５．５６", "5.56"),
            ("ÀÁÂÃÄÅ", "aaaaaa"),
            ("ÇĆČ", "ccc"),
            ("ÈÉÊËĚ", "eeeee"),
            ("ÌÍÎÏ", "iiii"),
            ("ÑŃŇ", "nnn"),
            ("ÒÓÔÕÖØ", "oooooo"),
            ("ŘŔ", "rr"),
            ("ŚŠŞ", "sss"),
            ("ŤŢ", "tt"),
            ("ÙÚÛÜŮ", "uuuuu"),
            ("ÝŸ", "yy"),
            ("ŹŻŽ", "zzz"),
            ("ĀĂĄ", "aaa"),
            ("ĒĔĖĘ", "eeee"),
            ("ĪĬĮ", "iii"),
            ("ŌŎŐ", "ooo"),
            ("ŪŬŰŲ", "uuuu"),
            ("Save", "save"),
            ("SAVE", "save"),
            ("Save-01", "save-01"),
            ("Stalker", "stalker"),
            ("S.T.A.L.K.E.R.", "s.t.a.l.k.e.r."),
            ("Cloud", "cloud"),
            ("BACKUP", "backup"),
            ("Doctor", "doctor"),
            ("Inventory", "inventory"),
            ("Compare", "compare"),
            ("Timeline", "timeline"),
            ("Settings", "settings"),
            ("Search", "search"),
            ("Update", "update"),
            ("Folder", "folder"),
            ("Warning", "warning"),
            ("Info", "info"),
            ("Україна", "україна"),
            ("ЇЖАК", "їжак"),
            ("ЄДНІСТЬ", "єдність"),
            ("ҐРУНТ", "ґрунт"),
            ("Привіт", "привіт"),
            ("ＳＡＶＥ", "save"),
            ("ＭＯＤ", "mod"),
            ("ＰＡＴＣＨ", "patch"),
            ("ＣＬＯＵＤ", "cloud"),
            ("ＢＡＣＫＵＰ", "backup"),
        ];
        assert!(cases.len() >= 80);
        for (source, expected) in cases {
            let mut folded = String::new();
            fold_into(source, SearchLocale::General, &mut folded);
            assert_eq!(folded, expected, "source={source}");
        }
    }

    #[test]
    fn turkish_i_pairs_are_locale_aware() {
        assert!(folded_contains("IĞDIR", "ığdır", SearchLocale::Turkish));
        assert!(folded_contains("İZMİR", "izmir", SearchLocale::Turkish));
        assert!(!folded_contains("I", "i", SearchLocale::Turkish));
        assert!(!folded_contains("İ", "ı", SearchLocale::Turkish));
    }

    #[test]
    fn folded_search_finds_reference_example() {
        assert!(folded_contains(
            "Автомат АК-74 сохранён",
            "ак-74",
            SearchLocale::General
        ));
        assert!(folded_contains("Ścieżka/ŻÓŁĆ", "sciezka/zolc", SearchLocale::General));
        assert!(folded_contains("ＦＩＬＥ－１２３", "file-123", SearchLocale::General));
        assert!(!folded_contains("АК-74", "АК-12", SearchLocale::General));
    }

    #[test]
    fn graphemes_keep_combining_and_emoji_sequences_together() {
        let mono = Mono;
        let combining = caret_positions("e\u{301}x", &mono);
        assert_eq!(combining.len(), 3);
        assert_eq!(combining.get(1).map(|item| item.byte), Some("e\u{301}".len()));

        let family = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
        let emoji = caret_positions(family, &mono);
        assert_eq!(emoji.len(), 2);
        assert_eq!(emoji.last().map(|item| item.byte), Some(family.len()));

        let flag = caret_positions("🇺🇦!", &mono);
        assert_eq!(flag.len(), 3);
    }

    #[test]
    fn hangul_jamo_form_one_cluster() {
        let mono = Mono;
        let text = "각";
        let carets = caret_positions(text, &mono);
        assert_eq!(carets.len(), 2);
        assert_eq!(carets.last().map(|item| item.byte), Some(text.len()));
    }

    #[test]
    fn hard_breaks_make_lines_and_crlf_is_one_break() {
        let mono = Mono;
        let lines = break_lines("abc\r\ndef", 50.0, &mono);
        assert_eq!(lines.len(), 2);
        assert!(lines.first().is_some_and(|line| line.hard_break));
        assert_eq!(lines.first().map(|line| line.end), Some(3));
    }

    #[test]
    fn trailing_hard_break_creates_empty_final_line() {
        let mono = Mono;
        let lines = break_lines("abc\n", 50.0, &mono);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines.last().map(|line| (line.start, line.end)), Some((4, 4)));
    }

    #[test]
    fn soft_hyphen_only_appears_when_break_is_used() {
        let mono = Mono;
        let text = "encyclo\u{00ad}pedia";
        assert_eq!(measure_text(text, &mono), 12.0);
        let lines = break_lines(text, 8.0, &mono);
        assert!(lines.first().is_some_and(|line| line.append_hyphen));
        assert!(lines.first().is_some_and(|line| line.width <= 8.0));
    }

    #[test]
    fn cjk_breaks_between_ideographs() {
        let mono = Mono;
        let lines = break_lines("保存ファイル一覧", 4.0, &mono);
        assert!(lines.len() >= 3);
        for line in &lines {
            assert!(line.width <= 6.0);
        }
    }

    #[test]
    fn kinsoku_does_not_start_line_with_closing_punctuation() {
        let mono = Mono;
        let text = "保存、保存。保存）保存";
        let lines = break_lines(text, 4.0, &mono);
        for line in lines {
            if let Some(slice) = text.get(line.start..line.end) {
                if let Some(first) = slice.chars().next() {
                    assert!(!is_kinsoku_start(first), "bad line start: {slice}");
                }
            }
        }
    }

    #[test]
    fn korean_breaks_at_words_not_inside_short_word() {
        let mono = Mono;
        let text = "저장 파일 열기";
        let lines = break_lines(text, 5.0, &mono);
        assert!(lines.len() >= 2);
        let first = lines.first().and_then(|line| text.get(line.start..line.end));
        assert!(first.is_some_and(|line| line.contains("저장")));
    }

    #[test]
    fn latin_and_cyrillic_break_on_spaces() {
        let mono = Mono;
        let latin = break_lines("alpha beta gamma", 7.0, &mono);
        let cyrillic = break_lines("альфа бета гамма", 7.0, &mono);
        assert!(latin.len() >= 3);
        assert!(cyrillic.len() >= 3);
    }

    #[test]
    fn end_ellipsis_respects_width() {
        let mono = Mono;
        let result = ellipsize_end("abcdefghij", 6.0, &mono);
        assert!(result.ends_with(ELLIPSIS));
        assert!(measure_text(&result, &mono) <= 6.0);
    }

    #[test]
    fn middle_ellipsis_preserves_both_ends() {
        let mono = Mono;
        let result = ellipsize_middle("/very/long/path/save.sav", 12.0, &mono);
        assert!(result.contains(ELLIPSIS));
        assert!(result.starts_with('/'));
        assert!(result.ends_with("sav"));
        assert!(measure_text(&result, &mono) <= 12.0);
    }

    #[test]
    fn ellipsis_returns_original_when_it_fits() {
        let mono = Mono;
        assert_eq!(ellipsize_end("save", 10.0, &mono), "save");
        assert_eq!(ellipsize_middle("save", 10.0, &mono), "save");
    }

    #[test]
    fn hit_test_never_splits_grapheme() {
        let mono = Mono;
        let text = "a👨\u{200d}👩b";
        let carets = caret_positions(text, &mono);
        let emoji_end = carets.get(2).map(|item| item.byte).unwrap_or(0);
        let hit = hit_test(text, &mono, 2.0);
        assert!(hit == 1 || hit == emoji_end);
    }

    #[test]
    fn kerning_is_counted() {
        let mono = Mono;
        assert!((measure_text("AV", &mono) - 1.75).abs() < f32::EPSILON);
    }

    #[test]
    fn empty_text_has_one_caret_and_one_empty_line() {
        let mono = Mono;
        assert_eq!(caret_positions("", &mono), vec![Caret { byte: 0, x: 0.0 }]);
        let lines = break_lines("", 10.0, &mono);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines.first().map(|line| (line.start, line.end)), Some((0, 0)));
    }

    #[test]
    fn tiny_width_does_not_split_utf8() {
        let mono = Mono;
        let text = "Ж中🙂";
        let lines = break_lines(text, 0.1, &mono);
        for line in lines {
            assert!(text.is_char_boundary(line.start));
            assert!(text.is_char_boundary(line.end));
        }
    }
}
