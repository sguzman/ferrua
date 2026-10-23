//! Experimental Ferrua-owned control-flow bytecode, separate from production fer.
//! This bounded stack VM evaluates integer, Boolean and nil Lua values with
//! lexical locals and control flow. It is not a complete Lua runtime or the register backend.

use crate::expr::{Expr, Statement, TableField, parse_chunk};
use crate::owned_vm::{floor_div, lua_mod, shift};
use std::collections::{HashMap, HashSet};
use std::fmt;

const MAX_CODE: usize = 16_384;
const MAX_SLOTS: usize = 16_384;
const MAX_STACK: usize = 16_384;
const MAX_TABLES: usize = 16_384;
const MAX_TABLE_FIELDS: usize = 16_384;
const MAX_TABLE_CELLS_TOTAL: usize = 65_536;
pub const DEFAULT_STEP_BUDGET: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    Integer(i64),
    Boolean(bool),
    Nil,
    /// Opaque index into the per-execution bounded heap.
    Table(usize),
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
    FloorDiv,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
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
    NewTable(usize),
    GetTable,
    SetTable,
    Negate,
    Not,
    BitNot,
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
    ReturnMany(usize),
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
    DivisionByZero,
    ReturnArity,
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
            Self::DivisionByZero => write!(out, "integer division by zero"),
            Self::ReturnArity => write!(out, "expected exactly one return value"),
            Self::StepBudgetExceeded => write!(out, "owned flow execution budget exhausted"),
            Self::InvalidForStep => write!(out, "numeric for step must not be zero"),
            Self::NoReturn => write!(out, "owned flow program exited without a return"),
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
    loop_breaks: Vec<Vec<usize>>,
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
            Expr::Unary { op, right } if matches!(*op, "-" | "not" | "~") => {
                self.expression(right, depth + 1)?;
                self.push(match *op {
                    "-" => Op::Negate,
                    "not" => Op::Not,
                    "~" => Op::BitNot,
                    _ => unreachable!("guarded unary operator"),
                })?;
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
            Expr::Index { object, key } => {
                self.expression(object, depth + 1)?;
                self.expression(key, depth + 1)?;
                self.push(Op::GetTable)?;
            }
            Expr::Table(fields) => {
                for field in fields {
                    let TableField::Value(value) = field else {
                        return Err(FlowError::Unsupported(
                            "only positional integer-key table fields".into(),
                        ));
                    };
                    self.expression(value, depth + 1)?;
                }
                self.push(Op::NewTable(fields.len()))?;
            }
            Expr::Binary { op, left, right } => {
                let operation = match *op {
                    "+" => Binary::Add,
                    "-" => Binary::Sub,
                    "*" => Binary::Mul,
                    "//" => Binary::FloorDiv,
                    "%" => Binary::Mod,
                    "&" => Binary::BitAnd,
                    "|" => Binary::BitOr,
                    "~" => Binary::BitXor,
                    "<<" => Binary::Shl,
                    ">>" => Binary::Shr,
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

    // Lua evaluates every explicit RHS expression, then truncates surplus
    // values or supplies nil for missing targets. Calls and multireturns
    // remain unsupported in this intentionally bounded prototype.
    fn values(&mut self, expressions: &[Expr<'a>], targets: usize) -> Result<(), FlowError> {
        for expression in expressions {
            self.expression(expression, 0)?;
        }
        for _ in targets..expressions.len() {
            self.push(Op::Discard)?;
        }
        for _ in expressions.len()..targets {
            self.push(Op::Constant(Value::Nil))?;
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
                    // Initializers see only previous bindings, including
                    // when extra RHS values are subsequently discarded.
                    self.values(initializers, names.len())?;
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
                    if let ([Expr::Index { object, key }], [value]) =
                        (targets.as_slice(), values.as_slice())
                    {
                        self.expression(object, 0)?;
                        self.expression(key, 0)?;
                        self.expression(value, 0)?;
                        self.push(Op::SetTable)?;
                        continue;
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
                    self.values(values, slots.len())?;
                    self.push(Op::AssignLocals(slots))?;
                }
                Statement::Return(values) => {
                    for value in values {
                        self.expression(value, 0)?;
                    }
                    if values.len() == 1 {
                        self.push(Op::Return)?;
                    } else {
                        self.push(Op::ReturnMany(values.len()))?;
                    }
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
                    self.loop_breaks.push(Vec::new());
                    let compiled = self.scope(body);
                    let breaks = self.loop_breaks.pop().expect("while loop break scope");
                    compiled?;
                    self.push(Op::Jump(begin))?;
                    let end = self.code.len();
                    self.patch(exit, end)?;
                    for address in breaks {
                        self.patch(address, end)?;
                    }
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
                    self.loop_breaks.push(Vec::new());
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
                    let breaks = self.loop_breaks.pop().expect("numeric for break scope");
                    self.depth -= 1;
                    self.read_only.remove(&counter);
                    self.frames.pop();
                    compiled?;
                    let end = self.code.len();
                    for address in breaks {
                        self.patch(address, end)?;
                    }
                }
                Statement::Repeat { body, condition } => {
                    if self.depth >= 64 {
                        return Err(FlowError::TooLarge);
                    }
                    let begin = self.code.len();
                    self.frames.push(HashMap::new());
                    self.depth += 1;
                    self.loop_breaks.push(Vec::new());
                    let result = (|| {
                        self.statements(body)?;
                        // Lua until conditions see locals from the repeat body.
                        self.expression(condition, 0)
                    })();
                    let breaks = self.loop_breaks.pop().expect("repeat break scope");
                    self.depth -= 1;
                    self.frames.pop();
                    result?;
                    self.push(Op::JumpIfFalse(begin))?;
                    let end = self.code.len();
                    for address in breaks {
                        self.patch(address, end)?;
                    }
                }
                Statement::Break => {
                    if self.loop_breaks.is_empty() {
                        return Err(FlowError::Unsupported("break outside loop".into()));
                    }
                    let address = self.push(Op::Jump(usize::MAX))?;
                    self.loop_breaks
                        .last_mut()
                        .expect("loop break scope")
                        .push(address);
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
        loop_breaks: Vec::new(),
    };
    compiler.statements(&statements)?;
    if !compiler
        .code
        .iter()
        .any(|op| matches!(op, Op::Return | Op::ReturnMany(_)))
    {
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
        (FloorDiv, Integer(a), Integer(b)) => Ok(Integer(
            floor_div(a, b).map_err(|_| FlowError::DivisionByZero)?,
        )),
        (Mod, Integer(a), Integer(b)) => Ok(Integer(
            lua_mod(a, b).map_err(|_| FlowError::DivisionByZero)?,
        )),
        (BitAnd, Integer(a), Integer(b)) => Ok(Integer(a & b)),
        (BitOr, Integer(a), Integer(b)) => Ok(Integer(a | b)),
        (BitXor, Integer(a), Integer(b)) => Ok(Integer(a ^ b)),
        (Shl, Integer(a), Integer(b)) => Ok(Integer(shift(a, b, true))),
        (Shr, Integer(a), Integer(b)) => Ok(Integer(shift(a, b, false))),
        (Less, Integer(a), Integer(b)) => Ok(Boolean(a < b)),
        (LessEqual, Integer(a), Integer(b)) => Ok(Boolean(a <= b)),
        (Greater, Integer(a), Integer(b)) => Ok(Boolean(a > b)),
        (GreaterEqual, Integer(a), Integer(b)) => Ok(Boolean(a >= b)),
        (Equal, a, b) => Ok(Boolean(a == b)),
        (NotEqual, a, b) => Ok(Boolean(a != b)),
        _ => Err(FlowError::TypeError),
    }
}

/// Run owned control-flow bytecode and return fixed-count supported Lua values.
/// No syscalls, global environment, host callbacks, or I/O.
pub fn execute_flow_values(program: &FlowProgram, budget: usize) -> Result<Vec<Value>, FlowError> {
    if program.instructions.len() > MAX_CODE || program.slots > MAX_SLOTS {
        return Err(FlowError::InvalidBytecode);
    }
    let mut stack = Vec::new();
    let mut slots: Vec<Option<Value>> = vec![None; program.slots];
    let mut tables: Vec<Vec<Option<Value>>> = Vec::new();
    let mut table_cells = 0usize;
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
            Op::NewTable(count) => {
                if *count > MAX_TABLE_FIELDS
                    || tables.len() >= MAX_TABLES
                    || table_cells.saturating_add(*count) > MAX_TABLE_CELLS_TOTAL
                    || stack.len() < *count
                {
                    return Err(FlowError::TooLarge);
                }
                let values = stack.split_off(stack.len() - *count);
                let handle = tables.len();
                tables.push(values.into_iter().map(Some).collect());
                table_cells += *count;
                stack.push(Value::Table(handle));
            }
            Op::GetTable => {
                let key = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                let table = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                let (Value::Table(handle), Value::Integer(index)) = (table, key) else {
                    return Err(FlowError::TypeError);
                };
                let field = usize::try_from(index).map_err(|_| FlowError::TypeError)?;
                if field == 0 || field > MAX_TABLE_FIELDS {
                    return Err(FlowError::TypeError);
                }
                let values = tables.get(handle).ok_or(FlowError::InvalidBytecode)?;
                stack.push(
                    values
                        .get(field - 1)
                        .copied()
                        .flatten()
                        .unwrap_or(Value::Nil),
                );
            }
            Op::SetTable => {
                let value = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                let key = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                let table = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                let (Value::Table(handle), Value::Integer(index)) = (table, key) else {
                    return Err(FlowError::TypeError);
                };
                let field = usize::try_from(index).map_err(|_| FlowError::TypeError)?;
                if field == 0 || field > MAX_TABLE_FIELDS {
                    return Err(FlowError::TypeError);
                }
                let values = tables.get_mut(handle).ok_or(FlowError::InvalidBytecode)?;
                if field > values.len() {
                    let growth = field - values.len();
                    if table_cells.saturating_add(growth) > MAX_TABLE_CELLS_TOTAL {
                        return Err(FlowError::TooLarge);
                    }
                    table_cells += growth;
                    values.resize(field, None);
                }
                values[field - 1] = Some(value);
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
            Op::BitNot => {
                let Value::Integer(value) = stack.pop().ok_or(FlowError::InvalidBytecode)? else {
                    return Err(FlowError::TypeError);
                };
                stack.push(Value::Integer(!value));
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
                return Ok(vec![stack.pop().ok_or(FlowError::InvalidBytecode)?]);
            }
            Op::ReturnMany(count) => {
                if stack.len() < *count {
                    return Err(FlowError::InvalidBytecode);
                }
                return Ok(stack.split_off(stack.len() - *count));
            }
        }
        if stack.len() > MAX_STACK {
            return Err(FlowError::TooLarge);
        }
        pc = next;
    }
    Err(FlowError::NoReturn)
}

/// Preserve the single-value API for existing consumers. Zero or multiple
/// result values are rejected instead of silently discarding their contents.
pub fn execute_flow_value(program: &FlowProgram, budget: usize) -> Result<Value, FlowError> {
    let mut values = execute_flow_values(program, budget)?;
    if values.len() != 1 {
        return Err(FlowError::ReturnArity);
    }
    Ok(values.pop().expect("exactly one result"))
}

/// Retain the original integer-only API for existing consumers. Non-integer
/// returns remain a type error here; use execute_flow_value for typed values.
pub fn execute_flow_chunk(program: &FlowProgram, budget: usize) -> Result<i64, FlowError> {
    match execute_flow_value(program, budget)? {
        Value::Integer(value) => Ok(value),
        _ => Err(FlowError::TypeError),
    }
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
        assert_eq!(
            evaluate("if false and (1+true) then return 99 else return 5 end"),
            5
        );
        assert_eq!(
            evaluate("if true or (1+true) then return 12 else return 9 end"),
            12
        );
        assert_eq!(
            evaluate("if 0 and not nil then return 6 else return 8 end"),
            6
        );
        assert_eq!(
            evaluate("if nil or false then return 1 else return 3 end"),
            3
        );
    }

    #[test]
    fn control_flow_reuses_register_vm_integer_semantics() {
        assert_eq!(evaluate("return -7//3"), -3);
        assert_eq!(evaluate("return -7%3"), 2);
        assert_eq!(evaluate("return 7%-3"), -2);
        assert_eq!(evaluate("return (5&3)|8"), 9);
        assert_eq!(evaluate("return (7~3)<<2"), 16);
        assert_eq!(evaluate("return 0x8000000000000000>>63"), 1);
        assert_eq!(evaluate("return ~0"), -1);
        assert_eq!(evaluate("return 8<<-1"), 4);
        assert_eq!(
            evaluate("local x=-7; if x//3==-3 and x%3==2 then return 1 else return 0 end"),
            1
        );
        assert_eq!(evaluate("return false and (1//0) or 4"), 4);
        assert_eq!(evaluate("if true then return 5 else return 1//0 end"), 5);
        for source in ["return 1//0", "return 1%0"] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_chunk(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::DivisionByZero)
            );
        }
        let program = compile_flow_chunk("return true & 1").unwrap();
        assert_eq!(
            execute_flow_chunk(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
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
    fn bounded_array_tables_preserve_reference_identity_and_missing_nil() {
        assert_eq!(evaluate("local t={4,5}; return t[2]"), 5);
        assert_eq!(evaluate("local t={4}; t[1]=7; return t[1]"), 7);
        assert_eq!(
            evaluate("local t={2}; local alias=t; alias[1]=8; return t[1]"),
            8
        );
        assert_eq!(evaluate("local t={1}; return t[9] or 3"), 3);
        assert_eq!(evaluate("local t={{2}}; return t[1][1]"), 2);
        assert_eq!(
            evaluate("local a={1}; local b=a; return (a==b) and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("local a={1}; local b={1}; return (a==b) and 1 or 0"),
            0
        );
    }

    #[test]
    fn bounded_tables_reject_unsupported_keys_and_fields() {
        assert!(compile_flow_chunk("local t={name=2}; return 1").is_err());
        for source in [
            "local t={1}; t[0]=7; return 1",
            "local t={1}; return t[true]",
            "local t=1; return t[1]",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_chunk(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError)
            );
        }
    }

    #[test]
    fn returns_fixed_count_lua_values_and_preserves_single_value_apis() {
        for (source, values) in [
            ("return", vec![]),
            (
                "return 1,2,3",
                vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)],
            ),
            (
                "return nil,7,false",
                vec![Value::Nil, Value::Integer(7), Value::Boolean(false)],
            ),
            (
                "local x,y=2,3; return y,x",
                vec![Value::Integer(3), Value::Integer(2)],
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Ok(values),
                "{source}"
            );
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::ReturnArity),
                "{source}"
            );
        }
        assert_eq!(
            execute_flow_value(
                &compile_flow_chunk("return true").unwrap(),
                DEFAULT_STEP_BUDGET
            ),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn returns_lua_boolean_nil_and_lazy_operand_values() {
        for (source, expected) in [
            ("return true", Value::Boolean(true)),
            ("return false", Value::Boolean(false)),
            ("return nil", Value::Nil),
            ("return false and (1//0)", Value::Boolean(false)),
            ("return true or (1//0)", Value::Boolean(true)),
            ("return nil or false", Value::Boolean(false)),
            ("return 0 and true", Value::Boolean(true)),
            ("return not 0", Value::Boolean(false)),
            ("return 5>3", Value::Boolean(true)),
            ("local x; return x", Value::Nil),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Ok(expected),
                "{source}"
            );
        }
        let compiled = compile_flow_chunk("return true").unwrap();
        assert_eq!(
            execute_flow_chunk(&compiled, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
    }

    #[test]
    fn lua_assignment_arity_and_nil_padding() {
        assert_eq!(
            evaluate("local x; if x==nil then return 6 else return 0 end"),
            6
        );
        assert_eq!(evaluate("local x=1,2; return x"), 1);
        assert_eq!(
            evaluate("local x,y=3; if y==nil then return x else return 0 end"),
            3
        );
        assert_eq!(
            evaluate("local x,y=3,4; x,y=7; if y==nil then return x else return 0 end"),
            7
        );
        assert_eq!(evaluate("local x,y=3,4; x=7,8; return x"), 7);
        assert_eq!(evaluate("local x,y=1,2; x,y=3,4,5; return x+y"), 7);
        for source in ["local x=1,1//0; return x", "local x=2; x=3,1//0; return x"] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_chunk(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::DivisionByZero)
            );
        }
    }

    #[test]
    fn break_targets_innermost_loop_and_skips_repeat_condition() {
        assert_eq!(
            evaluate("local x=0; while true do x=x+1; if x==3 then break end end; return x"),
            3
        );
        assert_eq!(
            evaluate("local x=0; repeat x=x+1; if x==2 then break end until x==100; return x"),
            2
        );
        assert_eq!(
            evaluate("local s=0; for i=1,10 do if i==4 then break end; s=s+i end; return s"),
            6
        );
        assert_eq!(
            evaluate(
                "local x=0; while x<3 do local j=0; while true do j=j+1; break end; x=x+j end; return x"
            ),
            3
        );
        assert_eq!(evaluate("while true do break end; return 6"), 6);
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
            "return true",
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
