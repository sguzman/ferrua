//! Experimental Ferrua-owned control-flow bytecode, separate from production fer.
//! This small stack VM evaluates integer-valued chunks with local variables and
//! Boolean conditions. It is not a complete Lua runtime or the register backend.

use crate::expr::{Expr, Statement, parse_chunk};
use std::collections::HashMap;
use std::fmt;

const MAX_CODE: usize = 16_384;
const MAX_SLOTS: usize = 16_384;
const MAX_STACK: usize = 16_384;
pub const DEFAULT_STEP_BUDGET: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    Integer(i64),
    Boolean(bool),
    Nil,
}

impl Value {
    fn truthy(self) -> bool {
        !matches!(self, Value::Boolean(false) | Value::Nil)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binary {
    Add,
    Sub,
    Mul,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Constant(Value),
    ReadLocal(usize),
    AssignLocals(Vec<usize>),
    Negate,
    Not,
    Binary(Binary),
    JumpIfFalse(usize),
    Jump(usize),
    Return,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowProgram {
    instructions: Vec<Op>,
    slots: usize,
}

impl FlowProgram {
    pub fn instructions(&self) -> &[Op] {
        &self.instructions
    }
    pub fn local_slots(&self) -> usize {
        self.slots
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowError {
    Unsupported(String),
    TooLarge,
    InvalidBytecode,
    TypeError,
    StepBudgetExceeded,
    NoReturn,
}

impl fmt::Display for FlowError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(reason) => write!(out, "unsupported owned flow subset: {reason}"),
            Self::TooLarge => write!(out, "owned flow program exceeds size limit"),
            Self::InvalidBytecode => write!(out, "invalid owned flow bytecode"),
            Self::TypeError => write!(out, "invalid operand type in owned flow bytecode"),
            Self::StepBudgetExceeded => write!(out, "owned flow execution budget exhausted"),
            Self::NoReturn => write!(out, "owned flow program exited without an integer return"),
        }
    }
}
impl std::error::Error for FlowError {}

struct Compiler<'a> {
    code: Vec<Op>,
    frames: Vec<HashMap<&'a str, usize>>,
    slots: usize,
    depth: usize,
}

impl<'a> Compiler<'a> {
    fn push(&mut self, op: Op) -> Result<usize, FlowError> {
        if self.code.len() >= MAX_CODE {
            return Err(FlowError::TooLarge);
        }
        let offset = self.code.len();
        self.code.push(op);
        Ok(offset)
    }

    fn slot(&self, name: &str) -> Result<usize, FlowError> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.get(name).copied())
            .ok_or_else(|| FlowError::Unsupported(format!("global or unbound variable: {name}")))
    }

    fn expression(&mut self, expr: &Expr<'a>, depth: usize) -> Result<(), FlowError> {
        if depth > 128 {
            return Err(FlowError::TooLarge);
        }
        match expr {
            Expr::Atom(text) => {
                let constant = match *text {
                    "true" => Some(Value::Boolean(true)),
                    "false" => Some(Value::Boolean(false)),
                    "nil" => Some(Value::Nil),
                    _ => {
                        let number = if let Some(hex) =
                            text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"))
                        {
                            u64::from_str_radix(hex, 16).map(|bits| bits as i64)
                        } else {
                            text.parse::<i64>()
                        };
                        number.ok().map(Value::Integer)
                    }
                };
                if let Some(value) = constant {
                    self.push(Op::Constant(value))?;
                } else {
                    let slot = self.slot(text)?;
                    self.push(Op::ReadLocal(slot))?;
                }
            }
            Expr::Unary { op, right } if matches!(*op, "-" | "not") => {
                self.expression(right, depth + 1)?;
                self.push(if *op == "-" { Op::Negate } else { Op::Not })?;
            }
            Expr::Binary { op, left, right } => {
                let operation = match *op {
                    "+" => Binary::Add,
                    "-" => Binary::Sub,
                    "*" => Binary::Mul,
                    "<" => Binary::Less,
                    "<=" => Binary::LessEqual,
                    ">" => Binary::Greater,
                    ">=" => Binary::GreaterEqual,
                    "==" => Binary::Equal,
                    "~=" => Binary::NotEqual,
                    _ => return Err(FlowError::Unsupported(format!("binary operator {op}"))),
                };
                self.expression(left, depth + 1)?;
                self.expression(right, depth + 1)?;
                self.push(Op::Binary(operation))?;
            }
            _ => {
                return Err(FlowError::Unsupported(
                    "call, table or other expression".into(),
                ));
            }
        }
        Ok(())
    }

    fn scope(&mut self, body: &[Statement<'a>]) -> Result<(), FlowError> {
        if self.depth >= 64 {
            return Err(FlowError::TooLarge);
        }
        self.frames.push(HashMap::new());
        self.depth += 1;
        let result = self.statements(body);
        self.depth -= 1;
        self.frames.pop();
        result
    }

    fn patch(&mut self, address: usize, target: usize) -> Result<(), FlowError> {
        if target > self.code.len() {
            return Err(FlowError::InvalidBytecode);
        }
        match self.code.get_mut(address) {
            Some(Op::JumpIfFalse(place)) | Some(Op::Jump(place)) => *place = target,
            _ => return Err(FlowError::InvalidBytecode),
        }
        Ok(())
    }

    fn statements(&mut self, nodes: &[Statement<'a>]) -> Result<(), FlowError> {
        for statement in nodes {
            match statement {
                Statement::Local {
                    names,
                    initializers,
                } => {
                    if names.len() != initializers.len() {
                        return Err(FlowError::Unsupported(
                            "every local requires exactly one initializer".into(),
                        ));
                    }
                    // All RHS expressions see the previous scope bindings.
                    for expression in initializers {
                        self.expression(expression, 0)?;
                    }
                    let mut targets = Vec::with_capacity(names.len());
                    for name in names {
                        if self.slots >= MAX_SLOTS {
                            return Err(FlowError::TooLarge);
                        }
                        let slot = self.slots;
                        self.slots += 1;
                        targets.push(slot);
                        self.frames
                            .last_mut()
                            .expect("root scope")
                            .insert(*name, slot);
                    }
                    self.push(Op::AssignLocals(targets))?;
                }
                Statement::Assign { targets, values } => {
                    if targets.len() != values.len() {
                        return Err(FlowError::Unsupported(
                            "assignment RHS count mismatch".into(),
                        ));
                    }
                    // Resolve targets and evaluate RHS before mutating slots.
                    let slots = targets
                        .iter()
                        .map(|target| {
                            let Expr::Atom(name) = target else {
                                return Err(FlowError::Unsupported(
                                    "only local assignment targets".into(),
                                ));
                            };
                            self.slot(name)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    for value in values {
                        self.expression(value, 0)?;
                    }
                    self.push(Op::AssignLocals(slots))?;
                }
                Statement::Return(values) if values.len() == 1 => {
                    self.expression(&values[0], 0)?;
                    self.push(Op::Return)?;
                }
                Statement::Block(body) => self.scope(body)?,
                Statement::If {
                    branches,
                    else_branch,
                } => {
                    let mut ends = Vec::new();
                    for (condition, body) in branches {
                        self.expression(condition, 0)?;
                        let skip = self.push(Op::JumpIfFalse(usize::MAX))?;
                        self.scope(body)?;
                        ends.push(self.push(Op::Jump(usize::MAX))?);
                        self.patch(skip, self.code.len())?;
                    }
                    if let Some(otherwise) = else_branch {
                        self.scope(otherwise)?;
                    }
                    let end = self.code.len();
                    for address in ends {
                        self.patch(address, end)?;
                    }
                }
                Statement::While { condition, body } => {
                    let begin = self.code.len();
                    self.expression(condition, 0)?;
                    let exit = self.push(Op::JumpIfFalse(usize::MAX))?;
                    self.scope(body)?;
                    self.push(Op::Jump(begin))?;
                    self.patch(exit, self.code.len())?;
                }
                Statement::Repeat { body, condition } => {
                    if self.depth >= 64 {
                        return Err(FlowError::TooLarge);
                    }
                    let begin = self.code.len();
                    self.frames.push(HashMap::new());
                    self.depth += 1;
                    let result = (|| {
                        self.statements(body)?;
                        // Lua until conditions see locals from the repeat body.
                        self.expression(condition, 0)
                    })();
                    self.depth -= 1;
                    self.frames.pop();
                    result?;
                    self.push(Op::JumpIfFalse(begin))?;
                }
                _ => {
                    return Err(FlowError::Unsupported(
                        "loop type, call, or return arity".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Compile the independent control-flow subset to executable stack bytecode.
/// Successful parsing does not imply full Lua grammar or runtime conformance.
pub fn compile_flow_chunk(source: &str) -> Result<FlowProgram, FlowError> {
    let statements = parse_chunk(source).map_err(|err| FlowError::Unsupported(err.to_string()))?;
    let mut compiler = Compiler {
        code: Vec::new(),
        frames: vec![HashMap::new()],
        slots: 0,
        depth: 0,
    };
    compiler.statements(&statements)?;
    if !compiler.code.iter().any(|op| matches!(op, Op::Return)) {
        return Err(FlowError::NoReturn);
    }
    Ok(FlowProgram {
        instructions: compiler.code,
        slots: compiler.slots,
    })
}

fn evaluate_binary(op: Binary, left: Value, right: Value) -> Result<Value, FlowError> {
    use Binary::*;
    use Value::*;
    match (op, left, right) {
        (Add, Integer(a), Integer(b)) => Ok(Integer(a.wrapping_add(b))),
        (Sub, Integer(a), Integer(b)) => Ok(Integer(a.wrapping_sub(b))),
        (Mul, Integer(a), Integer(b)) => Ok(Integer(a.wrapping_mul(b))),
        (Less, Integer(a), Integer(b)) => Ok(Boolean(a < b)),
        (LessEqual, Integer(a), Integer(b)) => Ok(Boolean(a <= b)),
        (Greater, Integer(a), Integer(b)) => Ok(Boolean(a > b)),
        (GreaterEqual, Integer(a), Integer(b)) => Ok(Boolean(a >= b)),
        (Equal, a, b) => Ok(Boolean(a == b)),
        (NotEqual, a, b) => Ok(Boolean(a != b)),
        _ => Err(FlowError::TypeError),
    }
}

/// Run owned control-flow bytecode with a strict instruction budget.
/// No syscalls, global environment, host callbacks, or I/O.
pub fn execute_flow_chunk(program: &FlowProgram, budget: usize) -> Result<i64, FlowError> {
    if program.instructions.len() > MAX_CODE || program.slots > MAX_SLOTS {
        return Err(FlowError::InvalidBytecode);
    }
    let mut stack = Vec::new();
    let mut slots: Vec<Option<Value>> = vec![None; program.slots];
    let mut pc = 0;
    let mut steps = 0;
    while pc < program.instructions.len() {
        if steps >= budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        steps += 1;
        let next = pc + 1;
        match &program.instructions[pc] {
            Op::Constant(value) => stack.push(*value),
            Op::ReadLocal(slot) => {
                let value = slots
                    .get(*slot)
                    .copied()
                    .flatten()
                    .ok_or(FlowError::InvalidBytecode)?;
                stack.push(value);
            }
            Op::AssignLocals(indices) => {
                if stack.len() < indices.len() {
                    return Err(FlowError::InvalidBytecode);
                }
                let values = stack.split_off(stack.len() - indices.len());
                for (index, value) in indices.iter().zip(values) {
                    *slots.get_mut(*index).ok_or(FlowError::InvalidBytecode)? = Some(value);
                }
            }
            Op::Negate => {
                let Value::Integer(value) = stack.pop().ok_or(FlowError::InvalidBytecode)? else {
                    return Err(FlowError::TypeError);
                };
                stack.push(Value::Integer(value.wrapping_neg()));
            }
            Op::Not => {
                let value = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                stack.push(Value::Boolean(!value.truthy()));
            }
            Op::Binary(op) => {
                let rhs = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                let lhs = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                stack.push(evaluate_binary(*op, lhs, rhs)?);
            }
            Op::JumpIfFalse(target) => {
                let value = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                if !value.truthy() {
                    if *target > program.instructions.len() {
                        return Err(FlowError::InvalidBytecode);
                    }
                    pc = *target;
                    continue;
                }
            }
            Op::Jump(target) => {
                if *target > program.instructions.len() {
                    return Err(FlowError::InvalidBytecode);
                }
                pc = *target;
                continue;
            }
            Op::Return => {
                let Value::Integer(value) = stack.pop().ok_or(FlowError::InvalidBytecode)? else {
                    return Err(FlowError::TypeError);
                };
                return Ok(value);
            }
        }
        if stack.len() > MAX_STACK {
            return Err(FlowError::TooLarge);
        }
        pc = next;
    }
    Err(FlowError::NoReturn)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn evaluate(source: &str) -> i64 {
        let compiled = compile_flow_chunk(source).unwrap();
        execute_flow_chunk(&compiled, DEFAULT_STEP_BUDGET).unwrap()
    }

    #[test]
    fn runs_while_loops_and_parallel_assignments() {
        assert_eq!(evaluate("local n=0; while n<5 do n=n+1 end; return n"), 5);
        assert_eq!(
            evaluate(
                "local a=1; local b=1; local n=0; while n<6 do a,b=b,a+b; n=n+1 end; return b"
            ),
            21
        );
        assert_eq!(evaluate("local x=0; while x<3 do x=x+1 end; return x"), 3);
    }

    #[test]
    fn branches_use_lua_truthiness_and_scoped_locals() {
        assert_eq!(
            evaluate("local x=0; if x==0 then x=9 else x=1 end; return x"),
            9
        );
        assert_eq!(evaluate("if 0 then return 7 else return 9 end"), 7);
        assert_eq!(
            evaluate("if false then return 7 elseif true then return 8 else return 9 end"),
            8
        );
        assert_eq!(
            evaluate("if not false then return 12 else return 13 end"),
            12
        );
        assert_eq!(
            evaluate("local x=2; if x<0 then return 1 elseif x<=2 then return 8 else return 5 end"),
            8
        );
    }

    #[test]
    fn repeat_until_condition_sees_body_scope() {
        assert_eq!(
            evaluate("local x=0; repeat local next=x+1; x=next until next>=4; return x"),
            4
        );
        assert_eq!(evaluate("local x=3; repeat x=x-1 until x==0; return x"), 0);
    }

    #[test]
    fn budget_prevents_infinite_loops() {
        let compiled = compile_flow_chunk("while true do end; return 3").unwrap();
        assert_eq!(
            execute_flow_chunk(&compiled, 50),
            Err(FlowError::StepBudgetExceeded)
        );
        assert_eq!(
            execute_flow_chunk(&compiled, 0),
            Err(FlowError::StepBudgetExceeded)
        );
    }

    #[test]
    fn rejects_unsupported_and_unbound_forms() {
        for source in [
            "x=1; return x",
            "return print(3)",
            "return 1.2",
            "local x=1; for i=1,5 do x=x+i end; return x",
            "local x=1; return x//2",
            "return true",
            "local x=1; local y; return x",
            "local x=1; return x and true",
        ] {
            match compile_flow_chunk(source) {
                Err(_) => {}
                Ok(program) => assert!(execute_flow_chunk(&program, 100).is_err(), "{source}"),
            }
        }
    }

    #[test]
    fn missing_returns_cannot_be_mistaken_for_success() {
        assert!(matches!(
            compile_flow_chunk("local n=1"),
            Err(FlowError::NoReturn)
        ));
        let program = compile_flow_chunk("if false then return 1 end").unwrap();
        assert_eq!(execute_flow_chunk(&program, 100), Err(FlowError::NoReturn));
    }
}
