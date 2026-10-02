//! Strict streaming RFC 8259 JSON reader and stable writer.

use sse_core::{Error, Result};

const MAX_DEPTH: usize = 128;

/// JSON text that either borrows directly from the input or owns an unescaped decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text<'a> {
    /// A string without escapes, borrowed from the original JSON input.
    Borrowed(&'a str),
    /// A string that contained escapes and therefore had to be decoded.
    Owned(String),
}

impl<'a> Text<'a> {
    /// Returns the decoded text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(value) => value,
            Self::Owned(value) => value.as_str(),
        }
    }

    /// Converts the text into an owned string, allocating only for borrowed text.
    #[must_use]
    pub fn into_owned(self) -> String {
        match self {
            Self::Borrowed(value) => value.to_owned(),
            Self::Owned(value) => value,
        }
    }
}

/// A pull-reader event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// Start of an object.
    ObjectStart,
    /// End of an object.
    ObjectEnd,
    /// Start of an array.
    ArrayStart,
    /// End of an array.
    ArrayEnd,
    /// An object key.
    Key(Text<'a>),
    /// A string value.
    String(Text<'a>),
    /// A number, kept as its original source text.
    Number(&'a str),
    /// A Boolean value.
    Bool(bool),
    /// The null value.
    Null,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjectState {
    KeyOrEnd,
    KeyRequired,
    Value,
    CommaOrEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArrayState {
    ValueOrEnd,
    ValueRequired,
    CommaOrEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frame {
    Object(ObjectState),
    Array(ArrayState),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RootState {
    Value,
    Done,
}

/// Pull JSON reader over a borrowed byte slice.
pub struct Reader<'a> {
    input: &'a [u8],
    position: usize,
    stack: Vec<Frame>,
    root: RootState,
}

impl<'a> Reader<'a> {
    /// Creates a reader. A UTF-8 BOM at the beginning is ignored.
    #[must_use]
    pub fn new(input: &'a [u8]) -> Self {
        let position = if input.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
        Self {
            input,
            position,
            stack: Vec::new(),
            root: RootState::Value,
        }
    }

    /// Returns the next JSON event.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for malformed UTF-8 or JSON, excessive nesting, truncated
    /// input, comments, trailing commas, lone surrogates, or trailing non-whitespace bytes.
    pub fn next(&mut self) -> Result<Option<Event<'a>>> {
        loop {
            self.skip_whitespace()?;
            let frame = self.stack.last().copied();
            match frame {
                None => match self.root {
                    RootState::Value => return self.parse_value(),
                    RootState::Done => {
                        if self.position == self.input.len() {
                            return Ok(None);
                        }
                        return Err(self.error("trailing data after JSON value"));
                    }
                },
                Some(Frame::Object(state)) => match state {
                    ObjectState::KeyOrEnd => {
                        if self.peek_byte() == Some(b'}') {
                            self.consume_byte()?;
                            self.stack.pop();
                            self.finish_value()?;
                            return Ok(Some(Event::ObjectEnd));
                        }
                        return self.parse_key();
                    }
                    ObjectState::KeyRequired => {
                        if self.peek_byte() == Some(b'}') {
                            return Err(self.error("trailing comma in object"));
                        }
                        return self.parse_key();
                    }
                    ObjectState::Value => return self.parse_value(),
                    ObjectState::CommaOrEnd => match self.peek_byte() {
                        Some(b',') => {
                            self.consume_byte()?;
                            self.set_top(Frame::Object(ObjectState::KeyRequired))?;
                        }
                        Some(b'}') => {
                            self.consume_byte()?;
                            self.stack.pop();
                            self.finish_value()?;
                            return Ok(Some(Event::ObjectEnd));
                        }
                        _ => return Err(self.error("expected ',' or '}' in object")),
                    },
                },
                Some(Frame::Array(state)) => match state {
                    ArrayState::ValueOrEnd => {
                        if self.peek_byte() == Some(b']') {
                            self.consume_byte()?;
                            self.stack.pop();
                            self.finish_value()?;
                            return Ok(Some(Event::ArrayEnd));
                        }
                        return self.parse_value();
                    }
                    ArrayState::ValueRequired => {
                        if self.peek_byte() == Some(b']') {
                            return Err(self.error("trailing comma in array"));
                        }
                        return self.parse_value();
                    }
                    ArrayState::CommaOrEnd => match self.peek_byte() {
                        Some(b',') => {
                            self.consume_byte()?;
                            self.set_top(Frame::Array(ArrayState::ValueRequired))?;
                        }
                        Some(b']') => {
                            self.consume_byte()?;
                            self.stack.pop();
                            self.finish_value()?;
                            return Ok(Some(Event::ArrayEnd));
                        }
                        _ => return Err(self.error("expected ',' or ']' in array")),
                    },
                },
            }
        }
    }

    /// Skips exactly one value at the current value position without building a tree.
    ///
    /// # Errors
    /// Returns an error if the reader is not positioned at a value or the skipped value is
    /// malformed.
    pub fn skip_value(&mut self) -> Result<()> {
        let first = self
            .next()?
            .ok_or_else(|| self.error("expected value to skip"))?;
        let mut depth = match first {
            Event::ObjectStart | Event::ArrayStart => 1_usize,
            Event::String(_) | Event::Number(_) | Event::Bool(_) | Event::Null => return Ok(()),
            Event::Key(_) | Event::ObjectEnd | Event::ArrayEnd => {
                return Err(self.error("reader is not positioned at a value"));
            }
        };

        while depth != 0 {
            let event = self
                .next()?
                .ok_or_else(|| self.error("truncated value while skipping"))?;
            match event {
                Event::ObjectStart | Event::ArrayStart => {
                    depth = depth
                        .checked_add(1)
                        .ok_or_else(|| self.error("JSON nesting overflow"))?;
                }
                Event::ObjectEnd | Event::ArrayEnd => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| self.error("JSON nesting underflow"))?;
                }
                Event::Key(_) | Event::String(_) | Event::Number(_) | Event::Bool(_) | Event::Null => {}
            }
        }
        Ok(())
    }

    fn parse_key(&mut self) -> Result<Option<Event<'a>>> {
        if self.peek_byte() != Some(b'"') {
            return Err(self.error("object key must be a string"));
        }
        let key = self.parse_string()?;
        self.skip_whitespace()?;
        if self.consume_byte()? != b':' {
            return Err(self.error("expected ':' after object key"));
        }
        self.set_top(Frame::Object(ObjectState::Value))?;
        Ok(Some(Event::Key(key)))
    }

    fn parse_value(&mut self) -> Result<Option<Event<'a>>> {
        self.skip_whitespace()?;
        let byte = self
            .peek_byte()
            .ok_or_else(|| self.error("unexpected end of JSON input"))?;
        let event = match byte {
            b'{' => {
                self.consume_byte()?;
                self.push_frame(Frame::Object(ObjectState::KeyOrEnd))?;
                Event::ObjectStart
            }
            b'[' => {
                self.consume_byte()?;
                self.push_frame(Frame::Array(ArrayState::ValueOrEnd))?;
                Event::ArrayStart
            }
            b'"' => {
                let text = self.parse_string()?;
                self.finish_value()?;
                Event::String(text)
            }
            b't' => {
                self.consume_keyword(b"true")?;
                self.finish_value()?;
                Event::Bool(true)
            }
            b'f' => {
                self.consume_keyword(b"false")?;
                self.finish_value()?;
                Event::Bool(false)
            }
            b'n' => {
                self.consume_keyword(b"null")?;
                self.finish_value()?;
                Event::Null
            }
            b'-' | b'0'..=b'9' => {
                let number = self.parse_number()?;
                self.finish_value()?;
                Event::Number(number)
            }
            _ => return Err(self.error("unexpected JSON token")),
        };
        Ok(Some(event))
    }

    fn parse_string(&mut self) -> Result<Text<'a>> {
        if self.consume_byte()? != b'"' {
            return Err(self.error("expected string"));
        }
        let start = self.position;
        let mut scan = start;

        loop {
            let byte = self
                .input
                .get(scan)
                .copied()
                .ok_or_else(|| self.error_at(scan, "unterminated JSON string"))?;
            match byte {
                b'"' => {
                    let raw = self
                        .input
                        .get(start..scan)
                        .ok_or_else(|| self.error_at(scan, "invalid string range"))?;
                    let value = core::str::from_utf8(raw)
                        .map_err(|_| self.error_at(start, "string is not valid UTF-8"))?;
                    self.position = scan
                        .checked_add(1)
                        .ok_or_else(|| self.error_at(scan, "string position overflow"))?;
                    return Ok(Text::Borrowed(value));
                }
                b'\\' => break,
                0x00..=0x1F => return Err(self.error_at(scan, "unescaped control byte in string")),
                _ => {
                    scan = scan
                        .checked_add(1)
                        .ok_or_else(|| self.error_at(scan, "string position overflow"))?;
                }
            }
        }

        let mut decoded = String::with_capacity(scan.checked_sub(start).unwrap_or_default());
        self.push_utf8_segment(&mut decoded, start, scan)?;
        self.position = scan;

        loop {
            let byte = self.consume_byte()?;
            match byte {
                b'"' => return Ok(Text::Owned(decoded)),
                b'\\' => self.decode_escape(&mut decoded)?,
                0x00..=0x1F => return Err(self.error("unescaped control byte in string")),
                _ => {
                    let segment_start = self
                        .position
                        .checked_sub(1)
                        .ok_or_else(|| self.error("string position underflow"))?;
                    let mut end = self.position;
                    loop {
                        match self.peek_byte() {
                            Some(b'"') | Some(b'\\') | None => break,
                            Some(0x00..=0x1F) => {
                                return Err(self.error("unescaped control byte in string"));
                            }
                            Some(_) => {
                                end = end
                                    .checked_add(1)
                                    .ok_or_else(|| self.error("string position overflow"))?;
                                self.position = end;
                            }
                        }
                    }
                    self.push_utf8_segment(&mut decoded, segment_start, end)?;
                }
            }
        }
    }

    fn decode_escape(&mut self, decoded: &mut String) -> Result<()> {
        let escape = self.consume_byte()?;
        match escape {
            b'"' => decoded.push('"'),
            b'\\' => decoded.push('\\'),
            b'/' => decoded.push('/'),
            b'b' => decoded.push('\u{0008}'),
            b'f' => decoded.push('\u{000C}'),
            b'n' => decoded.push('\n'),
            b'r' => decoded.push('\r'),
            b't' => decoded.push('\t'),
            b'u' => {
                let first = self.hex_quad()?;
                let scalar = if (0xD800..=0xDBFF).contains(&first) {
                    if self.consume_byte()? != b'\\' || self.consume_byte()? != b'u' {
                        return Err(self.error("high surrogate is not followed by a low surrogate"));
                    }
                    let second = self.hex_quad()?;
                    if !(0xDC00..=0xDFFF).contains(&second) {
                        return Err(self.error("invalid low surrogate"));
                    }
                    let high = u32::from(first)
                        .checked_sub(0xD800)
                        .ok_or_else(|| self.error("invalid high surrogate"))?;
                    let low = u32::from(second)
                        .checked_sub(0xDC00)
                        .ok_or_else(|| self.error("invalid low surrogate"))?;
                    0x1_0000_u32
                        .checked_add(
                            high.checked_shl(10)
                                .ok_or_else(|| self.error("surrogate overflow"))?,
                        )
                        .and_then(|value| value.checked_add(low))
                        .ok_or_else(|| self.error("surrogate overflow"))?
                } else if (0xDC00..=0xDFFF).contains(&first) {
                    return Err(self.error("lone low surrogate"));
                } else {
                    u32::from(first)
                };
                let character = char::from_u32(scalar)
                    .ok_or_else(|| self.error("invalid Unicode scalar value"))?;
                decoded.push(character);
            }
            _ => return Err(self.error("invalid JSON escape")),
        }
        Ok(())
    }

    fn hex_quad(&mut self) -> Result<u16> {
        let mut value = 0_u16;
        for _ in 0..4 {
            let byte = self.consume_byte()?;
            let digit = match byte {
                b'0'..=b'9' => u16::from(byte.checked_sub(b'0').unwrap_or_default()),
                b'a'..=b'f' => u16::from(byte.checked_sub(b'a').unwrap_or_default())
                    .checked_add(10)
                    .ok_or_else(|| self.error("hex digit overflow"))?,
                b'A'..=b'F' => u16::from(byte.checked_sub(b'A').unwrap_or_default())
                    .checked_add(10)
                    .ok_or_else(|| self.error("hex digit overflow"))?,
                _ => return Err(self.error("invalid hex digit in Unicode escape")),
            };
            value = value
                .checked_shl(4)
                .and_then(|shifted| shifted.checked_add(digit))
                .ok_or_else(|| self.error("Unicode escape overflow"))?;
        }
        Ok(value)
    }

    fn push_utf8_segment(&self, target: &mut String, start: usize, end: usize) -> Result<()> {
        let bytes = self
            .input
            .get(start..end)
            .ok_or_else(|| self.error_at(start, "invalid UTF-8 segment range"))?;
        let text = core::str::from_utf8(bytes)
            .map_err(|_| self.error_at(start, "string is not valid UTF-8"))?;
        target.push_str(text);
        Ok(())
    }

    fn parse_number(&mut self) -> Result<&'a str> {
        let start = self.position;
        if self.peek_byte() == Some(b'-') {
            self.consume_byte()?;
        }

        match self.peek_byte() {
            Some(b'0') => {
                self.consume_byte()?;
                if matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                    return Err(self.error("leading zero in JSON number"));
                }
            }
            Some(b'1'..=b'9') => self.consume_digits()?,
            _ => return Err(self.error("invalid JSON number")),
        }

        if self.peek_byte() == Some(b'.') {
            self.consume_byte()?;
            if !matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                return Err(self.error("fraction has no digits"));
            }
            self.consume_digits()?;
        }

        if matches!(self.peek_byte(), Some(b'e') | Some(b'E')) {
            self.consume_byte()?;
            if matches!(self.peek_byte(), Some(b'+') | Some(b'-')) {
                self.consume_byte()?;
            }
            if !matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                return Err(self.error("exponent has no digits"));
            }
            self.consume_digits()?;
        }

        if let Some(next) = self.peek_byte() {
            if !matches!(next, b' ' | b'\t' | b'\r' | b'\n' | b',' | b']' | b'}') {
                return Err(self.error("invalid byte after JSON number"));
            }
        }

        let bytes = self
            .input
            .get(start..self.position)
            .ok_or_else(|| self.error("invalid number range"))?;
        core::str::from_utf8(bytes).map_err(|_| self.error("number is not UTF-8"))
    }

    fn consume_digits(&mut self) -> Result<()> {
        while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
            self.consume_byte()?;
        }
        Ok(())
    }

    fn consume_keyword(&mut self, keyword: &[u8]) -> Result<()> {
        for expected in keyword.iter().copied() {
            if self.consume_byte()? != expected {
                return Err(self.error("invalid JSON keyword"));
            }
        }
        if let Some(next) = self.peek_byte() {
            if !matches!(next, b' ' | b'\t' | b'\r' | b'\n' | b',' | b']' | b'}') {
                return Err(self.error("invalid byte after JSON keyword"));
            }
        }
        Ok(())
    }

    fn push_frame(&mut self, frame: Frame) -> Result<()> {
        if self.stack.len() >= MAX_DEPTH {
            return Err(self.error("JSON nesting exceeds 128 levels"));
        }
        self.stack.push(frame);
        Ok(())
    }

    fn set_top(&mut self, frame: Frame) -> Result<()> {
        let top = self
            .stack
            .last_mut()
            .ok_or_else(|| Error::damaged("missing JSON container state"))?;
        *top = frame;
        Ok(())
    }

    fn finish_value(&mut self) -> Result<()> {
        let parent = self.stack.last().copied();
        match parent {
            None => {
                self.root = RootState::Done;
                Ok(())
            }
            Some(Frame::Object(ObjectState::Value)) => {
                self.set_top(Frame::Object(ObjectState::CommaOrEnd))
            }
            Some(Frame::Array(ArrayState::ValueOrEnd | ArrayState::ValueRequired)) => {
                self.set_top(Frame::Array(ArrayState::CommaOrEnd))
            }
            _ => Err(self.error("invalid JSON container state after value")),
        }
    }

    fn skip_whitespace(&mut self) -> Result<()> {
        while matches!(self.peek_byte(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.consume_byte()?;
        }
        Ok(())
    }

    fn peek_byte(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn consume_byte(&mut self) -> Result<u8> {
        let byte = self
            .input
            .get(self.position)
            .copied()
            .ok_or_else(|| self.error("unexpected end of JSON input"))?;
        self.position = self
            .position
            .checked_add(1)
            .ok_or_else(|| self.error("JSON position overflow"))?;
        Ok(byte)
    }

    fn error(&self, message: &str) -> Error {
        self.error_at(self.position, message)
    }

    fn error_at(&self, position: usize, message: &str) -> Error {
        Error::damaged(format!("JSON at byte {position}: {message}"))
    }
}

/// Numeric conversion helpers for JSON number source text.
pub trait NumberExt {
    /// Parses a JSON integer into `i64` without accepting fractions or exponents.
    fn as_i64(&self) -> Option<i64>;
    /// Parses a non-negative JSON integer into `u64`.
    fn as_u64(&self) -> Option<u64>;
    /// Converts a JSON number to binary64.
    fn as_f64(&self) -> Option<f64>;
}

impl NumberExt for str {
    fn as_i64(&self) -> Option<i64> {
        parse_i64(self)
    }

    fn as_u64(&self) -> Option<u64> {
        parse_u64(self)
    }

    fn as_f64(&self) -> Option<f64> {
        if !is_number(self.as_bytes()) {
            return None;
        }
        self.parse::<f64>().ok().filter(|value| value.is_finite())
    }
}

fn parse_i64(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let negative = bytes.first().copied() == Some(b'-');
    let start = if negative { 1 } else { 0 };
    let digits = bytes.get(start..)?;
    if digits.is_empty() || digits.iter().any(|byte| !byte.is_ascii_digit()) {
        return None;
    }
    let mut magnitude = 0_u64;
    for byte in digits.iter().copied() {
        let digit = u64::from(byte.checked_sub(b'0')?);
        magnitude = magnitude.checked_mul(10)?.checked_add(digit)?;
    }
    if negative {
        let limit = 9_223_372_036_854_775_808_u64;
        if magnitude > limit {
            return None;
        }
        if magnitude == limit {
            Some(i64::MIN)
        } else {
            let positive = i64::try_from(magnitude).ok()?;
            positive.checked_neg()
        }
    } else {
        i64::try_from(magnitude).ok()
    }
}

fn parse_u64(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.first().copied() == Some(b'-') {
        return None;
    }
    let mut value = 0_u64;
    for byte in bytes.iter().copied() {
        if !byte.is_ascii_digit() {
            return None;
        }
        let digit = u64::from(byte.checked_sub(b'0')?);
        value = value.checked_mul(10)?.checked_add(digit)?;
    }
    Some(value)
}

fn is_number(bytes: &[u8]) -> bool {
    let mut reader = Reader::new(bytes);
    matches!(reader.next(), Ok(Some(Event::Number(_)))) && matches!(reader.next(), Ok(None))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WriteFrame {
    Object { first: bool, expects_key: bool },
    Array { first: bool },
}

/// Stable JSON writer supporting compact and two-space-indented output.
pub struct Writer {
    output: Vec<u8>,
    stack: Vec<WriteFrame>,
    pretty: bool,
    root_written: bool,
}

impl Writer {
    /// Creates a compact JSON writer.
    #[must_use]
    pub fn new() -> Self {
        Self::compact()
    }

    /// Creates a compact JSON writer.
    #[must_use]
    pub fn compact() -> Self {
        Self {
            output: Vec::new(),
            stack: Vec::new(),
            pretty: false,
            root_written: false,
        }
    }

    /// Creates a two-space-indented JSON writer.
    #[must_use]
    pub fn indented() -> Self {
        Self {
            output: Vec::new(),
            stack: Vec::new(),
            pretty: true,
            root_written: false,
        }
    }

    /// Starts an object value.
    ///
    /// # Errors
    /// Returns an error for invalid writer state or excessive nesting.
    pub fn object_start(&mut self) -> Result<()> {
        self.before_value()?;
        if self.stack.len() >= MAX_DEPTH {
            return Err(Error::damaged("JSON writer nesting exceeds 128 levels"));
        }
        self.output.push(b'{');
        self.stack.push(WriteFrame::Object {
            first: true,
            expects_key: true,
        });
        Ok(())
    }

    /// Ends the current object.
    ///
    /// # Errors
    /// Returns an error if the current container is not a complete object.
    pub fn object_end(&mut self) -> Result<()> {
        let frame = self
            .stack
            .pop()
            .ok_or_else(|| Error::damaged("JSON writer has no object to end"))?;
        match frame {
            WriteFrame::Object { first, expects_key } if expects_key => {
                if self.pretty && !first {
                    self.newline_and_indent(self.stack.len())?;
                }
                self.output.push(b'}');
                Ok(())
            }
            WriteFrame::Object { .. } => Err(Error::damaged("JSON object key has no value")),
            WriteFrame::Array { .. } => Err(Error::damaged("JSON writer expected array end")),
        }
    }

    /// Starts an array value.
    ///
    /// # Errors
    /// Returns an error for invalid writer state or excessive nesting.
    pub fn array_start(&mut self) -> Result<()> {
        self.before_value()?;
        if self.stack.len() >= MAX_DEPTH {
            return Err(Error::damaged("JSON writer nesting exceeds 128 levels"));
        }
        self.output.push(b'[');
        self.stack.push(WriteFrame::Array { first: true });
        Ok(())
    }

    /// Ends the current array.
    ///
    /// # Errors
    /// Returns an error if the current container is not an array.
    pub fn array_end(&mut self) -> Result<()> {
        let frame = self
            .stack
            .pop()
            .ok_or_else(|| Error::damaged("JSON writer has no array to end"))?;
        match frame {
            WriteFrame::Array { first } => {
                if self.pretty && !first {
                    self.newline_and_indent(self.stack.len())?;
                }
                self.output.push(b']');
                Ok(())
            }
            WriteFrame::Object { .. } => Err(Error::damaged("JSON writer expected object end")),
        }
    }

    /// Writes an object key.
    ///
    /// # Errors
    /// Returns an error outside an object or when the previous key has no value.
    pub fn key(&mut self, key: &str) -> Result<()> {
        let index = self
            .stack
            .len()
            .checked_sub(1)
            .ok_or_else(|| Error::damaged("JSON key outside object"))?;
        let frame = self
            .stack
            .get(index)
            .copied()
            .ok_or_else(|| Error::damaged("JSON key outside object"))?;
        let (first, expects_key) = match frame {
            WriteFrame::Object { first, expects_key } => (first, expects_key),
            WriteFrame::Array { .. } => return Err(Error::damaged("JSON key inside array")),
        };
        if !expects_key {
            return Err(Error::damaged("previous JSON key has no value"));
        }
        if !first {
            self.output.push(b',');
        }
        if self.pretty {
            self.newline_and_indent(self.stack.len())?;
        }
        write_escaped_string(&mut self.output, key)?;
        self.output.push(b':');
        if self.pretty {
            self.output.push(b' ');
        }
        let slot = self
            .stack
            .get_mut(index)
            .ok_or_else(|| Error::damaged("JSON object state disappeared"))?;
        *slot = WriteFrame::Object {
            first: false,
            expects_key: false,
        };
        Ok(())
    }

    /// Writes a string value.
    ///
    /// # Errors
    /// Returns an error for invalid writer state.
    pub fn string(&mut self, value: &str) -> Result<()> {
        self.before_value()?;
        write_escaped_string(&mut self.output, value)
    }

    /// Writes a validated JSON number verbatim.
    ///
    /// # Errors
    /// Returns an error if `value` is not a valid finite RFC 8259 number or writer state is invalid.
    pub fn number(&mut self, value: &str) -> Result<()> {
        if !is_number(value.as_bytes()) {
            return Err(Error::damaged("invalid JSON number passed to writer"));
        }
        self.before_value()?;
        self.output.extend_from_slice(value.as_bytes());
        Ok(())
    }

    /// Writes an `i64` number.
    ///
    /// # Errors
    /// Returns an error for invalid writer state.
    pub fn i64(&mut self, value: i64) -> Result<()> {
        self.number(&value.to_string())
    }

    /// Writes a `u64` number.
    ///
    /// # Errors
    /// Returns an error for invalid writer state.
    pub fn u64(&mut self, value: u64) -> Result<()> {
        self.number(&value.to_string())
    }

    /// Writes a Boolean value.
    ///
    /// # Errors
    /// Returns an error for invalid writer state.
    pub fn bool(&mut self, value: bool) -> Result<()> {
        self.before_value()?;
        if value {
            self.output.extend_from_slice(b"true");
        } else {
            self.output.extend_from_slice(b"false");
        }
        Ok(())
    }

    /// Writes `null`.
    ///
    /// # Errors
    /// Returns an error for invalid writer state.
    pub fn null(&mut self) -> Result<()> {
        self.before_value()?;
        self.output.extend_from_slice(b"null");
        Ok(())
    }

    /// Finishes the writer and returns UTF-8 JSON bytes.
    ///
    /// # Errors
    /// Returns an error if a container is still open or no root value was written.
    pub fn finish(self) -> Result<Vec<u8>> {
        if !self.stack.is_empty() {
            return Err(Error::damaged("JSON writer has unclosed containers"));
        }
        if !self.root_written {
            return Err(Error::damaged("JSON writer has no root value"));
        }
        Ok(self.output)
    }

    fn before_value(&mut self) -> Result<()> {
        let index = self.stack.len().checked_sub(1);
        match index {
            None => {
                if self.root_written {
                    return Err(Error::damaged("JSON writer already has a root value"));
                }
                self.root_written = true;
            }
            Some(index) => {
                let frame = self
                    .stack
                    .get(index)
                    .copied()
                    .ok_or_else(|| Error::damaged("JSON writer state disappeared"))?;
                match frame {
                    WriteFrame::Object { first, expects_key } => {
                        if expects_key {
                            return Err(Error::damaged("JSON object value requires a key"));
                        }
                        let slot = self
                            .stack
                            .get_mut(index)
                            .ok_or_else(|| Error::damaged("JSON writer state disappeared"))?;
                        *slot = WriteFrame::Object {
                            first,
                            expects_key: true,
                        };
                    }
                    WriteFrame::Array { first } => {
                        if !first {
                            self.output.push(b',');
                        }
                        if self.pretty {
                            self.newline_and_indent(self.stack.len())?;
                        }
                        let slot = self
                            .stack
                            .get_mut(index)
                            .ok_or_else(|| Error::damaged("JSON writer state disappeared"))?;
                        *slot = WriteFrame::Array { first: false };
                    }
                }
            }
        }
        Ok(())
    }

    fn newline_and_indent(&mut self, depth: usize) -> Result<()> {
        self.output.push(b'\n');
        let spaces = depth
            .checked_mul(2)
            .ok_or_else(|| Error::damaged("JSON indentation overflow"))?;
        self.output.extend(core::iter::repeat_n(b' ', spaces));
        Ok(())
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

fn write_escaped_string(output: &mut Vec<u8>, value: &str) -> Result<()> {
    output.push(b'"');
    for character in value.chars() {
        match character {
            '"' => output.extend_from_slice(b"\\\""),
            '\\' => output.extend_from_slice(b"\\\\"),
            '\u{0008}' => output.extend_from_slice(b"\\b"),
            '\u{000C}' => output.extend_from_slice(b"\\f"),
            '\n' => output.extend_from_slice(b"\\n"),
            '\r' => output.extend_from_slice(b"\\r"),
            '\t' => output.extend_from_slice(b"\\t"),
            '\u{0000}'..='\u{001F}' => {
                output.extend_from_slice(b"\\u00");
                let code = u32::from(character);
                let high = u8::try_from(code.checked_shr(4).unwrap_or_default()).unwrap_or_default();
                let low = u8::try_from(code & 0xF).unwrap_or_default();
                output.push(hex_digit(high));
                output.push(hex_digit(low));
            }
            _ => {
                let mut buffer = [0_u8; 4];
                let encoded = character.encode_utf8(&mut buffer);
                output.extend_from_slice(encoded.as_bytes());
            }
        }
    }
    output.push(b'"');
    Ok(())
}

fn hex_digit(value: u8) -> u8 {
    match value {
        0..=9 => b'0'.checked_add(value).unwrap_or(b'0'),
        10..=15 => b'A'
            .checked_add(value.checked_sub(10).unwrap_or_default())
            .unwrap_or(b'A'),
        _ => b'0',
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, NumberExt, Reader, Text, Writer};

    #[test]
    fn borrowed_and_owned_strings_are_distinct() {
        let mut reader = Reader::new(br#"["plain","escaped\n", "\uD83D\uDE00"]"#);
        assert_eq!(reader.next(), Ok(Some(Event::ArrayStart)));
        assert!(matches!(reader.next(), Ok(Some(Event::String(Text::Borrowed("plain"))))));
        assert!(matches!(reader.next(), Ok(Some(Event::String(Text::Owned(value)))) if value == "escaped\n"));
        assert!(matches!(reader.next(), Ok(Some(Event::String(Text::Owned(value)))) if value == "😀"));
        assert_eq!(reader.next(), Ok(Some(Event::ArrayEnd)));
        assert_eq!(reader.next(), Ok(None));
    }

    #[test]
    fn ten_mib_unescaped_string_is_borrowed() {
        let count = 10_usize.checked_mul(1024).and_then(|v| v.checked_mul(1024)).unwrap_or_default();
        let mut input = Vec::with_capacity(count.checked_add(2).unwrap_or(count));
        input.push(b'"');
        input.extend(core::iter::repeat_n(b'a', count));
        input.push(b'"');
        let mut reader = Reader::new(&input);
        assert!(matches!(reader.next(), Ok(Some(Event::String(Text::Borrowed(value)))) if value.len() == count));
        assert_eq!(reader.next(), Ok(None));
    }

    #[test]
    fn rejects_depth_bomb_and_lone_surrogates() {
        let mut deep = Vec::new();
        deep.extend(core::iter::repeat_n(b'[', 129));
        deep.extend(core::iter::repeat_n(b']', 129));
        assert!(drain(&deep).is_err());
        for text in [br#""\uD800""#.as_slice(), br#""\uDC00""#.as_slice(), br#""\uD800\u0041""#.as_slice()] {
            assert!(drain(text).is_err());
        }
    }

    #[test]
    fn compact_and_indented_writer_are_stable() {
        let mut writer = Writer::compact();
        assert!(writer.object_start().is_ok());
        assert!(writer.key("a").is_ok());
        assert!(writer.array_start().is_ok());
        assert!(writer.i64(-1).is_ok());
        assert!(writer.string("x\ny").is_ok());
        assert!(writer.bool(true).is_ok());
        assert!(writer.array_end().is_ok());
        assert!(writer.key("n").is_ok());
        assert!(writer.null().is_ok());
        assert!(writer.object_end().is_ok());
        assert_eq!(writer.finish(), Ok(br#"{"a":[-1,"x\ny",true],"n":null}"#.to_vec()));

        let mut writer = Writer::indented();
        assert!(writer.array_start().is_ok());
        assert!(writer.string("a").is_ok());
        assert!(writer.string("b").is_ok());
        assert!(writer.array_end().is_ok());
        assert_eq!(writer.finish(), Ok(b"[\n  \"a\",\n  \"b\"\n]".to_vec()));
    }

    #[test]
    fn number_helpers_cover_boundaries() {
        assert_eq!("9223372036854775807".as_i64(), Some(i64::MAX));
        assert_eq!("-9223372036854775808".as_i64(), Some(i64::MIN));
        assert_eq!("9223372036854775808".as_i64(), None);
        assert_eq!("18446744073709551615".as_u64(), Some(u64::MAX));
        assert_eq!("18446744073709551616".as_u64(), None);
        assert_eq!("1.25e2".as_f64(), Some(125.0));
    }

    #[test]
    fn classic_accept_reject_cases() {
        let valid: [&[u8]; 32] = [
            b"null", b"true", b"false", b"0", b"-0", b"1", b"-1", b"1.0",
            b"1e0", b"1E+2", b"1e-2", b"[]", b"{}", b"[1]", b"[1,2,3]", b"{\"a\":1}",
            b"{\"a\":[],\"b\":{}}", br#"""#, br#""abc""#, br#""\\\/\b\f\n\r\t""#,
            br#""\u0000""#, br#""\u20AC""#, br#""\uD83D\uDE00""#, b" \r\n\t null ",
            b"[true,false,null]", b"{\"a\":1,\"a\":2}", b"[{}]", b"{\"x\":[[[]]]}",
            b"0.0", b"10e10", b"-12.34E-5", b"\xEF\xBB\xBF{}",
        ];
        for value in valid {
            assert!(drain(value).is_ok(), "valid case rejected: {:?}", value);
        }

        let invalid: [&[u8]; 32] = [
            b"", b" ", b"nul", b"True", b"FALSE", b"01", b"-", b"-.1", b"1.", b"1e",
            b"1e+", b"[", b"{", b"[1,]", b"{\"a\":1,}", b"{a:1}", b"{\"a\" 1}",
            b"[1 2]", br#""\x""#, br#""\u12""#, br#""\uD800""#, br#""\uDC00""#,
            b"//comment\n1", b"/*x*/1", b"1 2", b"[]x", b"[,,]", b"{,}", b"+1", b".1",
            b"NaN", b"Infinity",
        ];
        for value in invalid {
            assert!(drain(value).is_err(), "invalid case accepted: {:?}", value);
        }
    }

    #[test]
    fn truncation_of_nested_document_is_always_rejected_until_complete() {
        let document = br#"{"release":{"version":"1.3.1","assets":[{"name":"linux","size":12345},{"name":"win","size":67890}]},"ok":true}"#;
        for cut in 0..document.len() {
            let Some(prefix) = document.get(..cut) else {
                panic!("invalid test prefix");
            };
            assert!(drain(prefix).is_err(), "truncation {cut} was accepted");
        }
        assert!(drain(document).is_ok());
    }

    fn drain(input: &[u8]) -> sse_core::Result<Vec<String>> {
        let mut reader = Reader::new(input);
        let mut values = Vec::new();
        while let Some(event) = reader.next()? {
            values.push(format!("{event:?}"));
        }
        Ok(values)
    }
}