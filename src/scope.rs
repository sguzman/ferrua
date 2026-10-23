//! Experimental local-name resolver for the Ferrua-owned chunk AST.
//! This handles nested do/end blocks, not function scopes or closures.

use crate::expr::{Expr, ParseError, Statement, parse_chunk};
use crate::lex::{Kind, tokenize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Local(usize),
    Upvalue(usize),
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
    pub scope_depth: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference<'src> {
    pub name: &'src str,
    pub resolution: Resolution,
    pub access: Access,
    pub statement: usize,
    pub scope_depth: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeSummary<'src> {
    pub bindings: Vec<Binding<'src>>,
    pub references: Vec<Reference<'src>>,
    pub functions: Vec<FunctionInfo<'src>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionInfo<'src> {
    pub id: usize,
    pub parent: Option<usize>,
    pub parameters: Vec<&'src str>,
    pub implicit_self: bool,
    /// Lexical binding slots required by this closure, including captures
    /// needed transitively by its nested closures.
    pub captures: Vec<usize>,
}

fn identifier(text: &str) -> bool {
    tokenize(text).is_ok_and(|tokens| tokens.len() == 1 && tokens[0].kind == Kind::Identifier)
}

struct Resolver<'src> {
    scopes: Vec<HashMap<&'src str, usize>>,
    next_statement: usize,
    result: ScopeSummary<'src>,
    function_bases: Vec<usize>,
    function_ids: Vec<usize>,
}

impl<'src> Resolver<'src> {
    fn record(&mut self, name: &'src str, access: Access, statement: usize) {
        let binding = self
            .scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(depth, frame)| frame.get(name).map(|slot| (depth, *slot)));
        let resolution = binding.map_or(Resolution::Global, |(depth, slot)| {
            if self.function_bases.last().is_some_and(|base| depth < *base) {
                Resolution::Upvalue(slot)
            } else {
                Resolution::Local(slot)
            }
        });
        if let Some((depth, slot)) = binding {
            // An inner closure may require an outer function to carry a
            // captured binding even if that outer function never reads it.
            for (&base, &id) in self.function_bases.iter().zip(&self.function_ids) {
                if depth < base {
                    let captures = &mut self.result.functions[id].captures;
                    if !captures.contains(&slot) {
                        captures.push(slot);
                    }
                }
            }
        }
        self.result.references.push(Reference {
            name,
            resolution,
            access,
            statement,
            scope_depth: self.scopes.len() - 1,
        });
    }

    fn read_expression(&mut self, expr: &Expr<'src>, statement: usize) {
        match expr {
            Expr::Grouped(inner) => self.read_expression(inner, statement),
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
            Expr::MethodCall {
                object, arguments, ..
            } => {
                self.read_expression(object, statement);
                for arg in arguments {
                    self.read_expression(arg, statement);
                }
            }
            Expr::Field { object, .. } => self.read_expression(object, statement),
            Expr::Index { object, key } => {
                self.read_expression(object, statement);
                self.read_expression(key, statement);
            }
            Expr::Function {
                parameters, body, ..
            } => self.function_scope(parameters, body, false),
            Expr::Vararg => {}
            Expr::Table(fields) => {
                for field in fields {
                    match field {
                        crate::expr::TableField::Named { value, .. }
                        | crate::expr::TableField::Value(value) => {
                            self.read_expression(value, statement);
                        }
                        crate::expr::TableField::Indexed { key, value } => {
                            self.read_expression(key, statement);
                            self.read_expression(value, statement);
                        }
                    }
                }
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
        self.scopes
            .last_mut()
            .expect("at least root scope")
            .insert(name, slot);
        self.result.bindings.push(Binding {
            name,
            slot,
            statement,
            scope_depth: self.scopes.len() - 1,
        });
    }

    fn function_scope(
        &mut self,
        parameters: &[&'src str],
        body: &[Statement<'src>],
        implicit_self: bool,
    ) {
        let id = self.result.functions.len();
        self.result.functions.push(FunctionInfo {
            id,
            parent: self.function_ids.last().copied(),
            parameters: parameters.to_vec(),
            implicit_self,
            captures: Vec::new(),
        });
        self.function_bases.push(self.scopes.len());
        self.function_ids.push(id);
        self.scopes.push(HashMap::new());
        let declaration = self.next_statement;
        if implicit_self {
            self.bind("self", declaration);
        }
        for name in parameters {
            self.bind(name, declaration);
        }
        for node in body {
            self.statement(node);
        }
        self.scopes.pop();
        self.function_bases.pop();
        self.function_ids.pop();
    }

    fn scoped_statements(&mut self, nodes: &[Statement<'src>]) {
        self.scopes.push(HashMap::new());
        for child in nodes {
            self.statement(child);
        }
        self.scopes.pop();
    }

    fn statement(&mut self, node: &Statement<'src>) {
        let number = self.next_statement;
        self.next_statement += 1;
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
            Statement::Function {
                name,
                fields,
                method,
                local,
                parameters,
                body,
                ..
            } => {
                if *local {
                    // The local function name is in scope for its own body.
                    self.bind(name, number);
                } else if fields.is_empty() && method.is_none() {
                    self.record(name, Access::Write, number);
                } else {
                    // Dotted methods read the base table, not a new binding.
                    self.record(name, Access::Read, number);
                }
                self.function_scope(parameters, body, method.is_some());
            }
            Statement::Break => {}
            Statement::Return(values) => {
                for value in values {
                    self.read_expression(value, number);
                }
            }
            Statement::Block(body) => {
                self.scoped_statements(body);
            }
            Statement::While { condition, body } => {
                self.read_expression(condition, number);
                self.scoped_statements(body);
            }
            Statement::Repeat { body, condition } => {
                self.scopes.push(HashMap::new());
                for child in body {
                    self.statement(child);
                }
                // Lua repeat evaluates until inside its body scope.
                self.read_expression(condition, number);
                self.scopes.pop();
            }
            Statement::NumericFor {
                variable,
                start,
                limit,
                step,
                body,
            } => {
                // Lua numeric for control expressions evaluate before binding
                // the loop variable; that name belongs only to the loop frame.
                self.read_expression(start, number);
                self.read_expression(limit, number);
                if let Some(step) = step {
                    self.read_expression(step, number);
                }
                self.scopes.push(HashMap::new());
                self.bind(variable, number);
                for child in body {
                    self.statement(child);
                }
                self.scopes.pop();
            }
            Statement::GenericFor {
                variables,
                iterators,
                body,
            } => {
                // Iterator expressions evaluate before their loop locals exist.
                for iterator in iterators {
                    self.read_expression(iterator, number);
                }
                self.scopes.push(HashMap::new());
                for name in variables {
                    self.bind(name, number);
                }
                for child in body {
                    self.statement(child);
                }
                self.scopes.pop();
            }
            Statement::If {
                branches,
                else_branch,
            } => {
                for (condition, body) in branches {
                    self.read_expression(condition, number);
                    self.scoped_statements(body);
                }
                if let Some(body) = else_branch {
                    self.scoped_statements(body);
                }
            }
        }
    }
}

/// Analyze binding slots and references for the deliberately restricted
/// semicolon-delimited Lua chunk subset. Supports nested do/end scopes;
/// no function, upvalue, closure, or flow-sensitive resolution.
pub fn analyze(source: &str) -> Result<ScopeSummary<'_>, ParseError> {
    let statements = parse_chunk(source)?;
    let mut resolver = Resolver {
        scopes: vec![HashMap::new()],
        next_statement: 0,
        function_bases: Vec::new(),
        function_ids: Vec::new(),
        result: ScopeSummary {
            bindings: Vec::new(),
            references: Vec::new(),
            functions: Vec::new(),
        },
    };
    for stmt in &statements {
        resolver.statement(stmt);
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
            r.name == "y"
                && r.statement == 3
                && r.access == Access::Write
                && r.resolution == Resolution::Global
        }));
        assert!(result.references.iter().any(|r| {
            r.name == "print" && r.statement == 2 && r.resolution == Resolution::Global
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
    fn nested_blocks_shadow_and_restore_local_bindings() {
        let result = analyze(
            "local x=1; do local x=x+1; do local y=x; print(y,x) end; print(x) end; return x",
        )
        .unwrap();
        assert_eq!(result.bindings.len(), 3);
        assert_eq!(result.bindings[0].scope_depth, 0);
        assert_eq!(result.bindings[1].scope_depth, 1);
        assert_eq!(result.bindings[2].scope_depth, 2);
        assert!(
            result.references.iter().any(|r| r.name == "x"
                && r.scope_depth == 1
                && r.resolution == Resolution::Local(0))
        );
        assert!(
            result.references.iter().any(|r| r.name == "x"
                && r.scope_depth == 2
                && r.resolution == Resolution::Local(1))
        );
        assert!(
            result.references.iter().any(|r| r.name == "x"
                && r.scope_depth == 0
                && r.resolution == Resolution::Local(0))
        );
        assert!(
            result.references.iter().any(|r| r.name == "y"
                && r.scope_depth == 2
                && r.resolution == Resolution::Local(2))
        );
    }

    #[test]
    fn escaped_block_locals_are_not_visible_after_end() {
        let result = analyze("do local hidden=1 end; print(hidden)").unwrap();
        assert_eq!(result.bindings.len(), 1);
        let hidden = result
            .references
            .iter()
            .find(|r| r.name == "hidden")
            .unwrap();
        assert_eq!(hidden.scope_depth, 0);
        assert_eq!(hidden.resolution, Resolution::Global);
    }

    #[test]
    fn assignments_in_inner_blocks_resolve_nearest_visible_local() {
        let result = analyze("local x=1; do x=2; local x=3; x=4 end; x=5").unwrap();
        let writes: Vec<_> = result
            .references
            .iter()
            .filter(|r| r.name == "x" && r.access == Access::Write)
            .collect();
        assert_eq!(writes.len(), 3);
        assert_eq!(writes[0].resolution, Resolution::Local(0));
        assert_eq!(writes[1].resolution, Resolution::Local(1));
        assert_eq!(writes[2].resolution, Resolution::Local(0));
    }

    #[test]
    fn branches_and_loops_use_separate_lexical_scopes() {
        let summary = analyze(
            "local x=1; while x<3 do local y=x; x=y end; if x==3 then local z=x; print(z) else local z=0; print(z) end; return x"
        ).unwrap();
        let locals: Vec<_> = summary
            .bindings
            .iter()
            .map(|b| (b.name, b.scope_depth, b.slot))
            .collect();
        assert_eq!(
            locals,
            vec![("x", 0, 0), ("y", 1, 1), ("z", 1, 2), ("z", 1, 3)]
        );
        assert!(summary.references.iter().any(|r| r.name == "y"
            && r.access == Access::Read
            && r.resolution == Resolution::Local(1)));
        assert!(summary.references.iter().any(|r| r.name == "z"
            && r.access == Access::Read
            && r.resolution == Resolution::Local(2)));
        assert!(summary.references.iter().any(|r| r.name == "z"
            && r.access == Access::Read
            && r.resolution == Resolution::Local(3)));
        assert!(
            summary.references.iter().any(|r| r.name == "x"
                && r.scope_depth == 0
                && r.resolution == Resolution::Local(0))
        );
    }

    #[test]
    fn branch_local_names_do_not_escape_into_other_branches() {
        let summary =
            analyze("if flag then local temp=1; print(temp) else print(temp) end; print(temp)")
                .unwrap();
        let uses: Vec<_> = summary
            .references
            .iter()
            .filter(|r| r.name == "temp")
            .collect();
        assert_eq!(uses.len(), 3);
        assert_eq!(uses[0].resolution, Resolution::Local(0));
        assert_eq!(uses[1].resolution, Resolution::Global);
        assert_eq!(uses[2].resolution, Resolution::Global);
    }

    #[test]
    fn repeat_condition_sees_body_local_but_later_code_does_not() {
        let summary =
            analyze("repeat local inner=1\nprint(inner)\nuntil inner>2\nprint(inner)").unwrap();
        let uses: Vec<_> = summary
            .references
            .iter()
            .filter(|r| r.name == "inner")
            .collect();
        assert_eq!(uses.len(), 3);
        assert_eq!(uses[0].resolution, Resolution::Local(0));
        assert_eq!(uses[1].resolution, Resolution::Local(0));
        assert_eq!(uses[2].resolution, Resolution::Global);
    }

    #[test]
    fn numeric_for_initializer_uses_outer_variable_and_body_shadows_it() {
        let summary = analyze("local i=3; for i=i,10 do print(i) end; return i").unwrap();
        assert_eq!(summary.bindings.len(), 2);
        assert_eq!(summary.bindings[0].scope_depth, 0);
        assert_eq!(summary.bindings[1].scope_depth, 1);
        let uses: Vec<_> = summary
            .references
            .iter()
            .filter(|r| r.name == "i")
            .collect();
        assert_eq!(uses.len(), 3);
        assert_eq!(uses[0].resolution, Resolution::Local(0));
        assert_eq!(uses[1].resolution, Resolution::Local(1));
        assert_eq!(uses[2].resolution, Resolution::Local(0));
    }

    #[test]
    fn generic_for_iterators_do_not_see_new_bindings() {
        let result = analyze("local k=7; for k,v in pairs(k) do print(k,v) end; return k").unwrap();
        assert_eq!(result.bindings.len(), 3);
        assert_eq!(result.bindings[0].scope_depth, 0);
        assert_eq!(result.bindings[1].scope_depth, 1);
        assert_eq!(result.bindings[2].scope_depth, 1);
        let references: Vec<_> = result.references.iter().filter(|r| r.name == "k").collect();
        assert_eq!(references.len(), 3);
        assert_eq!(references[0].resolution, Resolution::Local(0));
        assert_eq!(references[1].resolution, Resolution::Local(1));
        assert_eq!(references[2].resolution, Resolution::Local(0));
    }

    #[test]
    fn method_name_is_not_resolved_as_a_variable() {
        let scope = analyze("local obj=1; obj:run(obj); return obj").unwrap();
        let names: Vec<_> = scope
            .references
            .iter()
            .map(|reference| reference.name)
            .collect();
        assert_eq!(names, vec!["obj", "obj", "obj"]);
        assert!(
            scope
                .references
                .iter()
                .all(|reference| { reference.resolution == Resolution::Local(0) })
        );
    }

    #[test]
    fn table_field_names_are_not_variable_references() {
        let summary = analyze("local a=1; local k=2; local t={named=a,[k]=a,a}; return t").unwrap();
        let names: Vec<_> = summary.references.iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["a", "k", "a", "a", "t"]);
        assert!(!names.contains(&"named"));
    }

    #[test]
    fn nested_closure_captures_are_transitive_and_stable() {
        let summary = analyze(
            "local root=1; local function outer(a) local middle=function(b) return function(c) return root+a+b+c end end; return middle end",
        )
        .unwrap();
        assert_eq!(summary.functions.len(), 3);
        assert_eq!(summary.functions[0].parent, None);
        assert_eq!(summary.functions[1].parent, Some(0));
        assert_eq!(summary.functions[2].parent, Some(1));
        assert_eq!(summary.functions[0].parameters, vec!["a"]);
        assert_eq!(summary.functions[1].parameters, vec!["b"]);
        assert_eq!(summary.functions[2].parameters, vec!["c"]);
        assert_eq!(summary.functions[0].captures, vec![0]);
        assert_eq!(summary.functions[1].captures, vec![0, 2]);
        assert_eq!(summary.functions[2].captures, vec![0, 2, 3]);
    }

    #[test]
    fn named_methods_have_implicit_self_and_read_the_base_table() {
        let summary =
            analyze("local obj=1; function obj:run(arg) return self,arg,obj end; return obj")
                .unwrap();
        let obj_refs: Vec<_> = summary
            .references
            .iter()
            .filter(|r| r.name == "obj")
            .collect();
        assert!(
            obj_refs
                .iter()
                .any(|r| r.access == Access::Read && r.resolution == Resolution::Local(0))
        );
        assert!(
            obj_refs
                .iter()
                .any(|r| r.resolution == Resolution::Upvalue(0))
        );
        let implicit = summary.bindings.iter().find(|b| b.name == "self").unwrap();
        assert!(
            summary
                .references
                .iter()
                .any(|r| r.name == "self" && r.resolution == Resolution::Local(implicit.slot))
        );
        assert!(!summary.references.iter().any(|r| r.name == "run"));
    }

    #[test]
    fn function_parameters_and_closure_captures_have_distinct_resolution() {
        let summary = analyze("local outer=2; local function f(n) if n==0 then return outer end; return f(n-1) end; return f(2)").unwrap();
        let binding = summary.bindings.iter().find(|b| b.name == "f").unwrap();
        let parameter = summary.bindings.iter().find(|b| b.name == "n").unwrap();
        assert!(
            summary
                .references
                .iter()
                .any(|r| r.name == "f" && r.resolution == Resolution::Upvalue(binding.slot))
        );
        assert!(
            summary
                .references
                .iter()
                .any(|r| r.name == "outer" && r.resolution == Resolution::Upvalue(0))
        );
        assert!(
            summary
                .references
                .iter()
                .any(|r| r.name == "n" && r.resolution == Resolution::Local(parameter.slot))
        );
        assert!(summary.references.iter().any(|r| r.name == "f"
            && r.scope_depth == 0
            && r.resolution == Resolution::Local(binding.slot)));
    }

    #[test]
    fn anonymous_function_captures_and_shadowed_parameters() {
        let summary = analyze(
            "local x=10; local f=function(x) return function(y) return x+y end end; return x",
        )
        .unwrap();
        let xs: Vec<_> = summary
            .references
            .iter()
            .filter(|r| r.name == "x")
            .collect();
        assert!(xs.iter().any(|r| r.resolution == Resolution::Upvalue(1)));
        assert!(xs.iter().any(|r| r.resolution == Resolution::Local(0)));
        assert!(!xs.iter().any(|r| r.resolution == Resolution::Upvalue(0)));
    }
}
