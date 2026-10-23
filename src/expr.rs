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
    MethodCall {
        object: Box<Expr<'a>>,
        method: &'a str,
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
    Function {
        parameters: Vec<&'a str>,
        vararg: bool,
        body: Vec<Statement<'a>>,
    },
    Vararg,
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
    loop_depth: usize,
    function_depth: usize,
    vararg_allowed: bool,
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
            "function" => {
                let (parameters, vararg, body) = self.function_body()?;
                Ok(Expr::Function {
                    parameters,
                    vararg,
                    body,
                })
            }
            "..." if self.vararg_allowed => Ok(Expr::Vararg),
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
                Some(":") => {
                    self.pop();
                    let method = self.name()?;
                    if self.peek().map(|token| token.spelling) != Some("(") {
                        return Err(self.error("expected parenthesized method arguments"));
                    }
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
                        return Err(self.error("expected closing parenthesis in method call"));
                    }
                    self.pop();
                    left = Expr::MethodCall {
                        object: Box::new(left),
                        method,
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
        loop_depth: 0,
        function_depth: 0,
        vararg_allowed: true,
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
    Function {
        name: &'a str,
        local: bool,
        parameters: Vec<&'a str>,
        vararg: bool,
        body: Vec<Statement<'a>>,
    },
    Break,
    /// A lexical scope closed with `end`. Nested Lua blocks are supported
    /// by this experimental subset, not arbitrary control-flow statements.
    Block(Vec<Statement<'a>>),
    While {
        condition: Expr<'a>,
        body: Vec<Statement<'a>>,
    },
    /// Lua repeat's condition can see locals declared in the loop body.
    Repeat {
        body: Vec<Statement<'a>>,
        condition: Expr<'a>,
    },
    NumericFor {
        variable: &'a str,
        start: Expr<'a>,
        limit: Expr<'a>,
        step: Option<Expr<'a>>,
        body: Vec<Statement<'a>>,
    },
    GenericFor {
        variables: Vec<&'a str>,
        iterators: Vec<Expr<'a>>,
        body: Vec<Statement<'a>>,
    },
    If {
        branches: Vec<(Expr<'a>, Vec<Statement<'a>>)>,
        else_branch: Option<Vec<Statement<'a>>>,
    },
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

    fn function_body(&mut self) -> Result<(Vec<&'a str>, bool, Vec<Statement<'a>>), ParseError> {
        if self.function_depth >= 64 {
            return Err(self.error("Lua function nesting limit exceeded"));
        }
        if self.peek().map(|t| t.spelling) != Some("(") {
            return Err(self.error("expected opening parenthesis for function"));
        }
        self.pop();
        let mut parameters = Vec::new();
        let mut vararg = false;
        while self.peek().map(|t| t.spelling) != Some(")") {
            if self.peek().is_none() {
                return Err(self.error("unterminated function parameters"));
            }
            if self.peek().map(|t| t.spelling) == Some("...") {
                self.pop();
                vararg = true;
                break;
            }
            parameters.push(self.name()?);
            if self.peek().map(|t| t.spelling) != Some(",") {
                break;
            }
            self.pop();
        }
        if self.peek().map(|t| t.spelling) != Some(")") {
            return Err(self.error("expected closing parenthesis for function"));
        }
        self.pop();
        let previous_loops = self.loop_depth;
        let previous_vararg = self.vararg_allowed;
        self.loop_depth = 0;
        self.vararg_allowed = vararg;
        self.function_depth += 1;
        let body = self.block(&["end"], self.function_depth)?;
        self.function_depth -= 1;
        self.loop_depth = previous_loops;
        self.vararg_allowed = previous_vararg;
        if self.peek().map(|t| t.spelling) != Some("end") {
            return Err(self.error("expected end to close function"));
        }
        self.pop();
        Ok((parameters, vararg, body))
    }

    fn expressions(&mut self) -> Result<Vec<Expr<'a>>, ParseError> {
        let mut values = vec![self.expression(0)?];
        while self.peek().is_some_and(|token| token.spelling == ",") {
            self.pop();
            values.push(self.expression(0)?);
        }
        Ok(values)
    }

    fn statement(&mut self, depth: usize) -> Result<Statement<'a>, ParseError> {
        if self
            .peek()
            .is_some_and(|token| token.spelling == "function")
        {
            self.pop();
            let name = self.name()?;
            let (parameters, vararg, body) = self.function_body()?;
            return Ok(Statement::Function {
                name,
                local: false,
                parameters,
                vararg,
                body,
            });
        }
        if self.peek().is_some_and(|token| token.spelling == "do") {
            self.pop();
            let body = self.block(&["end"], depth + 1)?;
            if self.peek().map(|token| token.spelling) != Some("end") {
                return Err(self.error("expected end to close do block"));
            }
            self.pop();
            return Ok(Statement::Block(body));
        }
        if self.peek().is_some_and(|token| token.spelling == "for") {
            self.pop();
            let variable = self.name()?;
            if self.peek().map(|token| token.spelling) == Some("=") {
                self.pop();
                let start = self.expression(0)?;
                if self.peek().map(|token| token.spelling) != Some(",") {
                    return Err(self.error("expected comma after numeric for start"));
                }
                self.pop();
                let limit = self.expression(0)?;
                let step = if self.peek().map(|token| token.spelling) == Some(",") {
                    self.pop();
                    Some(self.expression(0)?)
                } else {
                    None
                };
                if self.peek().map(|token| token.spelling) != Some("do") {
                    return Err(self.error("expected do in numeric for"));
                }
                self.pop();
                self.loop_depth += 1;
                let body = self.block(&["end"], depth + 1)?;
                self.loop_depth -= 1;
                if self.peek().map(|token| token.spelling) != Some("end") {
                    return Err(self.error("expected end to close numeric for"));
                }
                self.pop();
                return Ok(Statement::NumericFor {
                    variable,
                    start,
                    limit,
                    step,
                    body,
                });
            }
            let mut variables = vec![variable];
            while self.peek().map(|token| token.spelling) == Some(",") {
                self.pop();
                variables.push(self.name()?);
            }
            if self.peek().map(|token| token.spelling) != Some("in") {
                return Err(self.error("expected in after generic for names"));
            }
            self.pop();
            let iterators = self.expressions()?;
            if self.peek().map(|token| token.spelling) != Some("do") {
                return Err(self.error("expected do in generic for"));
            }
            self.pop();
            self.loop_depth += 1;
            let body = self.block(&["end"], depth + 1)?;
            self.loop_depth -= 1;
            if self.peek().map(|token| token.spelling) != Some("end") {
                return Err(self.error("expected end to close generic for"));
            }
            self.pop();
            return Ok(Statement::GenericFor {
                variables,
                iterators,
                body,
            });
        }
        if self.peek().is_some_and(|token| token.spelling == "repeat") {
            self.pop();
            self.loop_depth += 1;
            let body = self.block(&["until"], depth + 1)?;
            self.loop_depth -= 1;
            if self.peek().map(|token| token.spelling) != Some("until") {
                return Err(self.error("expected until to close repeat block"));
            }
            self.pop();
            let condition = self.expression(0)?;
            return Ok(Statement::Repeat { body, condition });
        }
        if self.peek().is_some_and(|token| token.spelling == "while") {
            self.pop();
            let condition = self.expression(0)?;
            if self.peek().map(|token| token.spelling) != Some("do") {
                return Err(self.error("expected do after while condition"));
            }
            self.pop();
            self.loop_depth += 1;
            let body = self.block(&["end"], depth + 1)?;
            self.loop_depth -= 1;
            if self.peek().map(|token| token.spelling) != Some("end") {
                return Err(self.error("expected end to close while block"));
            }
            self.pop();
            return Ok(Statement::While { condition, body });
        }
        if self.peek().is_some_and(|token| token.spelling == "if") {
            self.pop();
            let mut branches = Vec::new();
            loop {
                let condition = self.expression(0)?;
                if self.peek().map(|token| token.spelling) != Some("then") {
                    return Err(self.error("expected then after if condition"));
                }
                self.pop();
                let body = self.block(&["elseif", "else", "end"], depth + 1)?;
                branches.push((condition, body));
                if self.peek().map(|token| token.spelling) == Some("elseif") {
                    self.pop();
                    continue;
                }
                break;
            }
            let else_branch = if self.peek().map(|token| token.spelling) == Some("else") {
                self.pop();
                Some(self.block(&["end"], depth + 1)?)
            } else {
                None
            };
            if self.peek().map(|token| token.spelling) != Some("end") {
                return Err(self.error("expected end to close if block"));
            }
            self.pop();
            return Ok(Statement::If {
                branches,
                else_branch,
            });
        }
        if self.peek().is_some_and(|token| token.spelling == "break") {
            if self.loop_depth == 0 {
                return Err(self.error("break outside loop"));
            }
            self.pop();
            return Ok(Statement::Break);
        }
        if self.peek().is_some_and(|token| token.spelling == "local") {
            self.pop();
            if self.peek().map(|token| token.spelling) == Some("function") {
                self.pop();
                let name = self.name()?;
                let (parameters, vararg, body) = self.function_body()?;
                return Ok(Statement::Function {
                    name,
                    local: true,
                    parameters,
                    vararg,
                    body,
                });
            }
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
                || self.peek().is_some_and(|token| {
                    matches!(token.spelling, ";" | "end" | "else" | "elseif" | "until")
                }) {
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
        if matches!(first, Expr::Call { .. } | Expr::MethodCall { .. }) {
            Ok(Statement::Call(first))
        } else {
            Err(self.error("only calls can be standalone expression statements"))
        }
    }
}

impl<'a> Parser<'a> {
    fn block(
        &mut self,
        terminators: &[&str],
        depth: usize,
    ) -> Result<Vec<Statement<'a>>, ParseError> {
        // A bounded parser also protects library users from deeply nested
        // inputs that would otherwise exhaust the Rust stack.
        if depth > 64 {
            return Err(self.error("Lua block nesting limit exceeded"));
        }
        let mut result = Vec::new();
        while let Some(token) = self.peek() {
            if terminators.contains(&token.spelling) {
                break;
            }
            if token.spelling == ";" {
                self.pop();
                continue;
            }
            let statement = self.statement(depth)?;
            let terminal = matches!(statement, Statement::Return(_) | Statement::Break);
            result.push(statement);
            let ending_line = self
                .tokens
                .get(self.index.saturating_sub(1))
                .map_or(0, |token| {
                    let mut line = token.line;
                    let mut cr = false;
                    for byte in token.spelling.bytes() {
                        if byte == b'\r' || (byte == b'\n' && !cr) {
                            line += 1;
                        }
                        cr = byte == b'\r';
                    }
                    line
                });
            if self.peek().is_some_and(|token| token.spelling == ";") {
                self.pop();
            } else if self
                .peek()
                .is_some_and(|token| terminators.contains(&token.spelling))
            {
                // A block's closing token is a natural terminator.
            } else if self.peek().is_some_and(|token| token.line > ending_line) {
                // A newline may separate statements in this grammar subset.
            } else if self.peek().is_some() {
                return Err(self.error("expected statement separator"));
            }
            if terminal
                && self
                    .peek()
                    .is_some_and(|token| !terminators.contains(&token.spelling))
            {
                return Err(self.error("return or break must be final statement in block"));
            }
        }
        Ok(result)
    }
}

/// Parse a deliberately limited Lua chunk: semicolon-separated locals,
/// assignments, call statements and final returns, plus nested do/end
/// lexical blocks, conditionals, while/repeat loops and numeric/generic for loops.
/// Newline boundaries can separate supported statements. Other Lua constructs
/// remain unsupported.
pub fn parse_chunk(source: &str) -> Result<Vec<Statement<'_>>, ParseError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser {
        tokens,
        index: 0,
        length: source.len(),
        loop_depth: 0,
        function_depth: 0,
        vararg_allowed: true,
    };
    parser.block(&[], 0)
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
            Expr::MethodCall {
                object,
                method,
                arguments,
            } => {
                let text = arguments.iter().map(show).collect::<Vec<_>>().join(" ");
                format!("(method {} {method} [{}])", show(object), text)
            }
            Expr::Field { object, name } => format!("(field {} {name})", show(object)),
            Expr::Index { object, key } => format!("(index {} {})", show(object), show(key)),
            Expr::Function {
                parameters,
                vararg,
                body,
            } => format!(
                "(function [{}] {} {})",
                parameters.join(" "),
                vararg,
                body.len()
            ),
            Expr::Vararg => "...".to_owned(),
        }
    }

    #[test]
    fn owned_parser_recognizes_function_scopes_and_varargs() {
        let tree = parse_chunk(
            "local function fib(n) if n<=1 then return n end; return fib(n-1)+fib(n-2) end; return fib(5)"
        ).unwrap();
        assert!(
            matches!(&tree[0], Statement::Function {name: "fib", local: true, parameters, vararg: false, body} if parameters == &["n"] && body.len() == 2)
        );
        let tree = parse_chunk("function f(a,...) return a,... end; return f(1,2)").unwrap();
        assert!(
            matches!(&tree[0], Statement::Function {name: "f", local: false, vararg: true, body, ..} if matches!(&body[0], Statement::Return(xs) if xs.len()==2 && matches!(&xs[1], Expr::Vararg)))
        );
        assert!(matches!(
            parse_expression("function(x) return x end").unwrap(),
            Expr::Function { .. }
        ));
        assert!(parse_chunk("local f=function(a) return a end; return f(4)").is_ok());
    }

    #[test]
    fn function_syntax_rejects_invalid_parameters_and_escaped_break() {
        for source in [
            "local function f(a,) return a end",
            "local function f(a a) return a end",
            "local function f(a,...,b) return a end",
            "function f(x) return ... end",
            "while true do local function f() break end; break end",
            "local function f( return 1 end",
            "function f() return 1",
        ] {
            assert!(parse_chunk(source).is_err(), "{source}");
        }
        assert!(parse_chunk("local function f(...) return ... end").is_ok());
        assert!(parse_chunk("return ...").is_ok());
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
            "if true return 1 end",
            "local x=1; local y=",
        ] {
            assert!(parse_chunk(source).is_err(), "{source}");
        }
    }

    #[test]
    fn parses_nested_do_blocks_and_post_block_statements() {
        let parsed = parse_chunk(
            "local x=1; do local x=2; do local y=x; print(y) end; print(x) end; return x",
        )
        .unwrap();
        assert_eq!(parsed.len(), 3);
        match &parsed[1] {
            Statement::Block(outer) => {
                assert_eq!(outer.len(), 3);
                assert!(matches!(&outer[1], Statement::Block(inner) if inner.len() == 2));
            }
            _ => panic!("middle statement should be a scoped do block"),
        }
        assert!(matches!(&parsed[2], Statement::Return(_)));
    }

    #[test]
    fn rejects_malformed_do_blocks_and_excessive_nesting() {
        for source in [
            "do local x=1",
            "end",
            "do return 1; local x=2 end",
            "do local x=1 end print(x)",
            "do do local a=1 end",
        ] {
            assert!(parse_chunk(source).is_err(), "{source}");
        }
        let deeply_nested = format!("{}{}", "do ".repeat(65), "end ".repeat(65));
        assert!(
            parse_chunk(&deeply_nested)
                .unwrap_err()
                .message
                .contains("nesting")
        );
    }

    #[test]
    fn break_is_scoped_to_loops_and_must_terminate_its_block() {
        for source in [
            "while true do break end; return 1",
            "repeat if true then break end until false; return 2",
            "for i=1,10 do if i==3 then break end end; return 3",
            "for k in pairs(t) do break end; return 4",
            "while true do while true do break end; break end; return 5",
        ] {
            assert!(parse_chunk(source).is_ok(), "{source}");
        }
        for source in [
            "break",
            "do break end",
            "if true then break end",
            "while true do break; print(1) end",
            "repeat break; local x=2 until true",
        ] {
            assert!(parse_chunk(source).is_err(), "{source}");
        }
    }

    #[test]
    fn parses_while_and_if_elseif_else_scopes() {
        let source = "local x=0; while x<3 do x=x+1 end; if x==3 then local good=x; print(good) elseif x>3 then print(x) else print(0) end; return x";
        let statements = parse_chunk(source).unwrap();
        assert_eq!(statements.len(), 4);
        assert!(matches!(&statements[1], Statement::While { body, .. } if body.len() == 1));
        assert!(
            matches!(&statements[2], Statement::If { branches, else_branch }
            if branches.len() == 2 && branches[0].1.len() == 2 && else_branch.is_some())
        );
    }

    #[test]
    fn rejects_unclosed_or_malformed_control_flow() {
        for source in [
            "while true return 1 end",
            "while true do print(1)",
            "if true return 1 end",
            "if true then print(1)",
            "if true then else elseif false then end",
            "if true then return 1; print(2) end",
            "elseif true then print(1)",
        ] {
            assert!(parse_chunk(source).is_err(), "{source}");
        }
    }

    #[test]
    fn parses_repeat_with_body_local_in_until_condition() {
        let source = "local x=0\nrepeat\nlocal next=x+1\nx=next\nuntil next>3\nreturn x";
        let parsed = parse_chunk(source).unwrap();
        assert_eq!(parsed.len(), 3);
        assert!(matches!(&parsed[1], Statement::Repeat { body, .. } if body.len() == 2));
        assert!(matches!(&parsed[2], Statement::Return(_)));
    }

    #[test]
    fn supports_bare_return_at_end_of_nested_branch() {
        assert!(parse_chunk("if flag then return else return end").is_ok());
        assert!(parse_chunk("do return end").is_ok());
    }

    #[test]
    fn checks_repeat_terminators_and_newline_separators() {
        for source in [
            "repeat local x=1 end",
            "repeat local x=1 until",
            "local a=1 print(a)",
            "return 1\nprint(2)",
            "repeat local x=1; return 2; print(x) until true",
        ] {
            assert!(parse_chunk(source).is_err(), "{source}");
        }
    }

    #[test]
    fn parses_numeric_for_with_optional_step() {
        let parsed =
            parse_chunk("local total=0\nfor i=1,10,2 do\ntotal=total+i\nend\nreturn total")
                .unwrap();
        assert_eq!(parsed.len(), 3);
        assert!(
            matches!(&parsed[1], Statement::NumericFor { variable, step: Some(_), body, .. }
            if *variable == "i" && body.len() == 1)
        );
        let omitted_step = parse_chunk("for i=1,3 do print(i) end").unwrap();
        assert!(matches!(
            &omitted_step[0],
            Statement::NumericFor { step: None, .. }
        ));
    }

    #[test]
    fn rejects_unsupported_generic_or_malformed_numeric_for() {
        for source in [
            "for i list do print(i) end",
            "for i=1 do print(i) end",
            "for i=1,10 print(i) end",
            "for i=1,10, do print(i) end",
            "for i=1,10 do print(i)",
        ] {
            assert!(parse_chunk(source).is_err(), "{source}");
        }
    }

    #[test]
    fn parses_generic_for_with_multiple_names_and_iterators() {
        let source = "for k,v in pairs(t), auxiliary do\nprint(k,v)\nend";
        let parsed = parse_chunk(source).unwrap();
        assert_eq!(parsed.len(), 1);
        assert!(
            matches!(&parsed[0], Statement::GenericFor { variables, iterators, body }
            if variables == &["k", "v"] && iterators.len() == 2 && body.len() == 1)
        );
    }

    #[test]
    fn rejects_incomplete_generic_for_headers() {
        for source in [
            "for k in do print(k) end",
            "for k,v pairs(t) do print(k) end",
            "for k, in pairs(t) do print(k) end",
            "for k in pairs(t) print(k) end",
            "for k in pairs(t) do print(k)",
        ] {
            assert!(parse_chunk(source).is_err(), "{source}");
        }
    }

    #[test]
    fn parses_chained_method_calls_with_explicit_parenthesized_arguments() {
        let expression = parse_expression("obj.child:run(1,2).value").unwrap();
        assert_eq!(
            show(&expression),
            "(field (method (field obj child) run [1 2]) value)"
        );
        let statements = parse_chunk("obj:run(1,2); return 0").unwrap();
        assert!(matches!(
            &statements[0],
            Statement::Call(Expr::MethodCall { .. })
        ));
    }

    #[test]
    fn rejects_incomplete_or_non_identifier_method_calls() {
        for source in ["obj:run", "obj:run(", "obj:run(1,)", "obj:3(1)", "obj:(1)"] {
            assert!(parse_expression(source).is_err(), "{source}");
        }
    }

    #[test]
    fn reports_lexer_errors_without_panic() {
        assert!(parse_expression("'unfinished").is_err());
        assert!(parse_expression("0x").is_err());
    }
}
