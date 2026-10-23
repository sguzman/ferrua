//! Experimental Ferrua-owned control-flow bytecode, separate from production fer.
//! This bounded stack VM evaluates integer, Boolean and nil Lua values with
//! lexical locals and control flow. It is not a complete Lua runtime or the register backend.

use crate::expr::{Expr, Statement, TableField, parse_chunk};
use crate::owned_vm::{floor_div, lua_mod, shift};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

const MAX_CODE: usize = 16_384;
const MAX_SLOTS: usize = 16_384;
const MAX_STACK: usize = 16_384;
const MAX_TABLES: usize = 16_384;
const MAX_TABLE_FIELDS: usize = 16_384;
const MAX_TABLE_CELLS_TOTAL: usize = 65_536;
const MAX_CLOSURES: usize = 16_384;
const MAX_CALL_DEPTH: usize = 64;
const MAX_CELLS_TOTAL: usize = 65_536;
pub const DEFAULT_STEP_BUDGET: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    Integer(i64),
    Boolean(bool),
    Nil,
    /// Opaque index into the per-execution bounded heap.
    Table(usize),
    /// Opaque, execution-local function identity.
    Function(usize),
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
    PrepareLocal(usize),
    PrepareIndexed,
    AssignPrepared(usize),
    NewTable(usize),
    Duplicate,
    SetTableTail(usize),
    NewFunction(usize),
    Call(usize),
    CallMany(usize),
    CallSpread {
        prefix: usize,
        many: bool,
    },
    NewTableTail(usize),
    VarargSingle,
    VarargMany,
    AdjustTail {
        prefix: usize,
        targets: usize,
    },
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
        variable: usize,
        exit: usize,
    },
    ForStep {
        counter: usize,
        step: usize,
        exit: usize,
    },
    GenericForCheck {
        control: usize,
        variables: Vec<usize>,
        exit: usize,
    },
    Return,
    ReturnMany(usize),
    ReturnTail(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowProgram {
    instructions: Vec<Op>,
    slots: usize,
    functions: Vec<FunctionProgram>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FunctionProgram {
    instructions: Vec<Op>,
    slots: usize,
    parameters: Vec<usize>,
    captures: Vec<(usize, usize)>,
    vararg: bool,
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
    CallDepthExceeded,
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
            Self::CallDepthExceeded => write!(out, "owned flow call depth limit exceeded"),
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
    functions: Rc<RefCell<Vec<FunctionProgram>>>,
    vararg: bool,
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

    fn compile_function(
        &mut self,
        parameters: &[&'a str],
        vararg: bool,
        body: &[Statement<'a>],
    ) -> Result<usize, FlowError> {
        let id = self.functions.borrow().len();
        if id >= MAX_CLOSURES {
            return Err(FlowError::TooLarge);
        }
        self.functions.borrow_mut().push(FunctionProgram {
            instructions: Vec::new(),
            slots: 0,
            parameters: Vec::new(),
            captures: Vec::new(),
            vararg: false,
        });
        let mut child = Self {
            code: Vec::new(),
            frames: vec![HashMap::new()],
            slots: 0,
            depth: 0,
            read_only: HashSet::new(),
            loop_breaks: Vec::new(),
            functions: Rc::clone(&self.functions),
            vararg,
        };
        // Eagerly and deterministically capture visible locals; shadowing
        // resolves from outer to inner scope.
        let mut visible = BTreeMap::new();
        for frame in &self.frames {
            for (&name, &slot) in frame {
                visible.insert(name, slot);
            }
        }
        let mut captures = Vec::new();
        for (name, parent_slot) in visible {
            if child.slots >= MAX_SLOTS {
                return Err(FlowError::TooLarge);
            }
            let child_slot = child.slots;
            child.slots += 1;
            child.frames[0].insert(name, child_slot);
            captures.push((parent_slot, child_slot));
        }
        let mut formal_slots = Vec::new();
        for name in parameters {
            if child.slots >= MAX_SLOTS {
                return Err(FlowError::TooLarge);
            }
            let slot = child.slots;
            child.slots += 1;
            child.frames[0].insert(name, slot);
            formal_slots.push(slot);
        }
        child.statements(body)?;
        // A return inside one branch does not guarantee every path returns.
        // An unconditional fallthrough must yield an empty Lua result list.
        child.push(Op::ReturnMany(0))?;
        self.functions.borrow_mut()[id] = FunctionProgram {
            instructions: child.code,
            slots: child.slots,
            parameters: formal_slots,
            captures,
            vararg,
        };
        Ok(id)
    }

    /// A call's last argument is the only argument that may contribute
    /// multiple results. Nested calls use the same bounded adjustment path.
    fn call(
        &mut self,
        callee: &Expr<'a>,
        arguments: &[Expr<'a>],
        depth: usize,
        many: bool,
    ) -> Result<(), FlowError> {
        if depth > 128 {
            return Err(FlowError::TooLarge);
        }
        self.expression(callee, depth + 1)?;
        if let Some((last, before)) = arguments.split_last() {
            for argument in before {
                self.expression(argument, depth + 1)?;
            }
            if self.multiple_expression(last, depth + 1)? {
                self.push(Op::CallSpread {
                    prefix: before.len(),
                    many,
                })?;
                return Ok(());
            }
        }
        self.push(if many {
            Op::CallMany(arguments.len())
        } else {
            Op::Call(arguments.len())
        })?;
        Ok(())
    }

    fn expression(&mut self, expr: &Expr<'a>, depth: usize) -> Result<(), FlowError> {
        if depth > 128 {
            return Err(FlowError::TooLarge);
        }
        match expr {
            Expr::Grouped(inner) => self.expression(inner, depth + 1)?,
            Expr::Vararg if self.vararg => {
                self.push(Op::VarargSingle)?;
            }
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
            Expr::Function {
                parameters,
                vararg,
                body,
            } => {
                let function = self.compile_function(parameters, *vararg, body)?;
                self.push(Op::NewFunction(function))?;
            }
            Expr::Call { callee, arguments } => {
                self.call(callee, arguments, depth, false)?;
            }
            Expr::Index { object, key } => {
                self.expression(object, depth + 1)?;
                self.expression(key, depth + 1)?;
                self.push(Op::GetTable)?;
            }
            Expr::Table(fields) if fields.iter().any(|field| matches!(field, TableField::Indexed { .. })) => {
                // Construct mixed positional/integer-key tables in source order.
                // Keep the table on the stack while individual writes consume a copy.
                self.push(Op::NewTable(0))?;
                let mut positional = 0usize;
                for (position, field) in fields.iter().enumerate() {
                    match field {
                        TableField::Named { .. } => {
                            return Err(FlowError::Unsupported(
                                "named table fields require string-key support".into(),
                            ));
                        }
                        TableField::Indexed { key, value } => {
                            self.push(Op::Duplicate)?;
                            self.expression(key, depth + 1)?;
                            self.expression(value, depth + 1)?;
                            self.push(Op::SetTable)?;
                        }
                        TableField::Value(value) => {
                            positional += 1;
                            if position + 1 == fields.len()
                                && (matches!(value, Expr::Call { .. })
                                    || (self.vararg && matches!(value, Expr::Vararg)))
                            {
                                self.multiple_expression(value, depth + 1)?;
                                self.push(Op::SetTableTail(positional))?;
                            } else {
                                self.push(Op::Duplicate)?;
                                self.push(Op::Constant(Value::Integer(positional as i64)))?;
                                self.expression(value, depth + 1)?;
                                self.push(Op::SetTable)?;
                            }
                        }
                    }
                }
            }
            Expr::Table(fields) => {
                if let Some((last, before)) = fields.split_last() {
                    for field in before {
                        let TableField::Value(value) = field else {
                            return Err(FlowError::Unsupported(
                                "only positional integer-key table fields".into(),
                            ));
                        };
                        self.expression(value, depth + 1)?;
                    }
                    let TableField::Value(value) = last else {
                        return Err(FlowError::Unsupported(
                            "only positional integer-key table fields".into(),
                        ));
                    };
                    if self.multiple_expression(value, depth + 1)? {
                        self.push(Op::NewTableTail(before.len()))?;
                    } else {
                        self.push(Op::NewTable(fields.len()))?;
                    }
                } else {
                    self.push(Op::NewTable(0))?;
                }
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

    fn multiple_expression(&mut self, expr: &Expr<'a>, depth: usize) -> Result<bool, FlowError> {
        if depth > 128 {
            return Err(FlowError::TooLarge);
        }
        match expr {
            Expr::Call { callee, arguments } => {
                self.call(callee, arguments, depth + 1, true)?;
                Ok(true)
            }
            Expr::Vararg if self.vararg => {
                self.push(Op::VarargMany)?;
                Ok(true)
            }
            _ => {
                self.expression(expr, depth + 1)?;
                Ok(false)
            }
        }
    }

    // Only a final unparenthesized call or vararg expands. Earlier
    // expressions evaluate once, and result lists pad or truncate with nil.
    fn values(&mut self, expressions: &[Expr<'a>], targets: usize) -> Result<(), FlowError> {
        if let Some((last, before)) = expressions.split_last() {
            for expression in before {
                self.expression(expression, 0)?;
            }
            if self.multiple_expression(last, 0)? {
                self.push(Op::AdjustTail {
                    prefix: before.len(),
                    targets,
                })?;
                return Ok(());
            }
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
            | Some(Op::ForStep { exit: place, .. })
            | Some(Op::GenericForCheck { exit: place, .. }) => *place = target,
            _ => return Err(FlowError::InvalidBytecode),
        }
        Ok(())
    }

    fn statements(&mut self, nodes: &[Statement<'a>]) -> Result<(), FlowError> {
        for statement in nodes {
            match statement {
                Statement::Function {
                    name,
                    fields,
                    method,
                    local,
                    parameters,
                    vararg,
                    body,
                } => {
                    if !fields.is_empty() || method.is_some() {
                        return Err(FlowError::Unsupported(
                            "dotted and method function execution".into(),
                        ));
                    }
                    let slot = if *local {
                        if self.slots >= MAX_SLOTS {
                            return Err(FlowError::TooLarge);
                        }
                        let slot = self.slots;
                        self.slots += 1;
                        self.frames
                            .last_mut()
                            .expect("root scope")
                            .insert(*name, slot);
                        slot
                    } else {
                        self.slot(name)?
                    };
                    let function = self.compile_function(parameters, *vararg, body)?;
                    self.push(Op::NewFunction(function))?;
                    self.push(Op::AssignLocals(vec![slot]))?;
                }
                Statement::Call(call) => {
                    self.expression(call, 0)?;
                    self.push(Op::Discard)?;
                }
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
                    // Lua evaluates all LHS references first, then all RHS
                    // expressions, then performs the assignments.
                    for target in targets {
                        match target {
                            Expr::Atom(name) => {
                                let slot = self.slot(name)?;
                                if self.read_only.contains(&slot) {
                                    return Err(FlowError::Unsupported(
                                        "numeric for control variable is read-only".into(),
                                    ));
                                }
                                self.push(Op::PrepareLocal(slot))?;
                            }
                            Expr::Index { object, key } => {
                                self.expression(object, 0)?;
                                self.expression(key, 0)?;
                                self.push(Op::PrepareIndexed)?;
                            }
                            _ => {
                                return Err(FlowError::Unsupported(
                                    "only local and integer-index assignment targets".into(),
                                ));
                            }
                        }
                    }
                    self.values(values, targets.len())?;
                    self.push(Op::AssignPrepared(targets.len()))?;
                }
                Statement::Return(values) => {
                    if let Some((last, before)) = values.split_last() {
                        for value in before {
                            self.expression(value, 0)?;
                        }
                        if self.multiple_expression(last, 0)? {
                            self.push(Op::ReturnTail(before.len()))?;
                            continue;
                        }
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
                    if self.depth >= 64 || self.slots > MAX_SLOTS.saturating_sub(4) {
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
                    let visible = counter + 3;
                    self.slots += 4;
                    self.push(Op::AssignLocals(vec![counter, bound, stride]))?;
                    self.frames.push(HashMap::new());
                    self.frames
                        .last_mut()
                        .expect("numeric for scope")
                        .insert(*variable, visible);
                    self.read_only.insert(visible);
                    self.depth += 1;
                    self.loop_breaks.push(Vec::new());
                    let compiled = (|| {
                        let begin = self.code.len();
                        let check = self.push(Op::ForCheck {
                            counter,
                            limit: bound,
                            step: stride,
                            variable: visible,
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
                    self.read_only.remove(&visible);
                    self.frames.pop();
                    compiled?;
                    let end = self.code.len();
                    for address in breaks {
                        self.patch(address, end)?;
                    }
                }
                Statement::GenericFor {
                    variables,
                    iterators,
                    body,
                } => {
                    if self.depth >= 64
                        || variables.is_empty()
                        || self.slots > MAX_SLOTS.saturating_sub(3 + variables.len())
                    {
                        return Err(FlowError::TooLarge);
                    }
                    // Evaluate and adjust the iterator expression list once.
                    // Lua's optional fourth closing value is outside this subset.
                    self.values(iterators, 3)?;
                    let iterator = self.slots;
                    let state = iterator + 1;
                    let control = iterator + 2;
                    self.slots += 3;
                    self.push(Op::AssignLocals(vec![iterator, state, control]))?;
                    self.frames.push(HashMap::new());
                    self.depth += 1;
                    let mut value_slots = Vec::new();
                    for variable in variables {
                        let slot = self.slots;
                        self.slots += 1;
                        self.frames
                            .last_mut()
                            .expect("generic for scope")
                            .insert(*variable, slot);
                        self.read_only.insert(slot);
                        value_slots.push(slot);
                    }
                    self.loop_breaks.push(Vec::new());
                    let compiled = (|| {
                        let begin = self.code.len();
                        self.push(Op::ReadLocal(iterator))?;
                        self.push(Op::ReadLocal(state))?;
                        self.push(Op::ReadLocal(control))?;
                        self.push(Op::CallMany(2))?;
                        let check = self.push(Op::GenericForCheck {
                            control,
                            variables: value_slots.clone(),
                            exit: usize::MAX,
                        })?;
                        self.scope(body)?;
                        self.push(Op::Jump(begin))?;
                        self.patch(check, self.code.len())
                    })();
                    let breaks = self.loop_breaks.pop().expect("generic for break scope");
                    for slot in value_slots {
                        self.read_only.remove(&slot);
                    }
                    self.depth -= 1;
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
            }
        }
        Ok(())
    }
}

/// Compile the independent control-flow subset to executable stack bytecode.
/// Successful parsing does not imply full Lua grammar or runtime conformance.
pub fn compile_flow_chunk(source: &str) -> Result<FlowProgram, FlowError> {
    let statements = parse_chunk(source).map_err(|err| FlowError::Unsupported(err.to_string()))?;
    let functions = Rc::new(RefCell::new(Vec::new()));
    let mut compiler = Compiler {
        code: Vec::new(),
        frames: vec![HashMap::new()],
        slots: 0,
        depth: 0,
        read_only: HashSet::new(),
        loop_breaks: Vec::new(),
        functions: Rc::clone(&functions),
        vararg: false,
    };
    compiler.statements(&statements)?;
    if !compiler
        .code
        .iter()
        .any(|op| matches!(op, Op::Return | Op::ReturnMany(_) | Op::ReturnTail(_)))
    {
        return Err(FlowError::NoReturn);
    }
    let compiled_functions = functions.borrow().clone();
    Ok(FlowProgram {
        instructions: compiler.code,
        slots: compiler.slots,
        functions: compiled_functions,
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

#[derive(Clone, Copy)]
enum PendingTarget {
    Local(usize),
    Indexed { table: usize, field: usize },
}

#[derive(Clone)]
struct Closure {
    function: usize,
    captures: Vec<(usize, usize)>,
}

struct Execution<'a> {
    program: &'a FlowProgram,
    budget: usize,
    steps: usize,
    depth: usize,
    tables: Vec<Vec<Option<Value>>>,
    table_cells: usize,
    closures: Vec<Closure>,
    cells: Vec<Option<Value>>,
    captured_cells: HashSet<usize>,
}

impl Execution<'_> {
    /// Fresh lexical cell on successive loop iterations; old closures retain
    /// the prior cell rather than observing later iteration values.
    fn iteration_cell(&mut self, slots: &mut [usize], slot: usize) -> Result<usize, FlowError> {
        let cell = *slots.get(slot).ok_or(FlowError::InvalidBytecode)?;
        self.cells.get(cell).ok_or(FlowError::InvalidBytecode)?;
        if !self.captured_cells.contains(&cell) {
            return Ok(cell);
        }
        if self.cells.len() >= MAX_CELLS_TOTAL {
            return Err(FlowError::TooLarge);
        }
        let fresh = self.cells.len();
        self.cells.push(None);
        *slots.get_mut(slot).ok_or(FlowError::InvalidBytecode)? = fresh;
        Ok(fresh)
    }

    fn run(
        &mut self,
        instructions: &[Op],
        slot_count: usize,
        initial: &[(usize, Value)],
        captures: &[(usize, usize)],
        varargs: &[Value],
    ) -> Result<Vec<Value>, FlowError> {
        if instructions.len() > MAX_CODE || slot_count > MAX_SLOTS {
            return Err(FlowError::TooLarge);
        }
        let start = self.cells.len();
        // Captured slots already have a shared cell. Allocate fresh cells
        // only for parameters and locals owned by this invocation.
        let mut inherited = vec![None; slot_count];
        for &(local_slot, cell) in captures {
            if cell >= start {
                return Err(FlowError::InvalidBytecode);
            }
            let target = inherited
                .get_mut(local_slot)
                .ok_or(FlowError::InvalidBytecode)?;
            if target.replace(cell).is_some() {
                return Err(FlowError::InvalidBytecode);
            }
        }
        let mut slots = Vec::with_capacity(slot_count);
        for cell in inherited {
            if let Some(cell) = cell {
                slots.push(cell);
            } else {
                if self.cells.len() >= MAX_CELLS_TOTAL {
                    return Err(FlowError::TooLarge);
                }
                let cell = self.cells.len();
                self.cells.push(None);
                slots.push(cell);
            }
        }
        for &(slot, value) in initial {
            let cell = *slots.get(slot).ok_or(FlowError::InvalidBytecode)?;
            *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = Some(value);
        }
        let mut stack = Vec::new();
        let mut prepared = Vec::new();
        let mut last_multiple = None;
        let mut pc = 0;
        while pc < instructions.len() {
            if self.steps >= self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += 1;
            let next = pc + 1;
            match &instructions[pc] {
                Op::Constant(value) => stack.push(*value),
                Op::NewFunction(index) => {
                    if *index >= self.program.functions.len() || self.closures.len() >= MAX_CLOSURES
                    {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let spec = &self.program.functions[*index];
                    let mut captures = Vec::new();
                    for &(parent_slot, child_slot) in &spec.captures {
                        let cell = *slots.get(parent_slot).ok_or(FlowError::InvalidBytecode)?;
                        self.captured_cells.insert(cell);
                        captures.push((child_slot, cell));
                    }
                    let handle = self.closures.len();
                    self.closures.push(Closure {
                        function: *index,
                        captures,
                    });
                    stack.push(Value::Function(handle));
                }
                Op::Call(argc) | Op::CallMany(argc) | Op::CallSpread { prefix: argc, .. } => {
                    let arity = if matches!(instructions[pc], Op::CallSpread { .. }) {
                        let expanded = last_multiple.take().ok_or(FlowError::InvalidBytecode)?;
                        argc.checked_add(expanded).ok_or(FlowError::TooLarge)?
                    } else {
                        *argc
                    };
                    if stack.len() < arity.saturating_add(1) {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let args = stack.split_off(stack.len() - arity);
                    let Value::Function(handle) = stack.pop().ok_or(FlowError::InvalidBytecode)?
                    else {
                        return Err(FlowError::TypeError);
                    };
                    let closure = self
                        .closures
                        .get(handle)
                        .cloned()
                        .ok_or(FlowError::InvalidBytecode)?;
                    let function = self
                        .program
                        .functions
                        .get(closure.function)
                        .ok_or(FlowError::InvalidBytecode)?
                        .clone();
                    if self.depth >= MAX_CALL_DEPTH {
                        return Err(FlowError::CallDepthExceeded);
                    }
                    let mut initial = Vec::new();
                    for (position, slot) in function.parameters.iter().enumerate() {
                        initial.push((*slot, args.get(position).copied().unwrap_or(Value::Nil)));
                    }
                    self.depth += 1;
                    let varargs = if function.vararg && args.len() > function.parameters.len() {
                        &args[function.parameters.len()..]
                    } else {
                        &[]
                    };
                    let result = self.run(
                        &function.instructions,
                        function.slots,
                        &initial,
                        &closure.captures,
                        varargs,
                    );
                    self.depth -= 1;
                    let returned = result?;
                    if matches!(
                        instructions[pc],
                        Op::CallMany(_) | Op::CallSpread { many: true, .. }
                    ) {
                        last_multiple = Some(returned.len());
                        stack.extend(returned);
                    } else {
                        stack.push(returned.into_iter().next().unwrap_or(Value::Nil));
                    }
                }
                Op::VarargSingle => {
                    stack.push(varargs.first().copied().unwrap_or(Value::Nil));
                }
                Op::VarargMany => {
                    last_multiple = Some(varargs.len());
                    stack.extend_from_slice(varargs);
                }
                Op::AdjustTail { prefix, targets } => {
                    let multiple = last_multiple.take().ok_or(FlowError::InvalidBytecode)?;
                    if stack.len() < prefix.saturating_add(multiple) {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let tail = stack.split_off(stack.len() - multiple);
                    if *prefix > *targets {
                        for _ in *targets..*prefix {
                            stack.pop().ok_or(FlowError::InvalidBytecode)?;
                        }
                    }
                    let needed = targets.saturating_sub(*prefix);
                    stack.extend(tail.iter().copied().take(needed));
                    for _ in multiple..needed {
                        stack.push(Value::Nil);
                    }
                }
                Op::ReadLocal(slot) => {
                    let cell = *slots.get(*slot).ok_or(FlowError::InvalidBytecode)?;
                    let value = self
                        .cells
                        .get(cell)
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
                        let cell = *slots.get(*index).ok_or(FlowError::InvalidBytecode)?;
                        *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = Some(value);
                    }
                }
                Op::PrepareLocal(slot) => {
                    let cell = *slots.get(*slot).ok_or(FlowError::InvalidBytecode)?;
                    if prepared.len() >= MAX_SLOTS {
                        return Err(FlowError::TooLarge);
                    }
                    prepared.push(PendingTarget::Local(cell));
                }
                Op::PrepareIndexed => {
                    let key = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let table = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let (Value::Table(handle), Value::Integer(index)) = (table, key) else {
                        return Err(FlowError::TypeError);
                    };
                    let field = usize::try_from(index).map_err(|_| FlowError::TypeError)?;
                    if field == 0 || field > MAX_TABLE_FIELDS {
                        return Err(FlowError::TypeError);
                    }
                    if handle >= self.tables.len() {
                        return Err(FlowError::InvalidBytecode);
                    }
                    if prepared.len() >= MAX_SLOTS {
                        return Err(FlowError::TooLarge);
                    }
                    prepared.push(PendingTarget::Indexed {
                        table: handle,
                        field,
                    });
                }
                Op::AssignPrepared(count) => {
                    if stack.len() < *count || prepared.len() < *count {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let values = stack.split_off(stack.len() - count);
                    let addresses = prepared.split_off(prepared.len() - count);
                    for (address, value) in addresses.into_iter().zip(values) {
                        match address {
                            PendingTarget::Local(cell) => {
                                *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? =
                                    Some(value);
                            }
                            PendingTarget::Indexed { table, field } => {
                                let cells = self
                                    .tables
                                    .get_mut(table)
                                    .ok_or(FlowError::InvalidBytecode)?;
                                if field > cells.len() {
                                    let growth = field - cells.len();
                                    if self.table_cells.saturating_add(growth)
                                        > MAX_TABLE_CELLS_TOTAL
                                    {
                                        return Err(FlowError::TooLarge);
                                    }
                                    self.table_cells += growth;
                                    cells.resize(field, None);
                                }
                                cells[field - 1] = Some(value);
                            }
                        }
                    }
                }
                Op::Duplicate => {
                    let value = *stack.last().ok_or(FlowError::InvalidBytecode)?;
                    stack.push(value);
                }
                Op::SetTableTail(start) => {
                    let multiple = last_multiple.take().ok_or(FlowError::InvalidBytecode)?;
                    if stack.len() < multiple.saturating_add(1) {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let first = start.checked_sub(1).ok_or(FlowError::InvalidBytecode)?;
                    let end = first.checked_add(multiple).ok_or(FlowError::TooLarge)?;
                    if end > MAX_TABLE_FIELDS {
                        return Err(FlowError::TooLarge);
                    }
                    let results = stack.split_off(stack.len() - multiple);
                    let Value::Table(handle) = *stack.last().ok_or(FlowError::InvalidBytecode)?
                    else {
                        return Err(FlowError::TypeError);
                    };
                    let cells = self.tables.get_mut(handle).ok_or(FlowError::InvalidBytecode)?;
                    if end > cells.len() {
                        let growth = end - cells.len();
                        if self.table_cells.saturating_add(growth) > MAX_TABLE_CELLS_TOTAL {
                            return Err(FlowError::TooLarge);
                        }
                        self.table_cells += growth;
                        cells.resize(end, None);
                    }
                    for (offset, value) in results.into_iter().enumerate() {
                        cells[first + offset] = Some(value);
                    }
                }
                Op::NewTable(count) | Op::NewTableTail(count) => {
                    let arity = if matches!(instructions[pc], Op::NewTableTail(_)) {
                        let expanded = last_multiple.take().ok_or(FlowError::InvalidBytecode)?;
                        count.checked_add(expanded).ok_or(FlowError::TooLarge)?
                    } else {
                        *count
                    };
                    if arity > MAX_TABLE_FIELDS
                        || self.tables.len() >= MAX_TABLES
                        || self.table_cells.saturating_add(arity) > MAX_TABLE_CELLS_TOTAL
                        || stack.len() < arity
                    {
                        return Err(FlowError::TooLarge);
                    }
                    let values = stack.split_off(stack.len() - arity);
                    let handle = self.tables.len();
                    self.tables.push(values.into_iter().map(Some).collect());
                    self.table_cells += arity;
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
                    let values = self.tables.get(handle).ok_or(FlowError::InvalidBytecode)?;
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
                    let values = self
                        .tables
                        .get_mut(handle)
                        .ok_or(FlowError::InvalidBytecode)?;
                    if field > values.len() {
                        let growth = field - values.len();
                        if self.table_cells.saturating_add(growth) > MAX_TABLE_CELLS_TOTAL {
                            return Err(FlowError::TooLarge);
                        }
                        self.table_cells += growth;
                        values.resize(field, None);
                    }
                    values[field - 1] = Some(value);
                }
                Op::Negate => {
                    let Value::Integer(value) = stack.pop().ok_or(FlowError::InvalidBytecode)?
                    else {
                        return Err(FlowError::TypeError);
                    };
                    stack.push(Value::Integer(value.wrapping_neg()));
                }
                Op::Not => {
                    let value = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    stack.push(Value::Boolean(!value.truthy()));
                }
                Op::BitNot => {
                    let Value::Integer(value) = stack.pop().ok_or(FlowError::InvalidBytecode)?
                    else {
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
                        if *target > instructions.len() {
                            return Err(FlowError::InvalidBytecode);
                        }
                        pc = *target;
                        continue;
                    }
                }
                Op::JumpIfFalseKeep(target) | Op::JumpIfTrueKeep(target) => {
                    let value = *stack.last().ok_or(FlowError::InvalidBytecode)?;
                    let should_jump = match instructions[pc] {
                        Op::JumpIfFalseKeep(_) => !value.truthy(),
                        Op::JumpIfTrueKeep(_) => value.truthy(),
                        _ => unreachable!("guarded opcode"),
                    };
                    if should_jump {
                        if *target > instructions.len() {
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
                    if *target > instructions.len() {
                        return Err(FlowError::InvalidBytecode);
                    }
                    pc = *target;
                    continue;
                }
                Op::ForCheck {
                    counter,
                    limit,
                    step,
                    variable,
                    exit,
                } => {
                    if *exit > instructions.len() {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let read = |slot: usize| -> Result<i64, FlowError> {
                        let cell = *slots.get(slot).ok_or(FlowError::InvalidBytecode)?;
                        match self.cells.get(cell).copied().flatten() {
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
                    let cell = self.iteration_cell(&mut slots, *variable)?;
                    *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? =
                        Some(Value::Integer(current));
                }
                Op::ForStep {
                    counter,
                    step,
                    exit,
                } => {
                    if *exit > instructions.len() {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let counter_cell = *slots.get(*counter).ok_or(FlowError::InvalidBytecode)?;
                    let current = match self.cells.get(counter_cell).copied().flatten() {
                        Some(Value::Integer(value)) => value,
                        Some(_) => return Err(FlowError::TypeError),
                        None => return Err(FlowError::InvalidBytecode),
                    };
                    let step_cell = *slots.get(*step).ok_or(FlowError::InvalidBytecode)?;
                    let stride = match self.cells.get(step_cell).copied().flatten() {
                        Some(Value::Integer(value)) => value,
                        Some(_) => return Err(FlowError::TypeError),
                        None => return Err(FlowError::InvalidBytecode),
                    };
                    if let Some(updated) = current.checked_add(stride) {
                        *self
                            .cells
                            .get_mut(counter_cell)
                            .ok_or(FlowError::InvalidBytecode)? = Some(Value::Integer(updated));
                    } else {
                        pc = *exit;
                        continue;
                    }
                }
                Op::GenericForCheck {
                    control,
                    variables,
                    exit,
                } => {
                    if *exit > instructions.len() {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let count = last_multiple.take().ok_or(FlowError::InvalidBytecode)?;
                    if stack.len() < count {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let returned = stack.split_off(stack.len() - count);
                    let first = returned.first().copied().unwrap_or(Value::Nil);
                    if first == Value::Nil {
                        pc = *exit;
                        continue;
                    }
                    let control_cell = *slots.get(*control).ok_or(FlowError::InvalidBytecode)?;
                    *self
                        .cells
                        .get_mut(control_cell)
                        .ok_or(FlowError::InvalidBytecode)? = Some(first);
                    for (position, slot) in variables.iter().enumerate() {
                        let cell = self.iteration_cell(&mut slots, *slot)?;
                        let value = returned.get(position).copied().unwrap_or(Value::Nil);
                        *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = Some(value);
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
                Op::ReturnTail(prefix) => {
                    let multiple = last_multiple.take().ok_or(FlowError::InvalidBytecode)?;
                    let count = prefix.saturating_add(multiple);
                    if stack.len() < count {
                        return Err(FlowError::InvalidBytecode);
                    }
                    return Ok(stack.split_off(stack.len() - count));
                }
            }
            if stack.len() > MAX_STACK {
                return Err(FlowError::TooLarge);
            }
            pc = next;
        }
        Err(FlowError::NoReturn)
    }
}

/// Run owned control-flow bytecode with one shared instruction budget across calls.
/// No host callbacks, global environment, or I/O.
pub fn execute_flow_values(program: &FlowProgram, budget: usize) -> Result<Vec<Value>, FlowError> {
    let mut execution = Execution {
        program,
        budget,
        steps: 0,
        depth: 0,
        tables: Vec::new(),
        table_cells: 0,
        closures: Vec::new(),
        cells: Vec::new(),
        captured_cells: HashSet::new(),
    };
    execution.run(&program.instructions, program.slots, &[], &[], &[])
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
    fn generic_iterator_calls_and_early_exit() {
        assert_eq!(
            evaluate(
                "local function next(s,i) i=i+1; if i<=s then return i,i*2 end end; local sum=0; for i,v in next,3,0 do sum=sum+i+v end; return sum"
            ),
            18
        );
        assert_eq!(
            evaluate(
                "local function next(s,i) i=i+1; if i<=s then return i,i*2 end end; local sum=0; for i,v in next,6,0 do sum=sum+i; if i==3 then break end end; return sum"
            ),
            6
        );
        assert_eq!(
            evaluate(
                "local function next(s,i) i=i+1; if i<=s then return i end end; local sum=0; for i,v in next,2,0 do if v==nil then sum=sum+i end end; return sum"
            ),
            3
        );
        assert_eq!(
            evaluate(
                "local function next(s,i) i=i+1; if i<=s then return i end end; local function pack() return next,3,0 end; local sum=0; for i in pack() do sum=sum+i end; return sum"
            ),
            6
        );
        assert_eq!(
            evaluate(
                "local function next() return false end; local n=0; for x in next,nil,nil do n=n+1; break end; return n"
            ),
            1
        );
    }

    #[test]
    fn noncapturing_numeric_loops_do_not_exhaust_cell_heap() {
        let program =
            compile_flow_chunk("local total=0; for i=1,70000 do total=total+1 end; return total")
                .unwrap();
        assert_eq!(execute_flow_chunk(&program, 1_000_000), Ok(70000));
    }

    #[test]
    fn loop_closures_capture_distinct_iteration_cells() {
        assert_eq!(
            evaluate(
                "local t={}; for i=1,3 do t[i]=function() return i end end; return t[1]()*100+t[2]()*10+t[3]()"
            ),
            123
        );
        assert_eq!(
            evaluate(
                "local function next(s,i) i=i+1; if i<=s then return i end end; local t={}; for i in next,3,0 do t[i]=function() return i end end; return t[1]()*100+t[2]()*10+t[3]()"
            ),
            123
        );
        assert_eq!(
            evaluate(
                "local t={}; for i=3,1,-1 do t[i]=function() return i end end; return t[1]()*100+t[2]()*10+t[3]()"
            ),
            123
        );
    }

    #[test]
    fn generic_iterator_rejects_readonly_variable_and_noncallable() {
        assert!(compile_flow_chunk("for i in f,nil,nil do i=2 end; return 1").is_err());
        let program = compile_flow_chunk("for i in 3,0,0 do end; return 1").unwrap();
        assert_eq!(
            execute_flow_chunk(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
        let program = compile_flow_chunk(
            "local function next(_,i) return i end; for i in next,1,1 do end; return 0",
        )
        .unwrap();
        assert_eq!(
            execute_flow_chunk(&program, 25),
            Err(FlowError::StepBudgetExceeded)
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
    fn local_function_call_frames_and_self_recursion() {
        assert_eq!(
            evaluate("local function add(a,b) return a+b end; return add(3,4)"),
            7
        );
        assert_eq!(
            evaluate("local function twice(x) return x*2 end; local f=twice; return f(6)"),
            12
        );
        assert_eq!(
            evaluate("local f=function(x) return x+1 end; return f(4)"),
            5
        );
        assert_eq!(
            evaluate(
                "local function fact(n) if n<2 then return 1 end; return n*fact(n-1) end; return fact(5)"
            ),
            120
        );
        assert_eq!(
            evaluate("local function set(t) t[1]=9 end; local t={2}; set(t); return t[1]"),
            9
        );
        assert_eq!(
            evaluate("local function f(a) return a end; return f(5,6)"),
            5
        );
        let program =
            compile_flow_chunk("local function missing(x) return x end; return missing()").unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Ok(Value::Nil)
        );
    }

    #[test]
    fn conditional_function_fallthrough_returns_zero_values() {
        assert_eq!(
            evaluate(
                "local function maybe(x) if x then return 4 end end; return maybe(false) or 9"
            ),
            9
        );
        assert_eq!(
            evaluate("local function maybe(x) if x then return 4 end end; return maybe(true)"),
            4
        );
        assert_eq!(
            evaluate("local function empty() return end; return empty() or 8"),
            8
        );
        let result = compile_flow_chunk(
            "local function maybe(x) if x then return 4,5 end end; return maybe(false)",
        )
        .unwrap();
        assert_eq!(
            execute_flow_values(&result, DEFAULT_STEP_BUDGET),
            Ok(Vec::new())
        );
    }

    #[test]
    fn last_call_argument_and_table_field_expand_results() {
        assert_eq!(
            evaluate(
                "local function pair() return 3,4 end; local function add(a,b) return a+b end; return add(pair())"
            ),
            7
        );
        assert_eq!(
            evaluate(
                "local function pair() return 3,4 end; local function sum(a,b,c) return a+b+c end; return sum(2,pair())"
            ),
            9
        );
        assert_eq!(
            evaluate(
                "local function pair() return 3,4 end; local function f(a,b,c) return a*100+b*10+c end; return f(pair(),8,9)"
            ),
            389
        );
        assert_eq!(
            evaluate(
                "local function f(...) return ... end; local function sum(a,b) return a+b end; return sum(f(4,5))"
            ),
            9
        );
        assert_eq!(
            evaluate("local function pair() return 4,5 end; local t={2,pair()}; return t[2]+t[3]"),
            9
        );
        assert_eq!(
            evaluate(
                "local function pair() return 4,5 end; local t={2,(pair())}; return t[3] or 7"
            ),
            7
        );
        assert_eq!(
            evaluate("local function pair() return 4,5 end; local t={pair()}; return t[2]"),
            5
        );
        assert_eq!(
            evaluate("local function f(...) return ... end; local t={1,f(2,3)}; return t[3]"),
            3
        );
    }

    #[test]
    fn last_expression_multireturn_and_variadic_adjustment() {
        let eval_many = |source| {
            let program = compile_flow_chunk(source).unwrap();
            execute_flow_values(&program, DEFAULT_STEP_BUDGET).unwrap()
        };
        assert_eq!(
            eval_many("local function pair() return 2,3 end; return pair()"),
            vec![Value::Integer(2), Value::Integer(3)]
        );
        assert_eq!(
            eval_many("local function pair() return 2,3 end; return 1,pair()"),
            vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]
        );
        assert_eq!(
            eval_many("local function pair() return 2,3 end; return (pair())"),
            vec![Value::Integer(2)]
        );
        assert_eq!(
            evaluate("local function pair() return 2,3 end; local a,b=pair(); return a+b"),
            5
        );
        assert_eq!(
            evaluate("local function pair() return 2,3 end; local a,b=9,pair(); return a+b"),
            11
        );
        assert_eq!(
            evaluate("local function pair() return 2,3 end; local a,b=pair(),9; return a+b"),
            11
        );
        assert_eq!(
            eval_many("local function f(...) return ... end; return f(4,5,6)"),
            vec![Value::Integer(4), Value::Integer(5), Value::Integer(6)]
        );
        assert_eq!(
            eval_many("local function f(a,...) return a,... end; return f(2,3,4)"),
            vec![Value::Integer(2), Value::Integer(3), Value::Integer(4)]
        );
        assert_eq!(
            evaluate("local function f(...) local a,b=...; return a+b end; return f(3,4)"),
            7
        );
        assert_eq!(
            evaluate("local function f(...) local a,b=1,...; return a+b end; return f(3,4)"),
            4
        );
        assert!(compile_flow_chunk("return ...").is_err());
    }

    #[test]
    fn repeated_captured_calls_do_not_allocate_duplicate_upvalue_cells() {
        let program = compile_flow_chunk(
            "local n=0; local function bump(x) n=n+x end; for i=1,30000 do bump(1) end; return n",
        )
        .unwrap();
        assert_eq!(execute_flow_chunk(&program, 1_000_000), Ok(30_000));
    }

    #[test]
    fn captured_locals_share_mutable_cells_across_calls() {
        assert_eq!(
            evaluate("local x=3; local function f() return x end; x=8; return f()"),
            8
        );
        assert_eq!(
            evaluate("local x=0; local function next() x=x+1; return x end; return next()+next()"),
            3
        );
        assert_eq!(
            evaluate(
                "local function make(start) local n=start; local function next() n=n+1; return n end; return next end; local f=make(5); return f()+f()"
            ),
            13
        );
        assert_eq!(
            evaluate(
                "local function make() local n=0; local function next() n=n+1; return n end; return next end; local a=make(); local b=make(); return a()+b()+a()"
            ),
            4
        );
        assert_eq!(
            evaluate(
                "local x=1; local function outer() return function() return x end end; local inner=outer(); x=7; return inner()"
            ),
            7
        );
        assert_eq!(
            evaluate(
                "local function f(n) if n==0 then return 0 end; return f(n-1) end; local original=f; f=function(x) return 8 end; return original(3)"
            ),
            8
        );
    }

    #[test]
    fn bounded_calls_reject_excessive_recursion() {
        let recursion =
            compile_flow_chunk("local function forever() return forever() end; return forever()")
                .unwrap();
        assert_eq!(
            execute_flow_chunk(&recursion, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
        assert_eq!(
            execute_flow_chunk(&recursion, 5),
            Err(FlowError::StepBudgetExceeded)
        );
        let not_callable = compile_flow_chunk("local x=3; return x()").unwrap();
        assert_eq!(
            execute_flow_chunk(&not_callable, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
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
    fn mixed_parallel_lvalue_assignment_evaluates_addresses_before_rhs() {
        assert_eq!(
            evaluate("local t={1,2}; local x=7; t[1],x=5,9; return t[1]+x"),
            14
        );
        assert_eq!(
            evaluate("local t={1}; local x=3; x,t[1]=9,8; return x+t[1]"),
            17
        );
        assert_eq!(
            evaluate("local t={1,2}; local i=1; i,t[i]=2,9; return t[1]*10+t[2]"),
            92
        );
        assert_eq!(
            evaluate("local a={1}; local old=a; a,a[1]={4},6; return old[1]+a[1]"),
            10
        );
        assert_eq!(
            evaluate(
                "local t={1,2}; local i=0; local function at() i=i+1; return i end; t[at()],t[at()]=8,9; return t[1]*10+t[2]"
            ),
            89
        );
        assert_eq!(
            evaluate(
                "local t={1}; local x; local function f() return 4,5 end; t[1],x=f(); return t[1]*10+x"
            ),
            45
        );
        assert_eq!(
            evaluate("local t={1,2}; t[1],t[2]=t[2],t[1]; return t[1]*10+t[2]"),
            21
        );
        let invalid = compile_flow_chunk("local t={1}; t[0],t[1]=8,9; return t[1]").unwrap();
        assert_eq!(
            execute_flow_chunk(&invalid, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
    }

    #[test]
    fn computed_integer_keys_in_mixed_table_constructors() {
        assert_eq!(evaluate("local t={[3]=7,[1]=4}; return t[1]*10+t[3]"), 47);
        assert_eq!(evaluate("local t={2,[1]=9,3}; return t[1]*10+t[2]"), 93);
        assert_eq!(evaluate("local t={[3]=7,2,[2]=8}; return t[1]*100+t[2]*10+t[3]"), 287);
        assert_eq!(evaluate("local k=3; local t={[k]=9,1}; return t[1]*10+t[3]"), 19);
        assert_eq!(
            evaluate("local function vals() return 4,5 end; local t={[3]=9,2,vals()}; return t[1]*100+t[2]*10+t[3]"),
            245
        );
        assert_eq!(
            evaluate("local function vals() return end; local t={[3]=9,2,vals()}; return t[1]*10+t[3]"),
            29
        );
        assert_eq!(
            evaluate("local k=0; local function nextkey() k=k+1; return k end; local t={[nextkey()]=5,[nextkey()]=7}; return t[1]*10+t[2]"),
            57
        );
        assert!(compile_flow_chunk("local t={name=7}; return 1").is_err());
        let unsupported = compile_flow_chunk("local t={[0]=7,1}; return t[1]").unwrap();
        assert_eq!(
            execute_flow_chunk(&unsupported, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
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
