//! Experimental Ferrua-owned Lua expression parser (not full Lua syntax).

use crate::lex::{Kind, LexError, Token, tokenize};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr<'a> {
    Atom(&'a str),
    Unary {
        op: &'a str,
        right: Box<Expr<'a>>,
    },
    Binary {
        op: &'a str,
        left: Box<Expr<'a>>,
        right: Box<Expr<'a>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub byte_offset: usize,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "Lua expression at line {}, byte {}: {}", self.line, self.byte_offset, self.message)
    }
}
impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(err: LexError) -> Self {
        Self { line: err.line, byte_offset: err.byte_offset, message: err.message }
    }
}

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    index: usize,
    length: usize,
}

fn binding_power(op: &str) -> Option<(u8, u8)> {
    Some(match op {
        "or" => (1, 2),
        "and" => (3, 4),
        "<" | ">" | "<=" | ">=" | "~=" | "==" => (5, 6),
        "|" => (7, 8),
        "~" => (9, 10),
        "&" => (11, 12),
        "<<" | ">>" => (13, 14),
        ".." => (15, 15),
        "+" | "-" => (17, 18),
        "*" | "/" | "//" | "%" => (19, 20),
        "^" => (23, 23),
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token<'a>> {
        self.tokens.get(self.index)
    }
    fn pop(&mut self) -> Option<Token<'a>> {
        let token = self.tokens.get(self.index).cloned();
        if token.is_some() { self.index += 1; }
        token
    }
    fn error(&self, message: &'static str) -> ParseError {
        let next = self.peek();
        ParseError {
            line: next.map_or(1, |t| t.line),
            byte_offset: next.map_or(self.length, |t| t.byte_offset),
            message,
        }
    }
    fn prefix(&mut self) -> Result<Expr<'a>, ParseError> {
        let Some(token) = self.pop() else {
            return Err(self.error("expected an expression"));
        };
        match token.spelling {
            "-" | "not" | "#" | "~" => {
                let rhs = self.expression(21)?;
                Ok(Expr::Unary { op: token.spelling, right: Box::new(rhs) })
            }
            "(" => {
                let inner = self.expression(0)?;
                if !self.peek().is_some_and(|t| t.spelling == ")") {
                    return Err(self.error("expected closing parenthesis"));
                }
                self.pop();
                Ok(inner)
            }
            _ if matches!(token.kind, Kind::Identifier | Kind::Number | Kind::ShortString | Kind::LongString)
                || matches!(token.spelling, "true" | "false" | "nil") =>
            {
                Ok(Expr::Atom(token.spelling))
            }
            _ => Err(ParseError {
                line: token.line,
                byte_offset: token.byte_offset,
                message: "expected an expression",
            }),
        }
    }
    fn expression(&mut self, min_bp: u8) -> Result<Expr<'a>, ParseError> {
        let mut left = self.prefix()?;
        while let Some(op) = self.peek().map(|t| t.spelling) {
            let Some((l_bp, r_bp)) = binding_power(op) else { break; };
            if l_bp < min_bp { break; }
            self.pop();
            let right = self.expression(r_bp)?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }
}

/// Parse only a standalone Lua expression into a Ferrua-owned syntax tree.
/// Statement grammar, calls, table constructors, indexing, and lambda bodies
/// are not implemented. The working fer interpreter is unaffected.
pub fn parse_expression(source: &str) -> Result<Expr<'_>, ParseError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser { tokens, index: 0, length: source.len() };
    let result = parser.expression(0)?;
    if parser.peek().is_some() {
        return Err(parser.error("unexpected trailing token"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(expr: &Expr<'_>) -> String {
        match expr {
            Expr::Atom(s) => (*s).to_owned(),
            Expr::Unary { op, right } => format!("({op} {})", show(right)),
            Expr::Binary { op, left, right } => {
                format!("({op} {} {})", show(left), show(right))
            }
        }
    }

    #[test]
    fn multiplication_binds_before_addition() {
        assert_eq!(show(&parse_expression("1+2*3").unwrap()), "(+ 1 (* 2 3))");
    }

    #[test]
    fn lua_power_binds_tighter_than_unary_minus() {
        assert_eq!(show(&parse_expression("-2^2").unwrap()), "(- (^ 2 2))");
    }

    #[test]
    fn right_associative_power_and_concatenation() {
        assert_eq!(show(&parse_expression("2^3^4").unwrap()), "(^ 2 (^ 3 4))");
        assert_eq!(show(&parse_expression("'a'..'b'..'c'").unwrap()),
            "(.. 'a' (.. 'b' 'c'))");
    }

    #[test]
    fn logical_comparison_and_grouping() {
        assert_eq!(
            show(&parse_expression("not (a+b > 4) or false").unwrap()),
            "(or (not (> (+ a b) 4)) false)"
        );
    }

    #[test]
    fn rejects_statement_syntax_and_unfinished_expressions() {
        for source in ["local x=2", "x+", "(1+2", "1 2", "foo()"] {
            assert!(parse_expression(source).is_err(), "{source}");
        }
    }

    #[test]
    fn reports_lexer_errors_without_panic() {
        assert!(parse_expression("'unfinished").is_err());
        assert!(parse_expression("0x").is_err());
    }
}
