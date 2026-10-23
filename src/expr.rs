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
    Call {
        callee: Box<Expr<'a>>,
        arguments: Vec<Expr<'a>>,
    },
    Field {
        object: Box<Expr<'a>>,
        name: &'a str,
    },
    Index {
        object: Box<Expr<'a>>,
        key: Box<Expr<'a>>,
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
        write!(
            out,
            "Lua expression at line {}, byte {}: {}",
            self.line, self.byte_offset, self.message
        )
    }
}
impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(err: LexError) -> Self {
        Self {
            line: err.line,
            byte_offset: err.byte_offset,
            message: err.message,
        }
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
        if token.is_some() {
            self.index += 1;
        }
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
                Ok(Expr::Unary {
                    op: token.spelling,
                    right: Box::new(rhs),
                })
            }
            "(" => {
                let inner = self.expression(0)?;
                if !self.peek().is_some_and(|t| t.spelling == ")") {
                    return Err(self.error("expected closing parenthesis"));
                }
                self.pop();
                Ok(inner)
            }
            _ if matches!(
                token.kind,
                Kind::Identifier | Kind::Number | Kind::ShortString | Kind::LongString
            ) || matches!(token.spelling, "true" | "false" | "nil") =>
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
    fn postfix(&mut self, mut left: Expr<'a>) -> Result<Expr<'a>, ParseError> {
        loop {
            match self.peek().map(|token| token.spelling) {
                Some("(") => {
                    self.pop();
                    let mut arguments = Vec::new();
                    if self.peek().map(|token| token.spelling) != Some(")") {
                        loop {
                            arguments.push(self.expression(0)?);
                            if self.peek().map(|token| token.spelling) != Some(",") {
                                break;
                            }
                            self.pop();
                        }
                    }
                    if self.peek().map(|token| token.spelling) != Some(")") {
                        return Err(self.error("expected closing parenthesis in call"));
                    }
                    self.pop();
                    left = Expr::Call {
                        callee: Box::new(left),
                        arguments,
                    };
                }
                Some(".") => {
                    self.pop();
                    let token = self
                        .pop()
                        .ok_or_else(|| self.error("expected field name"))?;
                    if token.kind != Kind::Identifier {
                        return Err(ParseError {
                            line: token.line,
                            byte_offset: token.byte_offset,
                            message: "expected identifier after dot",
                        });
                    }
                    left = Expr::Field {
                        object: Box::new(left),
                        name: token.spelling,
                    };
                }
                Some("[") => {
                    self.pop();
                    let key = self.expression(0)?;
                    if self.peek().map(|token| token.spelling) != Some("]") {
                        return Err(self.error("expected closing bracket in index"));
                    }
                    self.pop();
                    left = Expr::Index {
                        object: Box::new(left),
                        key: Box::new(key),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }
    fn expression(&mut self, min_bp: u8) -> Result<Expr<'a>, ParseError> {
        let mut left = self.prefix()?;
        left = self.postfix(left)?;
        while let Some(op) = self.peek().map(|t| t.spelling) {
            let Some((l_bp, r_bp)) = binding_power(op) else {
                break;
            };
            if l_bp < min_bp {
                break;
            }
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
/// Statement grammar, method calls, table constructors, and lambda bodies
/// are not implemented. The working fer interpreter is unaffected.
pub fn parse_expression(source: &str) -> Result<Expr<'_>, ParseError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser {
        tokens,
        index: 0,
        length: source.len(),
    };
    let result = parser.expression(0)?;
    if parser.peek().is_some() {
        return Err(parser.error("unexpected trailing token"));
    }
    Ok(result)
}

/// A deliberately narrow first-party Lua statement subset.
///
/// A complete Lua chunk parser must additionally handle block scopes,
/// conditions, loops, function definitions, local attributes, labels, goto,
/// table constructors and the full call syntax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement<'a> {
    Local {
        names: Vec<&'a str>,
        initializers: Vec<Expr<'a>>,
    },
    Return(Vec<Expr<'a>>),
    Assign {
        targets: Vec<Expr<'a>>,
        values: Vec<Expr<'a>>,
    },
    Call(Expr<'a>),
}

impl<'a> Parser<'a> {
    fn name(&mut self) -> Result<&'a str, ParseError> {
        let token = self
            .pop()
            .ok_or_else(|| self.error("expected identifier"))?;
        if token.kind != Kind::Identifier {
            return Err(ParseError {
                line: token.line,
                byte_offset: token.byte_offset,
                message: "expected identifier",
            });
        }
        Ok(token.spelling)
    }

    fn expressions(&mut self) -> Result<Vec<Expr<'a>>, ParseError> {
        let mut values = vec![self.expression(0)?];
        while self.peek().is_some_and(|token| token.spelling == ",") {
            self.pop();
            values.push(self.expression(0)?);
        }
        Ok(values)
    }

    fn statement(&mut self) -> Result<Statement<'a>, ParseError> {
        if self.peek().is_some_and(|token| token.spelling == "local") {
            self.pop();
            let mut names = vec![self.name()?];
            while self.peek().is_some_and(|token| token.spelling == ",") {
                self.pop();
                names.push(self.name()?);
            }
            let initializers = if self.peek().is_some_and(|token| token.spelling == "=") {
                self.pop();
                self.expressions()?
            } else {
                Vec::new()
            };
            return Ok(Statement::Local {
                names,
                initializers,
            });
        }
        if self.peek().is_some_and(|token| token.spelling == "return") {
            self.pop();
            let values = if self.peek().is_none()
                || self.peek().is_some_and(|token| token.spelling == ";")
            {
                Vec::new()
            } else {
                self.expressions()?
            };
            return Ok(Statement::Return(values));
        }
        let first = self.expression(0)?;
        if self
            .peek()
            .is_some_and(|token| matches!(token.spelling, "," | "="))
        {
            let mut targets = vec![first];
            while self.peek().is_some_and(|token| token.spelling == ",") {
                self.pop();
                targets.push(self.expression(0)?);
            }
            if !self.peek().is_some_and(|token| token.spelling == "=") {
                return Err(self.error("expected assignment equals"));
            }
            for target in &targets {
                let valid = match target {
                    Expr::Atom(text) => tokenize(text).is_ok_and(|tokens| {
                        tokens.len() == 1 && tokens[0].kind == Kind::Identifier
                    }),
                    Expr::Field { .. } | Expr::Index { .. } => true,
                    _ => false,
                };
                if !valid {
                    return Err(self.error("invalid assignment target"));
                }
            }
            self.pop();
            return Ok(Statement::Assign {
                targets,
                values: self.expressions()?,
            });
        }
        if matches!(first, Expr::Call { .. }) {
            Ok(Statement::Call(first))
        } else {
            Err(self.error("only calls can be standalone expression statements"))
        }
    }
}

/// Parse a limited Lua chunk consisting of semicolon-delimited local
/// declarations, assignments, call statements and an optional final return.
/// This API does not imply complete Lua statement grammar or scope support.
pub fn parse_chunk(source: &str) -> Result<Vec<Statement<'_>>, ParseError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser {
        tokens,
        index: 0,
        length: source.len(),
    };
    let mut result = Vec::new();
    while parser.peek().is_some() {
        if parser.peek().is_some_and(|token| token.spelling == ";") {
            parser.pop();
            continue;
        }
        let statement = parser.statement()?;
        let terminal = matches!(statement, Statement::Return(_));
        result.push(statement);
        if parser.peek().is_some_and(|token| token.spelling == ";") {
            parser.pop();
        } else if parser.peek().is_some() {
            return Err(parser.error("expected statement separator"));
        }
        if terminal && parser.peek().is_some() {
            return Err(parser.error("return must be final statement"));
        }
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
            Expr::Call { callee, arguments } => {
                let text = arguments.iter().map(show).collect::<Vec<_>>().join(" ");
                format!("(call {} [{}])", show(callee), text)
            }
            Expr::Field { object, name } => format!("(field {} {name})", show(object)),
            Expr::Index { object, key } => format!("(index {} {})", show(object), show(key)),
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
        assert_eq!(
            show(&parse_expression("'a'..'b'..'c'").unwrap()),
            "(.. 'a' (.. 'b' 'c'))"
        );
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
        for source in ["local x=2", "x+", "(1+2", "1 2", "foo(1,"] {
            assert!(parse_expression(source).is_err(), "{source}");
        }
    }

    #[test]
    fn chained_calls_fields_and_indexing() {
        let parsed = parse_expression("obj.child[2](3, 4).value").unwrap();
        assert_eq!(
            show(&parsed),
            "(field (call (index (field obj child) 2) [3 4]) value)"
        );
    }

    #[test]
    fn empty_call_and_operator_precedence() {
        assert_eq!(
            show(&parse_expression("f()^2").unwrap()),
            "(^ (call f []) 2)"
        );
        assert_eq!(
            show(&parse_expression("-obj.x^2").unwrap()),
            "(- (^ (field obj x) 2))"
        );
    }

    #[test]
    fn rejects_malformed_postfix_arguments() {
        for source in ["f(1,)", "a[1", "a.", "obj.123", "f(1,2"] {
            assert!(parse_expression(source).is_err(), "{source}");
        }
    }

    #[test]
    fn local_assignment_call_and_final_return_chunk() {
        let parsed = parse_chunk("local a,b=2,3; a,b=b,a; print(a,b); return a+b").unwrap();
        assert_eq!(parsed.len(), 4);
        match &parsed[0] {
            Statement::Local {
                names,
                initializers,
            } => {
                assert_eq!(names, &["a", "b"]);
                assert_eq!(initializers.len(), 2);
            }
            _ => panic!("first statement must be local"),
        }
        match &parsed[1] {
            Statement::Assign { targets, values } => {
                assert_eq!(targets.len(), 2);
                assert_eq!(values.len(), 2);
            }
            _ => panic!("second statement must be assignment"),
        }
        assert!(matches!(&parsed[2], Statement::Call(_)));
        assert!(matches!(&parsed[3], Statement::Return(values) if values.len() == 1));
    }

    #[test]
    fn assignment_can_target_table_fields_and_indexes() {
        let parsed = parse_chunk("t.x, t[2] = 1, 2").unwrap();
        assert!(matches!(&parsed[0], Statement::Assign { targets, values }
            if matches!(&targets[0], Expr::Field { .. })
            && matches!(&targets[1], Expr::Index { .. })
            && values.len() == 2));
    }

    #[test]
    fn rejects_unsupported_or_invalid_chunk_statements() {
        for source in [
            "local = 1",
            "local a,=2",
            "a+1=2",
            "1=2",
            "true=3",
            "a",
            "1+2",
            "return 1; print(2)",
            "local a=1 print(a)",
            "if true then return 1 end",
            "local x=1; local y=",
        ] {
            assert!(parse_chunk(source).is_err(), "{source}");
        }
    }

    #[test]
    fn reports_lexer_errors_without_panic() {
        assert!(parse_expression("'unfinished").is_err());
        assert!(parse_expression("0x").is_err());
    }
}
