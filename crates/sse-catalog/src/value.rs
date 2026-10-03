//! DOM-like JSON value representation built on `sse_codecs::json`.

use sse_codecs::json::{Event, Reader, Writer};
use sse_core::{Error, Result};
use std::fmt;

/// A JSON value in memory.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    /// Null literal.
    Null,
    /// Boolean literal.
    Bool(bool),
    /// Numeric literal.
    Number(f64),
    /// String literal.
    String(String),
    /// Array of values.
    Array(Vec<JsonValue>),
    /// Key-value object mapping.
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// Returns the string slice if this is a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Returns boolean if this is a bool.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns float if this is a number.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Returns integer if this is an integral number.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Number(n) => {
                if n.is_finite() && n.fract() == 0.0 && *n >= -9_007_199_254_740_992.0 && *n <= 9_007_199_254_740_992.0
                {
                    format!("{n:.0}").parse::<i64>().ok()
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Returns unsigned integer if this is an integral number.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        self.as_i64().and_then(|i| u64::try_from(i).ok())
    }

    /// Returns slice of values if this is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            Self::Array(arr) => Some(arr.as_slice()),
            _ => None,
        }
    }

    /// Returns slice of entries if this is an object.
    #[must_use]
    pub fn as_object(&self) -> Option<&[(String, JsonValue)]> {
        match self {
            Self::Object(entries) => Some(entries.as_slice()),
            _ => None,
        }
    }

    /// Looks up a key in an object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            Self::Object(entries) => {
                for (k, v) in entries {
                    if k == key {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Writes this JSON value to a streaming writer.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] or [`Error::Refused`] on serialization failure.
    pub fn write_to(&self, writer: &mut Writer) -> Result<()> {
        match self {
            Self::Null => writer.null(),
            Self::Bool(b) => writer.bool(*b),
            Self::Number(n) => {
                if n.is_finite() && n.fract() == 0.0 && *n >= -9_007_199_254_740_992.0 && *n <= 9_007_199_254_740_992.0
                {
                    if let Ok(i) = format!("{n:.0}").parse::<i64>() {
                        return writer.i64(i);
                    }
                }
                writer.number(&format!("{n}"))
            }
            Self::String(s) => writer.string(s),
            Self::Array(arr) => {
                writer.array_start()?;
                for item in arr {
                    item.write_to(writer)?;
                }
                writer.array_end()
            }
            Self::Object(entries) => {
                writer.object_start()?;
                for (k, v) in entries {
                    writer.key(k)?;
                    v.write_to(writer)?;
                }
                writer.object_end()
            }
        }
    }

    /// Formats this JSON value as an indented string.
    #[must_use]
    pub fn to_indented_string(&self) -> String {
        let mut writer = Writer::indented();
        if self.write_to(&mut writer).is_err() {
            return String::new();
        }
        writer
            .finish()
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default()
    }
}

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_indented_string())
    }
}

/// Parses a JSON string slice into a [`JsonValue`].
///
/// # Errors
/// Returns [`Error::Damaged`] on malformed JSON input.
pub fn parse_json(input: &str) -> Result<JsonValue> {
    let mut reader = Reader::new(input.as_bytes());
    let first = reader.next_event()?.ok_or_else(|| Error::damaged("Empty JSON input"))?;
    let value = parse_value_from_event(first, &mut reader)?;
    if reader.next_event()?.is_some() {
        return Err(Error::damaged("Trailing data after top-level JSON value"));
    }
    Ok(value)
}

fn parse_value_from_event<'a>(event: Event<'a>, reader: &mut Reader<'a>) -> Result<JsonValue> {
    match event {
        Event::ObjectStart => parse_object(reader),
        Event::ArrayStart => parse_array(reader),
        Event::String(text) => Ok(JsonValue::String(text.into_owned())),
        Event::Number(num_str) => {
            let float = num_str
                .parse::<f64>()
                .map_err(|e| Error::damaged(format!("Invalid number '{num_str}': {e}")))?;
            Ok(JsonValue::Number(float))
        }
        Event::Bool(b) => Ok(JsonValue::Bool(b)),
        Event::Null => Ok(JsonValue::Null),
        Event::Key(_) | Event::ObjectEnd | Event::ArrayEnd => {
            Err(Error::damaged("Unexpected token where value was expected"))
        }
    }
}

fn parse_object<'a>(reader: &mut Reader<'a>) -> Result<JsonValue> {
    let mut entries = Vec::new();
    loop {
        let event = reader
            .next_event()?
            .ok_or_else(|| Error::damaged("Unexpected end of JSON in object"))?;
        match event {
            Event::ObjectEnd => break,
            Event::Key(key) => {
                let val_event = reader
                    .next_event()?
                    .ok_or_else(|| Error::damaged("Expected value after object key"))?;
                let val = parse_value_from_event(val_event, reader)?;
                entries.push((key.into_owned(), val));
            }
            _ => return Err(Error::damaged("Expected object key or object end")),
        }
    }
    Ok(JsonValue::Object(entries))
}

fn parse_array<'a>(reader: &mut Reader<'a>) -> Result<JsonValue> {
    let mut items = Vec::new();
    loop {
        let event = reader
            .next_event()?
            .ok_or_else(|| Error::damaged("Unexpected end of JSON in array"))?;
        if event == Event::ArrayEnd {
            break;
        }
        let val = parse_value_from_event(event, reader)?;
        items.push(val);
    }
    Ok(JsonValue::Array(items))
}
