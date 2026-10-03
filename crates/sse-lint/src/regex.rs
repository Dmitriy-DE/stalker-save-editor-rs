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
        max: Option<usize>,
        greedy: bool,
    },
    Capture {
        group: usize,
        node: Box<Self>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatchArm {
    Next,
    First,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Patch {
    instruction: usize,
    arm: PatchArm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fragment {
    start: usize,
    outs: Vec<Patch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Instruction {
    Consume(Matcher, Option<usize>),
    Split(Option<usize>, Option<usize>),
    Jump(Option<usize>),
    Save(usize, Option<usize>),
    Assert(Assertion, Option<usize>),
    Match,
}

#[derive(Debug, Clone)]
struct Program {
    instructions: Vec<Instruction>,
    start: usize,
    capture_slots: usize,
    options: RegexOptions,
}

#[derive(Debug, Clone)]
struct Compiler {
    instructions: Vec<Instruction>,
}

impl Compiler {
    fn new() -> Self {
        Self {
            instructions: Vec::new(),
        }
    }

    fn emit(&mut self, instruction: Instruction) -> Result<usize> {
        if self.instructions.len() >= MAX_PROGRAM_STATES {
            return Err(Error::Refused("regex NFA exceeds the state limit".to_owned()));
        }
        let at = self.instructions.len();
        self.instructions.push(instruction);
        Ok(at)
    }

    fn patch(&mut self, patches: &[Patch], target: usize) -> Result<()> {
        for patch in patches {
            let instruction = self
                .instructions
                .get_mut(patch.instruction)
                .ok_or_else(|| Error::damaged("regex patch points outside the program"))?;
            let slot = match (instruction, patch.arm) {
                (Instruction::Consume(_, next), PatchArm::Next)
                | (Instruction::Jump(next), PatchArm::Next)
                | (Instruction::Save(_, next), PatchArm::Next)
                | (Instruction::Assert(_, next), PatchArm::Next) => next,
                (Instruction::Split(first, _), PatchArm::First) => first,
                (Instruction::Split(_, second), PatchArm::Second) => second,
                _ => return Err(Error::damaged("regex patch arm does not match instruction")),
            };
            *slot = Some(target);
        }
        Ok(())
    }

    fn compile(&mut self, ast: &Ast) -> Result<Fragment> {
        match ast {
            Ast::Empty => {
                let start = self.emit(Instruction::Jump(None))?;
                Ok(Fragment {
                    start,
                    outs: vec![Patch {
                        instruction: start,
                        arm: PatchArm::Next,
                    }],
                })
            }
            Ast::Atom(matcher) => {
                let start = self.emit(Instruction::Consume(matcher.clone(), None))?;
                Ok(Fragment {
                    start,
                    outs: vec![Patch {
                        instruction: start,
                        arm: PatchArm::Next,
                    }],
                })
            }
            Ast::Assertion(assertion) => {
                let start = self.emit(Instruction::Assert(*assertion, None))?;
                Ok(Fragment {
                    start,
                    outs: vec![Patch {
                        instruction: start,
                        arm: PatchArm::Next,
                    }],
                })
            }
            Ast::Concat(nodes) => self.compile_concat(nodes),
            Ast::Alternation(nodes) => self.compile_alternation(nodes),
            Ast::Repeat {
                node,
                min,
                max,
                greedy,
            } => self.compile_repeat(node, *min, *max, *greedy),
            Ast::Capture { group, node } => self.compile_capture(*group, node),
        }
    }

    fn compile_concat(&mut self, nodes: &[Ast]) -> Result<Fragment> {
        let Some(first) = nodes.first() else {
            return self.compile(&Ast::Empty);
        };
        let mut fragment = self.compile(first)?;
        for node in nodes.iter().skip(1) {
            let next = self.compile(node)?;
            self.patch(&fragment.outs, next.start)?;
            fragment = Fragment {
                start: fragment.start,
                outs: next.outs,
            };
        }
        Ok(fragment)
    }

    fn compile_alternation(&mut self, nodes: &[Ast]) -> Result<Fragment> {
        let Some(first) = nodes.first() else {
            return self.compile(&Ast::Empty);
        };
        let mut fragment = self.compile(first)?;
        for node in nodes.iter().skip(1) {
            let right = self.compile(node)?;
            let split = self.emit(Instruction::Split(Some(fragment.start), Some(right.start)))?;
            let mut outs = fragment.outs;
            outs.extend(right.outs);
            fragment = Fragment { start: split, outs };
        }
        Ok(fragment)
    }

    fn compile_capture(&mut self, group: usize, node: &Ast) -> Result<Fragment> {
        let start_slot = group
            .checked_mul(2)
            .ok_or_else(|| Error::Refused("regex capture slot overflow".to_owned()))?;
        let end_slot = start_slot
            .checked_add(1)
            .ok_or_else(|| Error::Refused("regex capture slot overflow".to_owned()))?;
        let inner = self.compile(node)?;
        let end = self.emit(Instruction::Save(end_slot, None))?;
        self.patch(&inner.outs, end)?;
        let start = self.emit(Instruction::Save(start_slot, Some(inner.start)))?;
        Ok(Fragment {
            start,
            outs: vec![Patch {
                instruction: end,
                arm: PatchArm::Next,
            }],
        })
    }

    fn compile_repeat(&mut self, node: &Ast, min: usize, max: Option<usize>, greedy: bool) -> Result<Fragment> {
        if min > MAX_REPEAT || max.is_some_and(|value| value > MAX_REPEAT) {
            return Err(Error::Refused("regex repeat exceeds the configured limit".to_owned()));
        }
        if max.is_some_and(|value| value < min) {
            return Err(Error::damaged("regex repeat maximum is smaller than minimum"));
        }

        let mut result: Option<Fragment> = None;
        for _ in 0..min {
            let part = self.compile(node)?;
            result = Some(self.concatenate_optional(result, part)?);
        }

        match max {
            Some(limit) => {
                let optional = limit.saturating_sub(min);
                for _ in 0..optional {
                    let part = self.compile(node)?;
                    let split = if greedy {
                        self.emit(Instruction::Split(Some(part.start), None))?
                    } else {
                        self.emit(Instruction::Split(None, Some(part.start)))?
                    };
                    let exit_arm = if greedy { PatchArm::Second } else { PatchArm::First };
                    let mut outs = part.outs;
                    outs.push(Patch {
                        instruction: split,
                        arm: exit_arm,
                    });
                    let optional_fragment = Fragment { start: split, outs };
                    result = Some(self.concatenate_optional(result, optional_fragment)?);
                }
            }
            None => {
                let part = self.compile(node)?;
                let split = if greedy {
                    self.emit(Instruction::Split(Some(part.start), None))?
                } else {
                    self.emit(Instruction::Split(None, Some(part.start)))?
                };
                self.patch(&part.outs, split)?;
                let exit_arm = if greedy { PatchArm::Second } else { PatchArm::First };
                let star = Fragment {
                    start: split,
                    outs: vec![Patch {
                        instruction: split,
                        arm: exit_arm,
                    }],
                };
                result = Some(self.concatenate_optional(result, star)?);
            }
        }

        match result {
            Some(fragment) => Ok(fragment),
            None => self.compile(&Ast::Empty),
        }
    }

    fn concatenate_optional(&mut self, left: Option<Fragment>, right: Fragment) -> Result<Fragment> {
        if let Some(left_fragment) = left {
            self.patch(&left_fragment.outs, right.start)?;
            Ok(Fragment {
                start: left_fragment.start,
                outs: right.outs,
            })
        } else {
            Ok(right)
        }
    }
}

#[derive(Debug, Clone)]
struct Parser {
    chars: Vec<char>,
    position: usize,
    capture_count: usize,
}

impl Parser {
    fn new(pattern: &str) -> Result<Self> {
        let chars: Vec<char> = pattern.chars().collect();
        if chars.len() > MAX_PATTERN_CHARS {
            return Err(Error::Refused("regex pattern exceeds the character limit".to_owned()));
        }
        Ok(Self {
            chars,
            position: 0,
            capture_count: 0,
        })
    }

    fn parse(mut self) -> Result<(Ast, usize)> {
        let ast = self.parse_alternation()?;
        if self.peek().is_some() {
            return Err(self.error("unexpected trailing regex token"));
        }
        Ok((ast, self.capture_count))
    }

    fn parse_alternation(&mut self) -> Result<Ast> {
        let mut alternatives = vec![self.parse_concat()?];
        while self.peek() == Some('|') {
            self.bump()?;
            alternatives.push(self.parse_concat()?);
        }
        if alternatives.len() == 1 {
            alternatives
                .pop()
                .ok_or_else(|| self.error("regex alternation unexpectedly empty"))
        } else {
            Ok(Ast::Alternation(alternatives))
        }
    }

    fn parse_concat(&mut self) -> Result<Ast> {
        let mut nodes = Vec::new();
        while let Some(character) = self.peek() {
            if character == ')' || character == '|' {
                break;
            }
            nodes.push(self.parse_repeated()?);
        }
        if nodes.is_empty() {
            Ok(Ast::Empty)
        } else if nodes.len() == 1 {
            nodes.pop().ok_or_else(|| self.error("regex concatenation unexpectedly empty"))
        } else {
            Ok(Ast::Concat(nodes))
        }
    }

    fn parse_repeated(&mut self) -> Result<Ast> {
        let mut node = self.parse_atom()?;
        loop {
            let Some(character) = self.peek() else {
                break;
            };
            let quantifier = match character {
                '*' => {
                    self.bump()?;
                    Some((0, None))
                }
                '+' => {
                    self.bump()?;
                    Some((1, None))
                }
                '?' => {
                    self.bump()?;
                    Some((0, Some(1)))
                }
                '{' => Some(self.parse_braced_repeat()?),
                _ => None,
            };
            let Some((min, max)) = quantifier else {
                break;
            };
            let greedy = if self.peek() == Some('?') {
                self.bump()?;
                false
            } else {
                true
            };
            node = Ast::Repeat {
                node: Box::new(node),
                min,
                max,
                greedy,
            };
        }
        Ok(node)
    }

    fn parse_braced_repeat(&mut self) -> Result<(usize, Option<usize>)> {
        self.expect('{')?;
        let min = self.parse_usize()?;
        let max = match self.peek() {
            Some('}') => Some(min),
            Some(',') => {
                self.bump()?;
                if self.peek() == Some('}') {
                    None
                } else {
                    Some(self.parse_usize()?)
                }
            }
            _ => return Err(self.error("malformed regex repeat")),
        };
        self.expect('}')?;
        if min > MAX_REPEAT || max.is_some_and(|value| value > MAX_REPEAT) {
            return Err(Error::Refused("regex repeat exceeds the configured limit".to_owned()));
        }
        if max.is_some_and(|value| value < min) {
            return Err(self.error("regex repeat maximum is smaller than minimum"));
        }
        Ok((min, max))
    }

    fn parse_usize(&mut self) -> Result<usize> {
        let mut value = 0_usize;
        let mut digits = 0_usize;
        while let Some(character) = self.peek() {
            let Some(digit) = character.to_digit(10) else {
                break;
            };
            value = value
                .checked_mul(10)
                .and_then(|current| current.checked_add(usize::try_from(digit).ok()?))
                .ok_or_else(|| self.error("regex repeat number overflow"))?;
            digits = digits
                .checked_add(1)
                .ok_or_else(|| self.error("regex repeat digit count overflow"))?;
            self.bump()?;
        }
        if digits == 0 {
            Err(self.error("regex repeat requires a number"))
        } else {
            Ok(value)
        }
    }

    fn parse_atom(&mut self) -> Result<Ast> {
        let Some(character) = self.peek() else {
            return Err(self.error("regex atom is missing"));
        };
        match character {
            '(' => self.parse_group(),
            '[' => self.parse_class().map(|class| Ast::Atom(Matcher::Class(class))),
            '.' => {
                self.bump()?;
                Ok(Ast::Atom(Matcher::Dot))
            }
            '^' => {
                self.bump()?;
                Ok(Ast::Assertion(Assertion::Start))
            }
            '$' => {
                self.bump()?;
                Ok(Ast::Assertion(Assertion::End))
            }
            '\\' => self.parse_escape(false),
            '*' | '+' | '?' | '{' | '}' | ')' | '|' => Err(self.error("regex quantifier or delimiter has no atom")),
            literal => {
                self.bump()?;
                Ok(Ast::Atom(Matcher::Literal(literal)))
            }
        }
    }

    fn parse_group(&mut self) -> Result<Ast> {
        self.expect('(')?;
        let capturing = if self.peek() == Some('?') {
            self.bump()?;
            if self.peek() == Some(':') {
                self.bump()?;
                false
            } else {
                return Err(self.error("only non-capturing (?:...) group syntax is supported after ?"));
            }
        } else {
            true
        };
        let group = if capturing {
            self.capture_count = self
                .capture_count
                .checked_add(1)
                .ok_or_else(|| self.error("regex capture count overflow"))?;
            if self.capture_count >= MAX_CAPTURES {
                return Err(Error::Refused("regex has too many capture groups".to_owned()));
            }
            Some(self.capture_count)
        } else {
            None
        };
        let node = self.parse_alternation()?;
        self.expect(')')?;
        if let Some(group_index) = group {
            Ok(Ast::Capture {
                group: group_index,
                node: Box::new(node),
            })
        } else {
            Ok(node)
        }
    }

    fn parse_escape(&mut self, in_class: bool) -> Result<Ast> {
        self.expect('\\')?;
        let escaped = self
            .peek()
            .ok_or_else(|| self.error("regex escape is truncated"))?;
        self.bump()?;
        let class = |term: ClassTerm, negated: bool| {
            Ast::Atom(Matcher::Class(CharClass {
                negated,
                terms: vec![term],
            }))
        };
        let ast = match escaped {
            'd' => class(ClassTerm::Digit, false),
            'D' => class(ClassTerm::Digit, true),
            's' => class(ClassTerm::Space, false),
            'S' => class(ClassTerm::Space, true),
            'w' => class(ClassTerm::Word, false),
            'W' => class(ClassTerm::Word, true),
            'b' if !in_class => Ast::Assertion(Assertion::WordBoundary),
            'B' if !in_class => Ast::Assertion(Assertion::NotWordBoundary),
            'n' => Ast::Atom(Matcher::Literal('\n')),
            'r' => Ast::Atom(Matcher::Literal('\r')),
            't' => Ast::Atom(Matcher::Literal('\t')),
            other => Ast::Atom(Matcher::Literal(other)),
        };
        Ok(ast)
    }

    fn parse_class(&mut self) -> Result<CharClass> {
        self.expect('[')?;
        let negated = if self.peek() == Some('^') {
            self.bump()?;
            true
        } else {
            false
        };
        let mut terms = Vec::new();
        let mut first = true;
        while let Some(character) = self.peek() {
            if character == ']' && !first {
                self.bump()?;
                if terms.is_empty() {
                    return Err(self.error("regex character class is empty"));
                }
                return Ok(CharClass { negated, terms });
            }
            first = false;
            let left = self.parse_class_term()?;
            if self.peek() == Some('-') {
                let after_dash = self
                    .position
                    .checked_add(1)
                    .ok_or_else(|| self.error("regex class cursor overflow"))?;
                if self.chars.get(after_dash).copied().is_some_and(|next| next != ']') {
                    self.bump()?;
                    let right = self.parse_class_term()?;
                    match (left, right) {
                        (ClassTerm::Range(start, start_end), ClassTerm::Range(end, end_end))
                            if start == start_end && end == end_end =>
                        {
                            terms.push(ClassTerm::Range(start, end));
                        }
                        _ => return Err(self.error("regex ranges require literal endpoints")),
                    }
                    continue;
                }
            }
            terms.push(left);
        }
        Err(self.error("unterminated regex character class"))
    }

    fn parse_class_term(&mut self) -> Result<ClassTerm> {
        let character = self
            .peek()
            .ok_or_else(|| self.error("regex character class is truncated"))?;
        if character != '\\' {
            self.bump()?;
            return Ok(ClassTerm::Range(character, character));
        }
        self.bump()?;
        let escaped = self
            .peek()
            .ok_or_else(|| self.error("regex class escape is truncated"))?;
        self.bump()?;
        match escaped {
            'd' => Ok(ClassTerm::Digit),
            's' => Ok(ClassTerm::Space),
            'w' => Ok(ClassTerm::Word),
            'n' => Ok(ClassTerm::Range('\n', '\n')),
            'r' => Ok(ClassTerm::Range('\r', '\r')),
            't' => Ok(ClassTerm::Range('\t', '\t')),
            other => Ok(ClassTerm::Range(other, other)),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn bump(&mut self) -> Result<()> {
        self.position = self
            .position
            .checked_add(1)
            .ok_or_else(|| self.error("regex parser position overflow"))?;
        Ok(())
    }

    fn expect(&mut self, expected: char) -> Result<()> {
        if self.peek() != Some(expected) {
            return Err(self.error("unexpected regex token"));
        }
        self.bump()
    }

    fn error(&self, message: &str) -> Error {
        Error::damaged(format!("{message} at pattern char {}", self.position))
    }
}

#[derive(Debug, Clone)]
struct Thread {
    pc: usize,
    slots: Vec<Option<usize>>,
}

/// A compiled regular expression.
#[derive(Debug, Clone)]
pub struct Regex {
    program: Program,
    required_literal: Option<String>,
}

impl Regex {
    /// Compiles a case-sensitive regular expression.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for malformed syntax and [`Error::Refused`] for configured resource limits.
    pub fn new(pattern: &str) -> Result<Self> {
        Self::with_options(pattern, RegexOptions::default())
    }

    /// Compiles a regular expression using explicit options.
    ///
    /// A leading `(?i)` is also accepted and enables the same case-insensitive mode.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for malformed syntax and [`Error::Refused`] for configured resource limits.
    pub fn with_options(pattern: &str, mut options: RegexOptions) -> Result<Self> {
        let body = if let Some(rest) = pattern.strip_prefix("(?i)") {
            options.case_insensitive = true;
            rest
        } else {
            pattern
        };
        let (ast, capture_count) = Parser::new(body)?.parse()?;
        let required_literal = required_literal(&ast, options.case_insensitive);
        let mut compiler = Compiler::new();
        let inner = compiler.compile(&ast)?;
        let end_save = compiler.emit(Instruction::Save(1, None))?;
        compiler.patch(&inner.outs, end_save)?;
        let matched = compiler.emit(Instruction::Match)?;
        compiler.patch(
            &[Patch {
                instruction: end_save,
                arm: PatchArm::Next,
            }],
            matched,
        )?;
        let start_save = compiler.emit(Instruction::Save(0, Some(inner.start)))?;
        let groups = capture_count
            .checked_add(1)
            .ok_or_else(|| Error::Refused("regex capture count overflow".to_owned()))?;
        let capture_slots = groups
            .checked_mul(2)
            .ok_or_else(|| Error::Refused("regex capture slot overflow".to_owned()))?;
        Ok(Self {
            program: Program {
                instructions: compiler.instructions,
                start: start_save,
                capture_slots,
                options,
            },
            required_literal,
        })
    }

    /// Returns whether the expression occurs anywhere in `text`.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.run(text).is_some()
    }

    /// Returns the leftmost first match using greedy/lazy branch priority.
    #[must_use]
    pub fn find(&self, text: &str) -> Option<Match> {
        self.run(text).and_then(|captures| captures.get(0))
    }

    /// Returns capture ranges for the leftmost first match.
    #[must_use]
    pub fn captures(&self, text: &str) -> Option<Captures> {
        self.run(text)
    }

    /// Longest literal conservatively known to occur in every match, when one exists.
    #[must_use]
    pub fn required_literal(&self) -> Option<&str> {
        self.required_literal.as_deref()
    }

    fn run(&self, text: &str) -> Option<Captures> {
        let state_count = self.program.instructions.len();
        let mut active = Vec::new();
        let mut best: Option<Vec<Option<usize>>> = None;
        let mut position = 0_usize;
        let mut previous: Option<char> = None;

        loop {
            let next_character = text.get(position..).and_then(|tail| tail.chars().next());
            if best.is_none() {
                let mut seen = vec![false; state_count];
                for thread in &active {
                    if let Some(slot) = seen.get_mut(thread.pc) {
                        *slot = true;
                    }
                }
                let mut slots = vec![None; self.program.capture_slots];
                if let Some(start) = slots.get_mut(0) {
                    *start = Some(position);
                }
                if self
                    .add_thread(
                        &mut active,
                        &mut seen,
                        self.program.start,
                        slots,
                        position,
                        previous,
                        next_character,
                        text.len(),
                    )
                    .is_err()
                {
                    return None;
                }
            }

            self.record_match(&mut active, &mut best);
            if active.is_empty() {
                return best.map(|slots| captures_from_slots(&slots));
            }
            let Some(character) = next_character else {
                return best.map(|slots| captures_from_slots(&slots));
            };
            let next_position = position.checked_add(character.len_utf8())?;
            let following = text.get(next_position..).and_then(|tail| tail.chars().next());
            let mut next_threads = Vec::new();
            let mut seen = vec![false; state_count];
            for thread in active {
                let instruction = self.program.instructions.get(thread.pc)?;
                if let Instruction::Consume(matcher, Some(next_pc)) = instruction {
                    if matcher.matches(character, self.program.options.case_insensitive)
                        && self
                            .add_thread(
                                &mut next_threads,
                                &mut seen,
                                *next_pc,
                                thread.slots,
                                next_position,
                                Some(character),
                                following,
                                text.len(),
                            )
                            .is_err()
                    {
                        return None;
                    }
                }
            }
            active = next_threads;
            previous = Some(character);
            position = next_position;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_thread(
        &self,
        out: &mut Vec<Thread>,
        seen: &mut [bool],
        start_pc: usize,
        slots: Vec<Option<usize>>,
        position: usize,
        previous: Option<char>,
        next: Option<char>,
        input_len: usize,
    ) -> Result<()> {
        let mut stack = vec![(start_pc, slots)];
        while let Some((pc, mut captures)) = stack.pop() {
            let Some(was_seen) = seen.get_mut(pc) else {
                return Err(Error::damaged("regex VM program counter outside program"));
            };
            if *was_seen {
                continue;
            }
            *was_seen = true;
            let instruction = self
                .program
                .instructions
                .get(pc)
                .ok_or_else(|| Error::damaged("regex VM instruction missing"))?;
            match instruction {
                Instruction::Consume(_, _) | Instruction::Match => out.push(Thread { pc, slots: captures }),
                Instruction::Jump(Some(target)) => stack.push((*target, captures)),
                Instruction::Split(first, second) => {
                    if let Some(second_pc) = second {
                        stack.push((*second_pc, captures.clone()));
                    }
                    if let Some(first_pc) = first {
                        stack.push((*first_pc, captures));
                    }
                }
                Instruction::Save(slot, Some(target)) => {
                    let capture = captures
                        .get_mut(*slot)
                        .ok_or_else(|| Error::damaged("regex capture slot outside vector"))?;
                    *capture = Some(position);
                    stack.push((*target, captures));
                }
                Instruction::Assert(assertion, Some(target)) => {
                    if assertion_holds(*assertion, position, previous, next, input_len) {
                        stack.push((*target, captures));
                    }
                }
                Instruction::Consume(_, None)
                | Instruction::Jump(None)
                | Instruction::Save(_, None)
                | Instruction::Assert(_, None) => {
                    return Err(Error::damaged("regex VM contains an unpatched instruction"));
                }
            }
        }
        Ok(())
    }

    fn record_match(&self, active: &mut Vec<Thread>, best: &mut Option<Vec<Option<usize>>>) {
        let match_at = active.iter().position(|thread| {
            self.program
                .instructions
                .get(thread.pc)
                .is_some_and(|instruction| matches!(instruction, Instruction::Match))
        });
        if let Some(index) = match_at {
            if let Some(thread) = active.get(index) {
                *best = Some(thread.slots.clone());
            }
            active.truncate(index);
        }
    }
}

fn captures_from_slots(slots: &[Option<usize>]) -> Captures {
    let mut ranges = Vec::new();
    let mut position = 0_usize;
    while position < slots.len() {
        let end_position = position.saturating_add(1);
        let range = slots
            .get(position)
            .copied()
