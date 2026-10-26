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
        let previous = self
            .at
            .checked_sub(1)
            .and_then(|at| self.src.as_bytes().get(at))
            .copied();
        if (byte == b'\r' && previous != Some(b'\n')) || (byte == b'\n' && previous != Some(b'\r'))
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
            if byte != b'\\' {
                self.bump();
                continue;
            }
            self.bump();
            let escape = self
                .peek(0)
                .ok_or_else(|| self.error("unfinished escape"))?;
            match escape {
                b'a' | b'b' | b'f' | b'n' | b'r' | b't' | b'v' | b'\\' | b'\'' | b'"' => {
                    self.bump();
                }
                b'\r' | b'\n' => {
                    // Escaped line breaks are allowed in a short Lua string.
                    self.bump();
                    if self.peek(0) == Some(if escape == b'\r' { b'\n' } else { b'\r' }) {
                        self.bump();
                    }
                }
                b'z' => {
                    // Lua's z escape consumes subsequent whitespace including lines.
                    self.bump();
                    while self.peek(0).is_some_and(|next| {
                        matches!(next, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
                    }) {
                        self.bump();
                    }
                }
                b'x' => {
                    self.bump();
                    for _ in 0..2 {
                        if !self.peek(0).is_some_and(|next| next.is_ascii_hexdigit()) {
                            return Err(self.error("hex escape requires exactly two digits"));
                        }
                        self.bump();
                    }
                }
                b'u' => {
                    self.bump();
                    if self.peek(0) != Some(b'{') {
                        return Err(self.error("Unicode escape requires opening brace"));
                    }
                    self.bump();
                    let mut value = 0u32;
                    let mut count = 0;
                    while let Some(next) = self.peek(0) {
                        if !next.is_ascii_hexdigit() {
                            break;
                        }
                        let digit = (next as char).to_digit(16).expect("hex digit");
                        value = value
                            .checked_mul(16)
                            .and_then(|v| v.checked_add(digit))
                            .ok_or_else(|| self.error("Unicode escape value too large"))?;
                        if value > 0x7fff_ffff {
                            return Err(self.error("Unicode escape value too large"));
                        }
                        count += 1;
                        self.bump();
                    }
                    if count == 0 || self.peek(0) != Some(b'}') {
                        return Err(self.error("unfinished Unicode escape"));
                    }
                    self.bump();
                }
                b'0'..=b'9' => {
                    let mut value = 0u32;
                    for _ in 0..3 {
                        let Some(next) = self.peek(0) else {
                            break;
                        };
                        if !next.is_ascii_digit() {
                            break;
                        }
                        value = value * 10 + u32::from(next - b'0');
                        self.bump();
                    }
                    if value > 255 {
                        return Err(self.error("decimal escape exceeds one byte"));
                    }
                }
                _ => return Err(self.error("invalid string escape")),
            }
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
        // Lua 5.4 does not split an adjacent identifier or extra radix
        // point from a numeral: 12abc, 0x1g and 1..2 are malformed.
        // Preserve the separate concatenation operator after whitespace.
        if self
            .peek(0)
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        {
            return Err(self.error("malformed number"));
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
/// Escape syntax is checked, but this scanner does not parse grammar or run bytecode.
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
        // The pinned Lua 5.4 C-locale lexer recognizes ASCII letters
        // and underscores in identifiers, not arbitrary UTF-8 bytes.
        let kind = if b.is_ascii_alphabetic() || b == b'_' {
            scan.bump();
            while scan
                .peek(0)
                .is_some_and(|x| x.is_ascii_alphanumeric() || x == b'_')
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
            spellings("local x=0x1.fp+2+42.5e-1 ... 1 .. 2"),
            [
                "local", "x", "=", "0x1.fp+2", "+", "42.5e-1", "...", "1", "..", "2"
            ]
        );
    }
    #[test]
    fn lua_54_rejects_numbers_touching_names_or_extra_radix_points() {
        for source in [
            "return 12abc",
            "return 0x1g",
            "return 1e3foo",
            "return 0x1p2stuff",
            "return 9_000",
            "return 1..2",
            "return 1...2",
            "return 3.4.5",
            "return .5abc",
        ] {
            let error = tokenize(source).unwrap_err();
            assert_eq!(error.message, "malformed number", "{source}");
        }
        let error = tokenize("local x=12abc").unwrap_err();
        assert_eq!((error.line, error.byte_offset), (1, 10));
    }

    #[test]
    fn non_ascii_numeric_suffix_reports_unexpected_character() {
        for source in ["return 7é", "return 1e2é", "return 0x1é", "return .5é"] {
            let error = tokenize(source).unwrap_err();
            assert_eq!(error.message, "unexpected character", "{source}");
        }
        assert_eq!(tokenize("return 7é").unwrap_err().byte_offset, 8);
        // ASCII identifier suffixes remain malformed numeral errors.
        assert_eq!(
            tokenize("return 7abc").unwrap_err().message,
            "malformed number"
        );
    }

    #[test]
    fn distinct_lua_numerals_and_spaced_concatenation_remain_valid() {
        assert_eq!(
            spellings("return 1 .. 2, 0x1.fp+2 + 3.e1 - .5, 0xe+1"),
            [
                "return", "1", "..", "2", ",", "0x1.fp+2", "+", "3.e1", "-", ".5", ",", "0xe", "+",
                "1",
            ]
        );
        assert_eq!(
            spellings("return 1e-2+0xA.p+2"),
            ["return", "1e-2", "+", "0xA.p+2"]
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
    fn lua_z_escape_skips_whitespace_and_tracks_line() {
        let source = "local s=\"a\\z   \n b\"\nlocal t=1";
        let tokens = tokenize(source).unwrap();
        assert_eq!(tokens[3].kind, Kind::ShortString);
        assert_eq!(tokens[4].spelling, "local");
        assert_eq!(tokens[4].line, 3);
    }

    #[test]
    fn lua_z_escape_skips_c_locale_vertical_tab_and_form_feed() {
        let tokens = tokenize("local s='A\\z\x0b\x0cB'\nreturn s").unwrap();
        assert_eq!(tokens[3].kind, Kind::ShortString);
        assert_eq!(tokens[4].spelling, "return");
        assert_eq!(tokens[4].line, 2);
    }

    #[test]
    fn supports_lua_short_string_escapes_and_normalizes_newline_count() {
        for source in [
            r#""\x41""#,
            r#""\u{1F600}""#,
            r#""\u{7FFFFFFF}""#,
            r#""\255""#,
            r#""\a\b\f\n\r\t\v\\\"\'""#,
            r#""\0012""#,
            "\"continued\\\nnext\"",
            "\"continued\\\r\nnext\"",
            "\"continued\\\n\rnext\"",
        ] {
            assert!(tokenize(source).is_ok(), "{source:?}");
        }
        let source = "\"a\\\n\rb\"\nlocal x=1";
        let tokens = tokenize(source).unwrap();
        assert_eq!(tokens[1].spelling, "local");
        assert_eq!(tokens[1].line, 3);
    }

    #[test]
    fn rejects_invalid_short_string_escapes_with_bounded_errors() {
        for source in [
            r#""\q""#,
            r#""\x""#,
            r#""\xG0""#,
            r#""\x1""#,
            r#""\u""#,
            r#""\u{}""#,
            r#""\u{GG}""#,
            r#""\u{80000000}""#,
            r#""\999""#,
            r#""\256""#,
        ] {
            assert!(tokenize(source).is_err(), "{source:?}");
        }
    }

    #[test]
    fn lua_c_locale_identifiers_are_ascii_only() {
        assert_eq!(
            spellings("local cafe_42 = 7"),
            ["local", "cafe_42", "=", "7"]
        );
        for source in [
            "local café = 42",
            "local α = 2",
            "return naïve",
            "local emoji😀 = 1",
        ] {
            let error = tokenize(source).unwrap_err();
            assert_eq!(error.message, "unexpected character", "{source}");
        }
        let error = tokenize("local café = 42").unwrap_err();
        assert_eq!((error.line, error.byte_offset), (1, 9));
        // UTF-8 remains valid source data inside quoted strings,
        // long strings and comments.
        assert_eq!(
            spellings("local cafe = \"café\" -- naïve"),
            ["local", "cafe", "=", "\"café\""]
        );
        assert_eq!(spellings("return [=[α😀]=]"), ["return", "[=[α😀]=]"]);
    }
}
