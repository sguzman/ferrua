//! Experimental local-name resolver for the Ferrua-owned chunk AST.
//! Only a single un-nested scope is supported; this is not Lua's full binder.

use crate::expr::{Expr, ParseError, Statement, parse_chunk};
use crate::lex::{Kind, tokenize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Local(usize),
    Global,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding<'src> {
    pub name: &'src str,
    pub slot: usize,
    pub statement: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference<'src> {
    pub name: &'src str,
    pub resolution: Resolution,
    pub access: Access,
    pub statement: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeSummary<'src> {
    pub bindings: Vec<Binding<'src>>,
    pub references: Vec<Reference<'src>>,
}

fn identifier(text: &str) -> bool {
    tokenize(text).is_ok_and(|tokens| {
        tokens.len() == 1 && tokens[0].kind == Kind::Identifier
    })
}

struct Resolver<'src> {
    locals: HashMap<&'src str, usize>,
    result: ScopeSummary<'src>,
}

impl<'src> Resolver<'src> {
    fn record(&mut self, name: &'src str, access: Access, statement: usize) {
        let resolution = self.locals.get(name).copied().map_or(
            Resolution::Global,
            Resolution::Local,
        );
        self.result.references.push(Reference {
            name,
            resolution,
            access,
            statement,
        });
    }

    fn read_expression(&mut self, expr: &Expr<'src>, statement: usize) {
        match expr {
            Expr::Atom(value) => {
                if identifier(value) {
                    self.record(value, Access::Read, statement);
                }
            }
            Expr::Unary { right, .. } => self.read_expression(right, statement),
            Expr::Binary { left, right, .. } => {
                self.read_expression(left, statement);
                self.read_expression(right, statement);
            }
            Expr::Call { callee, arguments } => {
                self.read_expression(callee, statement);
                for arg in arguments {
                    self.read_expression(arg, statement);
                }
            }
            Expr::Field { object, .. } => self.read_expression(object, statement),
            Expr::Index { object, key } => {
                self.read_expression(object, statement);
                self.read_expression(key, statement);
            }
        }
    }

    fn write_target(&mut self, expr: &Expr<'src>, statement: usize) {
        match expr {
            Expr::Atom(name) if identifier(name) => self.record(name, Access::Write, statement),
            Expr::Field { object, .. } => self.read_expression(object, statement),
            Expr::Index { object, key } => {
                self.read_expression(object, statement);
                self.read_expression(key, statement);
            }
            _ => {}
        }
    }

    fn bind(&mut self, name: &'src str, statement: usize) {
        let slot = self.result.bindings.len();
        self.locals.insert(name, slot);
        self.result.bindings.push(Binding {
            name,
            slot,
            statement,
        });
    }

    fn statement(&mut self, node: &Statement<'src>, number: usize) {
        match node {
            Statement::Local {
                names,
                initializers,
            } => {
                // A local initializer resolves against outer/previous locals,
                // not the newly declared names in the same statement.
                for value in initializers {
                    self.read_expression(value, number);
                }
                for name in names {
                    self.bind(name, number);
                }
            }
            Statement::Assign { targets, values } => {
                for target in targets {
                    self.write_target(target, number);
                }
                for value in values {
                    self.read_expression(value, number);
                }
            }
            Statement::Call(call) => self.read_expression(call, number),
            Statement::Return(values) => {
                for value in values {
                    self.read_expression(value, number);
                }
            }
        }
    }
}

/// Analyze binding slots and references for the deliberately restricted
/// semicolon-delimited Lua chunk subset. No block, function or closure scopes.
pub fn analyze(source: &str) -> Result<ScopeSummary<'_>, ParseError> {
    let statements = parse_chunk(source)?;
    let mut resolver = Resolver {
        locals: HashMap::new(),
        result: ScopeSummary {
            bindings: Vec::new(),
            references: Vec::new(),
        },
    };
    for (index, stmt) in statements.iter().enumerate() {
        resolver.statement(stmt, index);
    }
    Ok(resolver.result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_shadowing_preserves_prior_initializer_binding() {
        let result = analyze("local x=1; local x=x+1; print(x); y=x").unwrap();
        assert_eq!(result.bindings.len(), 2);
        assert_eq!(result.bindings[0].slot, 0);
        assert_eq!(result.bindings[1].slot, 1);
        assert!(result.references.iter().any(|r| {
            r.name == "x" && r.statement == 1 && r.resolution == Resolution::Local(0)
        }));
        assert!(result.references.iter().any(|r| {
            r.name == "x" && r.statement == 2 && r.resolution == Resolution::Local(1)
        }));
        assert!(result.references.iter().any(|r| {
            r.name == "y" && r.statement == 3 && r.access == Access::Write
                && r.resolution == Resolution::Global
        }));
        assert!(result.references.iter().any(|r| {
            r.name == "print" && r.statement == 2
                && r.resolution == Resolution::Global
        }));
    }

    #[test]
    fn new_local_does_not_see_itself_in_initializer() {
        let result = analyze("local item=item; return item").unwrap();
        assert_eq!(result.references.len(), 2);
        assert_eq!(result.references[0].resolution, Resolution::Global);
        assert_eq!(result.references[1].resolution, Resolution::Local(0));
    }

    #[test]
    fn field_and_index_names_are_not_declared_bindings() {
        let result = analyze("local t=1; local k=2; t.field=t[k]").unwrap();
        assert_eq!(result.bindings.len(), 2);
        assert_eq!(result.references.len(), 3);
        assert!(result.references.iter().all(|r| r.access == Access::Read));
        assert!(!result.references.iter().any(|r| r.name == "field"));
    }

    #[test]
    fn literals_are_not_identifier_references() {
        let result = analyze("local x=true; return x").unwrap();
        assert_eq!(result.references.len(), 1);
        assert_eq!(result.references[0].name, "x");
        assert_eq!(result.references[0].resolution, Resolution::Local(0));
    }

    #[test]
    fn unsupported_grammar_is_rejected_instead_of_analyzed() {
        assert!(analyze("if true then local a=1 end").is_err());
    }
}
