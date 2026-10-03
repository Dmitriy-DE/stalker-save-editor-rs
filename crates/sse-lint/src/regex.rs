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
            Self::Dot => character != '\n' && character != '\r',
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
            .flatten()
            .zip(slots.get(end_position).copied().flatten())
            .map(|(start, end)| Match { start, end });
        ranges.push(range);
        position = position.saturating_add(2);
    }
    Captures { ranges }
}

fn assertion_holds(
    assertion: Assertion,
    position: usize,
    previous: Option<char>,
    next: Option<char>,
    input_len: usize,
) -> bool {
    match assertion {
        Assertion::Start => position == 0,
        Assertion::End => position == input_len,
        Assertion::WordBoundary => previous.is_some_and(is_word) != next.is_some_and(is_word),
        Assertion::NotWordBoundary => previous.is_some_and(is_word) == next.is_some_and(is_word),
    }
}

fn is_word(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric() || is_cyrillic_letter(character)
}

fn is_cyrillic_letter(character: char) -> bool {
    matches!(character, 'А'..='я' | 'Ё' | 'ё')
}

fn fold_case(character: char) -> char {
    if character.is_ascii_uppercase() {
        character.to_ascii_lowercase()
    } else {
        match character {
            'А' => 'а',
            'Б' => 'б',
            'В' => 'в',
            'Г' => 'г',
            'Д' => 'д',
            'Е' => 'е',
            'Ё' => 'ё',
            'Ж' => 'ж',
            'З' => 'з',
            'И' => 'и',
            'Й' => 'й',
            'К' => 'к',
            'Л' => 'л',
            'М' => 'м',
            'Н' => 'н',
            'О' => 'о',
            'П' => 'п',
            'Р' => 'р',
            'С' => 'с',
            'Т' => 'т',
            'У' => 'у',
            'Ф' => 'ф',
            'Х' => 'х',
            'Ц' => 'ц',
            'Ч' => 'ч',
            'Ш' => 'ш',
            'Щ' => 'щ',
            'Ъ' => 'ъ',
            'Ы' => 'ы',
            'Ь' => 'ь',
            'Э' => 'э',
            'Ю' => 'ю',
            'Я' => 'я',
            _ => character,
        }
    }
}

fn required_literal(ast: &Ast, case_insensitive: bool) -> Option<String> {
    let mut value = required_literal_inner(ast)?;
    if case_insensitive {
        value = value.chars().map(fold_case).collect();
    }
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn required_literal_inner(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(Matcher::Literal(character)) => Some(character.to_string()),
        Ast::Capture { node, .. } => required_literal_inner(node),
        Ast::Repeat { node, min, .. } if *min > 0 => required_literal_inner(node),
        Ast::Concat(nodes) => {
            let mut best: Option<String> = None;
            let mut run = String::new();
            for node in nodes {
                if let Some(exact) = exact_literal(node) {
                    run.push_str(&exact);
                } else {
                    choose_longer(&mut best, std::mem::take(&mut run));
                    if let Some(candidate) = required_literal_inner(node) {
                        choose_longer(&mut best, candidate);
                    }
                }
            }
            choose_longer(&mut best, run);
            best
        }
        Ast::Alternation(nodes) => {
            let first = nodes.first().and_then(required_literal_inner)?;
            if nodes
                .iter()
                .skip(1)
                .all(|node| required_literal_inner(node).as_deref() == Some(first.as_str()))
            {
                Some(first)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn exact_literal(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Empty => Some(String::new()),
        Ast::Atom(Matcher::Literal(character)) => Some(character.to_string()),
        Ast::Capture { node, .. } => exact_literal(node),
        Ast::Concat(nodes) => {
            let mut result = String::new();
            for node in nodes {
                result.push_str(&exact_literal(node)?);
            }
            Some(result)
        }
        Ast::Repeat {
            node,
            min,
            max: Some(max),
            ..
        } if min == max => {
            let literal = exact_literal(node)?;
            let mut result = String::new();
            for _ in 0..*min {
                result.push_str(&literal);
            }
            Some(result)
        }
        _ => None,
    }
}

fn choose_longer(best: &mut Option<String>, candidate: String) {
    if candidate.is_empty() {
        return;
    }
    if best.as_ref().is_none_or(|current| candidate.len() > current.len()) {
        *best = Some(candidate);
    }
}

#[derive(Debug, Clone)]
struct AcNode {
    transitions: [u32; ASCII_TRANSITIONS],
    fail: u32,
    outputs: Vec<usize>,
}

impl AcNode {
    fn new() -> Self {
        Self {
            transitions: [0; ASCII_TRANSITIONS],
            fail: 0,
            outputs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct AhoCorasick {
    nodes: Vec<AcNode>,
    case_insensitive: bool,
}

impl AhoCorasick {
    fn build(entries: &[(usize, String)], case_insensitive: bool) -> Result<Self> {
        let mut automaton = Self {
            nodes: vec![AcNode::new()],
            case_insensitive,
        };
        for (pattern_index, literal) in entries {
            automaton.insert(*pattern_index, literal)?;
        }
        automaton.complete_failures()?;
        Ok(automaton)
    }

    fn insert(&mut self, pattern_index: usize, literal: &str) -> Result<()> {
        let mut state = 0_usize;
        for byte in literal.as_bytes().iter().copied() {
            if !byte.is_ascii() {
                return Err(Error::damaged("Aho-Corasick literal is not ASCII"));
            }
            let folded = if self.case_insensitive {
                byte.to_ascii_lowercase()
            } else {
                byte
            };
            let column = usize::from(folded);
            let next_raw = self
                .nodes
                .get(state)
                .and_then(|node| node.transitions.get(column))
                .copied()
                .ok_or_else(|| Error::damaged("Aho-Corasick state outside table"))?;
            if next_raw == 0 {
                if self.nodes.len() >= MAX_AC_NODES {
                    return Err(Error::Refused("Aho-Corasick prefilter exceeds node limit".to_owned()));
                }
                let next = self.nodes.len();
                let next_u32 = u32::try_from(next)
                    .map_err(|_| Error::Refused("Aho-Corasick node id exceeds u32".to_owned()))?;
                self.nodes.push(AcNode::new());
                let transition = self
                    .nodes
                    .get_mut(state)
                    .and_then(|node| node.transitions.get_mut(column))
                    .ok_or_else(|| Error::damaged("Aho-Corasick transition outside table"))?;
                *transition = next_u32;
                state = next;
            } else {
                state = usize::try_from(next_raw)
                    .map_err(|_| Error::damaged("Aho-Corasick node id does not fit usize"))?;
            }
        }
        let outputs = self
            .nodes
            .get_mut(state)
            .ok_or_else(|| Error::damaged("Aho-Corasick terminal state missing"))?;
        outputs.outputs.push(pattern_index);
        Ok(())
    }

    fn complete_failures(&mut self) -> Result<()> {
        let mut queue = VecDeque::new();
        for column in 0..ASCII_TRANSITIONS {
            let child = self
                .nodes
                .first()
                .and_then(|node| node.transitions.get(column))
                .copied()
                .ok_or_else(|| Error::damaged("Aho-Corasick root transition missing"))?;
            if child != 0 {
                queue.push_back(usize::try_from(child).map_err(|_| Error::damaged("Aho node id conversion failed"))?);
            }
        }

        while let Some(state) = queue.pop_front() {
            let failure = self
                .nodes
                .get(state)
                .map(|node| node.fail)
                .ok_or_else(|| Error::damaged("Aho-Corasick failure state missing"))?;
            let failure_index = usize::try_from(failure).map_err(|_| Error::damaged("Aho failure id conversion failed"))?;
            for column in 0..ASCII_TRANSITIONS {
                let child = self
                    .nodes
                    .get(state)
                    .and_then(|node| node.transitions.get(column))
                    .copied()
                    .ok_or_else(|| Error::damaged("Aho-Corasick transition missing"))?;
                if child == 0 {
                    let inherited = self
                        .nodes
                        .get(failure_index)
                        .and_then(|node| node.transitions.get(column))
                        .copied()
                        .ok_or_else(|| Error::damaged("Aho-Corasick inherited transition missing"))?;
                    let slot = self
                        .nodes
                        .get_mut(state)
                        .and_then(|node| node.transitions.get_mut(column))
                        .ok_or_else(|| Error::damaged("Aho-Corasick transition slot missing"))?;
                    *slot = inherited;
                    continue;
                }

                let child_index = usize::try_from(child).map_err(|_| Error::damaged("Aho child id conversion failed"))?;
                let fallback = self
                    .nodes
                    .get(failure_index)
                    .and_then(|node| node.transitions.get(column))
                    .copied()
                    .ok_or_else(|| Error::damaged("Aho fallback transition missing"))?;
                if let Some(node) = self.nodes.get_mut(child_index) {
                    node.fail = fallback;
                } else {
                    return Err(Error::damaged("Aho child node missing"));
                }
                let fallback_index = usize::try_from(fallback)
                    .map_err(|_| Error::damaged("Aho fallback id conversion failed"))?;
                let inherited_outputs = self
                    .nodes
                    .get(fallback_index)
                    .map(|node| node.outputs.clone())
                    .ok_or_else(|| Error::damaged("Aho fallback outputs missing"))?;
                let child_node = self
                    .nodes
                    .get_mut(child_index)
                    .ok_or_else(|| Error::damaged("Aho child outputs missing"))?;
                child_node.outputs.extend(inherited_outputs);
                queue.push_back(child_index);
            }
        }
        Ok(())
    }

    fn mark_candidates(&self, input: &[u8], flags: &mut [bool]) -> Result<()> {
        let mut cursor = Cursor::new(input);
        let mut state = 0_usize;
        while cursor.remaining() > 0 {
            let mut byte = cursor.u8()?;
            if !byte.is_ascii() {
                state = 0;
                continue;
            }
            if self.case_insensitive {
                byte = byte.to_ascii_lowercase();
            }
            let column = usize::from(byte);
            let next = self
                .nodes
                .get(state)
                .and_then(|node| node.transitions.get(column))
                .copied()
                .ok_or_else(|| Error::damaged("Aho-Corasick scan transition missing"))?;
            state = usize::try_from(next).map_err(|_| Error::damaged("Aho scan state conversion failed"))?;
            let outputs = self
                .nodes
                .get(state)
                .map(|node| node.outputs.as_slice())
                .ok_or_else(|| Error::damaged("Aho scan state missing"))?;
            for pattern_index in outputs {
                let flag = flags
                    .get_mut(*pattern_index)
                    .ok_or_else(|| Error::damaged("Aho output pattern index outside set"))?;
                *flag = true;
            }
        }
        Ok(())
    }
}

/// A group of expressions with one shared Aho-Corasick literal prefilter.
#[derive(Debug, Clone)]
pub struct Set {
    regexes: Vec<Regex>,
    prefilter: AhoCorasick,
    unfiltered: Vec<usize>,
}

impl Set {
    /// Compiles all patterns with the same options and builds one prefilter.
    ///
    /// # Errors
    /// Returns compilation errors or a configured pattern/prefilter limit.
    pub fn new(patterns: &[&str], options: RegexOptions) -> Result<Self> {
        if patterns.len() > MAX_SET_PATTERNS {
            return Err(Error::Refused("regex set exceeds pattern limit".to_owned()));
        }
        let mut regexes = Vec::with_capacity(patterns.len());
        let mut literals = Vec::new();
        let mut unfiltered = Vec::new();
        for pattern in patterns {
            let regex = Regex::with_options(pattern, options)?;
            let index = regexes.len();
            if let Some(literal) = regex.required_literal().filter(|value| value.is_ascii() && !value.is_empty()) {
                literals.push((index, literal.to_owned()));
            } else {
                unfiltered.push(index);
            }
            regexes.push(regex);
        }
        let prefilter = AhoCorasick::build(&literals, options.case_insensitive)?;
        Ok(Self {
            regexes,
            prefilter,
            unfiltered,
        })
    }

    /// Returns indices of every pattern matching `text`, in input pattern order.
    ///
    /// # Errors
    /// Returns a damaged-input error only if the internally built prefilter is inconsistent.
    pub fn matches(&self, text: &str) -> Result<Vec<usize>> {
        let mut candidates = vec![false; self.regexes.len()];
        for index in &self.unfiltered {
            if let Some(flag) = candidates.get_mut(*index) {
                *flag = true;
            }
        }
        self.prefilter.mark_candidates(text.as_bytes(), &mut candidates)?;
        let mut result = Vec::new();
        for (index, regex) in self.regexes.iter().enumerate() {
            if candidates.get(index).copied().unwrap_or(false) && regex.is_match(text) {
                result.push(index);
            }
        }
        Ok(result)
    }

    /// Returns whether at least one pattern matches `text`.
    ///
    /// # Errors
    /// Returns a damaged-input error only if the internally built prefilter is inconsistent.
    pub fn is_match(&self, text: &str) -> Result<bool> {
        Ok(!self.matches(text)?.is_empty())
    }

    /// Creates a bounded-memory chunk scanner for line-oriented logs.
    #[must_use]
    pub fn scanner(&self) -> SetScanner<'_> {
        SetScanner {
            set: self,
            pending: Vec::new(),
            line_number: 1,
        }
    }
}

/// One pattern hit produced by [`SetScanner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamMatch {
    /// Zero-based pattern index in the [`Set`].
    pub pattern_index: usize,
    /// One-based log line number.
    pub line_number: u64,
}

/// Incremental, bounded-memory scanner for UTF-8 log lines.
#[derive(Debug)]
pub struct SetScanner<'a> {
    set: &'a Set,
    pending: Vec<u8>,
    line_number: u64,
}

impl SetScanner<'_> {
    /// Scans one arbitrary byte chunk. A UTF-8 code point or matching line may span chunks.
    ///
    /// Completed hits are appended to `out`. The scanner keeps at most one unfinished log line.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] for a line above 1 MiB and [`Error::Damaged`] for invalid UTF-8.
    pub fn push(&mut self, chunk: &[u8], out: &mut Vec<StreamMatch>) -> Result<()> {
        for byte in chunk.iter().copied() {
            if byte == b'\n' {
                self.scan_pending(out)?;
                self.pending.clear();
                self.line_number = self
                    .line_number
                    .checked_add(1)
                    .ok_or_else(|| Error::Refused("log line number overflow".to_owned()))?;
            } else {
                if self.pending.len() >= MAX_STREAM_LINE_BYTES {
                    return Err(Error::Refused("log line exceeds 1 MiB streaming limit".to_owned()));
                }
                self.pending.push(byte);
            }
        }
        Ok(())
    }

    /// Scans the final unterminated line, if any.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for invalid UTF-8.
    pub fn finish(&mut self, out: &mut Vec<StreamMatch>) -> Result<()> {
        if !self.pending.is_empty() {
            self.scan_pending(out)?;
            self.pending.clear();
        }
        Ok(())
    }

    fn scan_pending(&self, out: &mut Vec<StreamMatch>) -> Result<()> {
        let line = if self.pending.last().copied() == Some(b'\r') {
            self.pending
                .get(..self.pending.len().saturating_sub(1))
                .ok_or_else(|| Error::damaged("log CR trim range invalid"))?
        } else {
            self.pending.as_slice()
        };
        let text = std::str::from_utf8(line).map_err(|_| Error::damaged("log stream is not UTF-8"))?;
        for pattern_index in self.set.matches(text)? {
            out.push(StreamMatch {
                pattern_index,
                line_number: self.line_number,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Captures, Match, Regex, RegexOptions, Set, StreamMatch};
    use std::time::Instant;

    const CATALOGUE: [(&str, &str); 47] = [
        (r"wrong target for storyline quest:\s*logic@work5,\s*gar_smart_terrain_6_3", "wrong target for storyline quest: logic@work5, gar_smart_terrain_6_3"),
        (r"Insufficient smart_terrain jobs", "Insufficient smart_terrain jobs"),
        (r"cant find animation for slot", "cant find animation for slot"),
        (r"sim_squad_generic\.script:\d+:\s*attempt to index field '\?' \(a nil value\)", "sim_squad_generic.script:123: attempt to index field '?' (a nil value)"),
        (r"pstor_load_all: not registered type N \d+ encountered", "pstor_load_all: not registered type N 123 encountered"),
        (r"sim_combat\.script:\d+:\s*attempt to index field 'actor' \(a nil value\)", "sim_combat.script:123: attempt to index field 'actor' (a nil value)"),
        (r"sim_combat\.script:\d+:\s*attempt to index local 'attack_squad_obj'", "sim_combat.script:123: attempt to index local 'attack_squad_obj'"),
        (r"sim_squad_generic\.script:\d+:\s*attempt to index field 'current_action'", "sim_squad_generic.script:123: attempt to index field 'current_action'"),
        (r"sim_squad_generic\.script:\d+:\s*attempt to index local 'task' \(a nil value\)", "sim_squad_generic.script:123: attempt to index local 'task' (a nil value)"),
        (r"se_monster\.script:\d+:\s*attempt to index local 'squad' \(a nil value\)", "se_monster.script:123: attempt to index local 'squad' (a nil value)"),
        (r"heli_combat\.script:\d+:\s*attempt to perform arithmetic on field 'change_(?:dir|pos)_time'", "heli_combat.script:123: attempt to perform arithmetic on field 'change_dir_time'"),
        (r"sr_bloodsucker\.script:\d+:\s*attempt to index field 'npc_squad'", "sr_bloodsucker.script:123: attempt to index field 'npc_squad'"),
        (r"patrol path \[agr_stalker_leader_walk\] is inaccessible", "patrol path [agr_stalker_leader_walk] is inaccessible"),
        (r"esc_smart_terrain_3_7_walker_1_walk", "esc_smart_terrain_3_7_walker_1_walk"),
        (r"wrong target for storyline quest", "wrong target for storyline quest"),
        (r"Insufficient smart_terrain jobs mil_smart_terrain_2_1", "Insufficient smart_terrain jobs mil_smart_terrain_2_1"),
        (r"Path between \[mil_smart_terrain_7_11\] and \[mil_smart_terrain_7_10\] doesnt exist", "Path between [mil_smart_terrain_7_11] and [mil_smart_terrain_7_10] doesnt exist"),
        (r"Can't find model file 'dynamics\\equipments\\item_rukzak\.ogf'", "Can't find model file 'dynamics\\equipments\\item_rukzak.ogf'"),
        (r"xr_kamp\.script:\d+:\s*bad argument #1 to 'random' \(interval is empty\)", "xr_kamp.script:123: bad argument #1 to 'random' (interval is empty)"),
        (r"sr_robbery\.script:\d+:\s*attempt to index field '\?' \(a nil value\)", "sr_robbery.script:123: attempt to index field '?' (a nil value)"),
        (r"actor_reaction\.script:\d+:\s*attempt to index local 'manager'", "actor_reaction.script:123: attempt to index local 'manager'"),
        (r"task_objects\.script:\d+:\s*attempt to index field '\?' \(a nil value\)", "task_objects.script:123: attempt to index field '?' (a nil value)"),
        (r"bind_anomaly_zone\.script:\d+:\s*attempt to index local 'art'", "bind_anomaly_zone.script:123: attempt to index local 'art'"),
        (r"You are saving too much", "You are saving too much"),
        (r"patrol path\s*\[esc_smart_terrain_3_7_walker_1_walk\]", "patrol path [esc_smart_terrain_3_7_walker_1_walk]"),
        (r"patrol path\s*\[red_smart_terrain_3_2_patrol_1_walk\] is inaccessible", "patrol path [red_smart_terrain_3_2_patrol_1_walk] is inaccessible"),
        (r"patrol path\s*\[agr_stalker_leader_walk\] is inaccessible", "patrol path [agr_stalker_leader_walk] is inaccessible"),
        (r"Can't find model file 'dynamics\\equipments\\item_rukzak\.ogf'", "Can't find model file 'dynamics\\equipments\\item_rukzak.ogf'"),
        (r"Unable to give treasure \[gar_treasure_quest_smuggler_weapons\]", "Unable to give treasure [gar_treasure_quest_smuggler_weapons]"),
        (r"There is no squad \[red_pursuit_bounty_hunters_squad_\d+\] in sim_board", "There is no squad [red_pursuit_bounty_hunters_squad_123] in sim_board"),
        (r"Path between \[mil_smart_terrain_7_11\] and \[mil_smart_terrain_7_10\] doesnt exist", "Path between [mil_smart_terrain_7_11] and [mil_smart_terrain_7_10] doesnt exist"),
        (r"xr_gulag\.script:\d+:\s*attempt to index local 'job' \(a nil value\)", "xr_gulag.script:123: attempt to index local 'job' (a nil value)"),
        (r"heli_combat\.script:\d+:\s*attempt to perform arithmetic on field 'change_(?:dir|pos)_time'", "heli_combat.script:123: attempt to perform arithmetic on field 'change_dir_time'"),
        (r"xr_kamp\.script:\d+:\s*attempt to index field '\?' \(a nil value\)|get dest Vertex: nil", "xr_kamp.script:123: attempt to index field '?' (a nil value)"),
        (r"xr_danger\.script:\d+:\s*attempt to index field 'ignore_types' \(a nil value\)", "xr_danger.script:123: attempt to index field 'ignore_types' (a nil value)"),
        (r"xr_effects\.script:\d+:\s*attempt to index local 'bandit1' \(a nil value\)", "xr_effects.script:123: attempt to index local 'bandit1' (a nil value)"),
        (r"dBodyStateValide\(b\)", "dBodyStateValide(b)"),
        (r"entity not found\.\s*id_parent=\d+\s*id_entity=\d+", "entity not found. id_parent=123 id_entity=123"),
        (r"(?:SMapLocation|CMapLocation::UpdateSpot) binded to non-existent object", "SMapLocation binded to non-existent object"),
        (r"there is no specified level in the game graph|There is no proper graph point neighbour", "there is no specified level in the game graph"),
        (r"cannot find rank for", "cannot find rank for"),
        (r"bad argument #2 to 'format' \(string expected, got no value\)", "bad argument #2 to 'format' (string expected, got no value)"),
        (r"Can't find model file '", "Can't find model file '"),
        (r"Can't open section '", "Can't open section '"),
        (r"Can't find variable \S+ in \[", "Can't find variable token in ["),
        (r"string table xml file not found", "string table xml file not found"),
        (r"Expression\s*:\s*hFile>0", "Expression : hFile>0"),
    ];

    fn ignore_case() -> RegexOptions {
        RegexOptions {
            case_insensitive: true,
        }
    }

    #[test]
    fn every_catalogue_pattern_has_positive_and_negative_case() {
        for (pattern, positive) in CATALOGUE {
            let regex = Regex::with_options(pattern, ignore_case());
            assert!(regex.is_ok(), "compile {pattern}: {regex:?}");
            let regex = regex.unwrap_or_else(|_| unreachable!());
            assert!(regex.is_match(positive), "positive did not match: {pattern} / {positive}");
            assert!(!regex.is_match("unrelated diagnostic line that cannot be a catalogue crash"), "negative matched: {pattern}");
        }
    }

    #[test]
    fn captures_quantifiers_classes_boundaries_and_anchors_work() {
        let regex = Regex::new(r"^(foo|bar)\b\s+([A-Z]{2,3}?)-\d{2,4}$").unwrap_or_else(|_| unreachable!());
        let captures = regex.captures("foo ABC-123").unwrap_or_else(|| unreachable!());
        assert_eq!(captures.get(0), Some(Match { start: 0, end: 11 }));
        assert_eq!(captures.get(1), Some(Match { start: 0, end: 3 }));
        assert_eq!(captures.get(2), Some(Match { start: 4, end: 7 }));
        assert_eq!(captures.len(), 3);
        assert!(!captures.is_empty());
    }

    #[test]
    fn case_insensitive_ascii_and_cyrillic_are_explicitly_supported() {
        let regex = Regex::with_options("ПРИВЕТ-[a-z]+", ignore_case()).unwrap_or_else(|_| unreachable!());
        assert!(regex.is_match("prefix привет-AbCd suffix"));
        let inline = Regex::new("(?i)ЁЖИК").unwrap_or_else(|_| unreachable!());
        assert!(inline.is_match("ёжик"));
    }

    #[test]
    fn classic_backtracking_pathologies_use_bounded_nfa_states() {
        let nested = Regex::new("(a+)+b").unwrap_or_else(|_| unreachable!());
        let alternative = Regex::new("(a|aa)*b").unwrap_or_else(|_| unreachable!());
        let text = "a".repeat(200_000);
        assert!(!nested.is_match(&text));
        assert!(!alternative.is_match(&text));
    }

    #[test]
    fn set_prefilter_and_chunk_scanner_preserve_cross_chunk_lines() {
        let patterns: Vec<&str> = CATALOGUE.iter().map(|entry| entry.0).collect();
        let set = Set::new(&patterns, ignore_case()).unwrap_or_else(|_| unreachable!());
        let direct = set.matches("noise HELI_COMBAT.script:77: attempt to perform arithmetic on field 'change_pos_time' tail");
        assert!(direct.as_ref().is_ok_and(|matches| matches.contains(&10) && matches.contains(&32)));

        let mut scanner = set.scanner();
        let mut hits = Vec::new();
        assert_eq!(scanner.push(b"noise\nCan't find varia", &mut hits), Ok(()));
        assert_eq!(scanner.push(b"ble bad_name in [section]\r", &mut hits), Ok(()));
        assert_eq!(scanner.push(b"\nmore noise", &mut hits), Ok(()));
        assert_eq!(scanner.finish(&mut hits), Ok(()));
        assert!(hits.contains(&StreamMatch {
            pattern_index: 44,
            line_number: 2,
        }));
    }

    #[test]
    fn prefiltered_catalogue_scan_is_linear_and_reports_throughput() {
        let patterns: Vec<&str> = CATALOGUE.iter().map(|entry| entry.0).collect();
        let set = Set::new(&patterns, ignore_case()).unwrap_or_else(|_| unreachable!());
        let block = b"ordinary engine log line with no crash signature\n";
        let target_bytes = 8_usize.saturating_mul(1_048_576);
        let mut data = Vec::with_capacity(target_bytes);
        while data.len() < target_bytes {
            data.extend_from_slice(block);
        }
        let text = std::str::from_utf8(&data).unwrap_or_else(|_| unreachable!());
        let started = Instant::now();
        let matches = set.matches(text).unwrap_or_else(|_| unreachable!());
        let elapsed = started.elapsed();
        assert!(matches.is_empty());
        let seconds = elapsed.as_secs_f64().max(f64::EPSILON);
        let mib = (data.len() as f64) / 1_048_576.0;
        eprintln!("X29 prefiltered scan: {:.1} MiB/s", mib / seconds);
    }
}
