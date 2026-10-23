//! Experimental Ferrua-owned Lua lexical scanner. Not a replacement VM.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Identifier,
    Keyword,
    Number,
    ShortString,
    LongString,
    Symbol,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: Kind,
    pub spelling: &'a str,
    pub line: usize,
    pub byte_offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexError {
    pub line: usize,
    pub byte_offset: usize,
    pub message: &'static str,
}

impl fmt::Display for LexError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            out,
            "Lua lexer at line {}, byte {}: {}",
            self.line, self.byte_offset, self.message
        )
    }
}
impl std::error::Error for LexError {}

struct Scanner<'a> {
    src: &'a str,
    at: usize,
    line: usize,
}
impl Scanner<'_> {
    fn peek(&self, delta: usize) -> Option<u8> {
        self.src.as_bytes().get(self.at + delta).copied()
    }
    fn bump(&mut self) {
        let byte = self.peek(0).expect("within source");
        if byte == b'\r'
            || (byte == b'\n' && (self.at == 0 || self.src.as_bytes()[self.at - 1] != b'\r'))
        {
            self.line += 1;
        }
        self.at += 1;
    }
    fn error(&self, message: &'static str) -> LexError {
        LexError {
            line: self.line,
            byte_offset: self.at,
            message,
        }
    }
    fn long_open(&self) -> Option<(usize, usize)> {
        if self.peek(0) != Some(b'[') {
            return None;
        }
        let mut pos = self.at + 1;
        let bytes = self.src.as_bytes();
        while bytes.get(pos) == Some(&b'=') {
            pos += 1;
        }
        (bytes.get(pos) == Some(&b'[')).then_some((pos - self.at - 1, pos - self.at + 1))
    }
    fn long(&mut self) -> Result<(), LexError> {
        let (depth, open_len) = self
            .long_open()
            .ok_or_else(|| self.error("invalid long bracket"))?;
        for _ in 0..open_len {
            self.bump();
        }
        while self.peek(0).is_some() {
            if self.peek(0) == Some(b']') {
                let bytes = self.src.as_bytes();
                let mut close = self.at + 1;
                while bytes.get(close) == Some(&b'=') {
                    close += 1;
                }
                if close - self.at - 1 == depth && bytes.get(close) == Some(&b']') {
                    let end = close + 1;
                    while self.at < end {
                        self.bump();
                    }
                    return Ok(());
                }
            }
            self.bump();
        }
        Err(self.error("unfinished long bracket"))
    }
    fn quoted(&mut self) -> Result<(), LexError> {
        let quote = self.peek(0).expect("quote");
        self.bump();
        while let Some(byte) = self.peek(0) {
            if byte == quote {
                self.bump();
                return Ok(());
            }
            if byte == b'\r' || byte == b'\n' {
                return Err(self.error("unfinished short string"));
            }
            if byte == b'\\' {
                self.bump();
                if self.peek(0).is_none() {
                    return Err(self.error("unfinished escape"));
                }
            }
            self.bump();
        }
        Err(self.error("unfinished short string"))
    }
    fn digits(&mut self, hex: bool) -> usize {
        let start = self.at;
        while self.peek(0).is_some_and(|b| {
            if hex {
                b.is_ascii_hexdigit()
            } else {
                b.is_ascii_digit()
            }
        }) {
            self.bump();
        }
        self.at - start
    }
    fn number(&mut self) -> Result<(), LexError> {
        let hex = self.peek(0) == Some(b'0') && matches!(self.peek(1), Some(b'x' | b'X'));
        if hex {
            self.bump();
            self.bump();
        }
        let mut count = self.digits(hex);
        if self.peek(0) == Some(b'.') && self.peek(1) != Some(b'.') {
            self.bump();
            count += self.digits(hex);
        }
        if count == 0 {
            return Err(self.error("number has no digits"));
        }
        let exps = if hex { *b"pP" } else { *b"eE" };
        if self.peek(0).is_some_and(|b| exps.contains(&b)) {
            self.bump();
            if matches!(self.peek(0), Some(b'+' | b'-')) {
                self.bump();
            }
            if self.digits(false) == 0 {
                return Err(self.error("malformed exponent"));
            }
        }
        Ok(())
    }
}

fn keyword(s: &str) -> bool {
    matches!(
        s,
        "and"
            | "break"
            | "do"
            | "else"
            | "elseif"
            | "end"
            | "false"
            | "for"
            | "function"
            | "goto"
            | "if"
            | "in"
            | "local"
            | "nil"
            | "not"
            | "or"
            | "repeat"
            | "return"
            | "then"
            | "true"
            | "until"
            | "while"
    )
}

/// Tokenize a documented Lua lexical subset, preserving raw spelling and offsets.
/// This does not parse grammar, validate string escapes, or run Lua bytecode.
pub fn tokenize(source: &str) -> Result<Vec<Token<'_>>, LexError> {
    let mut scan = Scanner {
        src: source,
        at: 0,
        line: 1,
    };
    let mut tokens = Vec::new();
    if source.starts_with("#!") {
        while scan.peek(0).is_some_and(|b| b != b'\n' && b != b'\r') {
            scan.bump();
        }
    }
    while let Some(b) = scan.peek(0) {
        if b.is_ascii_whitespace() {
            scan.bump();
            continue;
        }
        if b == b'-' && scan.peek(1) == Some(b'-') {
            scan.bump();
            scan.bump();
            if scan.long_open().is_some() {
                scan.long()?;
            } else {
                while scan.peek(0).is_some_and(|x| x != b'\n' && x != b'\r') {
                    scan.bump();
                }
            }
            continue;
        }
        let start = scan.at;
        let line = scan.line;
        let kind = if b.is_ascii_alphabetic() || b == b'_' || b >= 128 {
            scan.bump();
            while scan
                .peek(0)
                .is_some_and(|x| x.is_ascii_alphanumeric() || x == b'_' || x >= 128)
            {
                scan.bump();
            }
            if keyword(&source[start..scan.at]) {
                Kind::Keyword
            } else {
                Kind::Identifier
            }
        } else if b == b'\'' || b == b'"' {
            scan.quoted()?;
            Kind::ShortString
        } else if b == b'[' && scan.long_open().is_some() {
            scan.long()?;
            Kind::LongString
        } else if b.is_ascii_digit()
            || (b == b'.' && scan.peek(1).is_some_and(|n| n.is_ascii_digit()))
        {
            scan.number()?;
            Kind::Number
        } else if b"+-*/%^#&~|<>=(){}[];:,.".contains(&b) {
            let tail = &source.as_bytes()[start..];
            let width = if tail.starts_with(b"...") {
                3
            } else if [
                b"..".as_slice(),
                b"//",
                b"<<",
                b">>",
                b"==",
                b"~=",
                b"<=",
                b">=",
                b"::",
            ]
            .iter()
            .any(|op| tail.starts_with(op))
            {
                2
            } else {
                1
            };
            for _ in 0..width {
                scan.bump();
            }
            Kind::Symbol
        } else {
            return Err(scan.error("unexpected character"));
        };
        tokens.push(Token {
            kind,
            spelling: &source[start..scan.at],
            line,
            byte_offset: start,
        });
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spellings(s: &str) -> Vec<&str> {
        tokenize(s)
            .unwrap()
            .into_iter()
            .map(|x| x.spelling)
            .collect()
    }
    #[test]
    fn recognizes_keywords_hex_literals_and_concat() {
        assert_eq!(
            spellings("local x=0x1.fp+2+42.5e-1 ... 1..2"),
            [
                "local", "x", "=", "0x1.fp+2", "+", "42.5e-1", "...", "1", "..", "2"
            ]
        );
    }
    #[test]
    fn long_brackets_and_comments_preserve_string_text() {
        assert_eq!(
            spellings("--[==[hi\nworld]==]\nlocal s=[=[moon\nsun]=]"),
            ["local", "s", "=", "[=[moon\nsun]=]"]
        );
    }
    #[test]
    fn shebang_and_crlf_track_lines() {
        let tokens = tokenize("#!/usr/bin/env fer\r\nlocal x=1\r\nprint(x)").unwrap();
        assert_eq!(tokens[0].line, 2);
        assert_eq!(tokens[4].line, 3);
    }
    #[test]
    fn malformed_literals_report_errors() {
        for source in ["'unterminated", "[==[unfinished ]=]", "0x", "1e+"] {
            assert!(tokenize(source).is_err(), "{source}");
        }
    }
    #[test]
    fn escaped_and_multibyte_identifiers_keep_spelling() {
        assert_eq!(
            spellings("local café = \"\\\"quoted\\\"\""),
            ["local", "café", "=", "\"\\\"quoted\\\"\""]
        );
    }
}
