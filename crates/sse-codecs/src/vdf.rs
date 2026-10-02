//! Valve text KeyValues (VDF) reader used for Steam library discovery.

use sse_core::{Error, Result};

const MAXIMUM_DEPTH: usize = 64;
const MAXIMUM_INPUT: usize = 16 * 1024 * 1024;

/// A Valve KeyValues value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A string value.
    String(String),
    /// A nested object.
    Object(Node),
}

/// An ordered KeyValues object.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Node {
    children: Vec<(String, Value)>,
}

impl Node {
    /// Returns entries in insertion order.
    #[must_use]
    pub fn children(&self) -> &[(String, Value)] {
        &self.children
    }

    /// Looks a value up using the reference parser's ordinal case-insensitive behaviour.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.children
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    }

    /// Looks a nested object up case-insensitively.
    #[must_use]
    pub fn get_object(&self, key: &str) -> Option<&Node> {
        match self.get(key) {
            Some(Value::Object(value)) => Some(value),
            _ => None,
        }
    }

    /// Looks a string up case-insensitively.
    #[must_use]
    pub fn get_string(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::String(value)) => Some(value.as_str()),
            _ => None,
        }
    }

    fn insert_reference_style(&mut self, key: String, value: Value) {
        if let Some((_, existing)) = self.children.iter_mut().find(|(candidate, _)| candidate == &key) {
            *existing = value;
        } else {
            self.children.push((key, value));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Value(String),
    Open,
    Close,
}

/// Parses Valve's text KeyValues format.
pub fn parse(text: &str) -> Result<Node> {
    if text.len() > MAXIMUM_INPUT {
        return Err(Error::damaged("Valve KeyValues input is too large"));
    }
    let tokens = tokenize(text)?;
    let mut position = 0_usize;
    let document = read_object(&tokens, &mut position, false, 0)?;
    if position != tokens.len() {
        return Err(Error::damaged("Unexpected trailing Valve KeyValues token"));
    }
    Ok(document)
}

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut position = 0_usize;
    while position < chars.len() {
        let character = chars
            .get(position)
            .copied()
            .ok_or_else(|| Error::damaged("Valve KeyValues cursor"))?;
        if character.is_whitespace() {
            position = position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Valve KeyValues position overflow"))?;
            continue;
        }
        if character == '/' {
            let next_position = position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Valve KeyValues position overflow"))?;
            if chars.get(next_position).copied() == Some('/') {
                position = next_position
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("Valve KeyValues position overflow"))?;
                while let Some(value) = chars.get(position).copied() {
                    if value == '\r' || value == '\n' {
                        break;
                    }
                    position = position
                        .checked_add(1)
                        .ok_or_else(|| Error::damaged("Valve KeyValues position overflow"))?;
                }
                continue;
            }
        }
        if character == '{' {
            tokens.push(Token::Open);
            position = position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Valve KeyValues position overflow"))?;
            continue;
        }
        if character == '}' {
            tokens.push(Token::Close);
            position = position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Valve KeyValues position overflow"))?;
            continue;
        }
        if character == '"' {
            let (value, next) = read_quoted(&chars, position)?;
            tokens.push(Token::Value(value));
            position = next;
            continue;
        }
        let start = position;
        while let Some(value) = chars.get(position).copied() {
            if value.is_whitespace() || value == '{' || value == '}' {
                break;
            }
            position = position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Valve KeyValues position overflow"))?;
        }
        if start == position {
            return Err(Error::damaged("Empty Valve KeyValues token"));
        }
        let value: String = chars
            .get(start..position)
            .ok_or_else(|| Error::damaged("Valve KeyValues token range"))?
            .iter()
            .collect();
        tokens.push(Token::Value(value));
    }
    Ok(tokens)
}

fn read_quoted(chars: &[char], quote_position: usize) -> Result<(String, usize)> {
    let mut position = quote_position
        .checked_add(1)
        .ok_or_else(|| Error::damaged("Valve KeyValues position overflow"))?;
    let mut value = String::new();
    while let Some(character) = chars.get(position).copied() {
        position = position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("Valve KeyValues position overflow"))?;
        if character == '"' {
            return Ok((value, position));
        }
        if character == '\\' {
            if let Some(escaped) = chars.get(position).copied() {
                if escaped == '\\' || escaped == '"' {
                    value.push(escaped);
                    position = position
                        .checked_add(1)
                        .ok_or_else(|| Error::damaged("Valve KeyValues position overflow"))?;
                } else {
                    value.push('\\');
                }
            } else {
                value.push('\\');
            }
        } else {
            value.push(character);
        }
    }
    Err(Error::damaged("Unterminated quoted Valve KeyValues value"))
}

fn read_object(tokens: &[Token], position: &mut usize, expect_close: bool, depth: usize) -> Result<Node> {
    if depth > MAXIMUM_DEPTH {
        return Err(Error::damaged("Valve KeyValues nesting is too deep"));
    }
    let mut result = Node::default();
    while let Some(token) = tokens.get(*position) {
        if matches!(token, Token::Close) {
            if !expect_close {
                return Err(Error::damaged("Unexpected closing brace in Valve KeyValues"));
            }
            *position = position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Valve KeyValues token position overflow"))?;
            return Ok(result);
        }
        let key = match token {
            Token::Value(value) => value.clone(),
            Token::Open => return Err(Error::damaged("Expected a Valve KeyValues key/value pair")),
            Token::Close => return Err(Error::damaged("Unexpected closing brace in Valve KeyValues")),
        };
        *position = position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("Valve KeyValues token position overflow"))?;
        let next = tokens
            .get(*position)
            .ok_or_else(|| Error::damaged("Expected a Valve KeyValues key/value pair"))?;
        *position = position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("Valve KeyValues token position overflow"))?;
        let value = match next {
            Token::Value(value) => Value::String(value.clone()),
            Token::Open => {
                let next_depth = depth
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("Valve KeyValues depth overflow"))?;
                Value::Object(read_object(tokens, position, true, next_depth)?)
            }
            Token::Close => return Err(Error::damaged("Expected a Valve KeyValues value or object")),
        };
        result.insert_reference_style(key, value);
    }
    if expect_close {
        return Err(Error::damaged("Unclosed Valve KeyValues object"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{parse, Value};

    #[test]
    fn library_folders_fixture_shape() {
        let text =
            r#""libraryfolders" { "0" "C:\\Steam" "1" { "path" "D:\\Games\\SteamLibrary" "apps" { "41700" "1" } } }"#;
        let root = parse(text).unwrap_or_else(|error| panic!("{error}"));
        let folders = root.get_object("LIBRARYFOLDERS").unwrap_or_else(|| panic!("folders"));
        assert_eq!(folders.get_string("0"), Some("C:\\Steam"));
        let modern = folders.get_object("1").unwrap_or_else(|| panic!("modern"));
        assert_eq!(modern.get_string("path"), Some("D:\\Games\\SteamLibrary"));
        let apps = modern.get_object("apps").unwrap_or_else(|| panic!("apps"));
        assert_eq!(apps.get_string("41700"), Some("1"));
    }

    #[test]
    fn comments_and_reference_escapes() {
        let node = parse("// hello\n key \"raw\\q\\\\slash\" ").unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(node.get_string("key"), Some("raw\\q\\slash"));
    }

    #[test]
    fn duplicate_exact_key_replaces_in_place() {
        let node = parse("a 1 b 2 a 3").unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(node.children().len(), 2);
        assert_eq!(node.get_string("a"), Some("3"));
    }

    #[test]
    fn rejects_unterminated_and_unexpected_close() {
        assert!(parse("\"unterminated").is_err());
        assert!(parse("}").is_err());
        assert!(parse("a {").is_err());
    }

    #[test]
    fn rejects_depth_bomb() {
        let mut text = String::new();
        let mut depth = 0_usize;
        while depth <= 65 {
            text.push_str("x {");
            depth = depth.checked_add(1).unwrap_or(66);
        }
        assert!(parse(&text).is_err());
    }

    #[test]
    fn value_enum_is_publicly_usable() {
        let node = parse("a { b c }").unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(node.get("a"), Some(Value::Object(_))));
    }
}
