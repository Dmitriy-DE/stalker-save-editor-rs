//! Dependency-free regular expressions for diagnostic log scanning.
//!
//! Patterns compile to a Thompson NFA and execute with a Pike VM. The VM keeps at
//! most one highest-priority thread per instruction, so matching time is linear in
//! the input length for a fixed compiled expression and does not use backtracking.
//! [`Set`] shares one dense Aho-Corasick prefilter across its expressions and
//! [`SetScanner`] preserves partial log lines across arbitrary input chunks.

use sse_core::{Cursor, Error, Result};
use std::collections::VecDeque;

const MAX_PATTERN_CHARS: usize = 65_536;
const MAX_PROGRAM_STATES: usize = 65_536;
const MAX_CAPTURES: usize = 64;
const MAX_REPEAT: usize = 10_000;
const MAX_SET_PATTERNS: usize = 4_096;
const MAX_AC_NODES: usize = 65_536;
const ASCII_TRANSITIONS: usize = 128;
const MAX_STREAM_LINE_BYTES: usize = 1_048_576;

/// Options that affect compilation and matching.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RegexOptions {
    /// Match ASCII and Russian Cyrillic letters without case distinctions.
    pub case_insensitive: bool,
}

/// A byte range in the searched UTF-8 string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    /// Inclusive byte offset at which the match begins.
    pub start: usize,
    /// Exclusive byte offset at which the match ends.
    pub end: usize,
}

/// Captured byte ranges. Group zero is the complete match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captures {
    ranges: Vec<Option<Match>>,
}

impl Captures {
    /// Returns one captured group, or `None` when the group did not participate.
    #[must_use]
    pub fn get(&self, group: usize) -> Option<Match> {
        self.ranges.get(group).copied().flatten()
    }

    /// Number of capture groups including group zero.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    /// Whether there are no capture slots. A successful result always has group zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClassTerm {
    Range(char, char),
    Digit,
    Space,
    Word,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CharClass {
    negated: bool,
    terms: Vec<ClassTerm>,
}

impl CharClass {
    fn matches(&self, character: char, case_insensitive: bool) -> bool {
        let folded = if case_insensitive {
            fold_case(character)
        } else {
            character
        };
        let found = self.terms.iter().any(|term| match *term {
            ClassTerm::Range(start, end) => {
                let left = if case_insensitive { fold_case(start) } else { start };
                let right = if case_insensitive { fold_case(end) } else { end };
                let low = left.min(right);
                let high = left.max(right);
                (low..=high).contains(&folded)
            }
            ClassTerm::Digit => folded.is_ascii_digit(),
            ClassTerm::Space => folded.is_whitespace(),
            ClassTerm::Word => is_word(folded),
        });
        found != self.negated
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Matcher {
    Literal(char),
    Dot,
    Class(CharClass),
}

impl Matcher {
    fn matches(&self, character: char, case_insensitive: bool) -> bool {
        match self {
            Self::Literal(expected) => {
                if case_insensitive {
                    fold_case(*expected) == fold_case(character)
                } else {
                    *expected == character
                }
            }
            Self::Dot => character != '
' && character != '',
            Self::Class(class) => class.matches(character, case_insensitive),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Assertion {
    Start,
    End,
    WordBoundary,
    NotWordBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ast {
    Empty,
    Atom(Matcher),
    Assertion(Assertion),
    Concat(Vec<Self>),
    Alternation(Vec<Self>),
    Repeat {
        node: Box<Self>,
        min: usize,
