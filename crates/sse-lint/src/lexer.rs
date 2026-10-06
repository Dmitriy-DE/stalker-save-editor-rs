//! Native Lua 5.1 lexer for game script static checks.
//!
//! Operates over raw bytes (Windows-1251 or UTF-8 text) without panics,
//! tracking 1-based line and column offsets.

/// Token kind produced by the Lua lexer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// Keyword (`function`, `local`, `if`, etc.)
    Keyword(String),
    /// Identifier name
    Identifier(String),
    /// String literal content
    StringLiteral(String),
    /// Numeric literal as string
    NumberLiteral(String),
    /// Punctuation or operator
    Symbol(String),
    /// Line comment
    LineComment(String),
    /// Block comment
    BlockComment(String),
    /// End of input
    Eof,
}

/// A token with its source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Token kind
    pub kind: TokenKind,
    /// 1-based line number
    pub line: usize,
    /// 1-based column number
    pub column: usize,
    /// Byte span in source: start offset
    pub start: usize,
    /// Byte span in source: end offset
    pub end: usize,
}

/// Lua 5.1 keywords.
const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil", "not", "or",
    "repeat", "return", "then", "true", "until", "while",
];

/// Checks if an identifier is a Lua keyword.
#[must_use]
pub fn is_keyword(ident: &str) -> bool {
    KEYWORDS.contains(&ident)
}

/// Streaming or buffered Lua lexer.
pub struct LuaLexer<'a> {
    source: &'a [u8],
    pos: usize,
    line: usize,
    column: usize,
    include_comments: bool,
}

impl<'a> LuaLexer<'a> {
    /// Creates a new lexer for the given byte slice.
    #[must_use]
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            pos: 0,
            line: 1,
            column: 1,
            include_comments: false,
        }
    }

    /// Peeks the byte at current position without advancing.
    fn peek(&self) -> Option<u8> {
        self.source.get(self.pos).copied()
    }

    /// Peeks the byte at offset `offset` from current position.
    fn peek_offset(&self, offset: usize) -> Option<u8> {
        self.pos
            .checked_add(offset)
            .and_then(|idx| self.source.get(idx).copied())
    }

    /// Advances the cursor by one byte, updating line and column counters.
    fn advance(&mut self) -> Option<u8> {
        if let Some(&b) = self.source.get(self.pos) {
            self.pos = self.pos.saturating_add(1);
            if b == b'\n' {
                self.line = self.line.saturating_add(1);
                self.column = 1;
            } else {
                self.column = self.column.saturating_add(1);
            }
            Some(b)
        } else {
            None
        }
    }

    /// Skips horizontal whitespace and newlines.
    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' || b == 0x0C {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Scans a long bracket delimiter level: `[` followed by zero or more `=` followed by `[`.
    /// Returns `Some(level)` if a valid opening bracket was found and consumes it.
    fn scan_open_bracket(&mut self) -> Option<usize> {
        if self.peek() != Some(b'[') {
            return None;
        }

        let mut offset = 1;
        let mut level: usize = 0;
        while let Some(b) = self.peek_offset(offset) {
            if b == b'=' {
                level = level.saturating_add(1);
                offset = offset.saturating_add(1);
            } else if b == b'[' {
                // Found opening bracket of level `level`
                for _ in 0..offset.saturating_add(1) {
                    self.advance();
                }
                return Some(level);
            } else {
                return None;
            }
        }
        None
    }

    /// Scans until the closing long bracket `]` followed by `level` `=` and `]`.
    fn scan_close_bracket_content(&mut self, level: usize) -> String {
        let mut content = Vec::new();
        // Lua rule: if the first character after `[=...[` is newline, it is ignored
        if let Some(b'\n') = self.peek() {
            self.advance();
        } else if let (Some(b'\r'), Some(b'\n')) = (self.peek(), self.peek_offset(1)) {
            self.advance();
            self.advance();
        }

        while let Some(b) = self.peek() {
            if b == b']' {
                let mut matches = true;
                for i in 1..=level {
                    if self.peek_offset(i) != Some(b'=') {
                        matches = false;
                        break;
                    }
                }
                if matches {
                    let close_offset = level.saturating_add(1);
                    if self.peek_offset(close_offset) == Some(b']') {
                        // Found matching close bracket
                        let total = level.saturating_add(2);
                        for _ in 0..total {
                            self.advance();
                        }
                        return String::from_utf8_lossy(&content).to_string();
                    }
                }
            }
            if let Some(c) = self.advance() {
                content.push(c);
            }
        }
        String::from_utf8_lossy(&content).to_string()
    }

    /// Scans a single or double-quoted string literal.
    fn scan_quoted_string(&mut self, quote: u8) -> String {
        let mut content = Vec::new();
        self.advance(); // consume opening quote

        while let Some(b) = self.peek() {
            if b == quote {
                self.advance(); // consume closing quote
                break;
            }
            if b == b'\\' {
                self.advance();
                if let Some(esc) = self.advance() {
                    content.push(b'\\');
                    content.push(esc);
                }
            } else if b == b'\n' || b == b'\r' {
                // Unterminated string on current line
                break;
            } else {
                self.advance();
                content.push(b);
            }
        }
        String::from_utf8_lossy(&content).to_string()
    }

    /// Scans an identifier or keyword.
    fn scan_identifier_or_keyword(&mut self) -> (TokenKind, String) {
        let mut bytes = Vec::new();
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.advance();
                bytes.push(b);
            } else {
                break;
            }
        }
        let text = String::from_utf8_lossy(&bytes).to_string();
        if is_keyword(&text) {
            (TokenKind::Keyword(text.clone()), text)
        } else {
            (TokenKind::Identifier(text.clone()), text)
        }
    }

    /// Scans a numeric literal.
    fn scan_number(&mut self) -> String {
        let mut bytes = Vec::new();
        let is_hex =
            self.peek() == Some(b'0') && (self.peek_offset(1) == Some(b'x') || self.peek_offset(1) == Some(b'X'));

        if is_hex {
            if let Some(b) = self.advance() {
                bytes.push(b);
            }
            if let Some(b) = self.advance() {
                bytes.push(b);
            }
            while let Some(b) = self.peek() {
                if b.is_ascii_hexdigit() {
                    self.advance();
                    bytes.push(b);
                } else {
                    break;
                }
            }
        } else {
            let mut seen_dot = false;
            let mut seen_exp = false;

            while let Some(b) = self.peek() {
                if b.is_ascii_digit() {
                    self.advance();
                    bytes.push(b);
                } else if b == b'.' && !seen_dot && !seen_exp {
                    // Check it's not a .. operator
                    if self.peek_offset(1) == Some(b'.') {
                        break;
                    }
                    seen_dot = true;
                    self.advance();
                    bytes.push(b);
                } else if (b == b'e' || b == b'E') && !seen_exp {
                    seen_exp = true;
                    self.advance();
                    bytes.push(b);
                    if let Some(next) = self.peek() {
                        if next == b'+' || next == b'-' {
                            self.advance();
                            bytes.push(next);
                        }
                    }
                } else {
                    break;
                }
            }
        }

        String::from_utf8_lossy(&bytes).to_string()
    }

    /// Scans the next token in the stream.
    pub fn next_token(&mut self) -> Token {
        loop {
            self.skip_whitespace();

            let start = self.pos;
            let line = self.line;
            let column = self.column;

            let Some(first) = self.peek() else {
                return Token {
                    kind: TokenKind::Eof,
                    line,
                    column,
                    start,
                    end: start,
                };
            };

            // Comments: `--`
            if first == b'-' && self.peek_offset(1) == Some(b'-') {
                self.advance();
                self.advance();

                // Check for block comment `--[`
                if let Some(level) = self.scan_open_bracket() {
                    let comment = self.scan_close_bracket_content(level);
                    if self.include_comments {
                        return Token {
                            kind: TokenKind::BlockComment(comment),
                            line,
                            column,
                            start,
                            end: self.pos,
                        };
                    }
                    continue;
                }

                // Line comment
                let mut comment = Vec::new();
                while let Some(b) = self.peek() {
                    if b == b'\n' || b == b'\r' {
                        break;
                    }
                    self.advance();
                    comment.push(b);
                }
                if self.include_comments {
                    return Token {
                        kind: TokenKind::LineComment(String::from_utf8_lossy(&comment).to_string()),
                        line,
                        column,
                        start,
                        end: self.pos,
                    };
                }
                continue;
            }

            // Long string literals: `[[...]]` or `[=[...]=]`
            if first == b'[' {
                let saved_pos = self.pos;
                let saved_line = self.line;
                let saved_col = self.column;
                if let Some(level) = self.scan_open_bracket() {
                    let content = self.scan_close_bracket_content(level);
                    return Token {
                        kind: TokenKind::StringLiteral(content),
                        line: saved_line,
                        column: saved_col,
                        start,
                        end: self.pos,
                    };
                }
                // Not a long bracket, restore and handle as normal `[`
                self.pos = saved_pos;
                self.line = saved_line;
                self.column = saved_col;
            }

            // Quoted strings: '...' or "..."
            if first == b'\'' || first == b'"' {
                let str_val = self.scan_quoted_string(first);
                return Token {
                    kind: TokenKind::StringLiteral(str_val),
                    line,
                    column,
                    start,
                    end: self.pos,
                };
            }

            // Identifiers and keywords
            if first.is_ascii_alphabetic() || first == b'_' {
                let (kind, _) = self.scan_identifier_or_keyword();
                return Token {
                    kind,
                    line,
                    column,
                    start,
                    end: self.pos,
                };
            }

            // Numbers: `0-9` or `.0-9`
            if first.is_ascii_digit() || (first == b'.' && self.peek_offset(1).is_some_and(|b| b.is_ascii_digit())) {
                let num_val = self.scan_number();
                return Token {
                    kind: TokenKind::NumberLiteral(num_val),
                    line,
                    column,
                    start,
                    end: self.pos,
                };
            }

            // Multi-character symbols: `==`, `~=`, `<=`, `>=`, `..`, `...`
            if let Some(two) = self.peek_offset(1) {
                let pair = [first, two];
                if pair == *b"==" || pair == *b"~=" || pair == *b"<=" || pair == *b">=" {
                    self.advance();
                    self.advance();
                    return Token {
                        kind: TokenKind::Symbol(String::from_utf8_lossy(&pair).to_string()),
                        line,
                        column,
                        start,
                        end: self.pos,
                    };
                }
                if pair == *b".." {
                    self.advance();
                    self.advance();
                    if self.peek() == Some(b'.') {
                        self.advance();
                        return Token {
                            kind: TokenKind::Symbol("...".to_string()),
                            line,
                            column,
                            start,
                            end: self.pos,
                        };
                    }
                    return Token {
                        kind: TokenKind::Symbol("..".to_string()),
                        line,
                        column,
                        start,
                        end: self.pos,
                    };
                }
            }

            // Single-character symbol
            self.advance();
            let sym = String::from_utf8_lossy(&[first]).to_string();
            return Token {
                kind: TokenKind::Symbol(sym),
                line,
                column,
                start,
                end: self.pos,
            };
        }
    }

    /// Tokenizes the entire source into a vector of tokens.
    #[must_use]
    pub fn tokenize_all(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token();
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        tokens
    }
}
