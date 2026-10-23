//! Experimental Lua expression parser using the Ferrua-owned lexer.
//! The CLI interpreter still uses the independently maintained VM.

use crate::lex::{self, Kind, Token};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr<'src> {
    Literal(&'src str),
    Name(&'src str),
    Group(Box<Expr<'src>>),
    Unary {
        operator: &'src str,
        operand: Box<Expr<'src>>,
    },
    Binary {
        operator: &'src str,
        left: Box<Expr<'src>>,
        right: Box<Expr<'src>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "Lua expression at byte {}: {}", self.offset, self.message)
    }
}
impl std::error::Error for ParseError {}

struct Parser<'tokens, 'src> {
    source_len: usize,
    tokens: &'tokens [Token<'src>],
    at: usize,
}

impl<'tokens, 'src> Parser<'tokens, 'src> {
    fn peek(&self) -> Option<&Token<'src>> {
        self.tokens.get(self.at)
    }

    fn error(&self, message: &'static str) -> ParseError {
        ParseError {
            offset: self.peek().map_or(self.source_len, |token| token.byte_offset),
            message,
        }
    }

    fn take(&mut self) -> Option<&Token<'src>> {
        let token = self.tokens.get(self.at)?;
        self.at += 1;
        Some(token)
    }

    fn expression(&mut self, min_power: u8) -> Result<Expr<'src>, ParseError> {
        let token = self.take().ok_or_else(|| self.error("expected expression"))?;
        let mut lhs = match (token.kind, token.spelling) {
            (Kind::Number | Kind::ShortString | Kind::LongString, spelling)
            | (Kind::Keyword, spelling @ ("true" | "false" | "nil")) => Expr::Literal(spelling),
            (Kind::Identifier, spelling) => Expr::Name(spelling),
            (Kind::Symbol, "(") => {
                let expr = self.expression(0)?;
                if self.peek().map(|token| token.spelling) != Some(")") {
                    return Err(self.error("expected closing parenthesis"));
                }
                self.take();
                Expr::Group(Box::new(expr))
            }
            (Kind::Symbol, operator @ ("-" | "#" | "~")) | (Kind::Keyword, operator @ "not") => {
                let operand = self.expression(22)?;
                Expr::Unary {
                    operator,
                    operand: Box::new(operand),
                }
            }
            _ => {
                return Err(ParseError {
                    offset: token.byte_offset,
                    message: "unexpected token in expression",
                });
            }
        };
        while let Some(token) = self.peek() {
            let Some((left_power, right_power)) = infix_power(token.spelling) else {
                break;
            };
            if left_power < min_power {
                break;
            }
            let operator = token.spelling;
            self.take();
            let rhs = self.expression(right_power)?;
            lhs = Expr::Binary {
                operator,
                left: Box::new(lhs),
                right: Box::new(rhs),
            };
        }
        Ok(lhs)
    }
}

// Lua 5.4: exponentiation binds above unary; concat and exponent are
// right-associative. All other binary operators associate left-to-right.
fn infix_power(op: &str) -> Option<(u8, u8)> {
    let level = match op {
        "or" => 1,
        "and" => 2,
        "<" | ">" | "<=" | ">=" | "~=" | "==" => 3,
        "|" => 4,
        "~" => 5,
        "&" => 6,
        "<<" | ">>" => 7,
        ".." => 8,
        "+" | "-" => 9,
        "*" | "/" | "//" | "%" => 10,
        "^" => 12,
        _ => return None,
    };
    let power = level * 2;
    if matches!(op, ".." | "^") {
        Some((power, power))
    } else {
        Some((power, power + 1))
    }
}

/// Parse exactly one expression from a Lua 5.4 source substring.
///
/// This experimental subset excludes table constructors, indexing, field
/// selection, function calls, anonymous functions and entire statements.
pub fn parse_expression(source: &str) -> Result<Expr<'_>, ParseError> {
    let tokens = lex::tokenize(source).map_err(|err| ParseError {
        offset: err.byte_offset,
        message: err.message,
    })?;
    let mut parser = Parser {
        source_len: source.len(),
        tokens: &tokens,
        at: 0,
    };
    let result = parser.expression(0)?;
    if parser.peek().is_some() {
        return Err(parser.error("unexpected trailing token"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(expr: &Expr<'_>) -> String {
        match expr {
            Expr::Literal(text) | Expr::Name(text) => (*text).into(),
            Expr::Group(operand) => format!("({})", shape(operand)),
            Expr::Unary { operator, operand } => format!("({operator} {})", shape(operand)),
            Expr::Binary { operator, left, right } => {
                format!("({operator} {} {})", shape(left), shape(right))
            }
        }
    }

    #[test]
    fn arithmetic_precedence_and_left_associativity() {
        let parsed = parse_expression("a + b * c - d").unwrap();
        assert_eq!(shape(&parsed), "(- (+ a (* b c)) d)");
    }

    #[test]
    fn exponentiation_binds_above_unary_and_associates_right() {
        assert_eq!(shape(&parse_expression("-2^3^4").unwrap()), "(- (^ 2 (^ 3 4)))");
        assert_eq!(shape(&parse_expression("2^-3").unwrap()), "(^ 2 (- 3))");
    }

    #[test]
    fn concatenation_is_right_associative() {
        assert_eq!(shape(&parse_expression("a .. b .. c").unwrap()), "(.. a (.. b c))");
    }

    #[test]
    fn lua_logical_precedence() {
        let parsed = parse_expression("not a == b or c and d").unwrap();
        assert_eq!(shape(&parsed), "(or (== (not a) b) (and c d))");
    }

    #[test]
    fn bitwise_and_arithmetic_precedence() {
        let parsed = parse_expression("1 << 2 + 3 & 7").unwrap();
        assert_eq!(shape(&parsed), "(& (<< 1 (+ 2 3)) 7)");
    }

    #[test]
    fn grouping_remains_explicit_in_ast() {
        assert_eq!(shape(&parse_expression("(a+b)*c").unwrap()), "(* ((+ a b)) c)");
    }

    #[test]
    fn literal_and_name_tokens() {
        assert_eq!(shape(&parse_expression("true").unwrap()), "true");
        assert_eq!(shape(&parse_expression("'moon'").unwrap()), "'moon'");
        assert_eq!(shape(&parse_expression("5.2").unwrap()), "5.2");
    }

    #[test]
    fn invalid_expressions_have_byte_offsets() {
        assert!(parse_expression("1 +").is_err());
        assert_eq!(parse_expression("1 + * 2").unwrap_err().offset, 4);
        assert!(parse_expression("a b").unwrap_err().message.contains("trailing"));
        assert!(parse_expression("(a+b").unwrap_err().message.contains("closing"));
        assert!(parse_expression("").is_err());
    }
}
