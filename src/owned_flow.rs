//! Experimental Ferrua-owned control-flow bytecode, separate from production fer.
//! This small stack VM evaluates integer-valued chunks with local variables and
//! Boolean conditions. It is not a complete Lua runtime or the register backend.

use crate::expr::{Expr, Statement, parse_chunk};
use std::collections::{HashMap, HashSet};
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
    JumpIfFalseKeep(usize),
    JumpIfTrueKeep(usize),
    Discard,
    Jump(usize),
    ForCheck {
        counter: usize,
        limit: usize,
        step: usize,
        exit: usize,
    },
    ForStep {
        counter: usize,
        step: usize,
        exit: usize,
    },
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
    InvalidForStep,
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
            Self::InvalidForStep => write!(out, "numeric for step must not be zero"),
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
    read_only: HashSet<usize>,
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
            Expr::Binary { op, left, right } if matches!(*op, "and" | "or") => {
                // Lua logical operators keep the chosen operand value.
                self.expression(left, depth + 1)?;
                let skip = self.push(if *op == "and" {
                    Op::JumpIfFalseKeep(usize::MAX)
                } else {
                    Op::JumpIfTrueKeep(usize::MAX)
                })?;
                self.push(Op::Discard)?;
                self.expression(right, depth + 1)?;
                self.patch(skip, self.code.len())?;
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
            Some(Op::JumpIfFalse(place))
            | Some(Op::JumpIfFalseKeep(place))
            | Some(Op::JumpIfTrueKeep(place))
            | Some(Op::Jump(place))
            | Some(Op::ForCheck { exit: place, .. })
            | Some(Op::ForStep { exit: place, .. }) => *place = target,
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
                            let slot = self.slot(name)?;
                            if self.read_only.contains(&slot) {
                                return Err(FlowError::Unsupported(
                                    "numeric for control variable is read-only".into(),
                                ));
                            }
                            Ok(slot)
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
                Statement::NumericFor {
                    variable,
                    start,
                    limit,
                    step,
                    body,
                } => {
                    if self.depth >= 64 || self.slots > MAX_SLOTS.saturating_sub(3) {
                        return Err(FlowError::TooLarge);
                    }
                    // Lua numeric controls evaluate once, before introducing
                    // the read-only loop variable in its own lexical scope.
                    self.expression(start, 0)?;
                    self.expression(limit, 0)?;
                    if let Some(expression) = step {
                        self.expression(expression, 0)?;
                    } else {
                        self.push(Op::Constant(Value::Integer(1)))?;
                    }
                    let counter = self.slots;
                    let bound = counter + 1;
                    let stride = counter + 2;
                    self.slots += 3;
                    self.push(Op::AssignLocals(vec![counter, bound, stride]))?;
                    self.frames.push(HashMap::new());
                    self.frames
                        .last_mut()
                        .expect("numeric for scope")
                        .insert(*variable, counter);
                    self.read_only.insert(counter);
                    self.depth += 1;
                    let compiled = (|| {
                        let begin = self.code.len();
                        let check = self.push(Op::ForCheck {
                            counter,
                            limit: bound,
                            step: stride,
                            exit: usize::MAX,
                        })?;
                        self.scope(body)?;
                        let next = self.push(Op::ForStep {
                            counter,
                            step: stride,
                            exit: usize::MAX,
                        })?;
                        self.push(Op::Jump(begin))?;
                        let exit = self.code.len();
                        self.patch(check, exit)?;
                        self.patch(next, exit)
                    })();
                    self.depth -= 1;
                    self.read_only.remove(&counter);
                    self.frames.pop();
                    compiled?;
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
        read_only: HashSet::new(),
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
            Op::JumpIfFalseKeep(target) | Op::JumpIfTrueKeep(target) => {
                let value = *stack.last().ok_or(FlowError::InvalidBytecode)?;
                let should_jump = match program.instructions[pc] {
                    Op::JumpIfFalseKeep(_) => !value.truthy(),
                    Op::JumpIfTrueKeep(_) => value.truthy(),
                    _ => unreachable!("guarded opcode"),
                };
                if should_jump {
                    if *target > program.instructions.len() {
                        return Err(FlowError::InvalidBytecode);
                    }
                    pc = *target;
                    continue;
                }
            }
            Op::Discard => {
                stack.pop().ok_or(FlowError::InvalidBytecode)?;
            }
            Op::Jump(target) => {
                if *target > program.instructions.len() {
                    return Err(FlowError::InvalidBytecode);
                }
                pc = *target;
                continue;
            }
            Op::ForCheck {
                counter,
                limit,
                step,
                exit,
            } => {
                if *exit > program.instructions.len() {
                    return Err(FlowError::InvalidBytecode);
                }
                let read = |slot: usize| -> Result<i64, FlowError> {
                    match slots.get(slot).copied().flatten() {
                        Some(Value::Integer(value)) => Ok(value),
                        Some(_) => Err(FlowError::TypeError),
                        None => Err(FlowError::InvalidBytecode),
                    }
                };
                let current = read(*counter)?;
                let bound = read(*limit)?;
                let stride = read(*step)?;
                if stride == 0 {
                    return Err(FlowError::InvalidForStep);
                }
                let enters = if stride > 0 {
                    current <= bound
                } else {
                    current >= bound
                };
                if !enters {
                    pc = *exit;
                    continue;
                }
            }
            Op::ForStep {
                counter,
                step,
                exit,
            } => {
                if *exit > program.instructions.len() {
                    return Err(FlowError::InvalidBytecode);
                }
                let current = match slots.get(*counter).copied().flatten() {
                    Some(Value::Integer(value)) => value,
                    Some(_) => return Err(FlowError::TypeError),
                    None => return Err(FlowError::InvalidBytecode),
                };
                let stride = match slots.get(*step).copied().flatten() {
                    Some(Value::Integer(value)) => value,
                    Some(_) => return Err(FlowError::TypeError),
                    None => return Err(FlowError::InvalidBytecode),
                };
                if let Some(updated) = current.checked_add(stride) {
                    *slots.get_mut(*counter).ok_or(FlowError::InvalidBytecode)? =
                        Some(Value::Integer(updated));
                } else {
                    pc = *exit;
                    continue;
                }
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
    fn logical_and_or_preserve_values_and_short_circuit() {
        assert_eq!(evaluate("return 0 or 99"), 0);
        assert_eq!(evaluate("return false or 9"), 9);
        assert_eq!(evaluate("return 3 and 7"), 7);
        assert_eq!(evaluate("return nil or 12"), 12);
        assert_eq!(evaluate("return false or 0 or 8"), 0);
        assert_eq!(evaluate("if false and (1+true) then return 99 else return 5 end"), 5);
        assert_eq!(evaluate("if true or (1+true) then return 12 else return 9 end"), 12);
        assert_eq!(evaluate("if 0 and not nil then return 6 else return 8 end"), 6);
        assert_eq!(evaluate("if nil or false then return 1 else return 3 end"), 3);
    }

    #[test]
    fn numeric_for_supports_both_directions_and_precomputed_bounds() {
        assert_eq!(evaluate("local s=0; for i=1,5 do s=s+i end; return s"), 15);
        assert_eq!(
            evaluate("local s=0; for i=6,2,-2 do s=s+i end; return s"),
            12
        );
        assert_eq!(
            evaluate("local s=0; for i=1,10,3 do s=s+i end; return s"),
            22
        );
        assert_eq!(evaluate("local s=0; for i=5,1 do s=s+1 end; return s"), 0);
        assert_eq!(
            evaluate("local s=0; local i=3; for i=i,5 do s=s+i end; return s+i"),
            15
        );
        assert_eq!(
            evaluate("local s=0; for i=1,3 do local x=i; s=s+x end; return s"),
            6
        );
    }

    #[test]
    fn numeric_for_rejects_mutation_and_zero_step() {
        assert!(compile_flow_chunk("for i=1,3 do i=2 end; return 1").is_err());
        let compiled = compile_flow_chunk("for i=1,3,0 do end; return 0").unwrap();
        assert_eq!(
            execute_flow_chunk(&compiled, 40),
            Err(FlowError::InvalidForStep)
        );
    }

    #[test]
    fn numeric_for_overflow_does_not_wrap_into_another_cycle() {
        assert_eq!(
            evaluate(
                "local s=0; for i=0x7ffffffffffffffe,0x7fffffffffffffff do s=s+1 end; return s"
            ),
            2
        );
        assert_eq!(
            evaluate(
                "local s=0; for i=-9223372036854775807,-9223372036854775807,-2 do s=s+1 end; return s"
            ),
            1
        );
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
            "for k in pairs(t) do return 1 end; return 0",
            "local x=1; return x//2",
            "return true",
            "local x=1; local y; return x",
            "local x=1; return x//2",
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
