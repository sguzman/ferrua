//! Experimental Ferrua-owned control-flow bytecode, separate from production fer.
//! This bounded stack VM evaluates integer, Boolean and nil Lua values with
//! lexical locals and control flow. It is not a complete Lua runtime or the register backend.

use crate::expr::{Expr, LocalAttribute, Statement, TableField, parse_chunk};
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
// Keep interpreter recursion well inside the default Rust test-thread stack.
// Lua recursion remains deliberately bounded in this experimental engine.
const MAX_CALL_DEPTH: usize = 16;
const MAX_CELLS_TOTAL: usize = 65_536;
const MAX_STRING_POOL_BYTES: usize = 65_536;
const MAX_PRINT_OUTPUT_BYTES: usize = 65_536;
const MAX_STRINGS: usize = 16_384;
pub const DEFAULT_STEP_BUDGET: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    Integer(i64),
    /// Raw IEEE-754 bits preserve Copy/Eq/Hash for the bounded VM value type.
    Float(u64),
    Boolean(bool),
    Nil,
    /// Index into the immutable compiler-generated Lua byte-string pool.
    String(usize),
    /// Opaque index into the per-execution bounded heap.
    Table(usize),
    /// Opaque, execution-local function identity.
    Function(usize),
    /// First-party, pure Lua core builtins have first-class function values.
    Builtin(Builtin),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Print,
    Type,
    ToString,
    ToNumber,
    RawEqual,
    RawGet,
    RawSet,
    RawLen,
    Select,
    Assert,
    Next,
    Pairs,
    Ipairs,
    IpairsIterator,
    GetMetatable,
    SetMetatable,
    Error,
    Pcall,
    Xpcall,
}

const BUILTIN_GLOBALS: &[(&[u8], Builtin)] = &[
    (b"print", Builtin::Print),
    (b"type", Builtin::Type),
    (b"tostring", Builtin::ToString),
    (b"tonumber", Builtin::ToNumber),
    (b"rawequal", Builtin::RawEqual),
    (b"rawget", Builtin::RawGet),
    (b"rawset", Builtin::RawSet),
    (b"rawlen", Builtin::RawLen),
    (b"select", Builtin::Select),
    (b"assert", Builtin::Assert),
    (b"error", Builtin::Error),
    (b"pcall", Builtin::Pcall),
    (b"xpcall", Builtin::Xpcall),
    (b"next", Builtin::Next),
    (b"pairs", Builtin::Pairs),
    (b"ipairs", Builtin::Ipairs),
    (b"getmetatable", Builtin::GetMetatable),
    (b"setmetatable", Builtin::SetMetatable),
];

impl Value {
    fn truthy(self) -> bool {
        !matches!(self, Value::Boolean(false) | Value::Nil)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binary {
    Concat,
    Div,
    Pow,
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
    ReadGlobal(usize),
    AssignLocals(Vec<usize>),
    DeclareLocals(Vec<usize>),
    RenewLocal(usize),
    PrepareLocal(usize),
    PrepareGlobal(usize),
    PrepareIndexed,
    AssignPrepared(usize),
    NewTable(usize),
    Duplicate,
    Swap,
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
    Length,
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
    strings: Vec<Vec<u8>>,
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
    /// A Lua script explicitly threw an error value rather than a VM fault.
    Thrown(Value),
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
            Self::Thrown(_) => write!(out, "explicit Lua error"),
        }
    }
}
impl std::error::Error for FlowError {}

/// A Lua label and goto carry their lexical block and active local slots.
#[derive(Clone)]
struct LabelSite<'a> {
    name: &'a str,
    block: usize,
    address: usize,
    locals: Vec<usize>,
}

#[derive(Clone)]
struct GotoSite<'a> {
    name: &'a str,
    block: usize,
    address: usize,
    locals: Vec<usize>,
}

struct Compiler<'a> {
    code: Vec<Op>,
    frames: Vec<HashMap<&'a str, usize>>,
    slots: usize,
    depth: usize,
    read_only: HashSet<usize>,
    loop_breaks: Vec<Vec<usize>>,
    block_parents: Vec<Option<usize>>,
    block_stack: Vec<usize>,
    active_locals: Vec<usize>,
    labels: Vec<LabelSite<'a>>,
    pending_gotos: Vec<GotoSite<'a>>,
    functions: Rc<RefCell<Vec<FunctionProgram>>>,
    strings: Rc<RefCell<Vec<Vec<u8>>>>,
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

    /// Lua 5.4 resolves an unbound identifier through the nearest lexical
    /// _ENV, if any. Otherwise use the execution's default environment.
    fn emit_global_read(&mut self, name: &str) -> Result<(), FlowError> {
        let key = self.intern_bytes(name.as_bytes().to_vec())?;
        if let Ok(slot) = self.slot("_ENV") {
            self.push(Op::ReadLocal(slot))?;
            self.push(Op::Constant(Value::String(key)))?;
            self.push(Op::GetTable)?;
        } else {
            self.push(Op::ReadGlobal(key))?;
        }
        Ok(())
    }

    /// Prepare a table address before computing the assignment's RHS.
    fn emit_global_target(&mut self, name: &str) -> Result<(), FlowError> {
        let key = self.intern_bytes(name.as_bytes().to_vec())?;
        if let Ok(slot) = self.slot("_ENV") {
            self.push(Op::ReadLocal(slot))?;
            self.push(Op::Constant(Value::String(key)))?;
            self.push(Op::PrepareIndexed)?;
        } else {
            self.push(Op::PrepareGlobal(key))?;
        }
        Ok(())
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
            block_parents: Vec::new(),
            block_stack: Vec::new(),
            active_locals: Vec::new(),
            labels: Vec::new(),
            pending_gotos: Vec::new(),
            functions: Rc::clone(&self.functions),
            strings: Rc::clone(&self.strings),
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
            if self.read_only.contains(&parent_slot) {
                child.read_only.insert(child_slot);
            }
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
        child.resolve_gotos()?;
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

    fn method_call(
        &mut self,
        object: &Expr<'a>,
        method: &str,
        arguments: &[Expr<'a>],
        depth: usize,
        many: bool,
    ) -> Result<(), FlowError> {
        if depth > 128 {
            return Err(FlowError::TooLarge);
        }
        self.expression(object, depth + 1)?;
        self.push(Op::Duplicate)?;
        let name = self.intern_bytes(method.as_bytes().to_vec())?;
        self.push(Op::Constant(Value::String(name)))?;
        self.push(Op::GetTable)?;
        // Stack is receiver, callable; the call expects callable, receiver.
        self.push(Op::Swap)?;
        if let Some((last, before)) = arguments.split_last() {
            for argument in before {
                self.expression(argument, depth + 1)?;
            }
            if self.multiple_expression(last, depth + 1)? {
                self.push(Op::CallSpread {
                    prefix: before.len() + 1,
                    many,
                })?;
                return Ok(());
            }
        }
        self.push(if many {
            Op::CallMany(arguments.len() + 1)
        } else {
            Op::Call(arguments.len() + 1)
        })?;
        Ok(())
    }

    fn intern_literal(&mut self, spelling: &str) -> Result<usize, FlowError> {
        self.intern_bytes(decode_lua_string(spelling)?)
    }

    fn intern_bytes(&mut self, bytes: Vec<u8>) -> Result<usize, FlowError> {
        let mut strings = self.strings.borrow_mut();
        if let Some(index) = strings.iter().position(|item| item == &bytes) {
            return Ok(index);
        }
        if strings.len() >= MAX_STRINGS
            || bytes.len() > MAX_STRING_POOL_BYTES
            || strings.iter().map(Vec::len).sum::<usize>()
                > MAX_STRING_POOL_BYTES.saturating_sub(bytes.len())
        {
            return Err(FlowError::TooLarge);
        }
        let index = strings.len();
        strings.push(bytes);
        Ok(index)
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
            Expr::Atom(text)
                if text.starts_with('\'') || text.starts_with('"') || text.starts_with('[') =>
            {
                let index = self.intern_literal(text)?;
                self.push(Op::Constant(Value::String(index)))?;
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
                        number.ok().map(Value::Integer).or_else(|| {
                            text.parse::<f64>()
                                .ok()
                                .or_else(|| parse_lua_hex_float(text))
                                .map(|value| Value::Float(value.to_bits()))
                        })
                    }
                };
                if let Some(value) = constant {
                    self.push(Op::Constant(value))?;
                } else {
                    match self.slot(text) {
                        Ok(slot) => {
                            self.push(Op::ReadLocal(slot))?;
                        }
                        Err(_) => self.emit_global_read(text)?,
                    }
                }
            }
            Expr::Unary { op, right } if matches!(*op, "-" | "not" | "~" | "#") => {
                self.expression(right, depth + 1)?;
                self.push(match *op {
                    "-" => Op::Negate,
                    "not" => Op::Not,
                    "#" => Op::Length,
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
            Expr::MethodCall {
                object,
                method,
                arguments,
            } => {
                self.method_call(object, method, arguments, depth, false)?;
            }
            Expr::Index { object, key } => {
                self.expression(object, depth + 1)?;
                self.expression(key, depth + 1)?;
                self.push(Op::GetTable)?;
            }
            Expr::Field { object, name } => {
                self.expression(object, depth + 1)?;
                let handle = self.intern_bytes(name.as_bytes().to_vec())?;
                self.push(Op::Constant(Value::String(handle)))?;
                self.push(Op::GetTable)?;
            }
            Expr::Table(fields)
                if fields
                    .iter()
                    .any(|field| !matches!(field, TableField::Value(_))) =>
            {
                // Construct mixed positional/integer-key tables in source order.
                // Keep the table on the stack while individual writes consume a copy.
                self.push(Op::NewTable(0))?;
                let mut positional = 0usize;
                for (position, field) in fields.iter().enumerate() {
                    match field {
                        TableField::Named { name, value } => {
                            self.push(Op::Duplicate)?;
                            let handle = self.intern_bytes(name.as_bytes().to_vec())?;
                            self.push(Op::Constant(Value::String(handle)))?;
                            self.expression(value, depth + 1)?;
                            self.push(Op::SetTable)?;
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
                                && (matches!(value, Expr::Call { .. } | Expr::MethodCall { .. })
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
                    ".." => Binary::Concat,
                    "+" => Binary::Add,
                    "-" => Binary::Sub,
                    "*" => Binary::Mul,
                    "/" => Binary::Div,
                    "^" => Binary::Pow,
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
            Expr::MethodCall {
                object,
                method,
                arguments,
            } => {
                self.method_call(object, method, arguments, depth + 1, true)?;
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
        let active = self.active_locals.len();
        self.frames.push(HashMap::new());
        self.depth += 1;
        let result = self.statements(body);
        self.depth -= 1;
        self.frames.pop();
        self.active_locals.truncate(active);
        result
    }

    fn resolve_gotos(&mut self) -> Result<(), FlowError> {
        // Resolve only labels in this block or a lexical ancestor. A label's
        // active locals must already exist at the jump site; leaving scopes
        // and jumping backward across declarations is permitted.
        let pending = std::mem::take(&mut self.pending_gotos);
        for jump in pending {
            let mut block = Some(jump.block);
            let mut destination = None;
            while let Some(id) = block {
                if let Some(label) = self
                    .labels
                    .iter()
                    .find(|site| site.block == id && site.name == jump.name)
                {
                    destination = Some((label.address, label.locals.clone()));
                    break;
                }
                block = *self
                    .block_parents
                    .get(id)
                    .ok_or(FlowError::InvalidBytecode)?;
            }
            let (address, locals) = destination.ok_or_else(|| {
                FlowError::Unsupported("goto label is not visible from this scope".into())
            })?;
            if locals.iter().any(|slot| !jump.locals.contains(slot)) {
                return Err(FlowError::Unsupported(
                    "goto enters a new local variable lifetime".into(),
                ));
            }
            self.patch(jump.address, address)?;
        }
        Ok(())
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
        let block = self.block_parents.len();
        self.block_parents.push(self.block_stack.last().copied());
        self.block_stack.push(block);
        for statement in nodes {
            match statement {
                Statement::Label(name) => {
                    if self
                        .labels
                        .iter()
                        .any(|site| site.block == block && site.name == *name)
                    {
                        return Err(FlowError::Unsupported("duplicate goto label".into()));
                    }
                    self.labels.push(LabelSite {
                        name,
                        block,
                        address: self.code.len(),
                        locals: self.active_locals.clone(),
                    });
                }
                Statement::Goto(name) => {
                    let address = self.push(Op::Jump(usize::MAX))?;
                    self.pending_gotos.push(GotoSite {
                        name,
                        block,
                        address,
                        locals: self.active_locals.clone(),
                    });
                }
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
                        if *local {
                            return Err(FlowError::Unsupported(
                                "local dotted function declaration".into(),
                            ));
                        }
                        if let Ok(slot) = self.slot(name) {
                            self.push(Op::ReadLocal(slot))?;
                        } else {
                            self.emit_global_read(name)?;
                        }
                        let mut path = fields.clone();
                        if let Some(method) = method {
                            path.push(method);
                        }
                        if path.is_empty() {
                            return Err(FlowError::InvalidBytecode);
                        }
                        for segment in &path[..path.len() - 1] {
                            let handle = self.intern_bytes(segment.as_bytes().to_vec())?;
                            self.push(Op::Constant(Value::String(handle)))?;
                            self.push(Op::GetTable)?;
                        }
                        let last = self.intern_bytes(path[path.len() - 1].as_bytes().to_vec())?;
                        self.push(Op::Constant(Value::String(last)))?;
                        let function = if method.is_some() {
                            let mut formals = Vec::with_capacity(parameters.len() + 1);
                            formals.push("self");
                            formals.extend_from_slice(parameters);
                            self.compile_function(&formals, *vararg, body)?
                        } else {
                            self.compile_function(parameters, *vararg, body)?
                        };
                        self.push(Op::NewFunction(function))?;
                        self.push(Op::SetTable)?;
                    } else {
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
                            self.active_locals.push(slot);
                            Some(slot)
                        } else {
                            self.slot(name).ok()
                        };
                        if slot.is_none() {
                            self.emit_global_target(name)?;
                        }
                        let function = self.compile_function(parameters, *vararg, body)?;
                        if *local {
                            // A locally recursive function must capture this
                            // iteration's cell, not an earlier loop visit.
                            self.push(Op::RenewLocal(slot.ok_or(FlowError::InvalidBytecode)?))?;
                        }
                        self.push(Op::NewFunction(function))?;
                        if let Some(slot) = slot {
                            self.push(Op::AssignLocals(vec![slot]))?;
                        } else {
                            self.push(Op::AssignPrepared(1))?;
                        }
                    }
                }
                Statement::Call(call) => {
                    self.expression(call, 0)?;
                    self.push(Op::Discard)?;
                }
                Statement::Local {
                    names,
                    attributes,
                    initializers,
                } => {
                    if attributes
                        .iter()
                        .any(|attribute| matches!(attribute, Some(LocalAttribute::Close)))
                    {
                        return Err(FlowError::Unsupported(
                            "to-be-closed Lua variables are not implemented".into(),
                        ));
                    }
                    // Initializers see only previous bindings, including
                    // when extra RHS values are subsequently discarded.
                    self.values(initializers, names.len())?;
                    let mut targets = Vec::with_capacity(names.len());
                    for (name, attribute) in names.iter().zip(attributes) {
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
                        self.active_locals.push(slot);
                        if matches!(attribute, Some(LocalAttribute::Const)) {
                            self.read_only.insert(slot);
                        }
                    }
                    self.push(Op::DeclareLocals(targets))?;
                }
                Statement::Assign { targets, values } => {
                    // Lua evaluates all LHS references first, then all RHS
                    // expressions, then performs the assignments.
                    for target in targets {
                        match target {
                            Expr::Atom(name) => {
                                if let Ok(slot) = self.slot(name) {
                                    if self.read_only.contains(&slot) {
                                        return Err(FlowError::Unsupported(
                                            "read-only Lua local or for control variable".into(),
                                        ));
                                    }
                                    self.push(Op::PrepareLocal(slot))?;
                                } else {
                                    self.emit_global_target(name)?;
                                }
                            }
                            Expr::Index { object, key } => {
                                self.expression(object, 0)?;
                                self.expression(key, 0)?;
                                self.push(Op::PrepareIndexed)?;
                            }
                            Expr::Field { object, name } => {
                                self.expression(object, 0)?;
                                let handle = self.intern_bytes(name.as_bytes().to_vec())?;
                                self.push(Op::Constant(Value::String(handle)))?;
                                self.push(Op::PrepareIndexed)?;
                            }
                            _ => {
                                return Err(FlowError::Unsupported(
                                    "only local and table-field assignment targets".into(),
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
                    let active = self.active_locals.len();
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
                    self.active_locals.truncate(active);
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
        self.block_stack.pop();
        Ok(())
    }
}

/// Compile the independent control-flow subset to executable stack bytecode.
/// Successful parsing does not imply full Lua grammar or runtime conformance.
pub fn compile_flow_chunk(source: &str) -> Result<FlowProgram, FlowError> {
    let statements = parse_chunk(source).map_err(|err| FlowError::Unsupported(err.to_string()))?;
    let functions = Rc::new(RefCell::new(Vec::new()));
    let strings = Rc::new(RefCell::new(Vec::new()));
    let mut compiler = Compiler {
        code: Vec::new(),
        frames: vec![HashMap::new()],
        slots: 0,
        depth: 0,
        read_only: HashSet::new(),
        loop_breaks: Vec::new(),
        block_parents: Vec::new(),
        block_stack: Vec::new(),
        active_locals: Vec::new(),
        labels: Vec::new(),
        pending_gotos: Vec::new(),
        functions: Rc::clone(&functions),
        strings: Rc::clone(&strings),
        vararg: false,
    };
    compiler.statements(&statements)?;
    compiler.resolve_gotos()?;
    // Lua chunks implicitly return zero values when control falls off the end.
    // Resolve jumps first: a label at EOF will now land on this real return.
    // An existing explicit return still exits before reaching the tail.
    compiler.push(Op::ReturnMany(0))?;
    let compiled_functions = functions.borrow().clone();
    let compiled_strings = strings.borrow().clone();
    Ok(FlowProgram {
        instructions: compiler.code,
        slots: compiler.slots,
        functions: compiled_functions,
        strings: compiled_strings,
    })
}

/// Lua 5.4 string escapes use the original 1–6 byte UTF-8 encoding,
/// including surrogate values and scalars above Unicode's 0x10FFFF limit.
/// Rust's char::from_u32 would incorrectly reject those byte sequences.
fn append_lua_utf8_escape(mut scalar: u32, result: &mut Vec<u8>) -> Result<(), FlowError> {
    if scalar > 0x7fff_ffff {
        return Err(FlowError::Unsupported("Unicode escape overflow".into()));
    }
    if scalar < 0x80 {
        result.push(scalar as u8);
        return Ok(());
    }
    let mut continuation = [0u8; 5];
    let mut count = 0;
    let mut max_first = 0x3fu32;
    loop {
        continuation[count] = 0x80 | (scalar as u8 & 0x3f);
        count += 1;
        scalar >>= 6;
        max_first >>= 1;
        if scalar <= max_first {
            break;
        }
    }
    result.push(((!max_first << 1) | scalar) as u8);
    for byte in continuation[..count].iter().rev() {
        result.push(*byte);
    }
    Ok(())
}

fn decode_lua_string(spelling: &str) -> Result<Vec<u8>, FlowError> {
    let bytes = spelling.as_bytes();
    if bytes.is_empty() {
        return Err(FlowError::InvalidBytecode);
    }
    if bytes[0] == b'[' {
        // Lua long-bracket delimiters have matching equals-sign depth.
        let equals = bytes.iter().skip(1).take_while(|b| **b == b'=').count();
        let start = equals + 2;
        let end = equals + 2;
        if bytes.get(start - 1) != Some(&b'[')
            || bytes.len() < start + end
            || bytes[bytes.len() - end] != b']'
            || bytes[bytes.len() - end + 1..bytes.len() - 1]
                .iter()
                .any(|b| *b != b'=')
            || *bytes.last().ok_or(FlowError::InvalidBytecode)? != b']'
        {
            return Err(FlowError::Unsupported("invalid long string".into()));
        }
        let mut body = &bytes[start..bytes.len() - end];
        if body.starts_with(b"\r\n") || body.starts_with(b"\n\r") {
            body = &body[2..];
        } else if body.starts_with(b"\r") || body.starts_with(b"\n") {
            body = &body[1..];
        }
        // Lua 5.4 normalizes all four physical newline spellings inside
        // long strings to LF. The optional first newline was already stripped.
        let mut normalized = Vec::with_capacity(body.len());
        let mut at = 0;
        while at < body.len() {
            let byte = body[at];
            at += 1;
            if byte == b'\r' || byte == b'\n' {
                if body
                    .get(at)
                    .is_some_and(|next| (*next == b'\r' || *next == b'\n') && *next != byte)
                {
                    at += 1;
                }
                normalized.push(b'\n');
            } else {
                normalized.push(byte);
            }
        }
        return Ok(normalized);
    }
    let quote = bytes[0];
    if !matches!(quote, b'\'' | b'"') || bytes.last() != Some(&quote) {
        return Err(FlowError::Unsupported("invalid short string".into()));
    }
    let mut result = Vec::new();
    let mut i = 1;
    while i + 1 < bytes.len() {
        let byte = bytes[i];
        i += 1;
        if byte != b'\\' {
            result.push(byte);
            continue;
        }
        let escaped = *bytes.get(i).ok_or(FlowError::InvalidBytecode)?;
        i += 1;
        let value = match escaped {
            b'a' => Some(7),
            b'b' => Some(8),
            b'f' => Some(12),
            b'n' => Some(10),
            b'r' => Some(13),
            b't' => Some(9),
            b'v' => Some(11),
            b'\\' | b'\'' | b'"' => Some(escaped),
            b'\n' | b'\r' => {
                if bytes.get(i) == Some(&if escaped == b'\n' { b'\r' } else { b'\n' }) {
                    i += 1;
                }
                Some(b'\n')
            }
            b'z' => {
                while bytes.get(i).is_some_and(|byte| byte.is_ascii_whitespace()) {
                    i += 1;
                }
                None
            }
            b'x' => {
                let digit = |byte: u8| (byte as char).to_digit(16);
                let a = bytes.get(i).copied().and_then(digit);
                let b = bytes.get(i + 1).copied().and_then(digit);
                let (Some(a), Some(b)) = (a, b) else {
                    return Err(FlowError::Unsupported("invalid hex escape".into()));
                };
                i += 2;
                Some((a * 16 + b) as u8)
            }
            b'u' => {
                if bytes.get(i) != Some(&b'{') {
                    return Err(FlowError::Unsupported("invalid Unicode escape".into()));
                }
                i += 1;
                let mut scalar = 0u32;
                let mut digits = 0;
                while let Some(next) = bytes.get(i).copied() {
                    let Some(value) = (next as char).to_digit(16) else {
                        break;
                    };
                    scalar = scalar
                        .checked_mul(16)
                        .and_then(|number| number.checked_add(value))
                        .ok_or_else(|| FlowError::Unsupported("Unicode escape overflow".into()))?;
                    digits += 1;
                    i += 1;
                }
                if digits == 0 || bytes.get(i) != Some(&b'}') {
                    return Err(FlowError::Unsupported("unfinished Unicode escape".into()));
                }
                i += 1;
                append_lua_utf8_escape(scalar, &mut result)?;
                None
            }
            b'0'..=b'9' => {
                let mut value = u32::from(escaped - b'0');
                for _ in 0..2 {
                    let Some(next) = bytes.get(i).copied() else {
                        break;
                    };
                    if !next.is_ascii_digit() {
                        break;
                    }
                    value = value * 10 + u32::from(next - b'0');
                    i += 1;
                }
                if value > 255 {
                    return Err(FlowError::Unsupported("decimal escape exceeds byte".into()));
                }
                Some(value as u8)
            }
            _ => {
                return Err(FlowError::Unsupported(
                    "string escape not yet implemented".into(),
                ));
            }
        };
        if let Some(value) = value {
            result.push(value);
        }
    }
    Ok(result)
}

/// Decode a bounded Lua hexadecimal floating literal independently of Rust's
/// decimal-only f64 parser. The lexer already validates lexical digits.
fn parse_lua_hex_float(source: &str) -> Option<f64> {
    let body = source
        .strip_prefix("0x")
        .or_else(|| source.strip_prefix("0X"))?;
    let exponent_at = body.find(['p', 'P']);
    let (mantissa, exponent) = if let Some(index) = exponent_at {
        let exponent = body.get(index + 1..)?.parse::<i32>().ok()?;
        (body.get(..index)?, exponent)
    } else {
        if !body.contains('.') {
            return None;
        }
        (body, 0)
    };
    if exponent.unsigned_abs() > 4096 {
        return None;
    }
    let mut value = 0.0_f64;
    let mut fractional = false;
    let mut scale = 1.0_f64;
    let mut digits = 0;
    for character in mantissa.chars() {
        if character == '.' && !fractional {
            fractional = true;
            continue;
        }
        let digit = character.to_digit(16)?;
        digits += 1;
        if fractional {
            scale /= 16.0;
            value += f64::from(digit) * scale;
        } else {
            value = value * 16.0 + f64::from(digit);
        }
    }
    (digits > 0).then(|| value * 2.0_f64.powi(exponent))
}

fn number_to_f64(value: Value) -> Result<f64, FlowError> {
    match value {
        Value::Integer(number) => Ok(number as f64),
        Value::Float(bits) => Ok(f64::from_bits(bits)),
        _ => Err(FlowError::TypeError),
    }
}

fn compare_integer_float(integer: i64, floating: f64) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    if floating.is_nan() {
        return None;
    }
    if floating >= 9_223_372_036_854_775_808.0 {
        return Some(Ordering::Less);
    }
    if floating < -9_223_372_036_854_775_808.0 {
        return Some(Ordering::Greater);
    }
    let whole = floating.trunc() as i64;
    match integer.cmp(&whole) {
        Ordering::Equal if floating.fract() > 0.0 => Some(Ordering::Less),
        Ordering::Equal if floating.fract() < 0.0 => Some(Ordering::Greater),
        other => Some(other),
    }
}

fn numeric_order(left: Value, right: Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Integer(a), Value::Integer(b)) => Some(a.cmp(&b)),
        (Value::Float(a), Value::Float(b)) => f64::from_bits(a).partial_cmp(&f64::from_bits(b)),
        (Value::Integer(a), Value::Float(b)) => compare_integer_float(a, f64::from_bits(b)),
        (Value::Float(a), Value::Integer(b)) => {
            compare_integer_float(b, f64::from_bits(a)).map(std::cmp::Ordering::reverse)
        }
        _ => None,
    }
}

fn numeric_equal(a: Value, b: Value) -> Result<bool, FlowError> {
    match (a, b) {
        (Value::Integer(left), Value::Integer(right)) => Ok(left == right),
        (Value::Float(left), Value::Float(right)) => {
            Ok(f64::from_bits(left) == f64::from_bits(right))
        }
        (Value::Integer(integer), Value::Float(bits))
        | (Value::Float(bits), Value::Integer(integer)) => {
            let float = f64::from_bits(bits);
            Ok(float.is_finite()
                && float.fract() == 0.0
                && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&float)
                && float as i64 == integer)
        }
        _ => Err(FlowError::TypeError),
    }
}

fn evaluate_number_binary(op: Binary, left: Value, right: Value) -> Result<Value, FlowError> {
    if !matches!(left, Value::Float(_))
        && !matches!(right, Value::Float(_))
        && !matches!(op, Binary::Div | Binary::Pow)
    {
        return evaluate_binary(op, left, right);
    }
    let (a, b) = (number_to_f64(left)?, number_to_f64(right)?);
    let result = match op {
        Binary::Add => a + b,
        Binary::Sub => a - b,
        Binary::Mul => a * b,
        Binary::Div => a / b,
        Binary::Pow => a.powf(b),
        Binary::FloorDiv => (a / b).floor(),
        Binary::Mod => a - (a / b).floor() * b,
        _ => return Err(FlowError::TypeError),
    };
    Ok(Value::Float(result.to_bits()))
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
enum TableKey {
    Positive(usize),
    Hashed(Value),
}

#[derive(Clone, Copy)]
enum PendingTarget {
    Local(usize),
    Global(usize),
    Indexed { table: usize, key: TableKey },
}

struct TableStorage {
    array: Vec<Option<Value>>,
    hashed: HashMap<Value, Value>,
    /// Optional first-party table metatable; metamethod dispatch remains future work.
    metatable: Option<usize>,
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
    tables: Vec<TableStorage>,
    table_cells: usize,
    closures: Vec<Closure>,
    cells: Vec<Option<Value>>,
    captured_cells: HashSet<usize>,
    free_cells: Vec<usize>,
    frame_allocations: Vec<Vec<usize>>,
    strings: Vec<Vec<u8>>,
    stdout: Vec<u8>,
    script_name: &'a str,
    script_args: &'a [String],
    arg_handle: Option<usize>,
    globals: HashMap<usize, Value>,
    global_table: Option<usize>,
}

impl Execution<'_> {
    fn table_key(&self, key: Value) -> Result<TableKey, FlowError> {
        match key {
            Value::Integer(index) if (1..=MAX_TABLE_FIELDS as i64).contains(&index) => {
                Ok(TableKey::Positive(index as usize))
            }
            Value::String(handle) => {
                self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
                Ok(TableKey::Hashed(key))
            }
            Value::Float(bits) => {
                let number = f64::from_bits(bits);
                if number.is_nan() {
                    return Err(FlowError::TypeError);
                }
                // Lua normalizes an integral floating table key to the
                // corresponding integer key, including both signed zeros.
                if number.fract() == 0.0
                    && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&number)
                {
                    let integer = number as i64;
                    if (1..=MAX_TABLE_FIELDS as i64).contains(&integer) {
                        return Ok(TableKey::Positive(integer as usize));
                    }
                    return Ok(TableKey::Hashed(Value::Integer(integer)));
                }
                Ok(TableKey::Hashed(Value::Float(bits)))
            }
            Value::Nil => Err(FlowError::TypeError),
            _ => Ok(TableKey::Hashed(key)),
        }
    }

    fn read_table(&self, handle: usize, key: TableKey) -> Result<Value, FlowError> {
        let table = self.tables.get(handle).ok_or(FlowError::InvalidBytecode)?;
        Ok(match key {
            TableKey::Positive(field) => table
                .array
                .get(field - 1)
                .copied()
                .flatten()
                .unwrap_or(Value::Nil),
            TableKey::Hashed(field) => table.hashed.get(&field).copied().unwrap_or(Value::Nil),
        })
    }

    fn write_table(&mut self, handle: usize, key: TableKey, value: Value) -> Result<(), FlowError> {
        let table = self
            .tables
            .get_mut(handle)
            .ok_or(FlowError::InvalidBytecode)?;
        match key {
            TableKey::Positive(field) => {
                // Deleting a nonexistent field is a no-op, not a reason to
                // grow the bounded array or consume execution heap quota.
                if value == Value::Nil && field > table.array.len() {
                    return Ok(());
                }
                if field > table.array.len() {
                    let growth = field - table.array.len();
                    if self.table_cells.saturating_add(growth) > MAX_TABLE_CELLS_TOTAL {
                        return Err(FlowError::TooLarge);
                    }
                    self.table_cells += growth;
                    table.array.resize(field, None);
                }
                table.array[field - 1] = (value != Value::Nil).then_some(value);
            }
            TableKey::Hashed(field) => {
                if value == Value::Nil {
                    if table.hashed.remove(&field).is_some() {
                        self.table_cells -= 1;
                    }
                } else {
                    if !table.hashed.contains_key(&field) {
                        if self.table_cells >= MAX_TABLE_CELLS_TOTAL {
                            return Err(FlowError::TooLarge);
                        }
                        self.table_cells += 1;
                    }
                    table.hashed.insert(field, value);
                }
            }
        }
        Ok(())
    }

    /// Lazily expose the real global environment as a writable Lua table.
    /// Prior global overrides are folded in before the table becomes visible.
    fn materialize_global_table(&mut self) -> Result<usize, FlowError> {
        if let Some(handle) = self.global_table {
            return Ok(handle);
        }
        if self.tables.len() >= MAX_TABLES {
            return Err(FlowError::TooLarge);
        }
        let handle = self.tables.len();
        self.tables.push(TableStorage {
            array: Vec::new(),
            hashed: HashMap::new(),
            metatable: None,
        });
        self.global_table = Some(handle);
        for &(name, builtin) in BUILTIN_GLOBALS {
            let key = self.intern_string(name.to_vec())?;
            self.write_table(handle, TableKey::Hashed(key), Value::Builtin(builtin))?;
        }
        let version = self.intern_string(b"_VERSION".to_vec())?;
        let number = self.intern_string(b"Lua 5.4".to_vec())?;
        self.write_table(handle, TableKey::Hashed(version), number)?;
        let self_key = self.intern_string(b"_G".to_vec())?;
        self.write_table(handle, TableKey::Hashed(self_key), Value::Table(handle))?;
        let arg_key = self.intern_string(b"arg".to_vec())?;
        let arguments = self.argument_table()?;
        self.write_table(handle, TableKey::Hashed(arg_key), arguments)?;
        for (key, value) in std::mem::take(&mut self.globals) {
            self.write_table(handle, TableKey::Hashed(Value::String(key)), value)?;
        }
        Ok(handle)
    }

    /// Global reads and writes share the same environment even when Lua code
    /// accesses it through _G. Prior to _G materialization, values are lazy.
    fn read_global(&mut self, key: usize) -> Result<Value, FlowError> {
        let name = self.strings.get(key).ok_or(FlowError::InvalidBytecode)?;
        if let Some(handle) = self.global_table {
            return self.lookup_with_metatable(handle, TableKey::Hashed(Value::String(key)));
        }
        if let Some(&value) = self.globals.get(&key) {
            return Ok(value);
        }
        if name == b"_G" || name == b"_ENV" {
            let handle = self.materialize_global_table()?;
            return Ok(Value::Table(handle));
        }
        let builtin = BUILTIN_GLOBALS
            .iter()
            .find_map(|&(builtin_name, builtin)| (builtin_name == name).then_some(builtin));
        if let Some(builtin) = builtin {
            return Ok(Value::Builtin(builtin));
        }
        if name == b"arg" {
            return self.argument_table();
        }
        if name == b"_VERSION" {
            return self.intern_string(b"Lua 5.4".to_vec());
        }
        Ok(Value::Nil)
    }

    /// Materialize the standard Lua argv table only when referenced. Repeated
    /// global lookups and calls share the same writable table identity.
    fn argument_table(&mut self) -> Result<Value, FlowError> {
        if let Some(handle) = self.arg_handle {
            return Ok(Value::Table(handle));
        }
        if self.tables.len() >= MAX_TABLES || self.script_args.len() > MAX_TABLE_FIELDS {
            return Err(FlowError::TooLarge);
        }
        let name = self.intern_string(self.script_name.as_bytes().to_vec())?;
        let handle = self.tables.len();
        self.tables.push(TableStorage {
            array: Vec::new(),
            hashed: HashMap::new(),
            metatable: None,
        });
        self.write_table(handle, TableKey::Hashed(Value::Integer(0)), name)?;
        for (index, argument) in self.script_args.iter().enumerate() {
            let text = self.intern_string(argument.as_bytes().to_vec())?;
            self.write_table(handle, TableKey::Positive(index + 1), text)?;
        }
        self.arg_handle = Some(handle);
        Ok(Value::Table(handle))
    }

    /// Bounded raw table traversal. Lua leaves hash traversal order unspecified;
    /// iteration uses the current table storage order, never a sorted-key promise.
    /// Missing/deleted previous keys are rejected rather than silently restarting.
    fn next_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let Value::Table(handle) = *args.first().ok_or(FlowError::TypeError)? else {
            return Err(FlowError::TypeError);
        };
        let table = self.tables.get(handle).ok_or(FlowError::InvalidBytecode)?;
        // The table scan is bounded by the same shared execution budget as
        // bytecode, preventing repeated next() calls from bypassing VM limits.
        let scan_cost = table.array.len().saturating_add(table.hashed.len());
        if self.steps.saturating_add(scan_cost) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += scan_cost;
        let mut entries = Vec::new();
        for (index, cell) in table.array.iter().enumerate() {
            if let Some(value) = cell.filter(|value| *value != Value::Nil) {
                entries.push((Value::Integer((index + 1) as i64), value));
            }
        }
        for (&key, &value) in &table.hashed {
            if value != Value::Nil {
                entries.push((key, value));
            }
        }
        let offset = match args.get(1).copied().unwrap_or(Value::Nil) {
            Value::Nil => 0,
            key => {
                let normalized = match self.table_key(key)? {
                    TableKey::Positive(index) => Value::Integer(index as i64),
                    TableKey::Hashed(value) => value,
                };
                entries
                    .iter()
                    .position(|(existing, _)| *existing == normalized)
                    .ok_or(FlowError::TypeError)?
                    + 1
            }
        };
        Ok(match entries.get(offset) {
            Some(&(key, value)) => vec![key, value],
            None => vec![Value::Nil],
        })
    }

    /// Lua 5.4 ipairs uses ordinary positive-integer table indexing, including
    /// table __index fallbacks, and terminates at the first nil.
    fn ipairs_iterator(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let Value::Table(handle) = *args.first().ok_or(FlowError::TypeError)? else {
            return Err(FlowError::TypeError);
        };
        let index = self.exact_integer_operand(*args.get(1).ok_or(FlowError::TypeError)?)?;
        let index = index.checked_add(1).ok_or(FlowError::TypeError)?;
        let key = self.table_key(Value::Integer(index))?;
        let value = self.lookup_with_metatable(handle, key)?;
        Ok(if value == Value::Nil {
            vec![Value::Nil]
        } else {
            vec![Value::Integer(index), value]
        })
    }

    /// Raw metatable field lookup, without invoking a metamethod.
    fn metatable_field(&self, handle: usize, name: &[u8]) -> Result<Option<Value>, FlowError> {
        let metatable = self
            .tables
            .get(handle)
            .ok_or(FlowError::InvalidBytecode)?
            .metatable;
        let Some(metatable) = metatable else {
            return Ok(None);
        };
        let Some(key) = self.strings.iter().position(|bytes| bytes == name) else {
            return Ok(None);
        };
        let value = self.read_table(metatable, TableKey::Hashed(Value::String(key)))?;
        Ok((value != Value::Nil).then_some(value))
    }

    /// Lua's __metatable field protects metatables from ordinary base-library
    /// reads and replacements. Its value is accessed without dispatch.
    fn protected_metatable(&self, handle: usize) -> Result<Option<Value>, FlowError> {
        self.metatable_field(handle, b"__metatable")
    }

    /// Convert canonicalized table keys back into their Lua runtime values.
    fn table_key_value(key: TableKey) -> Value {
        match key {
            TableKey::Positive(index) => Value::Integer(index as i64),
            TableKey::Hashed(value) => value,
        }
    }

    /// Table-valued __index fallbacks form a bounded chain.
    fn lookup_with_metatable(
        &mut self,
        mut handle: usize,
        key: TableKey,
    ) -> Result<Value, FlowError> {
        for _ in 0..MAX_CALL_DEPTH {
            if self.steps >= self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += 1;
            let value = self.read_table(handle, key)?;
            if value != Value::Nil {
                return Ok(value);
            }
            match self.metatable_field(handle, b"__index")? {
                Some(Value::Table(fallback)) => handle = fallback,
                Some(callback) if !matches!(callback, Value::Table(_)) => {
                    // A function-valued metamethod may be a builtin. Table
                    // values stay on the raw __index redirect path above.
                    let args = [Value::Table(handle), Self::table_key_value(key)];
                    if self.depth >= MAX_CALL_DEPTH {
                        return Err(FlowError::CallDepthExceeded);
                    }
                    self.depth += 1;
                    let result = self.protected_target(callback, &args);
                    self.depth -= 1;
                    let returned = result?;
                    return Ok(returned.first().copied().unwrap_or(Value::Nil));
                }
                None => return Ok(Value::Nil),
                Some(_) => return Err(FlowError::TypeError),
            }
        }
        Err(FlowError::CallDepthExceeded)
    }

    /// Existing keys bypass __newindex; absent keys may redirect to a table.
    fn assign_with_metatable(
        &mut self,
        mut handle: usize,
        key: TableKey,
        value: Value,
    ) -> Result<(), FlowError> {
        for _ in 0..MAX_CALL_DEPTH {
            if self.steps >= self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += 1;
            if self.read_table(handle, key)? != Value::Nil {
                return self.write_table(handle, key, value);
            }
            match self.metatable_field(handle, b"__newindex")? {
                Some(Value::Table(fallback)) => handle = fallback,
                Some(callback) if !matches!(callback, Value::Table(_)) => {
                    let args = [Value::Table(handle), Self::table_key_value(key), value];
                    if self.depth >= MAX_CALL_DEPTH {
                        return Err(FlowError::CallDepthExceeded);
                    }
                    self.depth += 1;
                    let result = self.protected_target(callback, &args);
                    self.depth -= 1;
                    result?;
                    return Ok(());
                }
                None => return self.write_table(handle, key, value),
                Some(_) => return Err(FlowError::TypeError),
            }
        }
        Err(FlowError::CallDepthExceeded)
    }

    fn getmetatable_builtin(&self, args: &[Value]) -> Result<Value, FlowError> {
        let value = *args.first().ok_or(FlowError::TypeError)?;
        let Value::Table(handle) = value else {
            // This bounded VM does not install host-defined primitive metatables.
            return Ok(Value::Nil);
        };
        let Some(metatable) = self
            .tables
            .get(handle)
            .ok_or(FlowError::InvalidBytecode)?
            .metatable
        else {
            return Ok(Value::Nil);
        };
        Ok(self
            .protected_metatable(handle)?
            .unwrap_or(Value::Table(metatable)))
    }

    fn setmetatable_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        let Value::Table(handle) = *args.first().ok_or(FlowError::TypeError)? else {
            return Err(FlowError::TypeError);
        };
        let metatable = match *args.get(1).ok_or(FlowError::TypeError)? {
            Value::Nil => None,
            Value::Table(handle) => {
                self.tables.get(handle).ok_or(FlowError::InvalidBytecode)?;
                Some(handle)
            }
            _ => return Err(FlowError::TypeError),
        };
        if self.protected_metatable(handle)?.is_some() {
            return Err(FlowError::TypeError);
        }
        self.tables
            .get_mut(handle)
            .ok_or(FlowError::InvalidBytecode)?
            .metatable = metatable;
        Ok(Value::Table(handle))
    }

    fn tonumber_builtin(&self, args: &[Value]) -> Result<Value, FlowError> {
        let value = args.first().copied().unwrap_or(Value::Nil);
        if let Some(base_value) = args.get(1).copied().filter(|arg| *arg != Value::Nil) {
            let base = self.exact_integer_operand(base_value)?;
            if !(2..=36).contains(&base) {
                return Err(FlowError::TypeError);
            }
            let Value::String(handle) = value else {
                return Err(FlowError::TypeError);
            };
            let bytes = self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
            let source = std::str::from_utf8(bytes).map_err(|_| FlowError::TypeError)?;
            let source = source.trim();
            let (negative, unsigned) = if let Some(rest) = source.strip_prefix('-') {
                (true, rest)
            } else {
                (false, source.strip_prefix('+').unwrap_or(source))
            };
            let Ok(number) = u64::from_str_radix(unsigned, base as u32) else {
                return Ok(Value::Nil);
            };
            if negative {
                if number == (i64::MAX as u64) + 1 {
                    return Ok(Value::Integer(i64::MIN));
                }
                if let Ok(integer) = i64::try_from(number) {
                    return Ok(Value::Integer(-integer));
                }
            } else if let Ok(integer) = i64::try_from(number) {
                return Ok(Value::Integer(integer));
            }
            return Ok(Value::Nil);
        }
        match value {
            Value::Integer(_) | Value::Float(_) => Ok(value),
            Value::String(handle) => {
                if let Ok(converted) = self.arithmetic_operand(value) {
                    return Ok(converted);
                }
                let bytes = self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
                let source = std::str::from_utf8(bytes).map_err(|_| FlowError::TypeError)?;
                Ok(parse_lua_hex_float(source.trim())
                    .map(|number| Value::Float(number.to_bits()))
                    .unwrap_or(Value::Nil))
            }
            _ => Ok(Value::Nil),
        }
    }

    /// Arithmetic first attempts Lua numeric coercion; only an operand type
    /// failure can trigger a table's first available metamethod (left first).
    /// Runtime arithmetic errors such as integer division by zero propagate.
    fn arithmetic_with_metamethod(
        &mut self,
        op: Binary,
        left: Value,
        right: Value,
    ) -> Result<Value, FlowError> {
        let direct = self
            .arithmetic_operand(left)
            .and_then(|a| self.arithmetic_operand(right).map(|b| (a, b)))
            .and_then(|(a, b)| evaluate_number_binary(op, a, b));
        match direct {
            Ok(value) => Ok(value),
            Err(FlowError::TypeError) => {
                let name: &[u8] = match op {
                    Binary::Add => b"__add",
                    Binary::Sub => b"__sub",
                    Binary::Mul => b"__mul",
                    Binary::Div => b"__div",
                    Binary::FloorDiv => b"__idiv",
                    Binary::Mod => b"__mod",
                    Binary::Pow => b"__pow",
                    _ => return Err(FlowError::InvalidBytecode),
                };
                self.binary_metamethod(name, left, right)
            }
            Err(error) => Err(error),
        }
    }

    /// Bitwise operators keep the existing bounded integer semantics.
    /// A type mismatch consults table-valued operand metamethods left first.
    fn bitwise_with_metamethod(
        &mut self,
        op: Binary,
        left: Value,
        right: Value,
    ) -> Result<Value, FlowError> {
        let direct = self
            .exact_integer_operand(left)
            .and_then(|a| self.exact_integer_operand(right).map(|b| (a, b)))
            .and_then(|(a, b)| evaluate_binary(op, Value::Integer(a), Value::Integer(b)));
        match direct {
            Ok(value) => Ok(value),
            Err(FlowError::TypeError) => {
                let name: &[u8] = match op {
                    Binary::BitAnd => b"__band",
                    Binary::BitOr => b"__bor",
                    Binary::BitXor => b"__bxor",
                    Binary::Shl => b"__shl",
                    Binary::Shr => b"__shr",
                    _ => return Err(FlowError::InvalidBytecode),
                };
                self.binary_metamethod(name, left, right)
            }
            Err(error) => Err(error),
        }
    }

    /// Resolve a binary metamethod with Lua's left-then-right table lookup.
    /// Missing fields are distinct from present but non-callable values.
    fn binary_metamethod_callback(
        &self,
        name: &[u8],
        left: Value,
        right: Value,
    ) -> Result<Option<Value>, FlowError> {
        for operand in [left, right] {
            let Value::Table(handle) = operand else {
                continue;
            };
            if let Some(method) = self.metatable_field(handle, name)? {
                return Ok(Some(method));
            }
        }
        Ok(None)
    }

    /// Invoke a binary metamethod as an ordinary Lua callable with a bounded
    /// host frame, including callable tables and builtin function values.
    fn invoke_binary_metamethod(
        &mut self,
        callback: Value,
        left: Value,
        right: Value,
    ) -> Result<Vec<Value>, FlowError> {
        if self.depth >= MAX_CALL_DEPTH {
            return Err(FlowError::CallDepthExceeded);
        }
        self.depth += 1;
        let result = self.protected_target(callback, &[left, right]);
        self.depth -= 1;
        result
    }

    /// Callbacks reuse closure frames, recursion depth and shared step limits.
    fn binary_metamethod(
        &mut self,
        name: &[u8],
        left: Value,
        right: Value,
    ) -> Result<Value, FlowError> {
        let callback = self
            .binary_metamethod_callback(name, left, right)?
            .ok_or(FlowError::TypeError)?;
        let returned = self.invoke_binary_metamethod(callback, left, right)?;
        Ok(returned.first().copied().unwrap_or(Value::Nil))
    }

    /// Lua table identity is raw for identical objects; different tables may
    /// dispatch __eq, with the resulting value interpreted by Lua truthiness.
    fn equality_with_metamethod(
        &mut self,
        op: Binary,
        left: Value,
        right: Value,
    ) -> Result<Value, FlowError> {
        let equal = if left == right {
            true
        } else if let Some(callback) = self.binary_metamethod_callback(b"__eq", left, right)? {
            self.invoke_binary_metamethod(callback, left, right)?
                .first()
                .copied()
                .unwrap_or(Value::Nil)
                .truthy()
        } else {
            false
        };
        Ok(Value::Boolean(if op == Binary::Equal {
            equal
        } else {
            !equal
        }))
    }

    /// Greater-than operators reverse operands. Lua less-equal falls back
    /// to negating the reversed less-than result when __le is absent.
    fn ordering_with_metamethod(
        &mut self,
        op: Binary,
        left: Value,
        right: Value,
    ) -> Result<Value, FlowError> {
        let (left, right, name) = match op {
            Binary::Less => (left, right, &b"__lt"[..]),
            Binary::LessEqual => (left, right, &b"__le"[..]),
            Binary::Greater => (right, left, &b"__lt"[..]),
            Binary::GreaterEqual => (right, left, &b"__le"[..]),
            _ => return Err(FlowError::InvalidBytecode),
        };
        if let Some(callback) = self.binary_metamethod_callback(name, left, right)? {
            let truthy = self
                .invoke_binary_metamethod(callback, left, right)?
                .first()
                .copied()
                .unwrap_or(Value::Nil)
                .truthy();
            return Ok(Value::Boolean(truthy));
        }
        if matches!(op, Binary::LessEqual | Binary::GreaterEqual) {
            let callback = self
                .binary_metamethod_callback(b"__lt", right, left)?
                .ok_or(FlowError::TypeError)?;
            let truthy = self
                .invoke_binary_metamethod(callback, right, left)?
                .first()
                .copied()
                .unwrap_or(Value::Nil)
                .truthy();
            return Ok(Value::Boolean(!truthy));
        }
        Err(FlowError::TypeError)
    }

    /// Lua concatenates byte strings and numbers directly; otherwise an
    /// operand's first available __concat closure handles the original values.
    fn concat_with_metamethod(&mut self, left: Value, right: Value) -> Result<Value, FlowError> {
        let direct = self.string_bytes(left).and_then(|mut bytes| {
            let suffix = self.string_bytes(right)?;
            if bytes.len().saturating_add(suffix.len()) > MAX_STRING_POOL_BYTES {
                return Err(FlowError::TooLarge);
            }
            bytes.extend_from_slice(&suffix);
            Ok(bytes)
        });
        match direct {
            Ok(bytes) => self.intern_string(bytes),
            Err(FlowError::TypeError) => self.binary_metamethod(b"__concat", left, right),
            Err(error) => Err(error),
        }
    }

    fn arithmetic_operand(&self, value: Value) -> Result<Value, FlowError> {
        match value {
            Value::Integer(_) | Value::Float(_) => Ok(value),
            Value::String(_) => match self.integer_operand(value) {
                Ok(number) => Ok(Value::Integer(number)),
                Err(_) => {
                    let Value::String(handle) = value else {
                        unreachable!("string guard")
                    };
                    let bytes = self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
                    let source = std::str::from_utf8(bytes).map_err(|_| FlowError::TypeError)?;
                    let source = source.trim();
                    if source.is_empty() || !source.bytes().any(|byte| byte.is_ascii_digit()) {
                        return Err(FlowError::TypeError);
                    }
                    source
                        .parse::<f64>()
                        .map(|value| Value::Float(value.to_bits()))
                        .map_err(|_| FlowError::TypeError)
                }
            },
            _ => Err(FlowError::TypeError),
        }
    }

    fn exact_integer_operand(&self, value: Value) -> Result<i64, FlowError> {
        match value {
            Value::Integer(number) => Ok(number),
            Value::Float(bits) => {
                let number = f64::from_bits(bits);
                if number.is_finite()
                    && number.fract() == 0.0
                    && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&number)
                {
                    Ok(number as i64)
                } else {
                    Err(FlowError::TypeError)
                }
            }
            _ => Err(FlowError::TypeError),
        }
    }

    fn integer_operand(&self, value: Value) -> Result<i64, FlowError> {
        let Value::String(handle) = value else {
            return match value {
                Value::Integer(number) => Ok(number),
                _ => Err(FlowError::TypeError),
            };
        };
        let bytes = self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
        let source = std::str::from_utf8(bytes).map_err(|_| FlowError::TypeError)?;
        let source = source.trim();
        let (negative, unsigned) = if let Some(value) = source.strip_prefix('-') {
            (true, value)
        } else {
            (false, source.strip_prefix('+').unwrap_or(source))
        };
        if let Some(hex) = unsigned
            .strip_prefix("0x")
            .or_else(|| unsigned.strip_prefix("0X"))
        {
            let number = i64::from_str_radix(hex, 16).map_err(|_| FlowError::TypeError)?;
            if negative {
                number.checked_neg().ok_or(FlowError::TypeError)
            } else {
                Ok(number)
            }
        } else {
            source.parse::<i64>().map_err(|_| FlowError::TypeError)
        }
    }

    /// Lua error(message, level) accepts nil or an integer-convertible
    /// optional stack level. The bounded VM does not yet prefix source
    /// locations to string errors, but must reject invalid level types.
    fn error_builtin(&self, args: &[Value]) -> FlowError {
        if let Some(level) = args.get(1).copied().filter(|level| *level != Value::Nil)
            && self
                .exact_integer_operand(level)
                .or_else(|_| self.integer_operand(level))
                .is_err()
        {
            return FlowError::TypeError;
        }
        FlowError::Thrown(args.first().copied().unwrap_or(Value::Nil))
    }

    /// An assertion raises the supplied Lua error value, or the default
    /// assertion message when no custom second argument is given.
    fn assertion_failure(&mut self, args: &[Value]) -> Result<FlowError, FlowError> {
        // Lua treats a missing first argument as a bad argument, not a false
        // assertion with the usual default message.
        if args.is_empty() {
            return Ok(FlowError::TypeError);
        }
        let message = match args.get(1).copied() {
            Some(value) => value,
            None => self.intern_string(b"assertion failed!".to_vec())?,
        };
        Ok(FlowError::Thrown(message))
    }

    /// Shared stringification for ordinary and protected builtin calls.
    /// Metamethod callbacks remain subject to the owned VM call-depth guard.
    fn tostring_builtin(&mut self, value: Value) -> Result<Value, FlowError> {
        let text = match value {
            Value::String(_) => value,
            Value::Integer(number) => self.intern_string(number.to_string().into_bytes())?,
            Value::Float(bits) => {
                self.intern_string(format!("{:?}", f64::from_bits(bits)).into_bytes())?
            }
            Value::Boolean(boolean) => self.intern_string(boolean.to_string().into_bytes())?,
            Value::Nil => self.intern_string(b"nil".to_vec())?,
            Value::Table(handle) => {
                if let Some(method) = self.metatable_field(handle, b"__tostring")? {
                    // Lua invokes __tostring as a normal callable value.
                    // Bound the host frame even for self-referential builtins.
                    if self.depth >= MAX_CALL_DEPTH {
                        return Err(FlowError::CallDepthExceeded);
                    }
                    self.depth += 1;
                    let result = self.protected_target(method, &[value]);
                    self.depth -= 1;
                    let returned = result?;
                    match returned.first().copied().unwrap_or(Value::Nil) {
                        Value::String(index) => Value::String(index),
                        Value::Integer(number) => {
                            self.intern_string(number.to_string().into_bytes())?
                        }
                        Value::Float(bits) => {
                            self.intern_string(format!("{:?}", f64::from_bits(bits)).into_bytes())?
                        }
                        _ => return Err(FlowError::TypeError),
                    }
                } else {
                    self.intern_string(format!("table: 0x{handle:x}").into_bytes())?
                }
            }
            Value::Function(handle) => {
                self.intern_string(format!("function: 0x{handle:x}").into_bytes())?
            }
            Value::Builtin(_) => self.intern_string(b"function: builtin".to_vec())?,
        };
        Ok(text)
    }

    /// Lua's print resolves the current global tostring instead of bypassing
    /// overrides. Limit nested conversions so tostring=print cannot exhaust
    /// the native Rust stack.
    fn print_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        if self.depth >= MAX_CALL_DEPTH {
            return Err(FlowError::CallDepthExceeded);
        }
        self.depth += 1;
        let result = self.print_builtin_inner(args);
        self.depth -= 1;
        result
    }

    fn print_builtin_inner(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let Value::String(key) = self.intern_string(b"tostring".to_vec())? else {
            return Err(FlowError::InvalidBytecode);
        };
        let converter = self.read_global(key)?;
        let mut line = Vec::new();
        for (index, &argument) in args.iter().enumerate() {
            if index > 0 {
                line.push(b'\t');
            }
            let converted = self.protected_target(converter, &[argument])?;
            let Value::String(handle) = converted.first().copied().unwrap_or(Value::Nil) else {
                return Err(FlowError::TypeError);
            };
            let bytes = self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
            if line.len().saturating_add(bytes.len()) >= MAX_PRINT_OUTPUT_BYTES {
                return Err(FlowError::TooLarge);
            }
            line.extend_from_slice(bytes);
        }
        line.push(b'\n');
        if self.stdout.len().saturating_add(line.len()) > MAX_PRINT_OUTPUT_BYTES {
            return Err(FlowError::TooLarge);
        }
        self.stdout.extend_from_slice(&line);
        Ok(Vec::new())
    }

    /// Raw table builtins never consult __index or __newindex callbacks.
    /// The ordinary opcode path and protected-call path share these checks.
    fn rawget_builtin(&self, args: &[Value]) -> Result<Value, FlowError> {
        let Value::Table(handle) = *args.first().ok_or(FlowError::TypeError)? else {
            return Err(FlowError::TypeError);
        };
        let key = *args.get(1).ok_or(FlowError::TypeError)?;
        let key = self.table_key(key)?;
        self.read_table(handle, key)
    }

    fn rawset_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        let Value::Table(handle) = *args.first().ok_or(FlowError::TypeError)? else {
            return Err(FlowError::TypeError);
        };
        let key = *args.get(1).ok_or(FlowError::TypeError)?;
        let value = *args.get(2).ok_or(FlowError::TypeError)?;
        let key = self.table_key(key)?;
        self.write_table(handle, key, value)?;
        Ok(Value::Table(handle))
    }

    /// Return Lua's iterator, state and initial control values for pairs
    /// and ipairs. The protected and ordinary call paths share this lowering.
    fn pairs_ipairs_builtin(
        &mut self,
        builtin: Builtin,
        args: &[Value],
    ) -> Result<Vec<Value>, FlowError> {
        // Lua 5.4 pairs/ipairs validate that an argument exists, but do not
        // require a table until the default iterator is actually invoked.
        let table = *args.first().ok_or(FlowError::TypeError)?;
        if builtin == Builtin::Pairs {
            if let Value::Table(handle) = table
                && let Some(method) = self.metatable_field(handle, b"__pairs")?
            {
                // __pairs is called using Lua's regular callable semantics:
                // an owned closure, a callable table, or a function builtin.
                // Count the host frame even for builtin-to-builtin redirects.
                if self.depth >= MAX_CALL_DEPTH {
                    return Err(FlowError::CallDepthExceeded);
                }
                self.depth += 1;
                let result = self.protected_target(method, &[table]);
                self.depth -= 1;
                let values = result?;
                return Ok(vec![
                    values.first().copied().unwrap_or(Value::Nil),
                    values.get(1).copied().unwrap_or(Value::Nil),
                    values.get(2).copied().unwrap_or(Value::Nil),
                ]);
            }
            Ok(vec![Value::Builtin(Builtin::Next), table, Value::Nil])
        } else {
            Ok(vec![
                Value::Builtin(Builtin::IpairsIterator),
                table,
                Value::Integer(0),
            ])
        }
    }

    /// Nested protected builtin invocations share the conservative host-stack
    /// depth limit with first-party Lua closure calls.
    fn nested_protected_builtin(
        &mut self,
        builtin: Builtin,
        args: &[Value],
    ) -> Result<Vec<Value>, FlowError> {
        if self.depth >= MAX_CALL_DEPTH {
            return Err(FlowError::CallDepthExceeded);
        }
        self.depth += 1;
        let result = match builtin {
            Builtin::Pcall => self.pcall_builtin(args),
            Builtin::Xpcall => self.xpcall_builtin(args),
            _ => Err(FlowError::TypeError),
        };
        self.depth -= 1;
        result
    }

    /// Protected calls share Lua's bounded callable-table redirects with
    /// ordinary bytecode calls. General builtins remain an incomplete subset.
    fn protected_target(
        &mut self,
        callable: Value,
        parameters: &[Value],
    ) -> Result<Vec<Value>, FlowError> {
        let mut callable = callable;
        let mut arguments = parameters.to_vec();
        let mut redirects = 0;
        while let Value::Table(handle) = callable {
            if redirects >= MAX_CALL_DEPTH {
                return Err(FlowError::CallDepthExceeded);
            }
            if self.steps >= self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += 1;
            let metamethod = self
                .metatable_field(handle, b"__call")?
                .ok_or(FlowError::TypeError)?;
            if arguments.len() >= MAX_STACK {
                return Err(FlowError::TooLarge);
            }
            arguments.insert(0, callable);
            callable = metamethod;
            redirects += 1;
        }
        match callable {
            Value::Function(handle) => self.call_owned_function(handle, &arguments),
            Value::Builtin(Builtin::Error) => Err(self.error_builtin(&arguments)),
            Value::Builtin(Builtin::Print) => self.print_builtin(&arguments),
            Value::Builtin(Builtin::Assert) => {
                if arguments.first().copied().unwrap_or(Value::Nil).truthy() {
                    Ok(arguments)
                } else {
                    Err(self.assertion_failure(&arguments)?)
                }
            }
            Value::Builtin(Builtin::Type) => {
                let argument = *arguments.first().ok_or(FlowError::TypeError)?;
                let name: &[u8] = match argument {
                    Value::Integer(_) | Value::Float(_) => b"number",
                    Value::Boolean(_) => b"boolean",
                    Value::Nil => b"nil",
                    Value::String(_) => b"string",
                    Value::Table(_) => b"table",
                    Value::Function(_) | Value::Builtin(_) => b"function",
                };
                Ok(vec![self.intern_string(name.to_vec())?])
            }
            Value::Builtin(Builtin::ToNumber) => {
                if arguments.is_empty() {
                    return Err(FlowError::TypeError);
                }
                Ok(vec![self.tonumber_builtin(&arguments)?])
            }
            Value::Builtin(Builtin::ToString) => {
                let argument = *arguments.first().ok_or(FlowError::TypeError)?;
                Ok(vec![self.tostring_builtin(argument)?])
            }
            Value::Builtin(Builtin::RawLen) => {
                let argument = *arguments.first().ok_or(FlowError::TypeError)?;
                Ok(vec![self.raw_len(argument)?])
            }
            Value::Builtin(Builtin::RawEqual) => {
                let left = *arguments.first().ok_or(FlowError::TypeError)?;
                let right = *arguments.get(1).ok_or(FlowError::TypeError)?;
                let equal = if matches!(left, Value::Integer(_) | Value::Float(_))
                    && matches!(right, Value::Integer(_) | Value::Float(_))
                {
                    numeric_equal(left, right)?
                } else {
                    left == right
                };
                Ok(vec![Value::Boolean(equal)])
            }
            Value::Builtin(Builtin::Pairs) => self.pairs_ipairs_builtin(Builtin::Pairs, &arguments),
            Value::Builtin(Builtin::Ipairs) => {
                self.pairs_ipairs_builtin(Builtin::Ipairs, &arguments)
            }
            Value::Builtin(Builtin::Pcall) => {
                self.nested_protected_builtin(Builtin::Pcall, &arguments)
            }
            Value::Builtin(Builtin::Xpcall) => {
                self.nested_protected_builtin(Builtin::Xpcall, &arguments)
            }
            Value::Builtin(Builtin::Select) => self.select_builtin(&arguments),
            Value::Builtin(Builtin::Next) => self.next_builtin(&arguments),
            Value::Builtin(Builtin::IpairsIterator) => self.ipairs_iterator(&arguments),
            Value::Builtin(Builtin::RawGet) => Ok(vec![self.rawget_builtin(&arguments)?]),
            Value::Builtin(Builtin::RawSet) => Ok(vec![self.rawset_builtin(&arguments)?]),
            Value::Builtin(Builtin::GetMetatable) => {
                Ok(vec![self.getmetatable_builtin(&arguments)?])
            }
            Value::Builtin(Builtin::SetMetatable) => {
                Ok(vec![self.setmetatable_builtin(&arguments)?])
            }
            _ => Err(FlowError::TypeError),
        }
    }

    /// A protected call must not swallow any global VM resource limit.
    fn unprotectable_error(error: &FlowError) -> bool {
        matches!(
            error,
            FlowError::StepBudgetExceeded | FlowError::TooLarge | FlowError::CallDepthExceeded
        )
    }

    fn protected_error_value(&mut self, error: FlowError) -> Result<Value, FlowError> {
        match error {
            FlowError::Thrown(value) => Ok(value),
            other => self.intern_string(other.to_string().into_bytes()),
        }
    }

    /// Bounded pcall returns false plus the original error value, or a string
    /// describing a runtime fault. Successful calls preserve all results.
    fn pcall_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let callable = args.first().copied().unwrap_or(Value::Nil);
        let parameters = args.get(1..).unwrap_or_default();
        match self.protected_target(callable, parameters) {
            Ok(mut results) => {
                let mut values = vec![Value::Boolean(true)];
                values.append(&mut results);
                Ok(values)
            }
            Err(error) if Self::unprotectable_error(&error) => Err(error),
            Err(error) => Ok(vec![
                Value::Boolean(false),
                self.protected_error_value(error)?,
            ]),
        }
    }

    /// xpcall(f, handler, ...) executes the first-party message handler only
    /// after a recoverable failure. Handler returns are truncated to one Lua
    /// value, while successful targets preserve their full result list.
    fn xpcall_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let callable = args.first().copied().unwrap_or(Value::Nil);
        let handler = args.get(1).copied().unwrap_or(Value::Nil);
        // Lua 5.4 requires an actual function as message handler:
        // a table with __call is *not* accepted in this position.
        // Validate before executing f, including on the success path.
        if !matches!(handler, Value::Function(_) | Value::Builtin(_)) {
            return Err(FlowError::TypeError);
        }
        let parameters = args.get(2..).unwrap_or_default();
        match self.protected_target(callable, parameters) {
            Ok(mut results) => {
                let mut values = vec![Value::Boolean(true)];
                values.append(&mut results);
                Ok(values)
            }
            Err(error) if Self::unprotectable_error(&error) => Err(error),
            Err(error) => {
                let mut message = self.protected_error_value(error)?;
                // Lua retries its message handler on ordinary errors from that
                // handler. Iterate rather than recursing on the Rust stack;
                // this experimental limit is not general Lua recursion parity.
                for _ in 0..MAX_CALL_DEPTH {
                    let handled = self.protected_target(handler, &[message]);
                    match handled {
                        Ok(values) => {
                            return Ok(vec![
                                Value::Boolean(false),
                                values.first().copied().unwrap_or(Value::Nil),
                            ]);
                        }
                        Err(error) if Self::unprotectable_error(&error) => return Err(error),
                        Err(error) => message = self.protected_error_value(error)?,
                    }
                }
                Ok(vec![
                    Value::Boolean(false),
                    self.intern_string(b"error in error handling".to_vec())?,
                ])
            }
        }
    }

    fn select_builtin(&self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let selector = *args.first().ok_or(FlowError::TypeError)?;
        if let Value::String(handle) = selector {
            let bytes = self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
            if bytes == b"#" {
                return Ok(vec![Value::Integer((args.len() - 1) as i64)]);
            }
        }
        let index = self
            .exact_integer_operand(selector)
            .or_else(|_| self.integer_operand(selector))?;
        let count = (args.len() - 1) as i64;
        let index = if index < 0 {
            count
                .checked_add(index)
                .and_then(|value| value.checked_add(1))
                .ok_or(FlowError::TypeError)?
        } else {
            index
        };
        if index <= 0 {
            return Err(FlowError::TypeError);
        }
        Ok(args.iter().skip(index as usize).copied().collect())
    }

    /// Lua table length may be replaced by a first-party closure. The rawlen
    /// builtin continues to bypass __len and returns the bounded raw border.
    fn length_with_metatable(&mut self, value: Value) -> Result<Value, FlowError> {
        if let Value::Table(handle) = value
            && let Some(method) = self.metatable_field(handle, b"__len")?
        {
            // Length callbacks follow ordinary Lua callable rules.
            // Charge a bounded host frame even for recursive builtins.
            if self.depth >= MAX_CALL_DEPTH {
                return Err(FlowError::CallDepthExceeded);
            }
            self.depth += 1;
            let result = self.protected_target(method, &[value, value]);
            self.depth -= 1;
            let returned = result?;
            return Ok(returned.first().copied().unwrap_or(Value::Nil));
        }
        self.raw_len(value)
    }

    fn raw_len(&self, value: Value) -> Result<Value, FlowError> {
        let length = match value {
            Value::String(handle) => self
                .strings
                .get(handle)
                .ok_or(FlowError::InvalidBytecode)?
                .len(),
            Value::Table(handle) => {
                let array = &self
                    .tables
                    .get(handle)
                    .ok_or(FlowError::InvalidBytecode)?
                    .array;
                // A first missing positive index is a valid Lua raw border.
                array
                    .iter()
                    .take_while(|cell| !matches!(cell, None | Some(Value::Nil)))
                    .count()
            }
            _ => return Err(FlowError::TypeError),
        };
        Ok(Value::Integer(length as i64))
    }

    fn string_bytes(&self, value: Value) -> Result<Vec<u8>, FlowError> {
        match value {
            Value::String(handle) => self
                .strings
                .get(handle)
                .cloned()
                .ok_or(FlowError::InvalidBytecode),
            Value::Integer(number) => Ok(number.to_string().into_bytes()),
            Value::Float(bits) => Ok(format!("{:?}", f64::from_bits(bits)).into_bytes()),
            _ => Err(FlowError::TypeError),
        }
    }

    fn intern_string(&mut self, bytes: Vec<u8>) -> Result<Value, FlowError> {
        if let Some(handle) = self.strings.iter().position(|item| item == &bytes) {
            return Ok(Value::String(handle));
        }
        if self.strings.len() >= MAX_STRINGS
            || bytes.len() > MAX_STRING_POOL_BYTES
            || self.strings.iter().map(Vec::len).sum::<usize>()
                > MAX_STRING_POOL_BYTES.saturating_sub(bytes.len())
        {
            return Err(FlowError::TooLarge);
        }
        let handle = self.strings.len();
        self.strings.push(bytes);
        Ok(Value::String(handle))
    }

    fn allocate_cell(&mut self) -> Result<usize, FlowError> {
        let cell = if let Some(cell) = self.free_cells.pop() {
            *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = None;
            cell
        } else {
            if self.cells.len() >= MAX_CELLS_TOTAL {
                return Err(FlowError::TooLarge);
            }
            let cell = self.cells.len();
            self.cells.push(None);
            cell
        };
        self.frame_allocations
            .last_mut()
            .ok_or(FlowError::InvalidBytecode)?
            .push(cell);
        Ok(cell)
    }

    /// Fresh lexical cell on successive loop iterations; old closures retain
    /// the prior cell rather than observing later iteration values.
    fn iteration_cell(&mut self, slots: &mut [usize], slot: usize) -> Result<usize, FlowError> {
        let cell = *slots.get(slot).ok_or(FlowError::InvalidBytecode)?;
        self.cells.get(cell).ok_or(FlowError::InvalidBytecode)?;
        if !self.captured_cells.contains(&cell) {
            return Ok(cell);
        }
        let fresh = self.allocate_cell()?;
        *slots.get_mut(slot).ok_or(FlowError::InvalidBytecode)? = fresh;
        Ok(fresh)
    }

    /// Single closure invocation path for bytecode calls and metamethods.
    /// The owned VM shares call-depth and instruction budgets across both.
    fn call_owned_function(
        &mut self,
        handle: usize,
        args: &[Value],
    ) -> Result<Vec<Value>, FlowError> {
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
        result
    }

    fn run(
        &mut self,
        instructions: &[Op],
        slot_count: usize,
        initial: &[(usize, Value)],
        captures: &[(usize, usize)],
        varargs: &[Value],
    ) -> Result<Vec<Value>, FlowError> {
        self.frame_allocations.push(Vec::new());
        let result = self.run_inner(instructions, slot_count, initial, captures, varargs);
        let allocated = self
            .frame_allocations
            .pop()
            .ok_or(FlowError::InvalidBytecode)?;
        for cell in allocated {
            if !self.captured_cells.contains(&cell) {
                *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = None;
                self.free_cells.push(cell);
            }
        }
        result
    }

    fn run_inner(
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
                slots.push(self.allocate_cell()?);
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
                Op::ReadGlobal(key) => stack.push(self.read_global(*key)?),
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
                    let mut args = stack.split_off(stack.len() - arity);
                    let mut callable = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    // A callable table supplies itself as the first argument
                    // to its __call metamethod. Table-valued metamethods can
                    // chain, but never escape the shared VM budget or depth.
                    let mut redirects = 0;
                    while let Value::Table(handle) = callable {
                        if redirects >= MAX_CALL_DEPTH {
                            return Err(FlowError::CallDepthExceeded);
                        }
                        if self.steps >= self.budget {
                            return Err(FlowError::StepBudgetExceeded);
                        }
                        self.steps += 1;
                        let metamethod = self
                            .metatable_field(handle, b"__call")?
                            .ok_or(FlowError::TypeError)?;
                        if args.len() >= MAX_STACK {
                            return Err(FlowError::TooLarge);
                        }
                        args.insert(0, callable);
                        callable = metamethod;
                        redirects += 1;
                    }
                    if let Value::Builtin(builtin) = callable {
                        if args.is_empty()
                            && !matches!(
                                builtin,
                                Builtin::Error
                                    | Builtin::Print
                                    | Builtin::Pcall
                                    | Builtin::Xpcall
                                    | Builtin::Assert
                            )
                        {
                            return Err(FlowError::TypeError);
                        }
                        if builtin == Builtin::Error {
                            return Err(self.error_builtin(&args));
                        }
                        if matches!(
                            builtin,
                            Builtin::Print
                                | Builtin::Select
                                | Builtin::Assert
                                | Builtin::Next
                                | Builtin::Pairs
                                | Builtin::Ipairs
                                | Builtin::IpairsIterator
                                | Builtin::Pcall
                                | Builtin::Xpcall
                        ) {
                            let returned = match builtin {
                                Builtin::Print => self.print_builtin(&args)?,
                                Builtin::Select => self.select_builtin(&args)?,
                                Builtin::Pcall => self.pcall_builtin(&args)?,
                                Builtin::Xpcall => self.xpcall_builtin(&args)?,
                                Builtin::Assert
                                    if args.first().copied().unwrap_or(Value::Nil).truthy() =>
                                {
                                    args
                                }
                                Builtin::Assert => return Err(self.assertion_failure(&args)?),
                                Builtin::Next => self.next_builtin(&args)?,
                                Builtin::Pairs | Builtin::Ipairs => {
                                    self.pairs_ipairs_builtin(builtin, &args)?
                                }
                                Builtin::IpairsIterator => self.ipairs_iterator(&args)?,
                                _ => unreachable!("only multi-result builtins enter this path"),
                            };
                            if matches!(
                                instructions[pc],
                                Op::CallMany(_) | Op::CallSpread { many: true, .. }
                            ) {
                                last_multiple = Some(returned.len());
                                stack.extend(returned);
                            } else {
                                stack.push(returned.into_iter().next().unwrap_or(Value::Nil));
                            }
                            if stack.len() > MAX_STACK {
                                return Err(FlowError::TooLarge);
                            }
                            pc = next;
                            continue;
                        }
                        let value = args.first().copied().unwrap_or(Value::Nil);
                        let result = match builtin {
                            Builtin::Type => {
                                let name: &[u8] = match value {
                                    Value::Integer(_) | Value::Float(_) => b"number",
                                    Value::Boolean(_) => b"boolean",
                                    Value::Nil => b"nil",
                                    Value::String(_) => b"string",
                                    Value::Table(_) => b"table",
                                    Value::Function(_) | Value::Builtin(_) => b"function",
                                };
                                self.intern_string(name.to_vec())?
                            }
                            Builtin::ToNumber => self.tonumber_builtin(&args)?,
                            Builtin::RawEqual => {
                                let left = *args.first().ok_or(FlowError::TypeError)?;
                                let right = *args.get(1).ok_or(FlowError::TypeError)?;
                                let equal = if matches!(left, Value::Integer(_) | Value::Float(_))
                                    && matches!(right, Value::Integer(_) | Value::Float(_))
                                {
                                    numeric_equal(left, right)?
                                } else {
                                    left == right
                                };
                                Value::Boolean(equal)
                            }
                            Builtin::RawGet => self.rawget_builtin(&args)?,
                            Builtin::RawSet => self.rawset_builtin(&args)?,
                            Builtin::RawLen => self.raw_len(value)?,
                            Builtin::GetMetatable => self.getmetatable_builtin(&args)?,
                            Builtin::SetMetatable => self.setmetatable_builtin(&args)?,
                            Builtin::Error
                            | Builtin::Print
                            | Builtin::Pcall
                            | Builtin::Xpcall
                            | Builtin::Select
                            | Builtin::Assert
                            | Builtin::Next
                            | Builtin::Pairs
                            | Builtin::Ipairs
                            | Builtin::IpairsIterator => {
                                unreachable!(
                                    "iteration/selection builtins handled as multiple results"
                                )
                            }
                            Builtin::ToString => self.tostring_builtin(value)?,
                        };
                        if matches!(
                            instructions[pc],
                            Op::CallMany(_) | Op::CallSpread { many: true, .. }
                        ) {
                            last_multiple = Some(1);
                        }
                        stack.push(result);
                        if stack.len() > MAX_STACK {
                            return Err(FlowError::TooLarge);
                        }
                        pc = next;
                        continue;
                    }
                    let Value::Function(handle) = callable else {
                        return Err(FlowError::TypeError);
                    };
                    let returned = self.call_owned_function(handle, &args)?;
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
                Op::DeclareLocals(indices) => {
                    if stack.len() < indices.len() {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let values = stack.split_off(stack.len() - indices.len());
                    for (index, value) in indices.iter().zip(values) {
                        let cell = self.iteration_cell(&mut slots, *index)?;
                        *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = Some(value);
                    }
                }
                Op::RenewLocal(slot) => {
                    self.iteration_cell(&mut slots, *slot)?;
                }
                Op::PrepareLocal(slot) => {
                    let cell = *slots.get(*slot).ok_or(FlowError::InvalidBytecode)?;
                    if prepared.len() >= MAX_SLOTS {
                        return Err(FlowError::TooLarge);
                    }
                    prepared.push(PendingTarget::Local(cell));
                }
                Op::PrepareGlobal(key) => {
                    self.strings.get(*key).ok_or(FlowError::InvalidBytecode)?;
                    if prepared.len() >= MAX_SLOTS {
                        return Err(FlowError::TooLarge);
                    }
                    prepared.push(PendingTarget::Global(*key));
                }
                Op::PrepareIndexed => {
                    let key = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let table = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let Value::Table(handle) = table else {
                        return Err(FlowError::TypeError);
                    };
                    let key = self.table_key(key)?;
                    if handle >= self.tables.len() {
                        return Err(FlowError::InvalidBytecode);
                    }
                    if prepared.len() >= MAX_SLOTS {
                        return Err(FlowError::TooLarge);
                    }
                    prepared.push(PendingTarget::Indexed { table: handle, key });
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
                            PendingTarget::Global(key) => {
                                if let Some(handle) = self.global_table {
                                    self.assign_with_metatable(
                                        handle,
                                        TableKey::Hashed(Value::String(key)),
                                        value,
                                    )?;
                                } else {
                                    self.globals.insert(key, value);
                                }
                            }
                            PendingTarget::Indexed { table, key } => {
                                self.assign_with_metatable(table, key, value)?;
                            }
                        }
                    }
                }
                Op::Duplicate => {
                    let value = *stack.last().ok_or(FlowError::InvalidBytecode)?;
                    stack.push(value);
                }
                Op::Swap => {
                    if stack.len() < 2 {
                        return Err(FlowError::InvalidBytecode);
                    }
                    let last = stack.len() - 1;
                    stack.swap(last - 1, last);
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
                    let cells = &mut self
                        .tables
                        .get_mut(handle)
                        .ok_or(FlowError::InvalidBytecode)?
                        .array;
                    if end > cells.len() {
                        let growth = end - cells.len();
                        if self.table_cells.saturating_add(growth) > MAX_TABLE_CELLS_TOTAL {
                            return Err(FlowError::TooLarge);
                        }
                        self.table_cells += growth;
                        cells.resize(end, None);
                    }
                    for (offset, value) in results.into_iter().enumerate() {
                        cells[first + offset] = (value != Value::Nil).then_some(value);
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
                    self.tables.push(TableStorage {
                        array: values
                            .into_iter()
                            .map(|value| (value != Value::Nil).then_some(value))
                            .collect(),
                        hashed: HashMap::new(),
                        metatable: None,
                    });
                    self.table_cells += arity;
                    stack.push(Value::Table(handle));
                }
                Op::GetTable => {
                    let key = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let table = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let Value::Table(handle) = table else {
                        return Err(FlowError::TypeError);
                    };
                    let key = self.table_key(key)?;
                    stack.push(self.lookup_with_metatable(handle, key)?);
                }
                Op::SetTable => {
                    let value = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let key = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let table = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let Value::Table(handle) = table else {
                        return Err(FlowError::TypeError);
                    };
                    let key = self.table_key(key)?;
                    self.assign_with_metatable(handle, key, value)?;
                }
                Op::Negate => {
                    let value = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let result = match self.arithmetic_operand(value) {
                        Ok(Value::Integer(number)) => Value::Integer(number.wrapping_neg()),
                        Ok(Value::Float(bits)) => Value::Float((-f64::from_bits(bits)).to_bits()),
                        Err(FlowError::TypeError) => {
                            self.binary_metamethod(b"__unm", value, value)?
                        }
                        Ok(_) => return Err(FlowError::InvalidBytecode),
                        Err(error) => return Err(error),
                    };
                    stack.push(result);
                }
                Op::Not => {
                    let value = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    stack.push(Value::Boolean(!value.truthy()));
                }
                Op::Length => {
                    let value = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    stack.push(self.length_with_metatable(value)?);
                }
                Op::BitNot => {
                    let value = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let result = match self.exact_integer_operand(value) {
                        Ok(number) => Value::Integer(!number),
                        Err(FlowError::TypeError) => {
                            self.binary_metamethod(b"__bnot", value, value)?
                        }
                        Err(error) => return Err(error),
                    };
                    stack.push(result);
                }
                Op::Binary(op) => {
                    let rhs = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let lhs = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let result = match *op {
                        Binary::Add
                        | Binary::Sub
                        | Binary::Mul
                        | Binary::Div
                        | Binary::Pow
                        | Binary::FloorDiv
                        | Binary::Mod => self.arithmetic_with_metamethod(*op, lhs, rhs)?,
                        Binary::BitAnd
                        | Binary::BitOr
                        | Binary::BitXor
                        | Binary::Shl
                        | Binary::Shr => self.bitwise_with_metamethod(*op, lhs, rhs)?,
                        Binary::Concat => self.concat_with_metamethod(lhs, rhs)?,
                        Binary::Less
                        | Binary::LessEqual
                        | Binary::Greater
                        | Binary::GreaterEqual
                            if matches!((lhs, rhs), (Value::String(_), Value::String(_))) =>
                        {
                            let (Value::String(a), Value::String(b)) = (lhs, rhs) else {
                                unreachable!("string operands guarded")
                            };
                            let a = self.strings.get(a).ok_or(FlowError::InvalidBytecode)?;
                            let b = self.strings.get(b).ok_or(FlowError::InvalidBytecode)?;
                            let matches = match *op {
                                Binary::Less => a < b,
                                Binary::LessEqual => a <= b,
                                Binary::Greater => a > b,
                                Binary::GreaterEqual => a >= b,
                                _ => unreachable!("guarded comparison operator"),
                            };
                            Value::Boolean(matches)
                        }
                        Binary::Equal | Binary::NotEqual
                            if matches!(lhs, Value::Integer(_) | Value::Float(_))
                                && matches!(rhs, Value::Integer(_) | Value::Float(_)) =>
                        {
                            let equal = numeric_equal(lhs, rhs)?;
                            Value::Boolean(if *op == Binary::Equal { equal } else { !equal })
                        }
                        Binary::Less
                        | Binary::LessEqual
                        | Binary::Greater
                        | Binary::GreaterEqual
                            if matches!(lhs, Value::Integer(_) | Value::Float(_))
                                && matches!(rhs, Value::Integer(_) | Value::Float(_)) =>
                        {
                            use std::cmp::Ordering;
                            let ordering = numeric_order(lhs, rhs);
                            Value::Boolean(match *op {
                                Binary::Less => ordering == Some(Ordering::Less),
                                Binary::LessEqual => {
                                    matches!(ordering, Some(Ordering::Less | Ordering::Equal))
                                }
                                Binary::Greater => ordering == Some(Ordering::Greater),
                                Binary::GreaterEqual => {
                                    matches!(ordering, Some(Ordering::Greater | Ordering::Equal))
                                }
                                _ => unreachable!("guarded ordered numeric operator"),
                            })
                        }
                        Binary::Equal | Binary::NotEqual
                            if matches!((lhs, rhs), (Value::Table(_), Value::Table(_))) =>
                        {
                            self.equality_with_metamethod(*op, lhs, rhs)?
                        }
                        Binary::Less
                        | Binary::LessEqual
                        | Binary::Greater
                        | Binary::GreaterEqual => self.ordering_with_metamethod(*op, lhs, rhs)?,
                        _ => evaluate_binary(*op, lhs, rhs)?,
                    };
                    stack.push(result);
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
                    let read = |slot: usize| -> Result<Value, FlowError> {
                        let cell = *slots.get(slot).ok_or(FlowError::InvalidBytecode)?;
                        self.cells
                            .get(cell)
                            .copied()
                            .flatten()
                            .ok_or(FlowError::InvalidBytecode)
                    };
                    let current = self.arithmetic_operand(read(*counter)?)?;
                    let bound = self.arithmetic_operand(read(*limit)?)?;
                    let stride = self.arithmetic_operand(read(*step)?)?;
                    let floating = [current, bound, stride]
                        .iter()
                        .any(|value| matches!(value, Value::Float(_)));
                    let (enters, visible) = if floating {
                        let current = number_to_f64(current)?;
                        let bound = number_to_f64(bound)?;
                        let stride = number_to_f64(stride)?;
                        if stride == 0.0 {
                            return Err(FlowError::InvalidForStep);
                        }
                        (
                            if stride > 0.0 {
                                current <= bound
                            } else {
                                current >= bound
                            },
                            Value::Float(current.to_bits()),
                        )
                    } else {
                        let (
                            Value::Integer(current),
                            Value::Integer(bound),
                            Value::Integer(stride),
                        ) = (current, bound, stride)
                        else {
                            return Err(FlowError::TypeError);
                        };
                        if stride == 0 {
                            return Err(FlowError::InvalidForStep);
                        }
                        (
                            if stride > 0 {
                                current <= bound
                            } else {
                                current >= bound
                            },
                            Value::Integer(current),
                        )
                    };
                    if !enters {
                        pc = *exit;
                        continue;
                    }
                    if floating {
                        let counter_cell =
                            *slots.get(*counter).ok_or(FlowError::InvalidBytecode)?;
                        *self
                            .cells
                            .get_mut(counter_cell)
                            .ok_or(FlowError::InvalidBytecode)? = Some(visible);
                    }
                    let cell = self.iteration_cell(&mut slots, *variable)?;
                    *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = Some(visible);
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
                    let step_cell = *slots.get(*step).ok_or(FlowError::InvalidBytecode)?;
                    let current = self
                        .cells
                        .get(counter_cell)
                        .copied()
                        .flatten()
                        .ok_or(FlowError::InvalidBytecode)?;
                    let stride = self
                        .cells
                        .get(step_cell)
                        .copied()
                        .flatten()
                        .ok_or(FlowError::InvalidBytecode)?;
                    let current = self.arithmetic_operand(current)?;
                    let stride = self.arithmetic_operand(stride)?;
                    let updated = match (current, stride) {
                        (Value::Integer(current), Value::Integer(stride)) => {
                            let Some(updated) = current.checked_add(stride) else {
                                pc = *exit;
                                continue;
                            };
                            Value::Integer(updated)
                        }
                        (current, stride) => Value::Float(
                            (number_to_f64(current)? + number_to_f64(stride)?).to_bits(),
                        ),
                    };
                    *self
                        .cells
                        .get_mut(counter_cell)
                        .ok_or(FlowError::InvalidBytecode)? = Some(updated);
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

/// Result of an owned execution, retaining output even when a script errors.
/// Output is a bounded byte stream; no host callbacks or filesystem I/O occur.
#[derive(Debug)]
pub struct FlowOutcome {
    pub result: Result<Vec<Value>, FlowError>,
    pub stdout: Vec<u8>,
}

/// Run bounded first-party Lua and capture the restricted print builtin.
/// Errors retain output emitted before failure. No production VM fallback.
pub fn execute_flow_capture_with_args<'a>(
    program: &'a FlowProgram,
    budget: usize,
    script_name: &'a str,
    script_args: &'a [String],
) -> FlowOutcome {
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
        free_cells: Vec::new(),
        frame_allocations: Vec::new(),
        strings: program.strings.clone(),
        stdout: Vec::new(),
        script_name,
        script_args,
        arg_handle: None,
        globals: HashMap::new(),
        global_table: None,
    };
    let result = execution.run(&program.instructions, program.slots, &[], &[], &[]);
    FlowOutcome {
        result,
        stdout: execution.stdout,
    }
}

/// The no-arguments execution API remains suitable for pure Rust tests.
pub fn execute_flow_capture(program: &FlowProgram, budget: usize) -> FlowOutcome {
    execute_flow_capture_with_args(program, budget, "=owned", &[])
}

/// Keep the pure semantic API independent of CLI output capture.
pub fn execute_flow_values(program: &FlowProgram, budget: usize) -> Result<Vec<Value>, FlowError> {
    execute_flow_capture(program, budget).result
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
    fn uncaptured_call_frames_reuse_cells_across_many_invocations() {
        let program = compile_flow_chunk(
            "local function add(x) return x+1 end; local n=0; while n<70000 do n=add(n) end; return n",
        )
        .unwrap();
        assert_eq!(execute_flow_chunk(&program, 1_500_000), Ok(70000));
        assert_eq!(
            evaluate(
                "local function make(x) return function() return x end end; local a=make(2); local b=make(3); return a()*10+b()"
            ),
            23
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
    fn repeated_block_local_declarations_keep_distinct_captured_cells() {
        assert_eq!(
            evaluate(
                "local t={}; for i=1,3 do local x=i; t[i]=function() return x end end; return t[1]()*100+t[2]()*10+t[3]()"
            ),
            123
        );
        assert_eq!(
            evaluate(
                "local t={}; local i=0; while i<3 do i=i+1; local x=i; t[i]=function() return x end end; return t[1]()*100+t[2]()*10+t[3]()"
            ),
            123
        );
        assert_eq!(
            evaluate(
                "local t={}; local i=0; repeat i=i+1; local x=i; t[i]=function() return x end until i==3; return t[1]()*100+t[2]()*10+t[3]()"
            ),
            123
        );
        assert_eq!(
            evaluate(
                "local t={}; for i=1,3 do local function f(n) if n==0 then return i end; return f(n-1) end; t[i]=f end; return t[1](1)*100+t[2](1)*10+t[3](1)"
            ),
            123
        );
        assert_eq!(evaluate("local x=2; do local x=x+1; return x end"), 3);
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
    fn numeric_for_handles_fractional_bounds_and_steps() {
        assert_eq!(
            evaluate("local n=0; for i=1.0,2.0,0.5 do n=n+1 end; return n"),
            3
        );
        assert_eq!(
            evaluate("local n=0; for i=3.0,1.0,-0.5 do n=n+1 end; return n"),
            5
        );
        assert_eq!(
            evaluate("local s=0; for i=1,2,0.5 do s=s+i end; return (s==4.5) and 1 or 0"),
            1
        );
        assert_eq!(evaluate("local n=0; for i=1.0,4 do n=n+1 end; return n"), 4);
        assert_eq!(
            evaluate("local s=0; for i=1.5,0,-0.5 do s=s+i end; return (s==3.0) and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("local n=0; for i='1.0','2.0','0.5' do n=n+1 end; return n"),
            3
        );
        let invalid = compile_flow_chunk("for i=1.0,2.0,0.0 do end; return 1").unwrap();
        assert_eq!(
            execute_flow_chunk(&invalid, DEFAULT_STEP_BUDGET),
            Err(FlowError::InvalidForStep)
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
        assert_eq!(evaluate("local t={1}; t[0],t[1]=8,9; return t[1]"), 9);
    }

    #[test]
    fn integer_formatted_lua_strings_coerce_for_arithmetic() {
        assert_eq!(evaluate("return '7'+5"), 12);
        assert_eq!(evaluate("return '10'-'3'"), 7);
        assert_eq!(evaluate("return '6'*'7'"), 42);
        assert_eq!(evaluate("return '7'//'2'"), 3);
        assert_eq!(evaluate("return '0x10'+1"), 17);
        assert_eq!(evaluate("return -'5'"), -5);
        let invalid_bitwise = compile_flow_chunk("return '7'&3").unwrap();
        assert_eq!(
            execute_flow_chunk(&invalid_bitwise, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
        assert_eq!(evaluate("return ('7'..'0')+2"), 72);
        let invalid = compile_flow_chunk("return 'hello'+1").unwrap();
        assert_eq!(
            execute_flow_chunk(&invalid, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
    }

    #[test]
    fn first_class_tonumber_builtin_converts_bounded_numerals() {
        assert_eq!(evaluate("return tonumber('7')+2"), 9);
        assert_eq!(evaluate("return (tonumber('2.5')==2.5) and 1 or 0"), 1);
        assert_eq!(evaluate("return tonumber('ff',16)"), 255);
        assert_eq!(evaluate("return tonumber('101',2)"), 5);
        assert_eq!(evaluate("return tonumber('  -10  ',2)"), -2);
        assert_eq!(evaluate("return tonumber('0x10')"), 16);
        assert_eq!(evaluate("return (tonumber('0x1.8p1')==3.0) and 1 or 0"), 1);
        assert_eq!(evaluate("return (tonumber('bad')==nil) and 1 or 0"), 1);
        assert_eq!(evaluate("return (tonumber(true)==nil) and 1 or 0"), 1);
        let no_argument = compile_flow_chunk("return tonumber()").unwrap();
        assert_eq!(
            execute_flow_value(&no_argument, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
        assert_eq!(evaluate("return (tonumber('g',16)==nil) and 1 or 0"), 1);
        assert_eq!(
            evaluate("local tonumber=function() return 8 end; return tonumber('no')"),
            8
        );
        let invalid = compile_flow_chunk("return tonumber(3,2)").unwrap();
        assert_eq!(
            execute_flow_chunk(&invalid, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
    }

    #[test]
    fn protected_calls_forward_callable_tables_and_enforce_redirect_limits() {
        for (source, expected) in [
            (
                "local t=setmetatable({base=4},{__call=function(self,x) return self.base+x end}); local ok,v=pcall(t,5); return ok and v or 0",
                9,
            ),
            (
                "local t=setmetatable({},{__call=function(_,x) return x,x+1 end}); local ok,a,b=pcall(t,7); return ok and a*10+b or 0",
                78,
            ),
            (
                "local inner=setmetatable({},{__call=function(_,self,x) return self.base+x end}); local t=setmetatable({base=4},{__call=inner}); local ok,v=pcall(t,5); return ok and v or 0",
                9,
            ),
            (
                "local t=setmetatable({},{__call=function() error(7) end}); local ok,e=pcall(t); return not ok and e==7 and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({},{__call=function(_,x) error(x) end}); local ok,e=xpcall(t,function(e) return e*2 end,5); return not ok and e==10 and 1 or 0",
                1,
            ),
            (
                "local t={}; local ok,e=pcall(t); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        let cyclic =
            compile_flow_chunk("local t={}; setmetatable(t,{__call=t}); return pcall(t)").unwrap();
        assert_eq!(
            execute_flow_values(&cyclic, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn nested_protected_call_builtins_preserve_values_and_bound_depth() {
        for source in [
            "local ok,inner,v=pcall(pcall,function() return 7 end); return ok and inner and v==7 and 1 or 0",
            "local ok,inner,e=pcall(pcall,error,9); return ok and not inner and e==9 and 1 or 0",
            "local ok,inner,v=pcall(xpcall,error,function(e) return e*2 end,4); return ok and not inner and v==8 and 1 or 0",
            "local ok,inner,v=pcall(xpcall,function() return 6 end,function(e) return 0 end); return ok and inner and v==6 and 1 or 0",
            "local ok,inner=xpcall(error,pcall,9); return not ok and inner==false and 1 or 0",
            "local ok,inner,v=pcall(pcall,tonumber,'8'); return ok and inner and v==8 and 1 or 0",
            "local ok,inner,v=pcall(pcall,rawlen,{1,2}); return ok and inner and v==2 and 1 or 0",
            "local ok,inner,v=pcall(pcall,assert,false,'bad'); return ok and not inner and v=='bad' and 1 or 0",
            "local ok,inner,v=pcall(xpcall,error,function(e) return false end,4); return ok and inner==false and v==false and 1 or 0",
            "return select('#',pcall(pcall,function() end))==2 and 1 or 0",
            "local ok,inner=pcall(xpcall,error,nil,5); return not ok and type(inner)=='string' and 1 or 0",
            "local n=0; local ok,inner,v=pcall(pcall,function() n=n+1; return n end); return ok and inner and v==1 and n==1 and 1 or 0",
            "local ok,inner,again,v=pcall(pcall,pcall,error,7); return ok and inner and again==false and v==7 and 1 or 0",
            "local inner=function() return 8,9 end; local ok,a,b,c=pcall(pcall,inner); return ok and a and b==8 and c==9 and 1 or 0",
            "local ok,a,b=pcall(xpcall,error,function(e) return e+1 end,9); return ok and a==false and b==10 and 1 or 0",
        ] {
            assert_eq!(evaluate(source), 1, "{source}");
        }
        let nested = vec!["pcall"; MAX_CALL_DEPTH + 4].join(",");
        let source = format!("return pcall({nested})");
        let program = compile_flow_chunk(&source).unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn callable_binary_metamethods_preserve_lua_dispatch_and_depth_limits() {
        for source in [
            "local h=setmetatable({n=9},{__call=function(self,a,b) return self.n end}); local t=setmetatable({},{__add=h}); return t+3==9 and 1 or 0",
            "local inner=setmetatable({},{__call=function(_,h,a,b) return h.n end}); local h=setmetatable({n=7},{__call=inner}); local t=setmetatable({},{__sub=h}); return t-2==7 and 1 or 0",
            "local t=setmetatable({},{__mul=type}); return t*2=='table' and 1 or 0",
            "local h=setmetatable({},{__call=function() return 5 end}); local t=setmetatable({},{__div=h}); return t/2==5 and 1 or 0",
            "local h=setmetatable({},{__call=function() return 6,9 end}); local t=setmetatable({},{__mod=h}); return t%2==6 and 1 or 0",
            "local h=setmetatable({},{__call=function() return 5 end}); local t=setmetatable({},{__band=h}); return (t&2)==5 and 1 or 0",
            "local h=setmetatable({},{__call=function() return 7 end}); local t=setmetatable({},{__bor=h}); return (t|2)==7 and 1 or 0",
            "local h=setmetatable({},{__call=function() return 'ok' end}); local t=setmetatable({},{__concat=h}); return t..'x'=='ok' and 1 or 0",
            "local h=setmetatable({},{__call=function(_,a,b) return true end}); local mt={__eq=h}; local a=setmetatable({},mt); local b=setmetatable({},mt); return a==b and 1 or 0",
            "local h=setmetatable({},{__call=function(_,a,b) return a.n<b.n end}); local mt={__lt=h}; local a=setmetatable({n=2},mt); local b=setmetatable({n=4},mt); return a<b and 1 or 0",
            "local h=setmetatable({},{__call=function(_,a,b) return a.n<=b.n end}); local mt={__le=h}; local a=setmetatable({n=2},mt); local b=setmetatable({n=4},mt); return a<=b and 1 or 0",
            "local h=setmetatable({},{__call=function(_,a,b) return a.n<b.n end}); local mt={__lt=h}; local a=setmetatable({n=2},mt); local b=setmetatable({n=4},mt); return a<=b and 1 or 0",
            "local h=setmetatable({},{__call=function() error(17) end}); local t=setmetatable({},{__add=h}); local ok,e=pcall(function() return t+2 end); return not ok and e==17 and 1 or 0",
            "local t=setmetatable({},{__add=9}); local ok,e=pcall(function() return t+2 end); return not ok and type(e)=='string' and 1 or 0",
            "local n=0; local h=setmetatable({},{__call=function() n=n+1; return false end}); local mt={__eq=h}; local t=setmetatable({},mt); return t==t and n==0 and 1 or 0",
            "local h=setmetatable({},{__call=function() return false end}); local mt={__lt=h}; local a=setmetatable({},mt); local b=setmetatable({},mt); return (a<b)==false and 1 or 0",
        ] {
            assert_eq!(evaluate(source), 1, "{source}");
        }
        for source in [
            "local h={}; setmetatable(h,{__call=h}); local t=setmetatable({}, {__add=h}); return pcall(function() return t+1 end)",
            "local t={}; setmetatable(t,{__add=function(x,y) return x+y end}); return pcall(function() return t+1 end)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::CallDepthExceeded),
                "{source}"
            );
        }
    }

    #[test]
    fn callable_index_and_newindex_hooks_preserve_raw_redirection_and_hard_depth() {
        for source in [
            "local t=setmetatable({}, {__index=type}); return t.missing=='table' and 1 or 0",
            "local t=setmetatable({x=4},{__index=type}); return t.x==4 and t.y=='table' and 1 or 0",
            "local h=setmetatable({missing=8},{__call=function() error(99) end}); local t=setmetatable({}, {__index=h}); return t.missing==8 and 1 or 0",
            "local inner=setmetatable({},{__call=function() error(99) end}); local h=setmetatable({unknown=9},{__call=inner}); local t=setmetatable({},{__index=h}); return t.unknown==9 and 1 or 0",
            "local n=0; local h=setmetatable({value=7},{__call=function() n=n+1; return 99 end}); local t=setmetatable({}, {__index=h}); return t.value==7 and n==0 and 1 or 0",
            "local h=setmetatable({x=7},{__call=function() error(9) end}); local t=setmetatable({}, {__index=h}); local ok,v=pcall(function() return t.x end); return ok and v==7 and 1 or 0",
            "local t=setmetatable({},{__index=error}); local ok,e=pcall(function() return t.x end); return not ok and type(e)=='string' and 1 or 0",
            "local t=setmetatable({},{__index=rawget}); return t.no==nil and 1 or 0",
            "local t=setmetatable({},{__newindex=rawset}); t.x=5; return rawget(t,'x')==5 and 1 or 0",
            "local t=setmetatable({x=4},{__newindex=error}); t.x=7; return rawget(t,'x')==7 and 1 or 0",
            "local h=setmetatable({},{__call=function() error(9) end}); local t=setmetatable({},{__newindex=h}); t.x=5; return rawget(t,'x')==nil and rawget(h,'x')==5 and 1 or 0",
            "local h=setmetatable({},{__call=function() error(17) end}); local t=setmetatable({},{__newindex=h}); local ok=pcall(function() t.x=4 end); return ok and rawget(h,'x')==4 and 1 or 0",
            "local t=setmetatable({},{__newindex=error}); local ok,e=pcall(function() t.y=4 end); return not ok and type(e)=='string' and 1 or 0",
            "local n=0; local h=setmetatable({},{__call=function() n=n+1 end}); local t=setmetatable({},{__newindex=h}); rawset(t,'x',2); return t.x==2 and n==0 and 1 or 0",
            "local n=0; local h=setmetatable({},{__call=function() n=n+1 end}); local t=setmetatable({},{__newindex=h}); t.a=6; return rawget(t,'a')==nil and rawget(h,'a')==6 and n==0 and 1 or 0",
            "local s=setmetatable({},{__call=function() error(7) end}); local t=setmetatable({},{__newindex=s}); t.x=4; return rawget(s,'x')==4 and 1 or 0",
            "local h=setmetatable({},{__call=function() error(9) end}); local t=setmetatable({}, {__newindex=h}); t[1]=8; return rawget(h,1)==8 and rawget(t,1)==nil and 1 or 0",
            "local h=setmetatable({},{__call=function() error(9) end}); local t=setmetatable({}, {__newindex=h}); t.x,t.y=4,5; return rawget(h,'x')+rawget(h,'y')==9 and 1 or 0",
        ] {
            assert_eq!(evaluate(source), 1, "{source}");
        }
        for source in [
            "local t={}; setmetatable(t,{__index=function(self,k) return self[k] end}); return pcall(function() return t.x end)",
            "local t={}; setmetatable(t,{__newindex=function(self,k,v) self[k]=v end}); return pcall(function() t.x=2 end)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::CallDepthExceeded),
                "{source}"
            );
        }
    }

    #[test]
    fn callable_pairs_metamethods_preserve_triples_and_hard_limits() {
        for source in [
            "local handler=setmetatable({},{__call=function(_,t) return next,t,nil end}); local t=setmetatable({x=7},{__pairs=handler}); local ok,f,s,k=pcall(pairs,t); local a,v=f(s,k); return ok and a=='x' and v==7 and 1 or 0",
            "local handler=setmetatable({},{__call=function(_,t) return next,t,nil end}); local t=setmetatable({x=7},{__pairs=handler}); local s=0; for _,v in pairs(t) do s=s+v end; return s==7 and 1 or 0",
            "local inner=setmetatable({},{__call=function(_,h,t) return next,t,nil end}); local outer=setmetatable({},{__call=inner}); local t=setmetatable({x=6},{__pairs=outer}); local ok,f,s,k=pcall(pairs,t); local a,v=f(s,k); return ok and a=='x' and v==6 and 1 or 0",
            "local t=setmetatable({9},{__pairs=ipairs}); local ok,f,s,k=pcall(pairs,t); local a,v=f(s,k); return ok and a==1 and v==9 and 1 or 0",
            "local t=setmetatable({5},{__pairs=next}); local ok,f,s,k=pcall(pairs,t); return ok and f==1 and s==5 and k==nil and 1 or 0",
            "local t=setmetatable({x=9},{__pairs=function(self) return next,self,nil,123 end}); local ok,f,s,k=pcall(pairs,t); return ok and type(f)=='function' and s==t and k==nil and 1 or 0",
            "local t=setmetatable({}, {__pairs=setmetatable({},{__call=function() error(7) end})}); local ok,e=pcall(pairs,t); return not ok and e==7 and 1 or 0",
            "local t=setmetatable({4},{__pairs=setmetatable({},{__call=function() error(7) end})}); local ok,f,s,k=pcall(ipairs,t); local i,v=f(s,k); return ok and i==1 and v==4 and 1 or 0",
            "local t=setmetatable({}, {__pairs=13}); local ok,e=pcall(pairs,t); return not ok and type(e)=='string' and 1 or 0",
            "local n=0; local h=setmetatable({},{__call=function(_,t) n=n+1; return next,t,nil end}); local t=setmetatable({v=3},{__pairs=h}); local ok,f,s,k=pcall(pairs,t); return ok and n==1 and type(f)=='function' and 1 or 0",
            "local t=setmetatable({}, {__pairs=setmetatable({},{__call=function() return nil end})}); local ok,f,s,k=pcall(pairs,t); return ok and f==nil and s==nil and k==nil and 1 or 0",
            "local h=setmetatable({},{__call=function(_,t) return next,t,nil end}); local t=setmetatable({}, {__pairs=h}); return select('#',pcall(pairs,t))==4 and 1 or 0",
        ] {
            assert_eq!(evaluate(source), 1, "{source}");
        }
        for source in [
            "local t={}; setmetatable(t,{__pairs=pairs}); return pcall(pairs,t)",
            "local method={}; setmetatable(method,{__call=method}); local t=setmetatable({}, {__pairs=method}); return pcall(pairs,t)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::CallDepthExceeded),
                "{source}"
            );
        }
    }

    #[test]
    fn protected_pairs_and_ipairs_preserve_iterator_triples() {
        for source in [
            "local t={a=3}; local ok,f,s,k=pcall(pairs,t); local key,val=f(s,k); return ok and key=='a' and val==3 and 1 or 0",
            "return select('#',pcall(pairs,{}))==4 and 1 or 0",
            "local ok,f,s,k=pcall(ipairs,{6,7}); local i,v=f(s,k); return ok and i==1 and v==6 and 1 or 0",
            "return select('#',pcall(ipairs,{}))==4 and 1 or 0",
            "local mt={__pairs=function(t) return function(_,k) if k==nil then return 'x',9 end end,t,nil end}; local t=setmetatable({},mt); local ok,f,s,k=pcall(pairs,t); local i,v=f(s,k); return ok and i=='x' and v==9 and 1 or 0",
            "local mt={__pairs=function() return nil end}; local ok,f,s,k=pcall(pairs,setmetatable({},mt)); return ok and f==nil and s==nil and k==nil and 1 or 0",
            "local t=setmetatable({5},{__pairs=function() error(5) end}); local ok,e=pcall(pairs,t); return not ok and e==5 and 1 or 0",
            "local t=setmetatable({5},{__pairs=function() error(5) end}); local ok,f,s,k=pcall(ipairs,t); local i,v=f(s,k); return ok and i==1 and v==5 and 1 or 0",
            "local ok,f,s,k=pcall(pairs,4); return ok and type(f)=='function' and s==4 and k==nil and 1 or 0",
            "local ok,f,s,k=pcall(ipairs,nil); return ok and type(f)=='function' and s==nil and k==0 and 1 or 0",
            "local t={}; local ok,e=xpcall(error,pairs,t); return not ok and type(e)=='function' and 1 or 0",
            "local t={}; local ok,e=xpcall(error,ipairs,t); return not ok and type(e)=='function' and 1 or 0",
            "local f,s,k=pcall(pairs,{x=2}); return f and type(s)=='function' and type(k)=='table' and 1 or 0",
            "local n=0; local t=setmetatable({},{__pairs=function(self) n=n+1; return next,self,nil end}); local ok,f,s,k=pcall(pairs,t); return ok and n==1 and type(f)=='function' and 1 or 0",
            "local ok,f,s,k=pcall(pairs,4); local works,e=pcall(f,s,k); return ok and not works and type(e)=='string' and 1 or 0",
            "local ok,f,s,k=pcall(ipairs,nil); local works,e=pcall(f,s,k); return ok and not works and type(e)=='string' and 1 or 0",
            "local ok,e=pcall(pairs); return not ok and type(e)=='string' and 1 or 0",
            "local ok,e=pcall(ipairs); return not ok and type(e)=='string' and 1 or 0",
            "local t=setmetatable({}, {__pairs=7}); local ok,e=pcall(pairs,t); return not ok and type(e)=='string' and 1 or 0",
            "local t=setmetatable({}, {__pairs=function() return function() end, {a=1}, nil,9 end}); return select('#',pcall(pairs,t))==4 and 1 or 0",
        ] {
            assert_eq!(evaluate(source), 1, "{source}");
        }
    }

    #[test]
    fn protected_multivalue_builtins_preserve_lua_result_adjustment() {
        for source in [
            "local ok,n=pcall(select,'#',3,4,5); return ok and n==3 and 1 or 0",
            "local ok,a,b=pcall(select,2,1,3,4); return ok and a==3 and b==4 and 1 or 0",
            "local ok,n=pcall(select,-1,3,4,5); return ok and n==5 and 1 or 0",
            "return select('#',pcall(select,3,7,8))==1 and 1 or 0",
            "local ok,k,v=pcall(next,{a=7}); return ok and k=='a' and v==7 and 1 or 0",
            "local ok,k=pcall(next,{}); return ok and k==nil and 1 or 0",
            "local ok,k,v=pcall(next,{[1]=3,[2]=4},1); return ok and k==2 and v==4 and 1 or 0",
            "local ok,e=pcall(next,{x=3},'missing'); return not ok and type(e)=='string' and 1 or 0",
            "local ok,e=pcall(select,0,1); return not ok and type(e)=='string' and 1 or 0",
            "local iter,t,i=ipairs({6,7}); local ok,k,v=pcall(iter,t,i); return ok and k==1 and v==6 and 1 or 0",
            "local iter,t,i=ipairs({}); local ok,k=pcall(iter,t,i); return ok and k==nil and 1 or 0",
            "local t=setmetatable({}, {__index=function(_,i) if i==1 then return 8 end end}); local iter,s,i=ipairs(t); local ok,k,v=pcall(iter,s,i); return ok and k==1 and v==8 and 1 or 0",
            "local ok,k=pcall(next,1); return not ok and type(k)=='string' and 1 or 0",
            "return select('#',pcall(next,{}))==2 and 1 or 0",
        ] {
            assert_eq!(evaluate(source), 1, "{source}");
        }
    }

    #[test]
    fn protected_raw_table_and_metatable_builtins_match_lua() {
        for source in [
            "local t=setmetatable({x=3},{__index=function() return 9 end}); local ok,v=pcall(rawget,t,'missing'); return ok and v==nil and 1 or 0",
            "local t={}; local ok,v=pcall(rawset,t,'x',7); return ok and v==t and t.x==7 and 1 or 0",
            "local t=setmetatable({},{__newindex=function() error('metahook') end}); local ok,v=pcall(rawset,t,'x',11); return ok and rawget(t,'x')==11 and 1 or 0",
            "local t={x=8}; local ok,v=pcall(rawget,t,'x'); return ok and v==8 and 1 or 0",
            "local ok,e=pcall(rawget,{}); return not ok and type(e)=='string' and 1 or 0",
            "local ok,e=pcall(rawset,{},nil,5); return not ok and type(e)=='string' and 1 or 0",
            "local t={}; local ok,v=pcall(getmetatable,t); return ok and v==nil and 1 or 0",
            "local mt={__metatable='sealed'}; local t=setmetatable({},mt); local ok,v=pcall(getmetatable,t); return ok and v=='sealed' and 1 or 0",
            "local mt={}; local t={}; local ok,v=pcall(setmetatable,t,mt); return ok and v==t and getmetatable(t)==mt and 1 or 0",
            "local t=setmetatable({}, {__metatable='sealed'}); local ok,e=pcall(setmetatable,t,nil); return not ok and type(e)=='string' and 1 or 0",
            "local t=setmetatable({x=9},{__metatable='sealed'}); local ok,v=pcall(rawget,t,'x'); return ok and v==9 and 1 or 0",
            "local mt={}; local t=setmetatable({},mt); local ok,v=pcall(setmetatable,t,nil); return ok and getmetatable(t)==nil and 1 or 0",
            "local t={}; local ok,e=xpcall(error,getmetatable,t); return not ok and e==nil and 1 or 0",
            "local t={}; local ok,e=pcall(rawset,t,1,5); return ok and e==t and t[1]==5 and 1 or 0",
        ] {
            assert_eq!(evaluate(source), 1, "{source}");
        }
    }

    #[test]
    fn callable_tostring_metamethods_and_builtin_callbacks_are_bounded() {
        for source in [
            "local h=setmetatable({name='hi'},{__call=function(self,t) return self.name end}); local t=setmetatable({},{__tostring=h}); local ok,v=pcall(tostring,t); return ok and v=='hi' and 1 or 0",
            "local h=setmetatable({},{__call=function(_,t) return 7 end}); local t=setmetatable({},{__tostring=h}); return tostring(t)=='7' and 1 or 0",
            "local h=setmetatable({},{__call=function(_,t) return 2.5 end}); local t=setmetatable({},{__tostring=h}); local ok,v=pcall(tostring,t); return ok and v=='2.5' and 1 or 0",
            "local inner=setmetatable({},{__call=function(_,h,t) return h.name end}); local h=setmetatable({name='ok'},{__call=inner}); local t=setmetatable({},{__tostring=h}); return tostring(t)=='ok' and 1 or 0",
            "local t=setmetatable({},{__tostring=type}); local ok,v=pcall(tostring,t); return ok and v=='table' and 1 or 0",
            "local t=setmetatable({},{__tostring=function() return 'first', 'extra' end}); return tostring(t)=='first' and 1 or 0",
            "local h=setmetatable({},{__call=function() return false end}); local t=setmetatable({},{__tostring=h}); local ok,e=pcall(tostring,t); return not ok and type(e)=='string' and 1 or 0",
            "local h=setmetatable({},{__call=function() return nil end}); local t=setmetatable({},{__tostring=h}); local ok,e=pcall(tostring,t); return not ok and type(e)=='string' and 1 or 0",
            "local t=setmetatable({},{__tostring=error}); local ok,e=pcall(tostring,t); return not ok and rawequal(e,t) and 1 or 0",
            "local h=setmetatable({},{__call=function() error(15) end}); local t=setmetatable({},{__tostring=h}); local ok,e=pcall(tostring,t); return not ok and e==15 and 1 or 0",
            "local t=setmetatable({},{__tostring=type}); local ok,v=xpcall(error,tostring,t); return not ok and v=='table' and 1 or 0",
            "local t=setmetatable({},{__tostring=9}); local ok,e=pcall(tostring,t); return not ok and type(e)=='string' and 1 or 0",
        ] {
            assert_eq!(evaluate(source), 1, "{source}");
        }
        for source in [
            "local t={}; setmetatable(t,{__tostring=tostring}); return pcall(tostring,t)",
            "local method={}; setmetatable(method,{__call=method}); local t=setmetatable({}, {__tostring=method}); return pcall(tostring,t)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::CallDepthExceeded),
                "{source}"
            );
        }
    }

    #[test]
    fn lua_error_builtin_validates_optional_level_in_normal_and_protected_calls() {
        for source in [
            "local ok,e=pcall(error,7,'bad'); return not ok and type(e)=='string' and 1 or 0",
            "local ok,e=pcall(error,7,{}); return not ok and type(e)=='string' and 1 or 0",
            "local ok,e=pcall(error,7,1); return not ok and e==7 and 1 or 0",
            "local ok,e=pcall(error,7,nil); return not ok and e==7 and 1 or 0",
            "local ok,e=pcall(error,7,'0'); return not ok and e==7 and 1 or 0",
            "local ok,e=pcall(error); return not ok and e==nil and 1 or 0",
            "local t={}; local ok,e=pcall(error,t,{}); return not ok and type(e)=='string' and 1 or 0",
            "local ok,e=xpcall(function() error(12,{}) end,function(e) return type(e) end); return not ok and e=='string' and 1 or 0",
        ] {
            assert_eq!(evaluate(source), 1, "{source}");
        }
    }

    #[test]
    fn callable_len_metamethods_preserve_lua_results_and_depth_safety() {
        for source in [
            "local h=setmetatable({n=7},{__call=function(self,t) return self.n end}); local t=setmetatable({},{__len=h}); return #t==7 and 1 or 0",
            "local h=setmetatable({},{__call=function(_,a,b) return rawequal(a,b) and 8 or 0 end}); local t=setmetatable({},{__len=h}); return #t==8 and 1 or 0",
            "local inner=setmetatable({},{__call=function(_,h,t) return h.n end}); local h=setmetatable({n=9},{__call=inner}); local t=setmetatable({},{__len=h}); return #t==9 and 1 or 0",
            "local t=setmetatable({7,8},{__len=rawlen}); return #t==2 and 1 or 0",
            "local t=setmetatable({},{__len=type}); return #t=='table' and 1 or 0",
            "local h=setmetatable({},{__call=function() return 6,7 end}); local t=setmetatable({},{__len=h}); return #t==6 and 1 or 0",
            "local h=setmetatable({},{__call=function() error(14) end}); local t=setmetatable({},{__len=h}); local ok,e=pcall(function() return #t end); return not ok and e==14 and 1 or 0",
            "local t=setmetatable({},{__len=error}); local ok,e=pcall(function() return #t end); return not ok and type(e)=='string' and 1 or 0",
            "local t=setmetatable({},{__len=3}); local ok,e=pcall(function() return #t end); return not ok and type(e)=='string' and 1 or 0",
            "local t=setmetatable({},{__len=function() return 'ok' end}); return #t=='ok' and 1 or 0",
            "local t=setmetatable({},{__len=function() return nil end}); return #t==nil and 1 or 0",
            "local t=setmetatable({5,6},{__len=function() return 1 end}); return #t==1 and rawlen(t)==2 and 1 or 0",
        ] {
            assert_eq!(evaluate(source), 1, "{source}");
        }
        for source in [
            "local t={}; setmetatable(t,{__len=function(x) return #x end}); return pcall(function() return #t end)",
            "local h={}; setmetatable(h,{__call=h}); local t=setmetatable({}, {__len=h}); return pcall(function() return #t end)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::CallDepthExceeded),
                "{source}"
            );
        }
    }

    #[test]
    fn protected_tostring_reuses_lua_metatable_callback_semantics() {
        for (source, expected) in [
            (
                "local ok,v=pcall(tostring,17); return ok and v=='17' and 1 or 0",
                1,
            ),
            (
                "local ok,v=pcall(tostring,nil); return ok and v=='nil' and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({x=7},{__tostring=function(self) return 'x'..self.x end}); local ok,v=pcall(tostring,t); return ok and v=='x7' and 1 or 0",
                1,
            ),
            (
                "local ok,v=xpcall(error,tostring,42); return not ok and v=='42' and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {__tostring=function() return 'special' end}); local ok,v=xpcall(error,tostring,t); return not ok and v=='special' and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {__tostring=function() return 1 end}); local ok,e=pcall(tostring,t); return ok and e=='1' and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {__tostring=function() return 2.5 end}); local ok,e=pcall(tostring,t); return ok and e=='2.5' and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {__tostring=function() return false end}); local ok,e=pcall(tostring,t); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
            (
                "local ok,e=pcall(tostring); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
    }

    #[test]
    fn selected_standard_builtins_work_inside_protected_calls() {
        for (source, expected) in [
            (
                "local ok,v=pcall(type,12); return ok and v=='number' and 1 or 0",
                1,
            ),
            (
                "local ok,v=pcall(tonumber,'ff',16); return ok and v or 0",
                255,
            ),
            (
                "local ok,v=pcall(tonumber,'bad'); return ok and v==nil and 1 or 0",
                1,
            ),
            ("local ok,v=pcall(rawlen,{1,2,3}); return ok and v or 0", 3),
            (
                "local ok,v=pcall(rawequal,7,7.0); return ok and v and 1 or 0",
                1,
            ),
            (
                "local ok,v=xpcall(error,type,42); return not ok and v=='number' and 1 or 0",
                1,
            ),
            (
                "local ok,v=xpcall(error,rawlen,{4,5}); return not ok and v==2 and 1 or 0",
                1,
            ),
            (
                "local ok,v=xpcall(error,tonumber,7); return not ok and v==7 and 1 or 0",
                1,
            ),
            (
                "local ok,e=pcall(type); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
            (
                "local ok,e=pcall(rawequal,8); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
    }

    #[test]
    fn xpcall_requires_function_handlers_before_invoking_target() {
        for source in [
            "return xpcall(error, {}, 8)",
            "return xpcall(error, nil, 8)",
            "return xpcall(function() return 5 end, {})",
            "return xpcall(function() return 5 end)",
            "local h=setmetatable({},{__call=function(_,e) return e end}); return xpcall(error,h,8)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
        for (source, expected) in [
            (
                "local calls=0; local f=function() calls=calls+1; return 5 end; local ok,v=xpcall(f,function(e) return e end); return ok and v==5 and calls or 0",
                1,
            ),
            (
                "local ok,v=xpcall(error,assert,8); return not ok and v or 0",
                8,
            ),
            (
                "local ok,e=xpcall(error,error,8); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
            (
                "local ok,e=xpcall(7,function(e) return type(e) end); return not ok and e=='string' and 1 or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
    }

    #[test]
    fn xpcall_runs_bounded_message_handlers_and_preserves_success_results() {
        for (source, expected) in [
            (
                "local ok,a,b=xpcall(function() return 5,6 end,function(e) return e end); return ok and a*10+b or 0",
                56,
            ),
            (
                "local ok,v=xpcall(function(a,b) return a+b end,function(e) return e end,3,4); return ok and v or 0",
                7,
            ),
            (
                "local ok,e=xpcall(error,function(e) return e*2 end,7); return not ok and e or 0",
                14,
            ),
            (
                "local ok,e=xpcall(error,function(e) return e end,false); return not ok and e==false and 1 or 0",
                1,
            ),
            (
                "local ok,e=xpcall(error,function(e) return 9,10 end,42); return not ok and e==9 and 1 or 0",
                1,
            ),
            (
                "local ok,e=xpcall(function() return nil+1 end,function(e) return type(e) end); return not ok and e=='string' and 1 or 0",
                1,
            ),
            (
                "local n=0; local ok=xpcall(function() return 5 end,function() n=n+1 end); return ok and n or 9",
                0,
            ),
            (
                "local ok,e=xpcall(error,function() error(6) end,4); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
            (
                "local n=0; local ok,e=xpcall(error,function(x) n=n+1; if n==1 then error(3) end; return x+10 end,2); return not ok and n==2 and e==13 and 1 or 0",
                1,
            ),
            (
                "local n=0; local ok,e=xpcall(error,function(x) n=n+1; if n<3 then error(n) end; return x*10 end,2); return not ok and n==3 and e==20 and 1 or 0",
                1,
            ),
            (
                "local n=0; local ok,e=xpcall(error,function(x) n=n+1; if n==1 then error(false) end; return x==false and 41 or 0 end,1); return not ok and n==2 and e==41 and 1 or 0",
                1,
            ),
            ("return type(xpcall)=='function' and 1 or 0", 1),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        let program = compile_flow_chunk(
            "local f; f=function() return xpcall(f,function(e) return e end) end; return f()",
        )
        .unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn bounded_lua_error_and_protected_closure_calls_preserve_values() {
        for (source, expected) in [
            (
                "local ok,a,b=pcall(function() return 5,6 end); return ok and a*10+b or 0",
                56,
            ),
            ("local ok=pcall(function() end); return ok and 1 or 0", 1),
            (
                "local ok,e=pcall(function() error(42) end); return not ok and e==42 and 1 or 0",
                1,
            ),
            (
                "local ok,e=pcall(error,false); return not ok and e==false and 1 or 0",
                1,
            ),
            (
                "local ok,e=pcall(error); return not ok and e==nil and 1 or 0",
                1,
            ),
            (
                "local ok,e=pcall(function() error('bad') end); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
            (
                "local ok,e=pcall(function() return nil+1 end); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
            (
                "local ok,e=pcall(function() return 1//0 end); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
            (
                "local ok,v=pcall(function(a,b) return a+b end,3,4); return ok and v or 0",
                7,
            ),
            (
                "local call=pcall; local ok,v=call(function() return 8 end); return ok and v or 0",
                8,
            ),
            (
                "local x=0; pcall(function() x=9; error('stop') end); return x",
                9,
            ),
            (
                "local ok,e=pcall(nil); return not ok and type(e)=='string' and 1 or 0",
                1,
            ),
            ("return select('#',pcall(function() end))", 1),
            ("return select('#',pcall(function() error() end))", 2),
            (
                "local ok,a,b=pcall(function() return pcall(error,7) end); return ok and a==false and b==7 and 1 or 0",
                1,
            ),
            (
                "return type(pcall)=='function' and type(error)=='function' and 1 or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for (source, expected) in [
            ("return error(9)", Value::Integer(9)),
            ("return error()", Value::Nil),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::Thrown(expected)),
                "{source}"
            );
        }
        let program =
            compile_flow_chunk("local f; f=function() return pcall(f) end; return f()").unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn first_class_assert_preserves_successful_lua_multi_values() {
        assert_eq!(evaluate("return assert(7)"), 7);
        assert_eq!(evaluate("return assert(false or 9)"), 9);
        assert_eq!(evaluate("local a,b,c=assert(true,2,3); return b+c"), 5);
        assert_eq!(
            evaluate("local t={assert(true,4,5)}; return t[2]*10+t[3]"),
            45
        );
        assert_eq!(
            evaluate("local function f(x) return assert(x,2) end; local a,b=f(4); return a+b"),
            6
        );
        assert_eq!(evaluate("return (type(assert)=='function') and 1 or 0"), 1);
        assert_eq!(evaluate("local f=assert; return f(11)"), 11);
        assert_eq!(
            evaluate("local assert=function() return 8 end; return assert(false)"),
            8
        );
        let program = compile_flow_chunk("return assert(true,8,9)").unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Ok(vec![
                Value::Boolean(true),
                Value::Integer(8),
                Value::Integer(9)
            ])
        );
        for source in ["return assert(false)", "return assert(nil,'bad')"] {
            let program = compile_flow_chunk(source).unwrap();
            assert!(
                matches!(
                    execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                    Err(FlowError::Thrown(Value::String(_)))
                ),
                "{source}"
            );
        }
        let program = compile_flow_chunk("return assert()").unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
        let program = compile_flow_chunk("return assert(false,42)").unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::Thrown(Value::Integer(42)))
        );
    }

    #[test]
    fn assertion_failures_preserve_custom_error_values_in_pcall() {
        for (source, expected) in [
            (
                "local ok,msg=pcall(assert,false); return not ok and msg=='assertion failed!' and 1 or 0",
                1,
            ),
            (
                "local ok,msg=pcall(assert); return not ok and type(msg)=='string' and 1 or 0",
                1,
            ),
            (
                "local ok,msg=pcall(assert,false,'custom'); return not ok and msg=='custom' and 1 or 0",
                1,
            ),
            (
                "local ok,msg=pcall(assert,false,42); return not ok and msg==42 and 1 or 0",
                1,
            ),
            (
                "local ok,msg=pcall(assert,false,false); return not ok and msg==false and 1 or 0",
                1,
            ),
            (
                "local ok,msg=pcall(function() assert(nil,'inner') end); return not ok and type(msg)=='string' and 1 or 0",
                1,
            ),
            (
                "local ok,a,b=pcall(assert,true,5); return ok and a==true and b==5 and 1 or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        let program = compile_flow_chunk("return assert(false,nil)").unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::Thrown(Value::Nil))
        );
    }

    #[test]
    fn first_class_select_adjusts_bounded_lua_result_lists() {
        assert_eq!(evaluate("return select('#',4,5,6)"), 3);
        assert_eq!(evaluate("return select('#')"), 0);
        assert_eq!(evaluate("return select(-1,4,5,6)"), 6);
        assert_eq!(evaluate("local a,b=select(2,10,20,30); return a+b"), 50);
        assert_eq!(evaluate("local a,b=select(2,10,20,30); return a*10+b"), 230);
        assert_eq!(evaluate("return select(4,1,2,3) or 9"), 9);
        assert_eq!(evaluate("return (select('2',10,20,30))"), 20);
        assert_eq!(
            evaluate("local function count(...) return select('#',...) end; return count(3,4,5)"),
            3
        );
        assert_eq!(
            evaluate("local t={select(2,1,2,3)}; return t[1]*10+t[2]"),
            23
        );
        assert_eq!(evaluate("local f=select; return f('#','a','b')"), 2);
        assert_eq!(
            evaluate("local select=function() return 8 end; return select(1,2,3)"),
            8
        );
        let program = compile_flow_chunk("return select(2,1,2,3)").unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Ok(vec![Value::Integer(2), Value::Integer(3)])
        );
        let program = compile_flow_chunk("return 8,select(2,1,2,3)").unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Ok(vec![
                Value::Integer(8),
                Value::Integer(2),
                Value::Integer(3)
            ])
        );
        for source in [
            "return select()",
            "return select(0,1,2)",
            "return select(-4,1,2,3)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError)
            );
        }
    }

    #[test]
    fn rawlen_reports_bounded_string_bytes_and_table_borders() {
        assert_eq!(evaluate("return rawlen('abc')"), 3);
        assert_eq!(evaluate("return rawlen('é')"), 2);
        assert_eq!(evaluate("return rawlen({1,2,3})"), 3);
        assert_eq!(evaluate("local t={a=4}; return rawlen(t)"), 0);
        assert_eq!(evaluate("local t={1,2}; t[3]=4; return rawlen(t)"), 3);
        assert_eq!(evaluate("local f=rawlen; return f('xyz')"), 3);
        assert_eq!(
            evaluate("local rawlen=function() return 6 end; return rawlen({})"),
            6
        );
        assert_eq!(evaluate("return (type(rawlen)=='function') and 1 or 0"), 1);
        for source in ["return rawlen()", "return rawlen(2)"] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError)
            );
        }
    }

    #[test]
    fn nil_table_fields_do_not_allocate_nonexistent_array_storage() {
        // Eight empty high-index deletions previously consumed the entire
        // bounded table cell budget despite not inserting any Lua keys.
        assert_eq!(
            evaluate(
                "local total=0; for i=1,8 do local t={}; t[16384]=nil; t[1]=i; total=total+t[1] end; return total"
            ),
            36
        );
        assert_eq!(
            evaluate("local t={1,2,3}; t[2]=nil; return t[2]==nil and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("local t={nil,5,nil}; return next(t)==2 and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("local function holes() return nil,4,nil end; local t={holes()}; return t[2]"),
            4
        );
        assert_eq!(
            evaluate("local t={[1]=5}; rawset(t,1,nil); return next(t)==nil and 1 or 0"),
            1
        );
    }

    #[test]
    fn ipairs_respects_index_fallbacks_without_changing_raw_iteration() {
        for (source, expected) in [
            (
                "local t=setmetatable({2},{__index=function(_,i) if i==2 then return 5 end end}); local sum=0; for i,v in ipairs(t) do sum=sum+v end; return sum",
                7,
            ),
            (
                "local t=setmetatable({},{__index={[1]=3,[2]=4}}); local sum=0; for i,v in ipairs(t) do sum=sum+i*10+v end; return sum",
                37,
            ),
            (
                "local t=setmetatable({}, {__index=function(_,i) if i<4 then return i*2 end end}); local sum=0; for i,v in ipairs(t) do sum=sum+v end; return sum",
                12,
            ),
            (
                "local t=setmetatable({1,nil,3}, {__index={[2]=9}}); local sum=0; for i,v in ipairs(t) do sum=sum+v end; return sum",
                13,
            ),
            (
                "local t=setmetatable({}, {__index={[1]=3}}); return rawget(t,1)==nil and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {__index={[1]=3}}); return next(t)==nil and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({1,2}, {__pairs=function() return function() end,nil,nil end}); local sum=0; for _,v in ipairs(t) do sum=sum+v end; return sum",
                3,
            ),
            (
                "local t=setmetatable({5}, {__index=function() return nil end}); local sum=0; for i,v in ipairs(t) do sum=sum+v end; return sum",
                5,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        let program = compile_flow_chunk(
            "local t={}; setmetatable(t,{__index=t}); local x=0; for i,v in ipairs(t) do x=x+v end; return x"
        )
        .unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn tostring_metatable_hook_coerces_numeric_and_bounds_recursive_calls() {
        for (source, expected) in [
            (
                "local t=setmetatable({}, {__tostring=function() return 'hello' end}); return tostring(t)=='hello' and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({x=7}, {__tostring=function(self) return 'n'..self.x end}); return tostring(t)=='n7' and 1 or 0",
                1,
            ),
            (
                "local count=0; local t=setmetatable({}, {__tostring=function() count=count+1; return 'yes','extra' end}); return tostring(t)=='yes' and count or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {__tostring=function(self) return tostring(12) end}); return tostring(t)=='12' and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {}); return type(tostring(t))=='string' and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {__tostring=function() return 'safe' end}); return type(t)=='table' and tostring(t)=='safe' and 1 or 0",
                1,
            ),
            (
                "return tostring(nil)=='nil' and tostring(false)=='false' and 1 or 0",
                1,
            ),
            ("return tostring(7)=='7' and 1 or 0", 1),
            (
                "local t=setmetatable({}, {__tostring=function() return 7 end}); return tostring(t)=='7' and 1 or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for source in [
            "local t=setmetatable({}, {__tostring=function() end}); return tostring(t)",
            "local t=setmetatable({}, {__tostring=9}); return tostring(t)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
        let program = compile_flow_chunk(
            "local t={}; setmetatable(t,{__tostring=function(self) return tostring(self) end}); return tostring(t)"
        )
        .unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn pairs_metatable_hook_preserves_generic_for_and_multiple_results() {
        for (source, expected) in [
            (
                "local t=setmetatable({a=3}, {__pairs=function(self) return next,self,nil end}); local total=0; for k,v in pairs(t) do total=total+v end; return total",
                3,
            ),
            (
                "local t=setmetatable({}, {__pairs=function() return function(_,k) if k==nil then return 5,6 end end,nil,nil end}); local total=0; for k,v in pairs(t) do total=total+k+v end; return total",
                11,
            ),
            (
                "local t=setmetatable({}, {__pairs=function() return next,{x=7},nil,99 end}); return select('#',pairs(t))",
                3,
            ),
            (
                "local t=setmetatable({}, {__pairs=function() return nil end}); return select('#',pairs(t))",
                3,
            ),
            (
                "local t=setmetatable({}, {__pairs=function() end}); return pairs(t)==nil and 1 or 0",
                1,
            ),
            (
                "local n=0; local t=setmetatable({}, {__pairs=function(self) n=n+1; return next,{x=4},nil end}); local s=0; for k,v in pairs(t) do s=s+v end; return s+n",
                5,
            ),
            (
                "local t=setmetatable({4,5}, {__pairs=function() return function() end,nil,nil end}); local s=0; for i,v in ipairs(t) do s=s+v end; return s",
                9,
            ),
            (
                "local t=setmetatable({a=3}, {__pairs=function() return next,{x=7},nil end}); local s=0; for k,v in pairs(t) do s=s+v end; return s",
                7,
            ),
            (
                "local t=setmetatable({a=3}, {__pairs=function() return next,{x=7},nil end}); return next(t)~=nil and 1 or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        let program =
            compile_flow_chunk("local t=setmetatable({}, {__pairs=7}); return pairs(t)").unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
        let program = compile_flow_chunk(
            "local t={}; setmetatable(t,{__pairs=function(self) return pairs(self) end}); return pairs(t)"
        )
        .unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn comparison_metamethods_preserve_identity_truthiness_and_ordering() {
        for (source, expected) in [
            ("return (3<5 and 'a'<'b' and 5>=5) and 1 or 0", 1),
            (
                "local mt={__lt=function(a,b) return a.x<b.x end}; local a=setmetatable({x=2},mt); local b=setmetatable({x=4},mt); return a<b and 1 or 0",
                1,
            ),
            (
                "local mt={__lt=function(a,b) return a.x<b.x end}; local a=setmetatable({x=2},mt); local b=setmetatable({x=4},mt); return b>a and 1 or 0",
                1,
            ),
            (
                "local mt={__lt=function(a,b) return a.x<b.x end}; local a=setmetatable({x=2},mt); local b=setmetatable({x=4},mt); return a<=b and 1 or 0",
                1,
            ),
            (
                "local mt={__lt=function(a,b) return a.x<b.x end}; local a=setmetatable({x=2},mt); local b=setmetatable({x=4},mt); return a>=b and 1 or 0",
                0,
            ),
            (
                "local mt={__le=function(a,b) return a.x<=b.x end}; local a=setmetatable({x=2},mt); local b=setmetatable({x=4},mt); return a<=b and 1 or 0",
                1,
            ),
            (
                "local mt={__le=function(a,b) return a.x<=b.x end}; local a=setmetatable({x=2},mt); local b=setmetatable({x=4},mt); return b>=a and 1 or 0",
                1,
            ),
            (
                "local mt={__le=function() return nil end, __lt=function() return true end}; local a=setmetatable({},mt); return a<=a and 1 or 0",
                0,
            ),
            (
                "local mt={__lt=function() return 0 end}; local a=setmetatable({},mt); return a<a and 1 or 0",
                1,
            ),
            (
                "local mt={__eq=function(a,b) return true end}; local a=setmetatable({},mt); local b=setmetatable({},mt); return a==b and 1 or 0",
                1,
            ),
            (
                "local mt={__eq=function() return false end}; local a=setmetatable({},mt); local b=setmetatable({},mt); return a~=b and 1 or 0",
                1,
            ),
            (
                "local mt={__eq=function() return false end}; local a=setmetatable({},mt); return a==a and 1 or 0",
                1,
            ),
            (
                "local a=setmetatable({}, {__eq=function() return true end}); return a=={} and 1 or 0",
                1,
            ),
            (
                "local a={}; local b=setmetatable({}, {__eq=function() return true end}); return a==b and 1 or 0",
                1,
            ),
            (
                "local a=setmetatable({}, {__eq=function() return true end}); return a==3 and 1 or 0",
                0,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for source in [
            "return {} < {}",
            "local a=setmetatable({}, {__lt=7}); return a<{}",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
        let program = compile_flow_chunk(
            "local a={}; setmetatable(a,{__lt=function(x,y) return x<y end}); return a<a",
        )
        .unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn concat_metamethod_preserves_byte_concat_and_bounded_callbacks() {
        for (source, expected) in [
            ("return ('ab'..'cd')=='abcd' and 1 or 0", 1),
            ("return ('a'..12)=='a12' and 1 or 0", 1),
            (
                "local t=setmetatable({name='X'},{__concat=function(a,b) return a.name..b end}); return (t..'Y')=='XY' and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({name='X'},{__concat=function(a,b) return a..b.name end}); return ('Y'..t)=='YX' and 1 or 0",
                1,
            ),
            (
                "local a=setmetatable({}, {__concat=function() return 11 end}); local b=setmetatable({}, {__concat=function() return 22 end}); return a..b",
                11,
            ),
            (
                "local b=setmetatable({}, {__concat=function() return 22 end}); return {}..b",
                22,
            ),
            (
                "local t=setmetatable({}, {__concat=function() return 9,10 end}); return t..'x'",
                9,
            ),
            (
                "local t=setmetatable({}, {__concat=function() end}); return (t..'x')==nil and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {__concat=function() return 'ok' end}); return (t..3)=='ok' and 1 or 0",
                1,
            ),
            (
                "local n=0; local t=setmetatable({}, {__concat=function() n=n+1; return 'ok' end}); return (t..3)=='ok' and n or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for source in [
            "return {}..'x'",
            "local t=setmetatable({}, {__concat=3}); return t..'x'",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
        let program = compile_flow_chunk(
            "local t={}; setmetatable(t,{__concat=function(a,b) return a..b end}); return t..'x'",
        )
        .unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn binary_bitwise_metamethods_preserve_integer_rules_and_bounded_callbacks() {
        for (source, expected) in [
            (
                "local t=setmetatable({}, {__band=function(a,b) return b+2 end}); return t&5",
                7,
            ),
            (
                "local t=setmetatable({}, {__bor=function(a,b) return a+3 end}); return 9|t",
                12,
            ),
            (
                "local t=setmetatable({}, {__bxor=function(a,b) return 8 end}); return t~3",
                8,
            ),
            (
                "local t=setmetatable({}, {__shl=function(a,b) return b*2 end}); return t<<5",
                10,
            ),
            (
                "local t=setmetatable({}, {__shr=function(a,b) return b+3 end}); return t>>5",
                8,
            ),
            (
                "local a=setmetatable({}, {__band=function() return 11 end}); local b=setmetatable({}, {__band=function() return 22 end}); return a&b",
                11,
            ),
            (
                "local b=setmetatable({}, {__band=function() return 22 end}); return {}&b",
                22,
            ),
            (
                "local t=setmetatable({}, {__band=function() return 4,5 end}); return t&1",
                4,
            ),
            (
                "local t=setmetatable({}, {__bor=function() end}); return (t|1)==nil and 1 or 0",
                1,
            ),
            (
                "local n=0; local t=setmetatable({}, {__shl=function(_,x) n=n+1; return n+x end}); return t<<4",
                5,
            ),
            ("return 7&3", 3),
            ("return 7|8", 15),
            ("return 7~3", 4),
            ("return 7<<2", 28),
            ("return 16>>2", 4),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for source in [
            "return {}&2",
            "local t=setmetatable({}, {__band=7}); return t&2",
            "return 1.5&2",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
        let program = compile_flow_chunk(
            "local t={}; setmetatable(t,{__band=function(a,b) return a&b end}); return t&2",
        )
        .unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn unary_metamethods_pass_duplicate_operand_and_keep_numeric_fallback() {
        for (source, expected) in [
            (
                "local t=setmetatable({}, {__unm=function(a,b) return rawequal(a,b) and 8 or 0 end}); return -t",
                8,
            ),
            (
                "local t=setmetatable({}, {__bnot=function(a,b) return rawequal(a,b) and 9 or 0 end}); return ~t",
                9,
            ),
            (
                "local t=setmetatable({x=14}, {__unm=function(self) return self.x end}); return -t",
                14,
            ),
            (
                "local t=setmetatable({}, {__bnot=function() return 17,18 end}); return ~t",
                17,
            ),
            (
                "local t=setmetatable({}, {__unm=function() end}); return (-t)==nil and 1 or 0",
                1,
            ),
            (
                "local n=2; local t=setmetatable({}, {__unm=function() n=n+1; return n end}); return -t+n",
                6,
            ),
            ("return -'5'", -5),
            ("return ~5", -6),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for source in ["return -{}", "return ~{}"] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
        let program = compile_flow_chunk(
            "local t={}; setmetatable(t,{__unm=function(x) return -x end}); return -t",
        )
        .unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn arithmetic_metamethods_reuse_owned_lua_closure_frames() {
        for (source, expected) in [
            (
                "local t=setmetatable({}, {__add=function(a,b) return b+3 end}); return t+5",
                8,
            ),
            (
                "local t=setmetatable({}, {__sub=function(a,b) return a.x-b end}); t.x=12; return t-3",
                9,
            ),
            (
                "local t=setmetatable({}, {__mul=function(a,b) return a.factor*b end}); t.factor=4; return t*5",
                20,
            ),
            (
                "local t=setmetatable({}, {__div=function(a,b) return 33 end}); return t/5",
                33,
            ),
            (
                "local t=setmetatable({}, {__idiv=function(a,b) return 44 end}); return t//5",
                44,
            ),
            (
                "local t=setmetatable({}, {__mod=function(a,b) return 55 end}); return t%5",
                55,
            ),
            (
                "local t=setmetatable({}, {__pow=function(a,b) return 66 end}); return t^5",
                66,
            ),
            (
                "local t=setmetatable({}, {__add=function(a,b) return a+7 end}); return 3+t",
                10,
            ),
            (
                "local t=setmetatable({}, {__add=function(a,b) return b end}); return rawequal(12+t,t) and 1 or 0",
                1,
            ),
            (
                "local a=setmetatable({}, {__add=function() return 11 end}); local b=setmetatable({}, {__add=function() return 22 end}); return a+b",
                11,
            ),
            (
                "local b=setmetatable({}, {__add=function() return 22 end}); return {}+b",
                22,
            ),
            (
                "local t=setmetatable({}, {__add=function() return 4,5 end}); return t+1",
                4,
            ),
            (
                "local t=setmetatable({}, {__add=function() end}); return (t+1)==nil and 1 or 0",
                1,
            ),
            (
                "local n=1; local t=setmetatable({}, {__add=function(self,x) n=n+1; return n+x end}); return t+3+n",
                7,
            ),
            (
                "local t=setmetatable({}, {__add=function(a,b) return b*10 end}); return (t+5)*2",
                100,
            ),
            (
                "local t=setmetatable({}, {__add=function() return 7 end}); return type(t+2)=='number' and 1 or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for source in [
            "return {}+1",
            "local t=setmetatable({}, {__add=7}); return t+1",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
        let program = compile_flow_chunk(
            "local t={}; setmetatable(t, {__add=function(a,b) return a+b end}); return t+1",
        )
        .unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn callable_tables_forward_lua_args_and_obey_bounded_call_depth() {
        for (source, expected) in [
            (
                "local t=setmetatable({base=4},{__call=function(self,x) return self.base+x end}); return t(3)",
                7,
            ),
            (
                "local t=setmetatable({}, {__call=function(self,x) return x*2 end}); local f=t; return f(6)",
                12,
            ),
            (
                "local n=0; local t=setmetatable({}, {__call=function(self,x) n=n+1; return x+n end}); return t(2)+t(3)",
                8,
            ),
            (
                "local t=setmetatable({}, {__call=function(self,f) return f(3) end}); return t(function(n) return n+5 end)",
                8,
            ),
            (
                "local t=setmetatable({}, {__call=type}); return t()=='table' and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {__call=function() end}); return t()==nil and 1 or 0",
                1,
            ),
            (
                "local a={}; local b={}; setmetatable(b,{__call=function(x,y,z) return rawequal(x,b) and rawequal(y,a) and z+3 or 0 end}); setmetatable(a,{__call=b}); return a(5)",
                8,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        let program = compile_flow_chunk(
            "local t=setmetatable({}, {__call=function(self,...) return select('#',...),... end}); return t(3,4)"
        )
        .unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Ok(vec![
                Value::Integer(2),
                Value::Integer(3),
                Value::Integer(4)
            ])
        );
        let program = compile_flow_chunk(
            "local t=setmetatable({}, {__call=function() return 3,4 end}); return t()",
        )
        .unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Ok(vec![Value::Integer(3), Value::Integer(4)])
        );
        for source in [
            "local t={}; return t(1)",
            "local t=setmetatable({}, {__call=12}); return t()",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
        let program =
            compile_flow_chunk("local t={}; setmetatable(t,{__call=t}); return t()").unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn length_metamethod_uses_owned_closures_and_preserves_rawlen() {
        assert_eq!(
            evaluate(
                "local t=setmetatable({1,2,3},{__len=function(a,b) return (a==b) and 90 or 0 end}); return #t"
            ),
            90
        );
        assert_eq!(
            evaluate(
                "local t=setmetatable({1,2,3},{__len=function() return 90 end}); return rawlen(t)"
            ),
            3
        );
        assert_eq!(
            evaluate("local t=setmetatable({}, {__len=function() return 4,5 end}); return #t"),
            4
        );
        assert_eq!(
            evaluate(
                "local t=setmetatable({}, {__len=function() end}); return (#t)==nil and 1 or 0"
            ),
            1
        );
        assert_eq!(
            evaluate(
                "local n=2; local t=setmetatable({}, {__len=function() n=n+1; return n end}); return #t+n"
            ),
            6
        );
        assert_eq!(evaluate("local t=setmetatable({1,2,3},{}); return #t"), 3);
        assert_eq!(evaluate("return #('é')"), 2);
        let program = compile_flow_chunk("local t=setmetatable({}, {__len=7}); return #t").unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
        let program = compile_flow_chunk(
            "local t={}; setmetatable(t,{__len=function(self) return #self end}); return #t",
        )
        .unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn callable_index_and_newindex_share_lexical_closure_frames() {
        assert_eq!(
            evaluate(
                "local n=0; local t=setmetatable({}, {__index=function(_,k) n=n+1; if k=='x' then return 10 end end}); return t.x+t.x+n"
            ),
            22
        );
        assert_eq!(
            evaluate(
                "local t={x=9}; setmetatable(t,{__index=function() return 2 end}); return t.x"
            ),
            9
        );
        assert_eq!(
            evaluate("local t=setmetatable({}, {__index=function() return 4,5 end}); return t.x"),
            4
        );
        assert_eq!(
            evaluate(
                "local t=setmetatable({}, {__index=function() end}); return t.x==nil and 1 or 0"
            ),
            1
        );
        assert_eq!(
            evaluate(
                "local t=setmetatable({}, {__index=function(_,k) return k*2 end}); return t[3]"
            ),
            6
        );
        assert_eq!(
            evaluate(
                "local dest={}; local t=setmetatable({}, {__newindex=function(_,k,v) dest[k]=v*2 end}); t.x=5; return dest.x"
            ),
            10
        );
        assert_eq!(
            evaluate(
                "local t=setmetatable({}, {__newindex=function(self,k,v) rawset(self,k,v+1) end}); t.x=6; return t.x"
            ),
            7
        );
        assert_eq!(
            evaluate(
                "local t={x=2}; local n=0; setmetatable(t,{__newindex=function() n=n+1 end}); t.x=4; return t.x*10+n"
            ),
            40
        );
        assert_eq!(
            evaluate(
                "local dest={}; local t=setmetatable({}, {__newindex=function(_,k,v) dest[k]=v end}); t.x,t.y=3,4; return dest.x*10+dest.y"
            ),
            34
        );
        assert_eq!(
            evaluate(
                "local t=setmetatable({}, {__index=function(self,k) return function(_,x) return x+2 end end}); return t:add(5)"
            ),
            7
        );
        assert_eq!(
            evaluate(
                "local target=setmetatable({}, {__index=function() return 8 end}); local t=setmetatable({}, {__index=target}); return t.m"
            ),
            8
        );
        assert_eq!(
            evaluate(
                "local t=setmetatable({}, {__index=function(self,k) rawset(self,k,5); return 5 end}); return t.x+t.x"
            ),
            10
        );
        let program = compile_flow_chunk(
            "local t={}; setmetatable(t,{__index=function(self,k) return self[k] end}); return t.x",
        )
        .unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
        let program = compile_flow_chunk(
            "local t={}; setmetatable(t,{__newindex=function(self,k,v) self[k]=v end}); t.x=1; return 0"
        )
        .unwrap();
        assert_eq!(
            execute_flow_value(&program, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn table_backed_index_and_newindex_fallbacks_remain_bounded() {
        for (source, expected) in [
            ("local t=setmetatable({}, {__index={x=7}}); return t.x", 7),
            (
                "local t=setmetatable({}, {__index={x=7}}); return rawget(t,'x')==nil and 1 or 0",
                1,
            ),
            (
                "local t={x=2}; setmetatable(t,{__index={x=7}}); return t.x",
                2,
            ),
            (
                "local t=setmetatable({}, {__index=setmetatable({}, {__index={x=7}})}); return t.x",
                7,
            ),
            (
                "local dest={}; local t=setmetatable({}, {__newindex=dest}); t.x=9; return dest.x",
                9,
            ),
            (
                "local dest={}; local t={x=2}; setmetatable(t,{__newindex=dest}); t.x=7; return t.x*10+(dest.x or 0)",
                70,
            ),
            (
                "local dest={}; local t=setmetatable({}, {__newindex=dest}); t['x']=9; return (rawget(t,'x')==nil and dest.x==9) and 1 or 0",
                1,
            ),
            (
                "local fallback={}; local t=setmetatable({}, {__newindex=fallback}); local i=1; t[i],i=8,2; return fallback[1]*10+i",
                82,
            ),
            (
                "local dest={}; local t=setmetatable({}, {__newindex=dest}); rawset(t,'x',8); return rawget(t,'x')==8 and dest.x==nil and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({}, {__index={add=function(self,x) return x+3 end}}); return t:add(4)",
                7,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for source in [
            "local t={}; setmetatable(t,{__index=t}); return t.missing",
            "local t={}; setmetatable(t,{__newindex=t}); t.missing=7; return 0",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::CallDepthExceeded),
                "{source}"
            );
        }
        for source in [
            "local t=setmetatable({}, {__index=42}); return t.x",
            "local t=setmetatable({}, {__newindex=true}); t.x=3; return 1",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
    }

    #[test]
    fn first_class_table_metatables_support_raw_storage_and_protection() {
        for (source, expected) in [
            ("local t={}; return getmetatable(t)==nil and 1 or 0", 1),
            ("return getmetatable(nil)==nil and 1 or 0", 1),
            (
                "local m={}; local t={}; setmetatable(t,m); return rawequal(getmetatable(t),m) and 1 or 0",
                1,
            ),
            (
                "local m={}; local t={}; local same=setmetatable(t,m); return rawequal(t,same) and 1 or 0",
                1,
            ),
            (
                "local m={__metatable='shield'}; local t=setmetatable({},m); return getmetatable(t)=='shield' and 1 or 0",
                1,
            ),
            (
                "local m={__metatable=false}; local t=setmetatable({},m); return getmetatable(t)==false and 1 or 0",
                1,
            ),
            (
                "local t=setmetatable({},{}); setmetatable(t,nil); return getmetatable(t)==nil and 1 or 0",
                1,
            ),
            (
                "local m={__metatable='locked'}; local t=setmetatable({},m); rawset(m,'__metatable',nil); setmetatable(t,nil); return getmetatable(t)==nil and 1 or 0",
                1,
            ),
            (
                "local get=getmetatable; local set=setmetatable; local t={}; local m={}; set(t,m); return rawequal(get(t),m) and 1 or 0",
                1,
            ),
            (
                "local getmetatable=function() return 7 end; return getmetatable({})",
                7,
            ),
            (
                "return type(getmetatable)=='function' and type(setmetatable)=='function' and 1 or 0",
                1,
            ),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for source in [
            "return getmetatable()",
            "return setmetatable()",
            "return setmetatable({})",
            "return setmetatable(3,{})",
            "return setmetatable({},3)",
            "local t=setmetatable({}, {__metatable='locked'}); return setmetatable(t,nil)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
    }

    #[test]
    fn first_class_next_pairs_and_ipairs_follow_bounded_lua_raw_iteration() {
        assert_eq!(
            evaluate("local t={[1]=12}; local k,v=next(t); return k*100+v"),
            112
        );
        assert_eq!(evaluate("return next({})==nil and 1 or 0"), 1);
        assert_eq!(evaluate("local t={7}; return next(t,1)==nil and 1 or 0"), 1);
        assert_eq!(
            evaluate(
                "local t={[0]=4,[-2]=5,[true]=6,[1]=7,[2]=8,x=9}; local s=0; local n=0; for k,v in pairs(t) do s=s+v; n=n+1 end; return s*10+n"
            ),
            396
        );
        assert_eq!(
            evaluate(
                "local t={4,5,6}; local s=0; for i,v in ipairs(t) do s=s+i*10+v end; return s"
            ),
            75
        );
        assert_eq!(
            evaluate(
                "local t={[1]=2,[3]=8}; local n=0; for i,v in ipairs(t) do n=n+i+v end; return n"
            ),
            3
        );
        assert_eq!(
            evaluate("local f=ipairs; local s=0; for i,v in f({2,3}) do s=s+v end; return s"),
            5
        );
        assert_eq!(
            evaluate("local f=pairs; local s=0; for k,v in f({name=4}) do s=s+v end; return s"),
            4
        );
        assert_eq!(
            evaluate("local iter,t,k=ipairs({4,5}); local i,v=iter(t,k); return i*10+v"),
            14
        );
        assert_eq!(
            evaluate(
                "local ipairs=function() return function(_,n) if n==nil then return 2,3 end end,nil,nil end; local s=0; for i,v in ipairs({}) do s=s+i+v end; return s"
            ),
            5
        );
        assert_eq!(evaluate("return type(pairs)=='function' and 1 or 0"), 1);
        assert_eq!(
            evaluate("local t={name=6}; local k,v=next(t); return k=='name' and v or 0"),
            6
        );
        assert_eq!(
            evaluate(
                "local key={}; local t={[key]=4}; local k,v=next(t); return rawequal(k,key) and v or 0"
            ),
            4
        );
        assert_eq!(
            evaluate(
                "local t={10,20}; local f={}; for i,v in ipairs(t) do f[i]=function() return v end end; return f[1]()+f[2]()"
            ),
            30
        );
        assert_eq!(
            evaluate(
                "local t={3,4}; local iter,state,k=pairs(t); return type(iter)=='function' and rawequal(state,t) and k==nil and 1 or 0"
            ),
            1
        );
        let entries = (1..=50)
            .map(|number| number.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let program =
            compile_flow_chunk(&format!("local t={{{entries}}}; return next(t)")).unwrap();
        assert_eq!(
            execute_flow_values(&program, 80),
            Err(FlowError::StepBudgetExceeded)
        );
        assert_eq!(
            execute_flow_values(&program, 200),
            Ok(vec![Value::Integer(1), Value::Integer(1)])
        );
        for (source, expected) in [
            (
                "return pairs(3)",
                vec![Value::Builtin(Builtin::Next), Value::Integer(3), Value::Nil],
            ),
            (
                "return ipairs(nil)",
                vec![
                    Value::Builtin(Builtin::IpairsIterator),
                    Value::Nil,
                    Value::Integer(0),
                ],
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Ok(expected),
                "{source}"
            );
        }
        for source in [
            "return next()",
            "return next(3)",
            "return pairs()",
            "return ipairs()",
            "local t={3}; return next(t,2)",
            "local t={}; return next(t,0)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
    }

    #[test]
    fn first_class_raw_table_builtins_preserve_lua_key_and_identity_semantics() {
        assert_eq!(evaluate("return rawequal(2,2.0) and 1 or 0"), 1);
        assert_eq!(evaluate("return rawequal(0,-0.0) and 1 or 0"), 1);
        assert_eq!(evaluate("return rawequal('abc',\"abc\") and 1 or 0"), 1);
        assert_eq!(evaluate("return rawequal({}, {}) and 1 or 0"), 0);
        assert_eq!(
            evaluate("local t={}; local alias=t; return rawequal(t,alias) and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("local f=function() end; return rawequal(f,f) and 1 or 0"),
            1
        );
        assert_eq!(evaluate("local t={name=7}; return rawget(t,'name')"), 7);
        assert_eq!(evaluate("local t={4}; return rawget(t,2) or 9"), 9);
        assert_eq!(evaluate("local t={}; rawset(t,'name',8); return t.name"), 8);
        assert_eq!(evaluate("local t={}; rawset(t,0,8); return t[0]"), 8);
        assert_eq!(
            evaluate("local t={}; local x=rawset(t,1,5); return rawequal(t,x) and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("local t={x=3}; rawset(t,'x',nil); return rawget(t,'x') or 4"),
            4
        );
        assert_eq!(
            evaluate("local f=rawget; local t={v=4}; return f(t,'v')"),
            4
        );
        assert_eq!(
            evaluate("local rawget=function() return 13 end; return rawget({})"),
            13
        );
        assert_eq!(evaluate("return (type(rawset)=='function') and 1 or 0"), 1);
        for source in [
            "return rawequal(1)",
            "return rawget(3,1)",
            "return rawset({},nil,2)",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_value(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError)
            );
        }
    }

    #[test]
    fn first_class_tostring_builtin_formats_bounded_lua_values() {
        assert_eq!(evaluate("return (tostring(3)=='3') and 1 or 0"), 1);
        assert_eq!(evaluate("return (tostring(2.5)=='2.5') and 1 or 0"), 1);
        assert_eq!(evaluate("return (tostring(3.0)=='3.0') and 1 or 0"), 1);
        assert_eq!(evaluate("return (tostring(true)=='true') and 1 or 0"), 1);
        assert_eq!(evaluate("return (tostring(nil)=='nil') and 1 or 0"), 1);
        assert_eq!(evaluate("return (tostring('abc')=='abc') and 1 or 0"), 1);
        assert_eq!(evaluate("return #tostring(12345)"), 5);
        assert_eq!(
            evaluate("return (type(tostring({}))=='string') and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("local f=tostring; return (f(4)=='4') and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate(
                "local tostring=function() return 'shadow' end; return (tostring(3)=='shadow') and 1 or 0"
            ),
            1
        );
        let invalid = compile_flow_chunk("return tostring()").unwrap();
        assert_eq!(
            execute_flow_chunk(&invalid, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
    }

    #[test]
    fn first_class_type_builtin_reports_lua_value_categories() {
        assert_eq!(evaluate("return (type(4)=='number') and 1 or 0"), 1);
        assert_eq!(evaluate("return (type(2.5)=='number') and 1 or 0"), 1);
        assert_eq!(evaluate("return (type(true)=='boolean') and 1 or 0"), 1);
        assert_eq!(evaluate("return (type(nil)=='nil') and 1 or 0"), 1);
        assert_eq!(evaluate("return (type('hi')=='string') and 1 or 0"), 1);
        assert_eq!(evaluate("return (type({})=='table') and 1 or 0"), 1);
        assert_eq!(
            evaluate("return (type(function() end)=='function') and 1 or 0"),
            1
        );
        assert_eq!(evaluate("return (type(type)=='function') and 1 or 0"), 1);
        assert_eq!(
            evaluate("local kind=type; return (kind(4)=='number') and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate(
                "local type=function(x) return 'shadow' end; return (type(4)=='shadow') and 1 or 0"
            ),
            1
        );
        let invalid = compile_flow_chunk("return type()").unwrap();
        assert_eq!(
            execute_flow_chunk(&invalid, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
    }

    #[test]
    fn bitwise_ops_accept_exact_integral_floats_only() {
        assert_eq!(evaluate("return 3.0&1"), 1);
        assert_eq!(evaluate("return 4.0|1"), 5);
        assert_eq!(evaluate("return 7.0~3"), 4);
        assert_eq!(evaluate("return 8.0>>1.0"), 4);
        assert_eq!(evaluate("return ~3.0"), -4);
        assert_eq!(evaluate("return (0x1p2)&2"), 0);
        assert_eq!(evaluate("return (3.0&1)==1 and 1 or 0"), 1);
        for source in ["return 3.5&1", "return ~1.5", "return 2.0<<1.2"] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_chunk(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::TypeError),
                "{source}"
            );
        }
    }

    #[test]
    fn mixed_large_integer_float_ordering_preserves_integer_precision() {
        assert_eq!(
            evaluate("return (9007199254740993>9007199254740992.0) and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("return (-9007199254740993<-9007199254740992.0) and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("return (9223372036854775807<9223372036854775808.0) and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("return (0x8000000000000000<=-9223372036854775808.0) and 1 or 0"),
            1
        );
        assert_eq!(
            evaluate("return (9007199254740993==9007199254740992.0) and 1 or 0"),
            0
        );
        assert_eq!(evaluate("return (0/0<0) and 1 or 0"), 0);
    }

    #[test]
    fn hex_floating_literals_use_binary_exponents() {
        let float = |source| {
            let program = compile_flow_chunk(source).unwrap();
            let values = execute_flow_values(&program, DEFAULT_STEP_BUDGET).unwrap();
            let [Value::Float(bits)] = values.as_slice() else {
                panic!("expected float from {source}");
            };
            f64::from_bits(*bits)
        };
        assert_eq!(float("return 0x1.8p1"), 3.0);
        assert_eq!(float("return 0X.8P+2"), 2.0);
        assert_eq!(float("return 0x1p-1"), 0.5);
        assert_eq!(float("return 0x10.0"), 16.0);
        assert_eq!(evaluate("return (0x1.8p1==3.0) and 1 or 0"), 1);
        assert_eq!(evaluate("return (0X.8P+2==2) and 1 or 0"), 1);
        assert_eq!(evaluate("return (0x1p-1<1) and 1 or 0"), 1);
        assert_eq!(evaluate("return (0x10.0+1)==17 and 1 or 0"), 1);
    }

    #[test]
    fn bounded_decimal_floats_support_division_power_and_mixed_arithmetic() {
        let eval_float = |source| {
            let program = compile_flow_chunk(source).unwrap();
            let values = execute_flow_values(&program, DEFAULT_STEP_BUDGET).unwrap();
            let [Value::Float(bits)] = values.as_slice() else {
                panic!("expected a floating-point result from {source}");
            };
            f64::from_bits(*bits)
        };
        assert_eq!(eval_float("return 5/2"), 2.5);
        assert_eq!(eval_float("return 2^3"), 8.0);
        assert_eq!(eval_float("return 2.5+1"), 3.5);
        assert_eq!(eval_float("return 2*1.5"), 3.0);
        assert_eq!(eval_float("return -1.5"), -1.5);
        assert_eq!(eval_float("return 7.0//2"), 3.0);
        assert_eq!(eval_float("return 7.0%2"), 1.0);
        assert_eq!(eval_float("return '1.5'+2"), 3.5);
        assert_eq!(evaluate("return (3==3.0) and 1 or 0"), 1);
        assert_eq!(evaluate("return (3~=3.5) and 1 or 0"), 1);
        assert_eq!(evaluate("return (2<2.5) and 1 or 0"), 1);
        assert_eq!(evaluate("return (2.5>2) and 1 or 0"), 1);
        assert_eq!(evaluate("return (2^3^2)==512 and 1 or 0"), 1);
        assert_eq!(evaluate("return #('x'..2.5)"), 4);
        assert_eq!(evaluate("return (2^3)==8 and 1 or 0"), 1);
    }

    #[test]
    fn lua_string_concat_and_lexical_order_use_bounded_byte_values() {
        assert_eq!(evaluate("return #('ab'..'cd')"), 4);
        assert_eq!(evaluate("return #('é'..'ñ')"), 4);
        assert_eq!(evaluate("return #(12 .. 'x')"), 3);
        assert_eq!(evaluate("return (('a'..'b')=='ab') and 1 or 0"), 1);
        assert_eq!(evaluate("return (('a'..'b')~='abc') and 1 or 0"), 1);
        assert_eq!(evaluate("return ('abc'<'abd') and 1 or 0"), 1);
        assert_eq!(evaluate("return ('abc'>='abc') and 1 or 0"), 1);
        assert_eq!(
            evaluate("local a='ab'; local function join(x) return a..x end; return #join('cd')"),
            4
        );
        let invalid = compile_flow_chunk("return true .. 7").unwrap();
        assert_eq!(
            execute_flow_chunk(&invalid, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
    }

    #[test]
    fn lua_byte_strings_are_interned_by_content_and_measure_bytes() {
        assert_eq!(evaluate("return #\"abc\""), 3);
        assert_eq!(evaluate("return #\"é\""), 2);
        assert_eq!(evaluate("return #''"), 0);
        assert_eq!(evaluate("return (#'a\\nb')"), 3);
        assert_eq!(evaluate("return #\"a\\x00b\""), 3);
        assert_eq!(evaluate("return #([=[hello]=])"), 5);
        assert_eq!(evaluate("return ('abc'==\"abc\") and 1 or 0"), 1);
        assert_eq!(evaluate("return ('abc'~='abd') and 1 or 0"), 1);
        assert_eq!(
            evaluate("local a='hello'; local b=\"hello\"; return (a==b) and #a or 0"),
            5
        );
    }

    #[test]
    fn long_brackets_normalize_crlf_lfcr_and_single_cr_line_breaks() {
        for (source, expected) in [
            ("[[a\r\nb]]", b"a\nb".as_slice()),
            ("[[a\n\rb]]", b"a\nb".as_slice()),
            ("[[a\rb]]", b"a\nb".as_slice()),
            ("[[a\nb]]", b"a\nb".as_slice()),
            ("[[\r\nabc]]", b"abc".as_slice()),
            ("[[\n\rabc]]", b"abc".as_slice()),
            ("[=[\rhello\r\nworld]=]", b"hello\nworld".as_slice()),
            ("[[a\r\rb]]", b"a\n\nb".as_slice()),
        ] {
            assert_eq!(decode_lua_string(source).unwrap(), expected, "{source:?}");
        }
    }

    #[test]
    fn lua_54_utf8_escapes_support_full_thirty_one_bit_byte_range() {
        for (source, expected) in [
            (r#""\u{0}""#, vec![0x00]),
            (r#""\u{7F}""#, vec![0x7f]),
            (r#""\u{80}""#, vec![0xc2, 0x80]),
            (r#""\u{D800}""#, vec![0xed, 0xa0, 0x80]),
            (r#""\u{10FFFF}""#, vec![0xf4, 0x8f, 0xbf, 0xbf]),
            (r#""\u{110000}""#, vec![0xf4, 0x90, 0x80, 0x80]),
            (r#""\u{200000}""#, vec![0xf8, 0x88, 0x80, 0x80, 0x80]),
            (r#""\u{4000000}""#, vec![0xfc, 0x84, 0x80, 0x80, 0x80, 0x80]),
            (
                r#""\u{7FFFFFFF}""#,
                vec![0xfd, 0xbf, 0xbf, 0xbf, 0xbf, 0xbf],
            ),
        ] {
            assert_eq!(decode_lua_string(source).unwrap(), expected, "{source}");
        }
    }

    #[test]
    fn unicode_scalar_escapes_encode_utf8_bytes() {
        assert_eq!(evaluate(r#"return #"\u{1F600}""#), 4);
        assert_eq!(evaluate(r#"return #"\u{E9}""#), 2);
        assert_eq!(evaluate(r#"return ("\u{E9}" == "é") and 1 or 0"#), 1);
        assert_eq!(evaluate(r#"return #"\u{D800}""#), 3);
        assert!(compile_flow_chunk(r#"return "\u{80000000}""#).is_err());
    }

    #[test]
    fn dotted_and_colon_function_declarations_store_closures_on_tables() {
        assert_eq!(
            evaluate("local t={v=4}; function t:add(x) return self.v+x end; return t:add(3)"),
            7
        );
        assert_eq!(
            evaluate("local t={}; function t.add(a,b) return a+b end; return t.add(3,4)"),
            7
        );
        assert_eq!(
            evaluate(
                "local t={inner={v=5}}; function t.inner:add(x) return self.v+x end; return t.inner:add(3)"
            ),
            8
        );
        assert_eq!(
            evaluate(
                "local v=5; local t={base=2}; function t:add(x) return v+x+self.base end; return t:add(3)"
            ),
            10
        );
        assert_eq!(
            evaluate(
                "local t={v=7}; function t:two() return self.v,2 end; local a,b=t:two(); return a*10+b"
            ),
            72
        );
    }

    #[test]
    fn method_calls_pass_receiver_once_and_adjust_tail_results() {
        assert_eq!(
            evaluate("local t={v=4,add=function(self,x) return self.v+x end}; return t:add(3)"),
            7
        );
        assert_eq!(
            evaluate(
                "local t={name='abc',size=function(self) return #self.name end}; return t:size()"
            ),
            3
        );
        assert_eq!(
            evaluate(
                "local t={v=5,f=function(self,x) return self.v+x end}; local n=0; local function get() n=n+1; return t end; return get():f(2)+n"
            ),
            8
        );
        assert_eq!(
            evaluate(
                "local function pair() return 3,4 end; local t={sum=function(self,a,b) return self.v+a+b end,v=2}; return t:sum(pair())"
            ),
            9
        );
        let multi = compile_flow_chunk(
            "local t={v=7,two=function(self) return self.v,2 end}; return t:two()",
        )
        .unwrap();
        assert_eq!(
            execute_flow_values(&multi, DEFAULT_STEP_BUDGET),
            Ok(vec![Value::Integer(7), Value::Integer(2)])
        );
    }

    #[test]
    fn floating_table_keys_normalize_integral_values_and_reject_nan() {
        assert_eq!(evaluate("local t={}; t[1.0]=7; return t[1]"), 7);
        assert_eq!(evaluate("local t={[1]=7}; return t[1.0]"), 7);
        assert_eq!(evaluate("local t={}; t[-0.0]=3; return t[0]"), 3);
        assert_eq!(evaluate("local t={}; t[1.5]=9; return t[1.5]"), 9);
        assert_eq!(evaluate("local t={}; t[1.5]=9; return t[1] or 4"), 4);
        assert_eq!(evaluate("local t={}; t[2^3]=5; return t[8]"), 5);
        assert_eq!(
            evaluate("local t={}; t[0.0]=3; t[0]=nil; return t[-0.0] or 6"),
            6
        );
        let invalid = compile_flow_chunk("local t={}; t[0/0]=1; return 0").unwrap();
        assert_eq!(
            execute_flow_chunk(&invalid, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
    }

    #[test]
    fn arbitrary_hashable_lua_table_keys_share_bounded_storage() {
        assert_eq!(evaluate("local t={}; t[0]=7; return t[0]"), 7);
        assert_eq!(evaluate("local t={}; t[-2]=5; return t[-2]"), 5);
        assert_eq!(
            evaluate("local t={[true]=7,[false]=3}; return t[true]*10+t[false]"),
            73
        );
        assert_eq!(
            evaluate("local t={}; local key={}; t[key]=8; return t[key]"),
            8
        );
        assert_eq!(
            evaluate("local f=function() return 1 end; local t={}; t[f]=9; return t[f]"),
            9
        );
        assert_eq!(evaluate("local t={}; t[20000]=4; return t[20000]"), 4);
        assert_eq!(
            evaluate("local t={}; t[false]=8; t[false]=nil; return t[false] or 3"),
            3
        );
        assert_eq!(evaluate("local t={}; t[0]=7; return #t"), 0);
    }

    #[test]
    fn string_keys_and_named_table_fields_share_one_lookup_path() {
        assert_eq!(evaluate("local t={name=4}; return t.name"), 4);
        assert_eq!(evaluate("local t={name=4}; return t['name']"), 4);
        assert_eq!(evaluate("local t={}; t.name=7; return t['name']"), 7);
        assert_eq!(evaluate("local t={}; t['n']=3; return t.n"), 3);
        assert_eq!(
            evaluate("local t={name=3,[2]=5,7}; return t.name+t[2]+t[1]"),
            15
        );
        assert_eq!(
            evaluate("local t={}; t.name=3; t.name=nil; return t.name or 7"),
            7
        );
        assert_eq!(
            evaluate("local key='name'; local t={[key]=9}; return t.name"),
            9
        );
        assert_eq!(evaluate("local t={['a']=4,b=2}; return t.a+t.b"), 6);
        assert_eq!(evaluate("local t={}; t.a,t.b=2,3; return t.a*10+t.b"), 23);
        assert_eq!(
            evaluate("local t={}; local key='x'; t[key]=4; return t.x"),
            4
        );
        assert_eq!(evaluate("local t={}; t.x=4; return #t"), 0);
    }

    #[test]
    fn table_length_uses_positive_index_border() {
        assert_eq!(evaluate("local t={}; return #t"), 0);
        assert_eq!(evaluate("local t={4,5,6}; return #t"), 3);
        assert_eq!(evaluate("local t={[1]=7,[2]=8}; return #t"), 2);
        assert_eq!(evaluate("local t={1,2}; t[3]=4; return #t"), 3);
        assert_eq!(
            evaluate("local t={1,2}; local function size(x) return #x end; return size(t)"),
            2
        );
        assert_eq!(evaluate("local t={1,2}; t[2]=nil; return #t"), 1);
        let invalid = compile_flow_chunk("return #true").unwrap();
        assert_eq!(
            execute_flow_chunk(&invalid, DEFAULT_STEP_BUDGET),
            Err(FlowError::TypeError)
        );
    }

    #[test]
    fn mixed_table_constructor_expands_last_method_result_list() {
        assert_eq!(
            evaluate(
                "local maker={pair=function(self) return 3,4 end}; local t={[5]=9,1,maker:pair()}; return t[1]*100+t[2]*10+t[3]"
            ),
            134
        );
        assert_eq!(
            evaluate(
                "local maker={pair=function(self) return 3,4 end}; local t={[5]=9,1,(maker:pair())}; return t[3] or 7"
            ),
            7
        );
    }

    #[test]
    fn computed_integer_keys_in_mixed_table_constructors() {
        assert_eq!(evaluate("local t={[3]=7,[1]=4}; return t[1]*10+t[3]"), 47);
        assert_eq!(evaluate("local t={2,[4]=9,3}; return t[1]*10+t[2]"), 23);
        assert_eq!(
            evaluate("local t={[3]=7,2,[2]=8}; return t[1]*100+t[2]*10+t[3]"),
            287
        );
        assert_eq!(
            evaluate("local k=3; local t={[k]=9,1}; return t[1]*10+t[3]"),
            19
        );
        assert_eq!(
            evaluate(
                "local function vals() return 4,5 end; local t={[5]=9,2,vals()}; return t[1]*100+t[2]*10+t[3]"
            ),
            245
        );
        assert_eq!(
            evaluate(
                "local function vals() return end; local t={[3]=9,2,vals()}; return t[1]*10+t[3]"
            ),
            29
        );
        assert_eq!(
            evaluate(
                "local k=0; local function nextkey() k=k+1; return k end; local t={[nextkey()]=5,[nextkey()]=7}; return t[1]*10+t[2]"
            ),
            57
        );
        assert_eq!(evaluate("local t={name=7}; return t.name"), 7);
        assert_eq!(evaluate("local t={[0]=7,1}; return t[0]*10+t[1]"), 71);
    }

    #[test]
    fn bounded_tables_reject_unsupported_keys_and_fields() {
        assert_eq!(evaluate("local t={name=2}; return t.name"), 2);
        for source in ["local t={1}; t[nil]=7; return 1", "local t=1; return t[1]"] {
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
    fn recursively_dispatched_metamethods_stop_before_native_stack_exhaustion() {
        for source in [
            "local t={}; setmetatable(t,{__index=function(self,k) return self[k] end}); return t.x",
            "local t={}; setmetatable(t,{__add=function(a,b) return a+b end}); return t+1",
            "local t={}; setmetatable(t,{__pairs=function(self) return pairs(self) end}); return pairs(t)",
            "local t={}; setmetatable(t,{__len=function(self) return #self end}); return #t",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(
                execute_flow_values(&program, DEFAULT_STEP_BUDGET),
                Err(FlowError::CallDepthExceeded),
                "{source}"
            );
        }
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
    fn const_locals_preserve_shadowing_capture_and_assignment_guards() {
        for (source, expected) in [
            ("local x<const> = 7; return x", 7),
            ("local x<const>, y = 4,5; y=8; return x+y", 12),
            ("local x<const> = {n=4}; x.n=8; return x.n", 8),
            (
                "local x<const> = 4; local function f() return x+1 end; return f()",
                5,
            ),
            ("local x<const> = 4; do local x=7; x=8 end; return x", 4),
            ("local x<const> = 2; do local x<const> = 7 end; return x", 2),
            ("local x<const>; return x==nil and 1 or 0", 1),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for source in [
            "local x<const> = 5; x=6; return x",
            "local x<const> = 5; do x=6 end; return x",
            "local x<const> = 5; local function f() x=6 end; return x",
            "local x<const>, y = 4,5; x=7; return y",
            "local x<close> = nil; return x",
        ] {
            assert!(compile_flow_chunk(source).is_err(), "{source}");
        }
    }

    #[test]
    fn break_statements_can_precede_unreachable_lua_statements() {
        for (script, expected) in [
            ("local x=1 while true do break x=2 end return x", 1),
            ("local x=1 for i=1,3 do break x=x+1 end return x", 1),
            ("local x=1 repeat break x=x+1 until true return x", 1),
            (
                "local x=0 while true do if true then break end x=x+1 end return x",
                0,
            ),
            (
                "local x=0 for i=1,3 do if i==1 then break end x=x+1 end return x",
                0,
            ),
            ("local x=0 while true do break; local y=2 end return x", 0),
            (
                "local x=0 while true do break; ::mark:: x=2 end return x",
                0,
            ),
            ("local x=0 while true do break break end return x", 0),
            (
                "local x=0 repeat if true then break end x=2 until false return x",
                0,
            ),
            ("local x=0 for i=1,2 do x=x+1 break; x=x+5 end return x", 1),
        ] {
            assert_eq!(evaluate(script), expected, "{script}");
        }
    }

    #[test]
    fn adjacent_statement_parsing_matches_lua_without_semicolons() {
        for (script, expected) in [
            ("local x=1 local y=2 return x+y", 3),
            ("local x=1 x=x+2 return x", 3),
            ("local n=0 if true then n=3 n=n+4 end return n", 7),
            ("local n=0 while n<3 do n=n+1 end return n", 3),
            ("local n=0 for i=1,3 do n=n+i end return n", 6),
            ("local n=0 do local x=4 n=n+x end return n", 4),
            ("local n=0 repeat n=n+1 until n==3 return n", 3),
            ("local function add(x) return x+2 end return add(4)", 6),
            ("local t={} t.x=5 return t.x", 5),
            ("local x=1 if false then x=2 else x=3 end return x", 3),
            ("local x=1 do goto done end ::done:: return x", 1),
            ("local x=1 x=x+1 x=x*3 return x", 6),
            ("local t={1} t[1]=2 return t[1]", 2),
            (
                "local x=0; ::again:: x=x+1 if x<3 then goto again end return x",
                3,
            ),
        ] {
            assert_eq!(evaluate(script), expected, "{script}");
        }
    }

    #[test]
    fn visible_enclosing_goto_labels_preserve_lexical_local_lifetimes() {
        for (script, expected) in [
            ("do goto out end; ::out:: return 7", 7),
            ("if true then goto done end; ::done:: return 8", 8),
            ("local x=1; do goto done end; x=4; ::done:: return x", 1),
            (
                "local x=1; ::again:: x=x+1; if x<4 then do goto again end; end; return x",
                4,
            ),
            ("while true do goto out end; ::out:: return 5", 5),
            ("local x=1; do local x=7; goto out end; ::out:: return x", 1),
            ("local x=1; goto out; do local x=9 end; ::out:: return x", 1),
            (
                "local x=0; do if true then goto out end; x=5 end; ::out:: return x",
                0,
            ),
            (
                "local x=0; for i=1,3 do if i==2 then goto done end; x=x+1 end; ::done:: return x",
                1,
            ),
            (
                "local x=0; for _,v in ipairs({4,5}) do if v==5 then goto done end; x=x+v end; ::done:: return x",
                4,
            ),
            (
                "local x=0; ::top:: x=x+1; local temp=x; if x<4 then do goto top end; end; return x",
                4,
            ),
            ("do goto finish; local x=8 end; ::finish:: return 11", 11),
            (
                "local x=0; do goto mark; ::mark:: x=x+1 end; ::mark:: return x",
                1,
            ),
        ] {
            assert_eq!(evaluate(script), expected, "{script}");
        }
        for script in [
            "do goto outer end; local x=3; ::outer:: return x",
            "do ::inner:: end goto inner; return 1",
            "goto missing; return 1",
            "do goto a end; do ::a:: end; return 1",
            "goto done; local x=2; ::done:: return x",
            "do ::a:: ::a:: end; return 1",
            "local function f() goto outside end ::outside:: return f()",
        ] {
            assert!(compile_flow_chunk(script).is_err(), "{script}");
        }
    }

    #[test]
    fn same_block_goto_respects_scope_and_local_lifetimes() {
        for (source, expected) in [
            ("::start:: goto done; ::done:: return 4", 4),
            ("goto done ::done:: return 4", 4),
            ("do ::inside:: end; return 4", 4),
            ("local x=1; goto done; x=20; ::done:: return x", 1),
            ("local x=3; ::name:: goto done; ::done:: return x", 3),
            ("do local x=2; goto done; x=9; ::done:: end; return 4", 4),
        ] {
            assert_eq!(evaluate(source), expected, "{source}");
        }
        for source in [
            "goto missing; return 1",
            "goto done; local x=1; ::done:: return x",
        ] {
            let result = compile_flow_chunk(source);
            assert!(
                matches!(result, Err(FlowError::Unsupported(_))),
                "unsupported jump must be explicitly rejected: {source}"
            );
        }
        let program = compile_flow_chunk("::loop:: goto loop; return 1").unwrap();
        assert_eq!(
            execute_flow_value(&program, 80),
            Err(FlowError::StepBudgetExceeded)
        );
    }

    #[test]
    fn rejects_unsupported_and_unbound_forms() {
        for source in [
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
    fn lexical_env_redirects_global_reads_writes_and_nested_functions() {
        let source = "local saved=_ENV; assert(saved==_G); local box={print=print,base=39}; do local _ENV=box; answer=base+3; function twice(x) return x*2 end; print(answer,twice(3)) end; assert(box.answer==42); assert(answer==nil)";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 5000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"42\t6\n");

        let source = "local env={print=print,x=4}; do local _ENV=env; function read() return x end; x=9; print(read()) end; assert(env.read()==9); print(env.x)";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 5000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"9\n9\n");
    }

    #[test]
    fn global_environment_table_and_globals_share_mutable_identity() {
        let program = compile_flow_chunk(
            "counter=1; assert(_G.counter==1); assert(_G._G==_G); _G.counter=3; assert(counter==3); counter=4; assert(_G.counter==4); local saved=print; _G.print=nil; assert(print==nil); print=saved; print(counter)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 2000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"4\n");

        let program = compile_flow_chunk(
            "setmetatable(_G,{__index=function(_,key) if key=='missing' then return 14 end end}); print(missing)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 1000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"14\n");
    }

    #[test]
    fn global_and_dotted_function_declarations_share_late_bound_environment() {
        let source = "function fact(n) if n<=1 then return 1 end return n*fact(n-1) end; print(fact(5)); box={base=40}; function box:add(n) return self.base+n end; print(box:add(2)); function box.twice(n) return n*2 end; print(box.twice(9))";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 20_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"120\n42\n18\n");

        let source =
            "function first() return second() end; function second() return 11 end; print(first())";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 1000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"11\n");
    }

    #[test]
    fn ordinary_globals_are_shared_across_calls_and_overrides_are_writable() {
        let program = compile_flow_chunk(
            "counter=2; local function bump() counter=counter+1; return counter end; print(bump(),bump()); print(missing==nil); local saved=print; print=function(x) return x*2 end; assert(print(21)==42); print=saved; print(_VERSION)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 5000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"3\t4\ntrue\nLua 5.4\n");

        let program = compile_flow_chunk(
            "local original=arg; arg=nil; assert(arg==nil); arg=original; print(arg[0]); x=7; x=nil; assert(x==nil)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 1000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"=owned\n");

        let program = compile_flow_chunk("x=8; x=x+3; return x").unwrap();
        assert_eq!(execute_flow_chunk(&program, 1000), Ok(11));
    }

    #[test]
    fn argument_table_is_lazy_shared_mutable_and_resource_bounded() {
        let program = compile_flow_chunk(
            "print(arg[0], arg[1], arg[2] == nil); local old=arg; arg[1]='changed'; assert(arg==old); print(arg[1])",
        )
        .unwrap();
        let arguments = vec!["first".to_owned()];
        let outcome = execute_flow_capture_with_args(&program, 1000, "script.lua", &arguments);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"script.lua\tfirst\ttrue\nchanged\n");

        let program = compile_flow_chunk("print(arg[0])").unwrap();
        let too_many = vec!["x".to_owned(); MAX_TABLE_FIELDS + 1];
        let outcome = execute_flow_capture_with_args(&program, 1000, "script.lua", &too_many);
        assert_eq!(outcome.result, Err(FlowError::TooLarge));
        let huge = vec!["x".repeat(MAX_STRING_POOL_BYTES + 1)];
        let outcome = execute_flow_capture_with_args(&program, 1000, "script.lua", &huge);
        assert_eq!(outcome.result, Err(FlowError::TooLarge));
    }

    #[test]
    fn print_uses_mutable_tostring_and_bounds_recursive_conversions() {
        let program = compile_flow_chunk(
            "local base=tostring; tostring=function(v) return '['..base(v)..']' end; print(6,true,'x')",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 2000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"[6]\t[true]\t[x]\n");

        let program = compile_flow_chunk(
            "local base=tostring; _G.tostring=function() return 'wrapped' end; print(7); _G.tostring=base; print(8)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 2000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"wrapped\n8\n");

        let program = compile_flow_chunk("tostring=function() return 7 end; print(1)").unwrap();
        assert_eq!(
            execute_flow_capture(&program, 300).result,
            Err(FlowError::TypeError)
        );
        let program = compile_flow_chunk("tostring=print; print(1)").unwrap();
        assert_eq!(
            execute_flow_capture(&program, 300).result,
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn print_emits_bounded_lua_tab_delimited_output() {
        let source = "print(6*7, 'hello', true, nil); print(); local f=print; f('last')";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 1000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"42\thello\ttrue\tnil\n\nlast\n");

        let program = compile_flow_chunk("print('before'); error('after')").unwrap();
        let outcome = execute_flow_capture(&program, 100);
        assert!(matches!(outcome.result, Err(FlowError::Thrown(_))));
        assert_eq!(outcome.stdout, b"before\n");

        let program =
            compile_flow_chunk("local f=print; local ok=pcall(f, 'protected', 9); assert(ok)")
                .unwrap();
        let outcome = execute_flow_capture(&program, 1000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"protected\t9\n");

        let program = compile_flow_chunk("for i=1,40000 do print('x') end").unwrap();
        let outcome = execute_flow_capture(&program, 1_000_000);
        assert_eq!(outcome.result, Err(FlowError::TooLarge));
        assert!(outcome.stdout.len() <= MAX_PRINT_OUTPUT_BYTES);
    }

    #[test]
    fn top_level_chunks_implicitly_return_zero_values_on_fallthrough() {
        for source in [
            "",
            "local n=1",
            "if false then return 1 end",
            "if true then local n=1 end",
            "do local n=2 end",
            "local n=1; while n<3 do n=n+1 end",
            "goto done; ::done::",
        ] {
            let program = compile_flow_chunk(source).unwrap();
            assert_eq!(execute_flow_values(&program, 1000), Ok(vec![]), "{source}");
            assert_eq!(
                execute_flow_value(&program, 1000),
                Err(FlowError::ReturnArity),
                "{source}"
            );
        }
        let returning = compile_flow_chunk("if true then return 9 end").unwrap();
        assert_eq!(
            execute_flow_values(&returning, 100),
            Ok(vec![Value::Integer(9)])
        );
        let failing = compile_flow_chunk("local n=1//0").unwrap();
        assert_eq!(
            execute_flow_values(&failing, 100),
            Err(FlowError::DivisionByZero)
        );
    }
}
