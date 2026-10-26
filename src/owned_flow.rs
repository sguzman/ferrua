//! Experimental Ferrua-owned control-flow bytecode, separate from production fer.
//! This bounded stack VM evaluates integer, Boolean and nil Lua values with
//! lexical locals and control flow. It is not a complete Lua runtime or the register backend.

use crate::expr::{Expr, LocalAttribute, Statement, TableField, parse_chunk};
use crate::owned_vm::{floor_div, lua_mod, shift};
use std::cell::{Cell, RefCell};
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
const MAX_WARNING_OUTPUT_BYTES: usize = 65_536;
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
    /// Opaque, execution-local coroutine identity.
    Thread(usize),
    /// First-party, pure Lua core builtins have first-class function values.
    Builtin(Builtin),
}

// Capture bounds are zero-based byte offsets; the Boolean tags position
// captures, which return Lua one-based integers instead of byte substrings.
type PatternCapture = (usize, usize, bool);
type PatternMatch = (usize, usize, Vec<PatternCapture>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Print,
    Warn,
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
    CollectGarbage,
    CoroutineCreate,
    CoroutineResume,
    CoroutineYield,
    CoroutineStatus,
    MathAbs,
    MathFloor,
    MathCeil,
    MathMax,
    MathMin,
    MathType,
    MathToInteger,
    MathSqrt,
    MathUlt,
    MathFmod,
    MathModf,
    MathSin,
    MathCos,
    MathTan,
    MathRad,
    MathDeg,
    MathExp,
    MathLog,
    MathAsin,
    MathAcos,
    MathAtan,
    MathRandom,
    MathRandomSeed,
    Utf8Char,
    Utf8Len,
    Utf8Codepoint,
    Utf8Offset,
    Utf8Codes,
    Utf8CodesIteratorStrict,
    Utf8CodesIteratorLax,
    StringLen,
    StringSub,
    StringByte,
    StringFind,
    StringMatch,
    StringGmatch,
    StringGmatchIterator(usize),
    StringGsub,
    StringChar,
    StringLower,
    StringUpper,
    StringReverse,
    StringRep,
    StringFormat,
    StringPack,
    StringUnpack,
    StringPackSize,
    TableConcat,
    TableUnpack,
    TablePack,
    TableInsert,
    TableRemove,
    TableMove,
    TableSort,
}

const COROUTINE_GLOBALS: &[(&[u8], Builtin)] = &[
    (b"create", Builtin::CoroutineCreate),
    (b"resume", Builtin::CoroutineResume),
    (b"yield", Builtin::CoroutineYield),
    (b"status", Builtin::CoroutineStatus),
];

const MAX_COROUTINES: usize = 64;

const MATH_GLOBALS: &[(&[u8], Builtin)] = &[
    (b"abs", Builtin::MathAbs),
    (b"floor", Builtin::MathFloor),
    (b"ceil", Builtin::MathCeil),
    (b"max", Builtin::MathMax),
    (b"min", Builtin::MathMin),
    (b"type", Builtin::MathType),
    (b"tointeger", Builtin::MathToInteger),
    (b"sqrt", Builtin::MathSqrt),
    (b"ult", Builtin::MathUlt),
    (b"fmod", Builtin::MathFmod),
    (b"modf", Builtin::MathModf),
    (b"sin", Builtin::MathSin),
    (b"cos", Builtin::MathCos),
    (b"tan", Builtin::MathTan),
    (b"rad", Builtin::MathRad),
    (b"deg", Builtin::MathDeg),
    (b"exp", Builtin::MathExp),
    (b"log", Builtin::MathLog),
    (b"asin", Builtin::MathAsin),
    (b"acos", Builtin::MathAcos),
    (b"atan", Builtin::MathAtan),
    (b"random", Builtin::MathRandom),
    (b"randomseed", Builtin::MathRandomSeed),
];

const UTF8_GLOBALS: &[(&[u8], Builtin)] = &[
    (b"char", Builtin::Utf8Char),
    (b"len", Builtin::Utf8Len),
    (b"codepoint", Builtin::Utf8Codepoint),
    (b"offset", Builtin::Utf8Offset),
    (b"codes", Builtin::Utf8Codes),
];

const STRING_GLOBALS: &[(&[u8], Builtin)] = &[
    (b"len", Builtin::StringLen),
    (b"sub", Builtin::StringSub),
    (b"byte", Builtin::StringByte),
    (b"find", Builtin::StringFind),
    (b"match", Builtin::StringMatch),
    (b"gmatch", Builtin::StringGmatch),
    (b"gsub", Builtin::StringGsub),
    (b"char", Builtin::StringChar),
    (b"lower", Builtin::StringLower),
    (b"upper", Builtin::StringUpper),
    (b"reverse", Builtin::StringReverse),
    (b"rep", Builtin::StringRep),
    (b"format", Builtin::StringFormat),
    (b"pack", Builtin::StringPack),
    (b"unpack", Builtin::StringUnpack),
    (b"packsize", Builtin::StringPackSize),
];

const TABLE_GLOBALS: &[(&[u8], Builtin)] = &[
    (b"concat", Builtin::TableConcat),
    (b"unpack", Builtin::TableUnpack),
    (b"pack", Builtin::TablePack),
    (b"insert", Builtin::TableInsert),
    (b"remove", Builtin::TableRemove),
    (b"move", Builtin::TableMove),
    (b"sort", Builtin::TableSort),
];

const BUILTIN_GLOBALS: &[(&[u8], Builtin)] = &[
    (b"print", Builtin::Print),
    (b"warn", Builtin::Warn),
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
    (b"collectgarbage", Builtin::CollectGarbage),
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
    /// Clear uncaptured block locals after their lexical lifetime ends.
    ClearLocals(Vec<usize>),
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
    string_index: Rc<RefCell<HashMap<Vec<u8>, usize>>>,
    string_bytes: Rc<Cell<usize>>,
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
            string_index: Rc::clone(&self.string_index),
            string_bytes: Rc::clone(&self.string_bytes),
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
        if let Some(&index) = self.string_index.borrow().get(bytes.as_slice()) {
            return Ok(index);
        }
        let mut strings = self.strings.borrow_mut();
        let used = self.string_bytes.get();
        if strings.len() >= MAX_STRINGS
            || bytes.len() > MAX_STRING_POOL_BYTES
            || used > MAX_STRING_POOL_BYTES.saturating_sub(bytes.len())
        {
            return Err(FlowError::TooLarge);
        }
        let index = strings.len();
        self.string_bytes.set(used + bytes.len());
        strings.push(bytes.clone());
        self.string_index.borrow_mut().insert(bytes, index);
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
        let exited = self.active_locals[active..].to_vec();
        self.depth -= 1;
        self.frames.pop();
        self.active_locals.truncate(active);
        result?;
        if !exited.is_empty() {
            self.push(Op::ClearLocals(exited))?;
        }
        Ok(())
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
    let string_index = Rc::new(RefCell::new(HashMap::new()));
    let string_bytes = Rc::new(Cell::new(0));
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
        string_index: Rc::clone(&string_index),
        string_bytes: Rc::clone(&string_bytes),
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
                while bytes
                    .get(i)
                    .is_some_and(|byte| matches!(*byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
                {
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
// Lua's C-locale conversion accepts only these six leading/trailing
// whitespace bytes. Rust's str::trim also accepts Unicode space separators.
fn trim_lua_numeric_space(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000b}' | '\u{000c}'))
}

// Lua 5.4's integer-string scanner accumulates hexadecimal digits in an
// unsigned Lua integer with modulo-2^64 arithmetic, unlike decimal integers.
fn parse_lua_hex_integer_wrapping(source: &str) -> Option<i64> {
    let digits = source
        .strip_prefix("0x")
        .or_else(|| source.strip_prefix("0X"))?;
    if digits.is_empty() {
        return None;
    }
    let mut value = 0u64;
    for byte in digits.bytes() {
        let digit = (byte as char).to_digit(16)?;
        value = value.wrapping_mul(16).wrapping_add(u64::from(digit));
    }
    Some(value as i64)
}

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

/// Values with managed Lua heap identities can disappear from weak tables.
/// Ordinary native builtins are permanent, but gmatch iterator builtins own
/// recyclable snapshots and behave as collectible Lua closures.
fn weak_value_unreachable(
    value: Value,
    marked_tables: &[bool],
    marked_closures: &[bool],
    marked_gmatch: &[bool],
) -> Result<bool, FlowError> {
    match value {
        Value::Table(handle) => marked_tables
            .get(handle)
            .map(|&live| !live)
            .ok_or(FlowError::InvalidBytecode),
        Value::Function(handle) => marked_closures
            .get(handle)
            .map(|&live| !live)
            .ok_or(FlowError::InvalidBytecode),
        Value::Builtin(Builtin::StringGmatchIterator(handle)) => marked_gmatch
            .get(handle)
            .map(|&live| !live)
            .ok_or(FlowError::InvalidBytecode),
        _ => Ok(false),
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

/// Bytecode stack roots retained for suspended Lua callers. The snapshot is
/// refreshed at each instruction boundary, before any operand is popped for
/// a function call or a metamethod callback.
#[derive(Default)]
struct GcFrameRoots {
    stack: Vec<Value>,
    slots: Vec<usize>,
    prepared: Vec<PendingTarget>,
}

// Lua 5.4.6's xoshiro256** generator, adapted using Rust's explicit
// wrapping u64 arithmetic. Only seeded sequences are conformance fixtures.
fn lua_random_next(state: &mut [u64; 4]) -> u64 {
    let [a, b, c, d] = *state;
    let result = b.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
    let mixed_c = c ^ a;
    let mixed_d = d ^ b;
    state[0] = a ^ mixed_d;
    state[1] = b ^ mixed_c;
    state[2] = mixed_c ^ (b << 17);
    state[3] = mixed_d.rotate_left(45);
    result
}

fn lua_random_seed(state: &mut [u64; 4], a: u64, b: u64) {
    *state = [a, 0xff, b, 0];
    for _ in 0..16 {
        lua_random_next(state);
    }
}

// Lua 5.4.6 decodes original 1..6-byte UTF-8, including non-scalar
// codepoints in lax mode. Strict mode disallows surrogates and >U+10FFFF.
fn lua_utf8_decode(bytes: &[u8], strict: bool) -> Option<(i64, usize)> {
    let first = *bytes.first()?;
    if first < 0x80 {
        return Some((i64::from(first), 1));
    }
    let width = first.leading_ones() as usize;
    if !(2..=6).contains(&width) || bytes.len() < width {
        return None;
    }
    let mask = (1_u32 << (7 - width)) - 1;
    let mut code = u32::from(first) & mask;
    for &byte in &bytes[1..width] {
        if byte & 0xC0 != 0x80 {
            return None;
        }
        code = (code << 6) | u32::from(byte & 0x3F);
    }
    let minimum = match width {
        2 => 0x80,
        3 => 0x800,
        4 => 0x10000,
        5 => 0x200000,
        6 => 0x4000000,
        _ => return None,
    };
    if code < minimum || code > 0x7FFF_FFFF {
        return None;
    }
    if strict && (code > 0x10FFFF || (0xD800..=0xDFFF).contains(&code)) {
        return None;
    }
    Some((i64::from(code), width))
}

fn lua_utf8_encode(code: u32, into: &mut Vec<u8>) {
    let width = match code {
        0..=0x7F => 1,
        0x80..=0x7FF => 2,
        0x800..=0xFFFF => 3,
        0x10000..=0x1FFFFF => 4,
        0x200000..=0x3FFFFFF => 5,
        _ => 6,
    };
    if width == 1 {
        into.push(code as u8);
        return;
    }
    let mut buffer = [0_u8; 6];
    let mut remainder = code;
    for index in (1..width).rev() {
        buffer[index] = 0x80 | ((remainder & 0x3F) as u8);
        remainder >>= 6;
    }
    let header = match width {
        2 => 0xC0,
        3 => 0xE0,
        4 => 0xF0,
        5 => 0xF8,
        _ => 0xFC,
    };
    buffer[0] = header | remainder as u8;
    into.extend_from_slice(&buffer[..width]);
}

fn lua_utf8_pos(pos: i64, length: usize) -> i64 {
    if pos >= 0 {
        pos
    } else {
        ((length as i128) + (pos as i128) + 1).max(0) as i64
    }
}

/// Independent closure-like state for each Lua string.gmatch iterator.
/// Subject/pattern bytes are owned so GC cannot invalidate iterator inputs.
#[derive(Clone, Copy)]
enum PackField {
    Integer {
        size: usize,
        signed: bool,
        little: bool,
    },
    Float {
        size: usize,
        little: bool,
    },
    FixedString(usize),
    NullTerminated,
    LengthPrefixed {
        size: usize,
        little: bool,
    },
    Padding,
    Align(usize),
}

impl PackField {
    fn size(self) -> usize {
        match self {
            Self::Integer { size, .. }
            | Self::Float { size, .. }
            | Self::FixedString(size)
            | Self::LengthPrefixed { size, .. } => size,
            Self::Padding => 1,
            Self::NullTerminated | Self::Align(_) => 0,
        }
    }
}

struct GmatchState {
    source: Vec<u8>,
    pattern: Vec<u8>,
    cursor: usize,
    last_end: Option<usize>,
    done: bool,
}

/// State that must survive a future suspended Lua call. It is deliberately
/// owned instead of referencing a Rust invocation stack. Current execution
/// is still synchronous; coroutine suspension remains unimplemented.
struct OwnedCallFrame {
    pc: usize,
    slots: Vec<usize>,
    stack: Vec<Value>,
    prepared: Vec<PendingTarget>,
    last_multiple: Option<usize>,
    varargs: Vec<Value>,
}

/// Owned child activation, retaining both its bytecode and its independent
/// return-value adjustment while a caller is suspended in the VM scheduler.
struct ActiveLuaCall {
    instructions: Vec<Op>,
    frame: OwnedCallFrame,
    many: bool,
}

/// Entire Lua continuation, including callers retained while a child runs.
/// Owning the root bytecode makes this state independent of Rust stack lifetimes.
struct OwnedScheduler {
    instructions: Vec<Op>,
    root: OwnedCallFrame,
    active: Vec<ActiveLuaCall>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CoroutineStatus {
    Suspended,
    Running,
    Dead,
}

/// Coroutine states retain owned Lua continuations and their independent GC
/// frame roots across yield/resume. Entries stay bounded and uncollected in
/// this first stage; complete thread GC is a later conformance requirement.
struct OwnedCoroutine {
    entry: usize,
    scheduler: Option<OwnedScheduler>,
    status: CoroutineStatus,
    allocations: Vec<Vec<usize>>,
    roots: Vec<GcFrameRoots>,
    awaiting_resume: Option<bool>,
}

struct SavedCallContext {
    allocations: Vec<Vec<usize>>,
    roots: Vec<GcFrameRoots>,
}

/// A bounded instruction slice is an internal checkpoint, not a coroutine
/// yield. Nested function calls remain synchronous until their own return.
enum FrameProgress {
    Paused,
    Returned(Vec<Value>),
    Yield { values: Vec<Value>, many: bool },
    /// The parent cursor has already advanced. A future trampoline can
    /// enqueue the child instead of invoking it on the Rust call stack.
    Call {
        handle: usize,
        args: Vec<Value>,
        many: bool,
    },
}

enum SchedulerOutcome {
    Returned(Vec<Value>),
    Yield { values: Vec<Value>, many: bool },
}

const FRAME_STEP_QUANTUM: usize = 64;

struct Execution<'a> {
    program: &'a FlowProgram,
    budget: usize,
    steps: usize,
    depth: usize,
    automatic_gc: bool,
    tables: Vec<TableStorage>,
    free_tables: Vec<usize>,
    retired_tables: HashSet<usize>,
    /// Registration order for objects with __gc at metatable assignment.
    gc_registrations: Vec<usize>,
    gc_finalized: HashSet<usize>,
    running_finalizers: bool,
    table_cells: usize,
    closures: Vec<Option<Closure>>,
    free_closures: Vec<usize>,
    cells: Vec<Option<Value>>,
    captured_cells: HashSet<usize>,
    free_cells: Vec<usize>,
    frame_allocations: Vec<Vec<usize>>,
    gc_frame_roots: Vec<GcFrameRoots>,
    /// Native builtins retain heap values across nested Lua callbacks.
    /// These roots must not disturb Lua call-frame accounting.
    gc_temporary_roots: Vec<Vec<Value>>,
    strings: Vec<Vec<u8>>,
    string_index: HashMap<Vec<u8>, usize>,
    string_bytes: usize,
    free_strings: Vec<usize>,
    retired_strings: HashSet<usize>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    warnings_enabled: bool,
    script_name: &'a str,
    script_args: &'a [String],
    arg_handle: Option<usize>,
    globals: HashMap<usize, Value>,
    global_table: Option<usize>,
    math_table: Option<usize>,
    random_state: [u64; 4],
    gmatch_states: Vec<Option<GmatchState>>,
    free_gmatch_states: Vec<usize>,
    gmatch_bytes: usize,
    string_table: Option<usize>,
    utf8_table: Option<usize>,
    table_library: Option<usize>,
    coroutine_table: Option<usize>,
    coroutines: Vec<Option<OwnedCoroutine>>,
    current_coroutine: Option<usize>,
    /// Roots from calling frames while a coroutine's own roots are active.
    saved_call_contexts: Vec<SavedCallContext>,
    /// The entry functions of coroutines currently executing via resume.
    active_coroutine_entries: Vec<Value>,
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

    /// Collect with the operands of an in-flight table write pinned. Some
    /// writes occur inside builtins (e.g. arg construction), not directly in
    /// bytecode, so the target table might not yet be in a lexical variable.
    fn collect_table_write(
        &mut self,
        handle: usize,
        key: TableKey,
        value: Value,
    ) -> Result<(), FlowError> {
        let Some(frame) = self.gc_frame_roots.last() else {
            return Err(FlowError::TooLarge);
        };
        let slots = frame.slots.clone();
        let prepared = frame.prepared.clone();
        let mut roots = vec![Value::Table(handle), value];
        if let TableKey::Hashed(key) = key {
            roots.push(key);
        }
        self.collect_root_tables(&roots, &slots, &prepared)
    }

    fn write_table(&mut self, handle: usize, key: TableKey, value: Value) -> Result<(), FlowError> {
        let table = self.tables.get(handle).ok_or(FlowError::InvalidBytecode)?;
        let growth = match key {
            TableKey::Positive(field) => {
                if value == Value::Nil && field > table.array.len() {
                    return Ok(());
                }
                field.saturating_sub(table.array.len())
            }
            TableKey::Hashed(field) => {
                usize::from(value != Value::Nil && !table.hashed.contains_key(&field))
            }
        };
        if self.automatic_gc && self.table_cells.saturating_add(growth) > MAX_TABLE_CELLS_TOTAL {
            self.collect_table_write(handle, key, value)?;
        }
        if self.table_cells.saturating_add(growth) > MAX_TABLE_CELLS_TOTAL {
            return Err(FlowError::TooLarge);
        }
        let table = self
            .tables
            .get_mut(handle)
            .ok_or(FlowError::InvalidBytecode)?;
        match key {
            TableKey::Positive(field) => {
                if field > table.array.len() {
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
                    if growth > 0 {
                        self.table_cells += 1;
                    }
                    table.hashed.insert(field, value);
                }
            }
        }
        Ok(())
    }

    /// Resolve the table's current Lua weak-reference mode from its raw
    /// metatable. String and number keys and values are never collectible
    /// weak references, even when the mode includes their side.
    fn table_weakness(&self, table: &TableStorage) -> Result<(bool, bool), FlowError> {
        let Some(meta_handle) = table.metatable else {
            return Ok((false, false));
        };
        let Some(&name) = self.string_index.get(b"__mode".as_slice()) else {
            return Ok((false, false));
        };
        if self.retired_tables.contains(&meta_handle) {
            return Err(FlowError::InvalidBytecode);
        }
        let meta = self
            .tables
            .get(meta_handle)
            .ok_or(FlowError::InvalidBytecode)?;
        let Some(Value::String(value)) = meta.hashed.get(&Value::String(name)) else {
            return Ok((false, false));
        };
        let mode = self.strings.get(*value).ok_or(FlowError::InvalidBytecode)?;
        // Lua 5.4.6 checks mode flags using strchr on the string bytes.
        // Repeated/unrelated characters are ignored, but a NUL terminates
        // the scan even when the Lua string has additional bytes after it.
        let prefix = mode.split(|&byte| byte == 0).next().unwrap_or_default();
        Ok((prefix.contains(&b'k'), prefix.contains(&b'v')))
    }

    /// The __gc field is sampled for registration when a metatable is
    /// assigned, then looked up again when that object is finalized. Merely
    /// installing __gc into an already assigned metatable must not register
    /// the earlier table retroactively.
    fn table_finalizer(&self, handle: usize) -> Result<Option<Value>, FlowError> {
        let meta = self
            .tables
            .get(handle)
            .ok_or(FlowError::InvalidBytecode)?
            .metatable;
        let Some(meta) = meta else {
            return Ok(None);
        };
        let Some(&key) = self.string_index.get(b"__gc".as_slice()) else {
            return Ok(None);
        };
        let metatable = self.tables.get(meta).ok_or(FlowError::InvalidBytecode)?;
        Ok(metatable
            .hashed
            .get(&Value::String(key))
            .copied()
            .filter(|value| *value != Value::Nil))
    }

    /// Lua 5.4's standard warning callback starts disabled.
    fn emit_warning(&mut self, message: &[u8]) -> Result<(), FlowError> {
        if !self.warnings_enabled {
            return Ok(());
        }
        const PREFIX: &[u8] = b"Lua warning: ";
        let additional = PREFIX.len().saturating_add(message.len()).saturating_add(1);
        if self.stderr.len().saturating_add(additional) > MAX_WARNING_OUTPUT_BYTES {
            return Err(FlowError::TooLarge);
        }
        self.stderr.extend_from_slice(PREFIX);
        self.stderr.extend_from_slice(message);
        self.stderr.push(b'\n');
        Ok(())
    }

    fn warn_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        if args.is_empty() {
            return Err(FlowError::TypeError);
        }
        let mut message = Vec::new();
        for &value in args {
            let Value::String(handle) = value else {
                return Err(FlowError::TypeError);
            };
            let bytes = self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
            if message.len().saturating_add(bytes.len()) > MAX_WARNING_OUTPUT_BYTES {
                return Err(FlowError::TooLarge);
            }
            message.extend_from_slice(bytes);
        }
        if args.len() == 1 && message.starts_with(b"@") {
            match message.as_slice() {
                b"@on" => self.warnings_enabled = true,
                b"@off" => self.warnings_enabled = false,
                _ => {}
            }
            return Ok(Vec::new());
        }
        self.emit_warning(&message)?;
        Ok(Vec::new())
    }

    fn warn_finalizer_error(&mut self, error: &FlowError, method: Value) -> Result<(), FlowError> {
        if !self.warnings_enabled {
            return Ok(());
        }
        let detail = if let FlowError::Thrown(Value::String(handle)) = error {
            self.strings
                .get(*handle)
                .ok_or(FlowError::InvalidBytecode)?
                .clone()
        } else if matches!(error, FlowError::TypeError) {
            // A noncallable finalizer produces an ordinary Lua call error.
            // This covers the direct incorrect-__gc case; arbitrary errors
            // raised within callable metamethods still need full diagnostics.
            let name = match method {
                Value::Integer(_) | Value::Float(_) => "number",
                Value::Boolean(_) => "boolean",
                Value::String(_) => "string",
                Value::Table(_) => "table",
                Value::Nil => "nil",
                Value::Function(_) | Value::Builtin(_) => "function",
            };
            format!("attempt to call a {name} value (metamethod '__gc')").into_bytes()
        } else {
            b"error object is not a string".to_vec()
        };
        let mut message = b"error in __gc (".to_vec();
        message.extend_from_slice(&detail);
        message.push(b')');
        self.emit_warning(&message)
    }

    /// Closing the Lua state must finalize even reachable registered tables.
    /// Take a reverse-registration snapshot: objects registered by finalizers
    /// during shutdown are not finalized in this closing pass. Leave their
    /// storage intact until all callbacks have finished.
    fn finalize_on_shutdown(&mut self) -> Result<(), FlowError> {
        if self.running_finalizers {
            return Err(FlowError::InvalidBytecode);
        }
        let pending: Vec<usize> = self
            .gc_registrations
            .iter()
            .rev()
            .copied()
            .filter(|handle| {
                !self.gc_finalized.contains(handle) && !self.retired_tables.contains(handle)
            })
            .collect();
        self.running_finalizers = true;
        let outcome = (|| -> Result<(), FlowError> {
            for handle in pending {
                if self.steps >= self.budget {
                    return Err(FlowError::StepBudgetExceeded);
                }
                self.steps += 1;
                self.gc_finalized.insert(handle);
                if let Some(method) = self.table_finalizer(handle)? {
                    match self.protected_target(method, &[Value::Table(handle)]) {
                        Ok(_) => {}
                        Err(error @ (FlowError::Thrown(_) | FlowError::TypeError)) => {
                            self.warn_finalizer_error(&error, method)?;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            Ok(())
        })();
        self.running_finalizers = false;
        outcome
    }

    /// Bounded mark-and-sweep across active Lua call frames. Each suspended
    /// caller retains its pre-instruction stack and prepared table targets;
    /// ancestor cell allocations are conservatively retained as extra roots.
    fn collect_root_tables(
        &mut self,
        stack: &[Value],
        slots: &[usize],
        prepared: &[PendingTarget],
    ) -> Result<(), FlowError> {
        // Reentrant full collections from a finalizer are outside this
        // bounded stop-the-world preview. Never recurse through a partially
        // swept heap or call the same finalizer twice.
        if self.running_finalizers {
            return Err(FlowError::TooLarge);
        }
        if self.frame_allocations.is_empty()
            || self.frame_allocations.len() != self.gc_frame_roots.len()
        {
            return Err(FlowError::InvalidBytecode);
        }
        let mut marked_tables = vec![false; self.tables.len()];
        let mut marked_closures = vec![false; self.closures.len()];
        let mut marked_gmatch = vec![false; self.gmatch_states.len()];
        let mut marked_strings = vec![false; self.strings.len()];
        // All compiler string handles are embedded in immutable bytecode and
        // must remain valid even if no executed path has referenced them.
        if self.program.strings.len() > marked_strings.len() {
            return Err(FlowError::InvalidBytecode);
        }
        marked_strings[..self.program.strings.len()].fill(true);
        let mut work = stack.to_vec();
        // Saved caller stacks include operands that were popped to construct
        // calls, plus temporarily prepared indexed assignment addresses.
        for frame in &self.gc_frame_roots {
            work.extend_from_slice(&frame.stack);
            for &target in &frame.prepared {
                if let PendingTarget::Indexed { table, key } = target {
                    work.push(Value::Table(table));
                    if let TableKey::Hashed(value) = key {
                        work.push(value);
                    }
                }
            }
        }
        // Native operations can retain values outside Lua operand stacks.
        for pending in &self.gc_temporary_roots {
            work.extend_from_slice(pending);
        }
        // Ancestor frames are suspended and their exact lexical-slot state
        // is not available here. Keeping all their allocated cells is safe
        // and bounded; the executing frame uses the precise slot list below.
        for frame in self.frame_allocations.iter().rev().skip(1) {
            for &cell in frame {
                if let Some(Some(value)) = self.cells.get(cell) {
                    work.push(*value);
                }
            }
        }
        for &cell in slots {
            // A released lexical slot can point at an old captured cell
            // still held by a closure, without rooting it from this frame.
            if cell == usize::MAX {
                continue;
            }
            if let Some(Some(value)) = self.cells.get(cell) {
                work.push(*value);
            }
        }
        work.extend(self.globals.values().copied());
        for &target in prepared {
            if let PendingTarget::Indexed { table, key } = target {
                work.push(Value::Table(table));
                if let TableKey::Hashed(value) = key {
                    work.push(value);
                }
            }
        }
        for handle in [
            self.arg_handle,
            self.global_table,
            self.math_table,
            self.string_table,
            self.utf8_table,
            self.table_library,
            self.coroutine_table,
        ]
        .into_iter()
        .flatten()
        {
            work.push(Value::Table(handle));
        }
        // Weak-key/strong-value tables are ephemerons: a value is traversed
        // only after its key is marked from some other strong path. A value's
        // own reference back to its key cannot make the key reachable.
        let mut ephemerons = Vec::new();
        let mut activated = Vec::new();
        loop {
            while let Some(value) = work.pop() {
                if self.steps >= self.budget {
                    return Err(FlowError::StepBudgetExceeded);
                }
                self.steps += 1;
                match value {
                    Value::Table(handle) => {
                        let marked = marked_tables
                            .get_mut(handle)
                            .ok_or(FlowError::InvalidBytecode)?;
                        if *marked {
                            continue;
                        }
                        if self.retired_tables.contains(&handle) {
                            return Err(FlowError::InvalidBytecode);
                        }
                        let table = self.tables.get(handle).ok_or(FlowError::InvalidBytecode)?;
                        *marked = true;
                        if let Some(meta) = table.metatable {
                            work.push(Value::Table(meta));
                        }
                        let (weak_keys, weak_values) = self.table_weakness(table)?;
                        // Array indices are always primitive numeric keys.
                        // Even weak values retain Lua's value-like strings.
                        for &value in table.array.iter().flatten() {
                            if !weak_values || matches!(value, Value::String(_)) {
                                work.push(value);
                            }
                        }
                        for (&key, &value) in &table.hashed {
                            // Strings are never collectible weak references.
                            if !weak_keys || matches!(key, Value::String(_)) {
                                work.push(key);
                            }
                            if !weak_values {
                                if weak_keys
                                    && matches!(
                                        key,
                                        Value::Table(_)
                                            | Value::Function(_)
                                            | Value::Builtin(Builtin::StringGmatchIterator(_))
                                    )
                                {
                                    ephemerons.push((key, value));
                                    activated.push(false);
                                } else {
                                    // Strong keys or uncollectible primitive
                                    // keys directly retain their values.
                                    work.push(value);
                                }
                            } else if matches!(value, Value::String(_)) {
                                work.push(value);
                            }
                        }
                    }
                    Value::String(handle) => {
                        let mark = marked_strings
                            .get_mut(handle)
                            .ok_or(FlowError::InvalidBytecode)?;
                        if self.retired_strings.contains(&handle) {
                            return Err(FlowError::InvalidBytecode);
                        }
                        *mark = true;
                    }
                    Value::Function(handle) => {
                        let marked = marked_closures
                            .get_mut(handle)
                            .ok_or(FlowError::InvalidBytecode)?;
                        if *marked {
                            continue;
                        }
                        *marked = true;
                        let closure = self
                            .closures
                            .get(handle)
                            .and_then(Option::as_ref)
                            .ok_or(FlowError::InvalidBytecode)?;
                        for &(_, cell) in &closure.captures {
                            if let Some(Some(value)) = self.cells.get(cell) {
                                work.push(*value);
                            }
                        }
                    }
                    Value::Builtin(Builtin::StringGmatchIterator(handle)) => {
                        let marked = marked_gmatch
                            .get_mut(handle)
                            .ok_or(FlowError::InvalidBytecode)?;
                        if self
                            .gmatch_states
                            .get(handle)
                            .and_then(Option::as_ref)
                            .is_none()
                        {
                            return Err(FlowError::InvalidBytecode);
                        }
                        *marked = true;
                    }
                    _ => {}
                }
            }

            let mut changed = false;
            for (&(key, value), active) in ephemerons.iter().zip(activated.iter_mut()) {
                if self.steps >= self.budget {
                    return Err(FlowError::StepBudgetExceeded);
                }
                self.steps += 1;
                if !*active
                    && !weak_value_unreachable(
                        key,
                        &marked_tables,
                        &marked_closures,
                        &marked_gmatch,
                    )?
                {
                    *active = true;
                    work.push(value);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        // Determine the objects whose first finalization is due this cycle.
        // Registration order is observable in Lua: last registered, first
        // finalized. Delay freeing *any* object in a cycle that invokes
        // callbacks so finalizer closures and their referenced graphs remain
        // accessible, even if not strongly marked.
        let pending_finalizers: Vec<usize> = self
            .gc_registrations
            .iter()
            .rev()
            .copied()
            .filter(|&handle| {
                marked_tables.get(handle) == Some(&false)
                    && !self.retired_tables.contains(&handle)
                    && !self.gc_finalized.contains(&handle)
            })
            .collect();

        // Prepare weak-reference removal without changing the heap. All
        // candidate handles are validated before the budget gate, so even a
        // failed ephemeron cycle leaves the original table entries intact.
        let mut weak_clears = Vec::new();
        let mut weak_scan_cost = 0_usize;
        for (handle, &live) in marked_tables.iter().enumerate() {
            if !live {
                continue;
            }
            let table = &self.tables[handle];
            let (weak_keys, weak_values) = self.table_weakness(table)?;
            if !weak_keys && !weak_values {
                continue;
            }
            weak_scan_cost = weak_scan_cost
                .saturating_add(table.array.len())
                .saturating_add(table.hashed.len());
            let mut array_positions = Vec::new();
            for (index, &value) in table.array.iter().enumerate() {
                if weak_values
                    && let Some(value) = value
                    && weak_value_unreachable(
                        value,
                        &marked_tables,
                        &marked_closures,
                        &marked_gmatch,
                    )?
                {
                    array_positions.push(index);
                }
            }
            let mut hash_keys = Vec::new();
            for (&key, &value) in &table.hashed {
                let pending_finalizer_key = matches!(
                    key,
                    Value::Table(handle) if pending_finalizers.contains(&handle)
                );
                let dead_key = if weak_keys && !pending_finalizer_key {
                    weak_value_unreachable(key, &marked_tables, &marked_closures, &marked_gmatch)?
                } else {
                    false
                };
                let dead_value = if weak_values {
                    weak_value_unreachable(value, &marked_tables, &marked_closures, &marked_gmatch)?
                } else {
                    false
                };
                if dead_key || dead_value {
                    hash_keys.push(key);
                }
            }
            weak_clears.push((handle, array_positions, hash_keys));
        }
        // A full sweep traverses heap slots and captured-cell bookkeeping
        // even when every object is unreachable and marking is cheap.
        // Charge that bounded work before mutating any heap entry, so
        // repeated manual sweeps cannot bypass the interpreter budget.
        let sweep_cost = marked_tables
            .len()
            .saturating_add(marked_closures.len())
            .saturating_add(marked_gmatch.len())
            .saturating_add(marked_strings.len())
            .saturating_add(self.captured_cells.len())
            .saturating_add(weak_scan_cost)
            .saturating_add(self.frame_allocations.iter().map(Vec::len).sum::<usize>());
        if self.steps.saturating_add(sweep_cost) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += sweep_cost;

        // Clear dead weak values before the corresponding objects are
        // recycled. Positive array slots keep their reserved capacity; hash
        // entries actually release their accounted cell.
        for (handle, array_positions, hash_keys) in weak_clears {
            let table = self
                .tables
                .get_mut(handle)
                .ok_or(FlowError::InvalidBytecode)?;
            for index in array_positions {
                *table
                    .array
                    .get_mut(index)
                    .ok_or(FlowError::InvalidBytecode)? = None;
            }
            for key in hash_keys {
                if table.hashed.remove(&key).is_some() {
                    self.table_cells -= 1;
                }
            }
        }

        if !pending_finalizers.is_empty() {
            // Lua makes tables with a queued finalizer available to callbacks
            // for at least this collection. Preserve the entire bounded heap
            // until the next explicit cycle; completed callbacks cannot be
            // scheduled a second time even if they resurrect their object.
            self.running_finalizers = true;
            let outcome = (|| -> Result<(), FlowError> {
                for handle in pending_finalizers {
                    self.gc_finalized.insert(handle);
                    if let Some(method) = self.table_finalizer(handle)? {
                        match self.protected_target(method, &[Value::Table(handle)]) {
                            Ok(_) => {}
                            Err(error @ (FlowError::Thrown(_) | FlowError::TypeError)) => {
                                self.warn_finalizer_error(&error, method)?;
                            }
                            Err(error) => return Err(error),
                        }
                    }
                }
                Ok(())
            })();
            self.running_finalizers = false;
            return outcome;
        }

        // Gmatch iterator closures own Rust-side byte snapshots rather than
        // Lua string handles. After weak edges are cleared, release dead
        // snapshots and recycle their identities only when unreferenced.
        for (handle, live) in marked_gmatch.into_iter().enumerate() {
            if !live && let Some(state) = self.gmatch_states[handle].take() {
                self.gmatch_bytes -= state.source.len().saturating_add(state.pattern.len());
                self.free_gmatch_states.push(handle);
            }
        }

        // Drop unreachable execution-only interned byte strings. Tombstone
        // slots are reused, while bytecode-owned literal handles stay pinned.
        // No handle is recycled until the full ephemeron fixpoint and sweep
        // budget checks have completed.
        for (handle, live) in marked_strings.into_iter().enumerate() {
            if !live && !self.retired_strings.contains(&handle) {
                let bytes = std::mem::take(
                    self.strings
                        .get_mut(handle)
                        .ok_or(FlowError::InvalidBytecode)?,
                );
                self.string_bytes -= bytes.len();
                self.string_index.remove(bytes.as_slice());
                self.retired_strings.insert(handle);
                self.free_strings.push(handle);
            }
        }

        // Build live captured cells before retiring any closure; a single
        // lexical cell can be shared by many surviving closures.
        let mut live_captures = HashSet::new();
        for (handle, &live) in marked_closures.iter().enumerate() {
            if live {
                let closure = self.closures[handle]
                    .as_ref()
                    .ok_or(FlowError::InvalidBytecode)?;
                live_captures.extend(closure.captures.iter().map(|&(_, cell)| cell));
            }
        }
        // Sweep only after all roots have been traced within the budget.
        for (handle, live) in marked_closures.into_iter().enumerate() {
            if !live && self.closures[handle].take().is_some() {
                self.free_closures.push(handle);
            }
        }
        // A cell captured only by dead closures no longer needs capture
        // protection. Retain active stack-frame slots and release orphaned
        // cells, removing them from frame cleanup bookkeeping to avoid a
        // duplicate free when the root frame finally returns.
        let mut active_slots: HashSet<usize> = slots.iter().copied().collect();
        for frame in self.frame_allocations.iter().rev().skip(1) {
            active_slots.extend(frame.iter().copied());
        }
        let abandoned: Vec<usize> = self
            .captured_cells
            .iter()
            .copied()
            .filter(|cell| !live_captures.contains(cell))
            .collect();
        let mut released = HashSet::new();
        for cell in abandoned {
            self.captured_cells.remove(&cell);
            if !active_slots.contains(&cell) {
                *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = None;
                self.free_cells.push(cell);
                released.insert(cell);
            }
        }
        if !released.is_empty() {
            for frame in &mut self.frame_allocations {
                frame.retain(|cell| !released.contains(cell));
            }
        }
        for (handle, live) in marked_tables.into_iter().enumerate() {
            if !live && !self.retired_tables.contains(&handle) {
                let table = &mut self.tables[handle];
                self.table_cells -= table.array.len() + table.hashed.len();
                *table = TableStorage {
                    array: Vec::new(),
                    hashed: HashMap::new(),
                    metatable: None,
                };
                self.retired_tables.insert(handle);
                self.free_tables.push(handle);
                self.gc_finalized.remove(&handle);
                self.gc_registrations.retain(|&item| item != handle);
            }
        }
        Ok(())
    }

    /// Lazy standard-library tables must share the constructor's recycled
    /// table-slot path. An exhausted pool may contain unreachable tables, or
    /// already-recycled slots from a manual collection while GC is stopped.
    fn reserve_lazy_table_slot(&mut self, pending: &[Value]) -> Result<(), FlowError> {
        if self.tables.len() >= MAX_TABLES && self.free_tables.is_empty() {
            if self.automatic_gc {
                let frame = self
                    .gc_frame_roots
                    .last()
                    .ok_or(FlowError::InvalidBytecode)?;
                let mut roots = frame.stack.clone();
                roots.extend_from_slice(pending);
                let slots = frame.slots.clone();
                let prepared = frame.prepared.clone();
                self.collect_root_tables(&roots, &slots, &prepared)?;
            }
            if self.tables.len() >= MAX_TABLES && self.free_tables.is_empty() {
                return Err(FlowError::TooLarge);
            }
        }
        Ok(())
    }

    fn insert_table(&mut self, table: TableStorage) -> Result<usize, FlowError> {
        if let Some(handle) = self.free_tables.pop() {
            self.gc_finalized.remove(&handle);
            self.gc_registrations.retain(|&item| item != handle);
            if !self.retired_tables.remove(&handle) {
                return Err(FlowError::InvalidBytecode);
            }
            *self
                .tables
                .get_mut(handle)
                .ok_or(FlowError::InvalidBytecode)? = table;
            return Ok(handle);
        }
        if self.tables.len() >= MAX_TABLES {
            return Err(FlowError::TooLarge);
        }
        let handle = self.tables.len();
        self.tables.push(table);
        Ok(handle)
    }

    /// Approximate retained storage in the bounded owned heap. Lua's count
    /// reports allocator usage; this independently managed VM can report only
    /// its own primary table, closure, lexical-cell and string pools. Keep the
    /// result numeric and responsive to reclamation, without claiming
    /// byte-for-byte parity with an upstream Lua allocator.
    fn tracked_heap_kib(&self) -> f64 {
        use std::mem::size_of;

        let mut bytes = self
            .tables
            .capacity()
            .saturating_mul(size_of::<TableStorage>());
        bytes = bytes.saturating_add(
            self.closures
                .capacity()
                .saturating_mul(size_of::<Option<Closure>>()),
        );
        bytes = bytes.saturating_add(
            self.cells
                .capacity()
                .saturating_mul(size_of::<Option<Value>>()),
        );
        let string_slots = self.strings.capacity().saturating_mul(size_of::<Vec<u8>>());
        bytes = bytes.saturating_add(string_slots);
        for table in &self.tables {
            bytes = bytes.saturating_add(
                table
                    .array
                    .capacity()
                    .saturating_mul(size_of::<Option<Value>>()),
            );
            bytes = bytes.saturating_add(
                table
                    .hashed
                    .capacity()
                    .saturating_mul(size_of::<(Value, Value)>()),
            );
        }
        for closure in self.closures.iter().flatten() {
            let captures = closure
                .captures
                .capacity()
                .saturating_mul(size_of::<(usize, usize)>());
            bytes = bytes.saturating_add(captures);
        }
        for string in &self.strings {
            bytes = bytes.saturating_add(string.capacity());
        }
        bytes = bytes.saturating_add(self.gmatch_bytes);
        bytes = bytes.saturating_add(
            self.gmatch_states
                .capacity()
                .saturating_mul(size_of::<Option<GmatchState>>()),
        );
        bytes as f64 / 1024.0
    }

    /// Lua 5.4 collector controls available in the owned bounded preview:
    /// explicit full cycles, stop/restart of automatic quota collection,
    /// querying automatic-collection state, and estimated managed-heap size.
    fn collectgarbage_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        let mode = match args.first().copied() {
            None => b"collect".to_vec(),
            Some(Value::String(handle)) => self
                .strings
                .get(handle)
                .ok_or(FlowError::InvalidBytecode)?
                .clone(),
            Some(_) => return Err(FlowError::TypeError),
        };
        match mode.as_slice() {
            b"count" => Ok(Value::Float(self.tracked_heap_kib().to_bits())),
            b"isrunning" => Ok(Value::Boolean(self.automatic_gc)),
            b"stop" => {
                self.automatic_gc = false;
                Ok(Value::Integer(0))
            }
            b"restart" => {
                self.automatic_gc = true;
                Ok(Value::Integer(0))
            }
            b"collect" => {
                // Lua does not run a nested collector cycle from within a
                // finalizer. Suppress the request without revisiting heap
                // objects while their finalizer callbacks are executing.
                if self.running_finalizers {
                    // lua_gc refuses to recurse from a finalizer. Lua's
                    // collectgarbage wrapper then produces no results;
                    // this scalar builtin path represents that as nil.
                    return Ok(Value::Nil);
                }
                // Explicit full passes run even while automatic GC is
                // stopped; snapshots preserve roots of suspended callers.
                let frame = self
                    .gc_frame_roots
                    .last()
                    .ok_or(FlowError::InvalidBytecode)?;
                let slots = frame.slots.clone();
                let prepared = frame.prepared.clone();
                self.collect_root_tables(args, &slots, &prepared)?;
                Ok(Value::Integer(0))
            }
            _ => Err(FlowError::Unsupported(
                "owned collector supports collect, count, stop, restart and isrunning".into(),
            )),
        }
    }

    /// Validate Lua math arguments without losing the first-party error value.
    /// luaL_checknumber/checkinteger distinguish missing and wrong-typed inputs
    /// from numeric values that cannot be represented as integers.
    fn checked_math_number(
        &mut self,
        function: &str,
        args: &[Value],
        index: usize,
        integral: bool,
    ) -> Result<Value, FlowError> {
        let parsed = args
            .get(index)
            .copied()
            .and_then(|value| self.arithmetic_operand(value).ok());
        if let Some(number) = parsed {
            if !integral {
                return Ok(number);
            }
            if let Ok(integer) = self.exact_integer_operand(number) {
                return Ok(Value::Integer(integer));
            }
        }
        let detail = if integral && parsed.is_some() {
            "number has no integer representation".to_string()
        } else {
            let kind = match args.get(index).copied() {
                None => "no value",
                Some(Value::Nil) => "nil",
                Some(Value::Boolean(_)) => "boolean",
                Some(Value::String(_)) => "string",
                Some(Value::Table(_)) => "table",
                Some(Value::Function(_) | Value::Builtin(_)) => "function",
                Some(Value::Integer(_) | Value::Float(_)) => "number",
            };
            format!("number expected, got {kind}")
        };
        let message = format!("bad argument #{} to '{function}' ({detail})", index + 1);
        let value = self.intern_rooted_string(message.into_bytes(), args)?;
        Err(FlowError::Thrown(value))
    }

    /// Mimic the Lua 5.4 default weak seed without importing random crates.
    /// This generator is explicitly not a cryptographic source.
    fn weak_random_seeds(&self) -> (u64, u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let second = (self as *const _ as usize as u64)
            ^ ((now.subsec_nanos() as u64) << 32)
            ^ (std::process::id() as u64);
        (now.as_secs(), second)
    }

    /// Every rejection draw is charged to the shared owned VM budget.
    fn draw_random(&mut self) -> Result<u64, FlowError> {
        if self.steps >= self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += 1;
        Ok(lua_random_next(&mut self.random_state))
    }

    /// Lua math.random uses unbiased rejection sampling rather than modulo.
    fn math_random_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        let raw = self.draw_random()?;
        if args.is_empty() {
            let fraction = ((raw >> 11) as f64) * (1.0 / 9007199254740992.0);
            return Ok(Value::Float(fraction.to_bits()));
        }
        if args.len() > 2 {
            let message = self.intern_rooted_string(b"wrong number of arguments".to_vec(), args)?;
            return Err(FlowError::Thrown(message));
        }
        let (low, up) = if args.len() == 1 {
            let Value::Integer(up) = self.checked_math_number("math.random", args, 0, true)? else {
                return Err(FlowError::InvalidBytecode);
            };
            if up == 0 {
                return Ok(Value::Integer(raw as i64));
            }
            (1_i64, up)
        } else {
            let Value::Integer(low) = self.checked_math_number("math.random", args, 0, true)?
            else {
                return Err(FlowError::InvalidBytecode);
            };
            let Value::Integer(up) = self.checked_math_number("math.random", args, 1, true)? else {
                return Err(FlowError::InvalidBytecode);
            };
            (low, up)
        };
        if low > up {
            let message = self.intern_rooted_string(
                b"bad argument #1 to 'math.random' (interval is empty)".to_vec(),
                args,
            )?;
            return Err(FlowError::Thrown(message));
        }
        let range = (up as u64).wrapping_sub(low as u64);
        let sample = if range & range.wrapping_add(1) == 0 {
            raw & range
        } else {
            let mut mask = range;
            mask |= mask >> 1;
            mask |= mask >> 2;
            mask |= mask >> 4;
            mask |= mask >> 8;
            mask |= mask >> 16;
            mask |= mask >> 32;
            let mut candidate = raw;
            loop {
                let projected = candidate & mask;
                if projected <= range {
                    break projected;
                }
                candidate = self.draw_random()?;
            }
        };
        Ok(Value::Integer((sample.wrapping_add(low as u64)) as i64))
    }

    /// Both seed components are observable Lua return values.
    fn math_randomseed_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let (first, second) = if args.is_empty() {
            let (first, second) = self.weak_random_seeds();
            (first as i64, second as i64)
        } else {
            let Value::Integer(first) =
                self.checked_math_number("math.randomseed", args, 0, true)?
            else {
                return Err(FlowError::InvalidBytecode);
            };
            let second = if args.get(1).is_some_and(|v| *v != Value::Nil) {
                let Value::Integer(second) =
                    self.checked_math_number("math.randomseed", args, 1, true)?
                else {
                    return Err(FlowError::InvalidBytecode);
                };
                second
            } else {
                0
            };
            (first, second)
        };
        lua_random_seed(&mut self.random_state, first as u64, second as u64);
        Ok(vec![Value::Integer(first), Value::Integer(second)])
    }

    /// A small first-party Lua 5.4 numeric library. Unsupported math
    /// functions remain absent rather than falling back to another runtime.
    fn math_builtin(&mut self, builtin: Builtin, args: &[Value]) -> Result<Value, FlowError> {
        let first = args.first().copied().unwrap_or(Value::Nil);
        match builtin {
            Builtin::MathType => {
                // Lua checks that argument #1 is present, but accepts nil
                // as a supplied value and returns nil for its numeric type.
                let value = *args.first().ok_or(FlowError::TypeError)?;
                match value {
                    Value::Integer(_) => self.intern_rooted_string(b"integer".to_vec(), args),
                    Value::Float(_) => self.intern_rooted_string(b"float".to_vec(), args),
                    _ => Ok(Value::Nil),
                }
            }
            Builtin::MathToInteger => {
                // Missing argument is an error; explicitly passed nil is not.
                let first = *args.first().ok_or(FlowError::TypeError)?;
                // Lua's lua_tointegerx also recognizes hexadecimal numeric
                // strings, including wrapped 64-bit integer spellings.
                let operand = self.lua_numeric_operand(first);
                Ok(operand
                    .ok()
                    .and_then(|number| self.exact_integer_operand(number).ok())
                    .map(Value::Integer)
                    .unwrap_or(Value::Nil))
            }
            // Lua's square root always returns a floating-point value, even
            // for an exact integer input. Preserve IEEE-754 NaN/inf results.
            Builtin::MathSqrt => {
                let number = match self.checked_math_number("math.sqrt", args, 0, false)? {
                    Value::Integer(number) => number as f64,
                    Value::Float(bits) => f64::from_bits(bits),
                    _ => return Err(FlowError::InvalidBytecode),
                };
                Ok(Value::Float(number.sqrt().to_bits()))
            }
            // luaL_checkinteger accepts exactly integral floats and numeric
            // strings; unsigned ordering uses the full 64-bit Lua integer.
            Builtin::MathUlt => {
                let left = self.checked_math_number("math.ult", args, 0, true)?;
                let right = self.checked_math_number("math.ult", args, 1, true)?;
                let (Value::Integer(left), Value::Integer(right)) = (left, right) else {
                    return Err(FlowError::InvalidBytecode);
                };
                Ok(Value::Boolean((left as u64) < (right as u64)))
            }
            Builtin::MathFmod => {
                let left = self.checked_math_number("math.fmod", args, 0, false)?;
                let right = self.checked_math_number("math.fmod", args, 1, false)?;
                // Lua selects integer remainder only when *both original
                // arguments* are integers; numeric strings use the float path.
                if let (Some(Value::Integer(a)), Some(Value::Integer(b))) =
                    (args.first().copied(), args.get(1).copied())
                {
                    if b == 0 {
                        let error = self.intern_rooted_string(
                            b"bad argument #2 to 'math.fmod' (zero)".to_vec(),
                            args,
                        )?;
                        return Err(FlowError::Thrown(error));
                    }
                    return Ok(Value::Integer(if b == -1 { 0 } else { a % b }));
                }
                let number = |value: Value| match value {
                    Value::Integer(n) => n as f64,
                    Value::Float(bits) => f64::from_bits(bits),
                    _ => unreachable!("checked Lua math number"),
                };
                Ok(Value::Float((number(left) % number(right)).to_bits()))
            }
            Builtin::MathSin
            | Builtin::MathCos
            | Builtin::MathTan
            | Builtin::MathRad
            | Builtin::MathDeg => {
                let function = match builtin {
                    Builtin::MathSin => "math.sin",
                    Builtin::MathCos => "math.cos",
                    Builtin::MathTan => "math.tan",
                    Builtin::MathRad => "math.rad",
                    Builtin::MathDeg => "math.deg",
                    _ => return Err(FlowError::InvalidBytecode),
                };
                let operand = self.checked_math_number(function, args, 0, false)?;
                let value = match operand {
                    Value::Integer(integer) => integer as f64,
                    Value::Float(bits) => f64::from_bits(bits),
                    _ => return Err(FlowError::InvalidBytecode),
                };
                let result = match builtin {
                    Builtin::MathSin => value.sin(),
                    Builtin::MathCos => value.cos(),
                    Builtin::MathTan => value.tan(),
                    Builtin::MathRad => value * (std::f64::consts::PI / 180.0),
                    Builtin::MathDeg => value * (180.0 / std::f64::consts::PI),
                    _ => return Err(FlowError::InvalidBytecode),
                };
                Ok(Value::Float(result.to_bits()))
            }

            Builtin::MathRandom => self.math_random_builtin(args),
            Builtin::MathExp
            | Builtin::MathLog
            | Builtin::MathAsin
            | Builtin::MathAcos
            | Builtin::MathAtan => {
                let function = match builtin {
                    Builtin::MathExp => "math.exp",
                    Builtin::MathLog => "math.log",
                    Builtin::MathAsin => "math.asin",
                    Builtin::MathAcos => "math.acos",
                    Builtin::MathAtan => "math.atan",
                    _ => return Err(FlowError::InvalidBytecode),
                };
                let input = self.checked_math_number(function, args, 0, false)?;
                let number = match input {
                    Value::Integer(number) => number as f64,
                    Value::Float(bits) => f64::from_bits(bits),
                    _ => return Err(FlowError::InvalidBytecode),
                };
                let second = match builtin {
                    Builtin::MathAtan if args.get(1).is_some_and(|v| *v != Value::Nil) => {
                        Some(self.checked_math_number(function, args, 1, false)?)
                    }
                    Builtin::MathLog if args.get(1).is_some_and(|v| *v != Value::Nil) => {
                        Some(self.checked_math_number(function, args, 1, false)?)
                    }
                    _ => None,
                };
                let second = second.map(|number| match number {
                    Value::Integer(value) => value as f64,
                    Value::Float(bits) => f64::from_bits(bits),
                    _ => unreachable!("checked Lua math number"),
                });
                let result = match builtin {
                    Builtin::MathExp => number.exp(),
                    Builtin::MathAsin => number.asin(),
                    Builtin::MathAcos => number.acos(),
                    Builtin::MathAtan => number.atan2(second.unwrap_or(1.0)),
                    Builtin::MathLog => match second {
                        None => number.ln(),
                        Some(2.0) => number.log2(),
                        Some(10.0) => number.log10(),
                        Some(base) => number.ln() / base.ln(),
                    },
                    _ => return Err(FlowError::InvalidBytecode),
                };
                Ok(Value::Float(result.to_bits()))
            }
            Builtin::MathAbs => match self.arithmetic_operand(first)? {
                Value::Integer(number) => Ok(Value::Integer(number.wrapping_abs())),
                Value::Float(bits) => Ok(Value::Float(f64::from_bits(bits).abs().to_bits())),
                _ => Err(FlowError::TypeError),
            },
            Builtin::MathFloor | Builtin::MathCeil => {
                let input = self.arithmetic_operand(first)?;
                let Value::Float(bits) = input else {
                    return Ok(input);
                };
                let number = f64::from_bits(bits);
                let rounded = if builtin == Builtin::MathFloor {
                    number.floor()
                } else {
                    number.ceil()
                };
                if rounded.is_finite()
                    && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0)
                        .contains(&rounded)
                {
                    Ok(Value::Integer(rounded as i64))
                } else {
                    Ok(Value::Float(rounded.to_bits()))
                }
            }
            Builtin::MathMax | Builtin::MathMin => {
                // Lua 5.4 uses lua_compare(L, best, candidate, LUA_OPLT)
                // for max and its reversed operands for min. This includes
                // table __lt callbacks, not only primitive numeric/string
                // ordering. Comparisons are strict: ties retain the first
                // argument. Roots retain all operands across callbacks that
                // can run the collector or mutate their source tables.
                let mut best = *args.first().ok_or(FlowError::TypeError)?;
                self.gc_temporary_roots.push(args.to_vec());
                let result = (|| {
                    for &value in &args[1..] {
                        let replace = if builtin == Builtin::MathMax {
                            self.table_sort_less(best, value, None)?
                        } else {
                            self.table_sort_less(value, best, None)?
                        };
                        if replace {
                            best = value;
                        }
                    }
                    Ok(best)
                })();
                self.gc_temporary_roots.pop();
                result
            }
            _ => Err(FlowError::InvalidBytecode),
        }
    }

    /// Lua math.modf returns two values. Preserve integer-valued whole
    /// parts when representable and always return a floating fractional part.
    fn math_modf_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let number = self.checked_math_number("math.modf", args, 0, false)?;
        if let Value::Integer(integer) = number {
            return Ok(vec![
                Value::Integer(integer),
                Value::Float(0.0_f64.to_bits()),
            ]);
        }
        let Value::Float(bits) = number else {
            return Err(FlowError::InvalidBytecode);
        };
        let value = f64::from_bits(bits);
        let whole = value.trunc();
        // C modf preserves the sign of zero even for exact negative inputs,
        // and returns signed zero rather than NaN for infinite inputs.
        let fraction = if value.is_infinite() || whole == value {
            0.0_f64.copysign(value)
        } else {
            value - whole
        };
        let whole = match self.exact_integer_operand(Value::Float(whole.to_bits())) {
            Ok(integer) => Value::Integer(integer),
            Err(_) => Value::Float(whole.to_bits()),
        };
        Ok(vec![whole, Value::Float(fraction.to_bits())])
    }

    /// Create one bounded numeric-library table for this execution.
    fn materialize_math_table(&mut self, pending: &[Value]) -> Result<usize, FlowError> {
        if let Some(handle) = self.math_table {
            return Ok(handle);
        }
        self.reserve_lazy_table_slot(pending)?;
        let handle = self.insert_table(TableStorage {
            array: Vec::new(),
            hashed: HashMap::new(),
            metatable: None,
        })?;
        self.math_table = Some(handle);
        for &(name, builtin) in MATH_GLOBALS {
            let key = self.intern_rooted_string(name.to_vec(), pending)?;
            self.write_table(handle, TableKey::Hashed(key), Value::Builtin(builtin))?;
        }
        for (name, value) in [
            (
                b"pi".as_slice(),
                Value::Float(std::f64::consts::PI.to_bits()),
            ),
            (b"huge".as_slice(), Value::Float(f64::INFINITY.to_bits())),
            (b"maxinteger".as_slice(), Value::Integer(i64::MAX)),
            (b"mininteger".as_slice(), Value::Integer(i64::MIN)),
        ] {
            let key = self.intern_rooted_string(name.to_vec(), pending)?;
            self.write_table(handle, TableKey::Hashed(key), value)?;
        }
        Ok(handle)
    }

    /// Create one bounded writable Lua string library table per execution.
    fn materialize_string_table(&mut self, pending: &[Value]) -> Result<usize, FlowError> {
        if let Some(handle) = self.string_table {
            return Ok(handle);
        }
        self.reserve_lazy_table_slot(pending)?;
        let handle = self.insert_table(TableStorage {
            array: Vec::new(),
            hashed: HashMap::new(),
            metatable: None,
        })?;
        // Keep the partial table rooted when string interning triggers GC.
        self.string_table = Some(handle);
        for &(name, builtin) in STRING_GLOBALS {
            let key = self.intern_rooted_string(name.to_vec(), pending)?;
            self.write_table(handle, TableKey::Hashed(key), Value::Builtin(builtin))?;
        }
        Ok(handle)
    }

    /// Lazily root a mutable Lua 5.4.6 byte-oriented UTF-8 table.
    fn materialize_coroutine_table(&mut self, pending: &[Value]) -> Result<usize, FlowError> {
        if let Some(handle) = self.coroutine_table {
            return Ok(handle);
        }
        self.reserve_lazy_table_slot(pending)?;
        let handle = self.insert_table(TableStorage {
            array: Vec::new(),
            hashed: HashMap::new(),
            metatable: None,
        })?;
        self.coroutine_table = Some(handle);
        for &(name, builtin) in COROUTINE_GLOBALS {
            let key = self.intern_rooted_string(name.to_vec(), pending)?;
            self.write_table(handle, TableKey::Hashed(key), Value::Builtin(builtin))?;
        }
        Ok(handle)
    }

    fn materialize_utf8_table(&mut self, pending: &[Value]) -> Result<usize, FlowError> {
        if let Some(handle) = self.utf8_table {
            return Ok(handle);
        }
        self.reserve_lazy_table_slot(pending)?;
        let handle = self.insert_table(TableStorage {
            array: Vec::new(),
            hashed: HashMap::new(),
            metatable: None,
        })?;
        self.utf8_table = Some(handle);
        for &(name, builtin) in UTF8_GLOBALS {
            let key = self.intern_rooted_string(name.to_vec(), pending)?;
            self.write_table(handle, TableKey::Hashed(key), Value::Builtin(builtin))?;
        }
        // Matches lutf8lib.c's literal charpattern byte range, including NUL.
        let pattern =
            self.intern_rooted_string(b"[\0-\x7F\xC2-\xFD][\x80-\xBF]*".to_vec(), pending)?;
        let key = self.intern_rooted_string(b"charpattern".to_vec(), &[pattern])?;
        self.write_table(handle, TableKey::Hashed(key), pattern)?;
        Ok(handle)
    }

    /// Create the bounded table library only when first accessed. Root its
    /// partially populated table while interning builtin field-name strings.
    fn materialize_table_library(&mut self, pending: &[Value]) -> Result<usize, FlowError> {
        if let Some(handle) = self.table_library {
            return Ok(handle);
        }
        self.reserve_lazy_table_slot(pending)?;
        let handle = self.insert_table(TableStorage {
            array: Vec::new(),
            hashed: HashMap::new(),
            metatable: None,
        })?;
        self.table_library = Some(handle);
        for &(name, builtin) in TABLE_GLOBALS {
            let key = self.intern_rooted_string(name.to_vec(), pending)?;
            self.write_table(handle, TableKey::Hashed(key), Value::Builtin(builtin))?;
        }
        Ok(handle)
    }

    /// Lazily expose the real global environment as a writable Lua table.
    /// Prior global overrides are folded in before the table becomes visible.
    fn materialize_global_table(&mut self) -> Result<usize, FlowError> {
        if let Some(handle) = self.global_table {
            return Ok(handle);
        }
        self.reserve_lazy_table_slot(&[])?;
        let handle = self.insert_table(TableStorage {
            array: Vec::new(),
            hashed: HashMap::new(),
            metatable: None,
        })?;
        self.global_table = Some(handle);
        for &(name, builtin) in BUILTIN_GLOBALS {
            let key = self.intern_rooted_string(name.to_vec(), &[])?;
            self.write_table(handle, TableKey::Hashed(key), Value::Builtin(builtin))?;
        }
        let version = self.intern_rooted_string(b"_VERSION".to_vec(), &[])?;
        let number = self.intern_rooted_string(b"Lua 5.4".to_vec(), &[version])?;
        self.write_table(handle, TableKey::Hashed(version), number)?;
        let self_key = self.intern_rooted_string(b"_G".to_vec(), &[])?;
        self.write_table(handle, TableKey::Hashed(self_key), Value::Table(handle))?;
        let arg_key = self.intern_rooted_string(b"arg".to_vec(), &[])?;
        let arguments = self.argument_table(&[arg_key])?;
        self.write_table(handle, TableKey::Hashed(arg_key), arguments)?;
        let math_key = self.intern_rooted_string(b"math".to_vec(), &[])?;
        let math = self.materialize_math_table(&[math_key])?;
        self.write_table(handle, TableKey::Hashed(math_key), Value::Table(math))?;
        let string_key = self.intern_rooted_string(b"string".to_vec(), &[])?;
        let string = self.materialize_string_table(&[string_key])?;
        self.write_table(handle, TableKey::Hashed(string_key), Value::Table(string))?;
        let utf8_key = self.intern_rooted_string(b"utf8".to_vec(), &[])?;
        let utf8 = self.materialize_utf8_table(&[utf8_key])?;
        self.write_table(handle, TableKey::Hashed(utf8_key), Value::Table(utf8))?;
        let table_key = self.intern_rooted_string(b"table".to_vec(), &[])?;
        let table = self.materialize_table_library(&[table_key])?;
        self.write_table(handle, TableKey::Hashed(table_key), Value::Table(table))?;
        let co_key = self.intern_rooted_string(b"coroutine".to_vec(), &[])?;
        let coroutines = self.materialize_coroutine_table(&[co_key])?;
        self.write_table(handle, TableKey::Hashed(co_key), Value::Table(coroutines))?;
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
            return self.argument_table(&[]);
        }
        if name == b"math" {
            return Ok(Value::Table(self.materialize_math_table(&[])?));
        }
        if name == b"string" {
            return Ok(Value::Table(self.materialize_string_table(&[])?));
        }
        if name == b"utf8" {
            return Ok(Value::Table(self.materialize_utf8_table(&[])?));
        }
        if name == b"table" {
            return Ok(Value::Table(self.materialize_table_library(&[])?));
        }
        if name == b"coroutine" {
            return Ok(Value::Table(self.materialize_coroutine_table(&[])?));
        }
        if name == b"_VERSION" {
            return self.intern_rooted_string(b"Lua 5.4".to_vec(), &[]);
        }
        Ok(Value::Nil)
    }

    /// Materialize the standard Lua argv table only when referenced. Repeated
    /// global lookups and calls share the same writable table identity.
    fn argument_table(&mut self, pending: &[Value]) -> Result<Value, FlowError> {
        if let Some(handle) = self.arg_handle {
            return Ok(Value::Table(handle));
        }
        if self.script_args.len() > MAX_TABLE_FIELDS {
            return Err(FlowError::TooLarge);
        }
        self.reserve_lazy_table_slot(pending)?;
        let name = self.intern_rooted_string(self.script_name.as_bytes().to_vec(), pending)?;
        let handle = self.insert_table(TableStorage {
            array: Vec::new(),
            hashed: HashMap::new(),
            metatable: None,
        })?;
        // Root the partial arg table before allocating further strings.
        self.arg_handle = Some(handle);
        self.write_table(handle, TableKey::Hashed(Value::Integer(0)), name)?;
        for (index, argument) in self.script_args.iter().enumerate() {
            let text = self.intern_rooted_string(argument.as_bytes().to_vec(), pending)?;
            self.write_table(handle, TableKey::Positive(index + 1), text)?;
        }
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
        if self.table_finalizer(handle)?.is_some() {
            if self.gc_finalized.remove(&handle) {
                // Lua allows a finalizing or resurrected object to be
                // registered again for a later collection. Reinsert at the
                // newest registration position; the active pass already
                // captured its pending list, so it cannot finalize twice
                // within one collection or shutdown snapshot.
                self.gc_registrations.retain(|&item| item != handle);
                self.gc_registrations.push(handle);
            } else if !self.gc_registrations.contains(&handle) {
                self.gc_registrations.push(handle);
            }
        }
        Ok(Value::Table(handle))
    }

    fn tonumber_builtin(&self, args: &[Value]) -> Result<Value, FlowError> {
        // Lua's luaL_checkany rejects a missing argument, while an
        // explicitly supplied nil is valid and converts to nil.
        let value = *args.first().ok_or(FlowError::TypeError)?;
        if let Some(base_value) = args.get(1).copied().filter(|arg| *arg != Value::Nil) {
            // luaL_checkinteger accepts numeric-string bases when they
            // represent exact integers, just as it accepts integer floats.
            let base = self.exact_integer_operand(self.lua_numeric_operand(base_value)?)?;
            if !(2..=36).contains(&base) {
                return Err(FlowError::TypeError);
            }
            let Value::String(handle) = value else {
                return Err(FlowError::TypeError);
            };
            let bytes = self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
            let Ok(source) = std::str::from_utf8(bytes) else {
                return Ok(Value::Nil);
            };
            let source = trim_lua_numeric_space(source);
            let (negative, unsigned) = if let Some(rest) = source.strip_prefix('-') {
                (true, rest)
            } else {
                (false, source.strip_prefix('+').unwrap_or(source))
            };
            // Lua's b_str2int accumulates as lua_Unsigned and wraps at
            // 64 bits, even for a non-hexadecimal base and a negative sign.
            // Standard u64 parsing rejects the high-bit and overflow cases.
            if unsigned.is_empty() {
                return Ok(Value::Nil);
            }
            let mut number = 0u64;
            for byte in unsigned.bytes() {
                let Some(digit) = (byte as char).to_digit(base as u32) else {
                    return Ok(Value::Nil);
                };
                number = number
                    .wrapping_mul(base as u64)
                    .wrapping_add(u64::from(digit));
            }
            return Ok(Value::Integer(if negative {
                number.wrapping_neg() as i64
            } else {
                number as i64
            }));
        }
        match value {
            Value::Integer(_) | Value::Float(_) => Ok(value),
            Value::String(_) => Ok(self.lua_numeric_operand(value).unwrap_or(Value::Nil)),
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
            Ok(bytes) => self.intern_rooted_string(bytes, &[left, right]),
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
            None => self.intern_rooted_string(b"assertion failed!".to_vec(), args)?,
        };
        Ok(FlowError::Thrown(message))
    }

    /// Shared stringification for ordinary and protected builtin calls.
    /// Metamethod callbacks remain subject to the owned VM call-depth guard.
    fn tostring_builtin(&mut self, value: Value, roots: &[Value]) -> Result<Value, FlowError> {
        let text = match value {
            Value::String(_) => value,
            Value::Integer(number) => {
                self.intern_rooted_string(number.to_string().into_bytes(), roots)?
            }
            Value::Float(bits) => self
                .intern_rooted_string(format!("{:?}", f64::from_bits(bits)).into_bytes(), roots)?,
            Value::Boolean(boolean) => {
                self.intern_rooted_string(boolean.to_string().into_bytes(), roots)?
            }
            Value::Nil => self.intern_rooted_string(b"nil".to_vec(), roots)?,
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
                            self.intern_rooted_string(number.to_string().into_bytes(), roots)?
                        }
                        Value::Float(bits) => self.intern_rooted_string(
                            format!("{:?}", f64::from_bits(bits)).into_bytes(),
                            roots,
                        )?,
                        _ => return Err(FlowError::TypeError),
                    }
                } else {
                    self.intern_rooted_string(format!("table: 0x{handle:x}").into_bytes(), roots)?
                }
            }
            Value::Function(handle) => {
                self.intern_rooted_string(format!("function: 0x{handle:x}").into_bytes(), roots)?
            }
            Value::Builtin(_) => self.intern_rooted_string(b"function: builtin".to_vec(), roots)?,
        };
        Ok(text)
    }

    /// A bounded, host-independent implementation of Lua's print protocol.
    /// Lua 5.4 intentionally ignores overrides of the global tostring function.
    /// Conversion uses __tostring metamethods and shared VM safety limits.
    fn print_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let mut line = Vec::new();
        for (index, &argument) in args.iter().enumerate() {
            if index > 0 {
                line.push(b'\t');
            }
            let converted = self.tostring_builtin(argument, args)?;
            let Value::String(handle) = converted else {
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
            Value::Builtin(Builtin::Warn) => self.warn_builtin(&arguments),
            Value::Builtin(Builtin::CollectGarbage) => {
                Ok(vec![self.collectgarbage_builtin(&arguments)?])
            }
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
                Ok(vec![self.intern_rooted_string(name.to_vec(), &arguments)?])
            }
            Value::Builtin(Builtin::ToNumber) => {
                if arguments.is_empty() {
                    return Err(FlowError::TypeError);
                }
                Ok(vec![self.tonumber_builtin(&arguments)?])
            }
            Value::Builtin(Builtin::ToString) => {
                let argument = *arguments.first().ok_or(FlowError::TypeError)?;
                Ok(vec![self.tostring_builtin(argument, &arguments)?])
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
            // Multi-result math.modf must precede the generic math guard.
            Value::Builtin(Builtin::MathModf) => self.math_modf_builtin(&arguments),
            Value::Builtin(Builtin::MathRandomSeed) => self.math_randomseed_builtin(&arguments),
            Value::Builtin(builtin) if MATH_GLOBALS.iter().any(|&(_, item)| item == builtin) => {
                Ok(vec![self.math_builtin(builtin, &arguments)?])
            }
            Value::Builtin(Builtin::Utf8Char) => Ok(vec![self.utf8_char_builtin(&arguments)?]),
            Value::Builtin(Builtin::Utf8Len) => self.utf8_len_builtin(&arguments),
            Value::Builtin(Builtin::Utf8Codepoint) => self.utf8_codepoint_builtin(&arguments),
            Value::Builtin(Builtin::Utf8Offset) => Ok(vec![self.utf8_offset_builtin(&arguments)?]),
            Value::Builtin(Builtin::Utf8Codes) => self.utf8_codes_builtin(&arguments),
            Value::Builtin(Builtin::Utf8CodesIteratorStrict) => {
                self.utf8_codes_iterator(&arguments, true)
            }
            Value::Builtin(Builtin::Utf8CodesIteratorLax) => {
                self.utf8_codes_iterator(&arguments, false)
            }
            Value::Builtin(Builtin::StringByte) => self.string_byte_builtin(&arguments),
            Value::Builtin(Builtin::StringFind) => self.string_find_literal_builtin(&arguments),
            Value::Builtin(Builtin::StringMatch) => self.string_match_builtin(&arguments),
            Value::Builtin(Builtin::StringGmatch) => {
                Ok(vec![self.string_gmatch_builtin(&arguments)?])
            }
            Value::Builtin(Builtin::StringGmatchIterator(handle)) => {
                self.string_gmatch_iterator(handle, &arguments)
            }
            Value::Builtin(Builtin::StringGsub) => self.string_gsub_builtin(&arguments),
            Value::Builtin(
                Builtin::StringPack | Builtin::StringUnpack | Builtin::StringPackSize,
            ) => {
                let Value::Builtin(builtin) = callable else {
                    unreachable!()
                };
                self.string_pack_dispatch(builtin, &arguments)
            }
            Value::Builtin(Builtin::TableUnpack) => self.table_unpack_builtin(&arguments),
            Value::Builtin(Builtin::TablePack) => Ok(vec![self.table_pack_builtin(&arguments)?]),
            Value::Builtin(Builtin::TableInsert) => self.table_insert_builtin(&arguments),
            Value::Builtin(Builtin::TableRemove) => {
                Ok(vec![self.table_remove_builtin(&arguments)?])
            }
            Value::Builtin(Builtin::TableMove) => Ok(vec![self.table_move_builtin(&arguments)?]),
            Value::Builtin(Builtin::TableSort) => self.table_sort_builtin(&arguments),
            Value::Builtin(Builtin::TableConcat) => {
                Ok(vec![self.table_concat_builtin(&arguments)?])
            }
            Value::Builtin(builtin) if STRING_GLOBALS.iter().any(|&(_, item)| item == builtin) => {
                Ok(vec![self.string_builtin(builtin, &arguments)?])
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

    fn protected_error_value(
        &mut self,
        error: FlowError,
        roots: &[Value],
    ) -> Result<Value, FlowError> {
        match error {
            FlowError::Thrown(value) => Ok(value),
            other => self.intern_rooted_string(other.to_string().into_bytes(), roots),
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
                self.protected_error_value(error, args)?,
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
                let mut message = self.protected_error_value(error, args)?;
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
                        Err(error) => {
                            let mut roots = args.to_vec();
                            roots.push(message);
                            message = self.protected_error_value(error, &roots)?;
                        }
                    }
                }
                Ok(vec![
                    Value::Boolean(false),
                    self.intern_rooted_string(b"error in error handling".to_vec(), args)?,
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

    /// A bounded sequence range. Lua 5.4 table.concat evaluates luaL_len
    /// eagerly (even for an explicit end); table.unpack evaluates it only
    /// when its end index is omitted or nil. Both honor __len, while indexed
    /// reads still use the bounded raw table subset.
    fn table_library_range(
        &mut self,
        args: &[Value],
        start_argument: usize,
        end_argument: usize,
        eager_length: bool,
    ) -> Result<(Value, i64, usize), FlowError> {
        let target = *args.first().ok_or(FlowError::TypeError)?;
        if !matches!(target, Value::Table(_) | Value::String(_)) {
            return Err(FlowError::TypeError);
        }
        let default_end = if eager_length {
            let length = self.length_with_metatable(target)?;
            Some(
                self.exact_integer_operand(length)
                    .or_else(|_| self.integer_operand(length))?,
            )
        } else {
            None
        };
        let start = match args.get(start_argument).copied() {
            Some(value) if value != Value::Nil => self
                .exact_integer_operand(value)
                .or_else(|_| self.integer_operand(value))?,
            _ => 1,
        };
        let end = match args.get(end_argument).copied() {
            Some(value) if value != Value::Nil => self
                .exact_integer_operand(value)
                .or_else(|_| self.integer_operand(value))?,
            _ => match default_end {
                Some(length) => length,
                None => {
                    let length = self.length_with_metatable(target)?;
                    self.exact_integer_operand(length)
                        .or_else(|_| self.integer_operand(length))?
                }
            },
        };
        if end < start {
            return Ok((target, start, 0));
        }
        let size = end
            .checked_sub(start)
            .and_then(|width| width.checked_add(1))
            .ok_or(FlowError::TooLarge)?;
        let size = usize::try_from(size).map_err(|_| FlowError::TooLarge)?;
        if size > MAX_STACK {
            return Err(FlowError::TooLarge);
        }
        Ok((target, start, size))
    }

    /// table.pack stores the argument count in n so nil holes and trailing
    /// nils remain observable even when the raw sequence border is shorter.
    /// The partially populated result is pinned during new string interning.
    fn table_pack_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        if args.len() > MAX_TABLE_FIELDS {
            return Err(FlowError::TooLarge);
        }
        if self.steps.saturating_add(args.len()) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += args.len();
        self.reserve_lazy_table_slot(args)?;
        let handle = self.insert_table(TableStorage {
            array: Vec::new(),
            hashed: HashMap::new(),
            metatable: None,
        })?;
        let mut roots = args.to_vec();
        roots.push(Value::Table(handle));
        let name = self.intern_rooted_string(b"n".to_vec(), &roots)?;
        self.write_table(
            handle,
            TableKey::Hashed(name),
            Value::Integer(args.len() as i64),
        )?;
        for (index, &value) in args.iter().enumerate() {
            if value != Value::Nil {
                self.write_table(handle, TableKey::Positive(index + 1), value)?;
            }
        }
        Ok(Value::Table(handle))
    }

    /// table.insert follows Lua's two/three-argument array insertion
    /// conventions. Charge shifting work up front and preserve bounded
    /// table-slot limits instead of implicitly growing an unbounded Vec.
    fn table_insert_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let Value::Table(handle) = *args.first().ok_or(FlowError::TypeError)? else {
            return Err(FlowError::TypeError);
        };
        // Lua's aux_getn observes __len before insertion/removal. Convert
        // integer-compatible results exactly as luaL_len requires.
        let reported_size = self.length_with_metatable(Value::Table(handle))?;
        let size = self
            .exact_integer_operand(reported_size)
            .or_else(|_| self.integer_operand(reported_size))?;
        let last = size.checked_add(1).ok_or(FlowError::TooLarge)?;
        let (position, value) = match args {
            [_, value] => (last, *value),
            [_, position, value] => (
                self.exact_integer_operand(*position)
                    .or_else(|_| self.integer_operand(*position))?,
                *value,
            ),
            _ => return Err(FlowError::TypeError),
        };
        if position < 1 || position > last {
            return Err(FlowError::TypeError);
        }
        if last > MAX_TABLE_FIELDS as i64 {
            return Err(FlowError::TooLarge);
        }
        let work = (last - position + 1) as usize;
        if self.steps.saturating_add(work) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += work;
        // lua_geti/lua_seti honor __index/__newindex. Keep values returned
        // by callbacks rooted across later Lua callbacks and possible GC.
        self.gc_temporary_roots.push(args.to_vec());
        let result = (|| {
            for index in (position..last).rev() {
                let item =
                    self.lookup_with_metatable(handle, self.table_key(Value::Integer(index))?)?;
                self.gc_temporary_roots
                    .last_mut()
                    .ok_or(FlowError::InvalidBytecode)?
                    .push(item);
                self.assign_with_metatable(
                    handle,
                    self.table_key(Value::Integer(index + 1))?,
                    item,
                )?;
            }
            self.assign_with_metatable(handle, self.table_key(Value::Integer(position))?, value)?;
            Ok(Vec::new())
        })();
        self.gc_temporary_roots.pop();
        result
    }

    /// table.remove returns the removed value while shifting subsequent
    /// elements left. Lua permits a no-op removal at size + 1.
    fn table_remove_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        let Value::Table(handle) = *args.first().ok_or(FlowError::TypeError)? else {
            return Err(FlowError::TypeError);
        };
        // Lua's aux_getn observes __len before insertion/removal. Convert
        // integer-compatible results exactly as luaL_len requires.
        let reported_size = self.length_with_metatable(Value::Table(handle))?;
        let size = self
            .exact_integer_operand(reported_size)
            .or_else(|_| self.integer_operand(reported_size))?;
        let position = match args.get(1).copied() {
            Some(value) if value != Value::Nil => self
                .exact_integer_operand(value)
                .or_else(|_| self.integer_operand(value))?,
            _ => size,
        };
        let last = size.checked_add(1).ok_or(FlowError::TooLarge)?;
        if position != size && (position < 1 || position > last) {
            return Err(FlowError::TypeError);
        }
        let work = if position < size {
            (size - position + 1) as usize
        } else {
            1
        };
        if self.steps.saturating_add(work) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += work;
        // lua_geti/lua_seti honor __index/__newindex. Retain the removed
        // value and intermediate callback results across collection.
        self.gc_temporary_roots.push(args.to_vec());
        let result = (|| {
            let removed =
                self.lookup_with_metatable(handle, self.table_key(Value::Integer(position))?)?;
            self.gc_temporary_roots
                .last_mut()
                .ok_or(FlowError::InvalidBytecode)?
                .push(removed);
            for index in position..size {
                let item =
                    self.lookup_with_metatable(handle, self.table_key(Value::Integer(index + 1))?)?;
                self.gc_temporary_roots
                    .last_mut()
                    .ok_or(FlowError::InvalidBytecode)?
                    .push(item);
                self.assign_with_metatable(handle, self.table_key(Value::Integer(index))?, item)?;
            }
            // Lua's loop advances the index to size only when shifting.
            // Removing at size + 1 must clear that absent index, not size.
            let clear_index = if position < size { size } else { position };
            self.assign_with_metatable(
                handle,
                self.table_key(Value::Integer(clear_index))?,
                Value::Nil,
            )?;
            Ok(removed)
        })();
        self.gc_temporary_roots.pop();
        result
    }

    /// Copy a bounded indexed range like Lua 5.4 table.move. A destination
    /// overlapping the source on its right must be copied backward.
    fn table_move_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        let Value::Table(source) = *args.first().ok_or(FlowError::TypeError)? else {
            return Err(FlowError::TypeError);
        };
        let parse_index = |value: Value| {
            self.exact_integer_operand(value)
                .or_else(|_| self.integer_operand(value))
        };
        let from = parse_index(*args.get(1).ok_or(FlowError::TypeError)?)?;
        let end = parse_index(*args.get(2).ok_or(FlowError::TypeError)?)?;
        let start = parse_index(*args.get(3).ok_or(FlowError::TypeError)?)?;
        let target = match args.get(4).copied().unwrap_or(Value::Nil) {
            Value::Nil => source,
            Value::Table(handle) => handle,
            _ => return Err(FlowError::TypeError),
        };
        self.tables.get(source).ok_or(FlowError::InvalidBytecode)?;
        self.tables.get(target).ok_or(FlowError::InvalidBytecode)?;
        if end < from {
            return Ok(Value::Table(target));
        }
        let count = end
            .checked_sub(from)
            .and_then(|n| n.checked_add(1))
            .ok_or(FlowError::TooLarge)?;
        let count = usize::try_from(count).map_err(|_| FlowError::TooLarge)?;
        if count > MAX_TABLE_FIELDS || start.checked_add(count as i64 - 1).is_none() {
            return Err(FlowError::TooLarge);
        }
        if self.steps.saturating_add(count) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += count;
        let backward = source == target && start > from && start <= end;
        // Lua's lua_geti/lua_seti invoke __index/__newindex for array
        // transfers. Root values returned by callbacks across later writes,
        // since those writes may invoke Lua and collect the owned heap.
        self.gc_temporary_roots.push(args.to_vec());
        let result = (|| {
            for offset in 0..count {
                let offset = if backward { count - offset - 1 } else { offset };
                let offset = offset as i64;
                let source_index = from.checked_add(offset).ok_or(FlowError::TooLarge)?;
                let target_index = start.checked_add(offset).ok_or(FlowError::TooLarge)?;
                let key = self.table_key(Value::Integer(source_index))?;
                let value = self.lookup_with_metatable(source, key)?;
                self.gc_temporary_roots
                    .last_mut()
                    .ok_or(FlowError::InvalidBytecode)?
                    .push(value);
                let target_key = self.table_key(Value::Integer(target_index))?;
                self.assign_with_metatable(target, target_key, value)?;
            }
            Ok(Value::Table(target))
        })();
        self.gc_temporary_roots.pop();
        result
    }

    /// Match Lua's ordinary less-than relation for native numbers/bytes and
    /// __lt tables. Optional comparators use normal protected-target execution
    /// with bounded native call depth and Lua truthiness.
    fn table_sort_less(
        &mut self,
        left: Value,
        right: Value,
        comparator: Option<Value>,
    ) -> Result<bool, FlowError> {
        if let Some(callback) = comparator {
            if self.depth >= MAX_CALL_DEPTH {
                return Err(FlowError::CallDepthExceeded);
            }
            self.depth += 1;
            let called = self.protected_target(callback, &[left, right]);
            self.depth -= 1;
            return Ok(called?.first().copied().unwrap_or(Value::Nil).truthy());
        }
        match (left, right) {
            (Value::Integer(_) | Value::Float(_), Value::Integer(_) | Value::Float(_)) => {
                Ok(numeric_order(left, right) == Some(std::cmp::Ordering::Less))
            }
            (Value::String(a), Value::String(b)) => {
                Ok(self.strings.get(a).ok_or(FlowError::InvalidBytecode)?
                    < self.strings.get(b).ok_or(FlowError::InvalidBytecode)?)
            }
            _ => Ok(self
                .ordering_with_metamethod(Binary::Less, left, right)?
                .truthy()),
        }
    }

    /// Sort a bounded snapshot using insertion sort. Unlike Lua's in-place
    /// quicksort, the owned VM commits only after successful comparisons.
    /// GC roots keep all snapshot entries live through comparator callbacks.
    fn table_sort_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let Value::Table(handle) = *args.first().ok_or(FlowError::TypeError)? else {
            return Err(FlowError::TypeError);
        };
        // Lua 5.4 table.sort uses luaL_len, which observes __len and
        // converts an integral numeric result to the sequence boundary.
        // This must run before the trivial-list shortcut and comparator
        // validation, and uses the same bounded metamethod dispatcher as #t.
        let reported_length = self.length_with_metatable(Value::Table(handle))?;
        let length = self
            .exact_integer_operand(reported_length)
            .or_else(|_| self.integer_operand(reported_length))?;
        let length = usize::try_from(length).map_err(|_| FlowError::TooLarge)?;
        if length > MAX_TABLE_FIELDS {
            return Err(FlowError::TooLarge);
        }
        // Lua's table.sort returns without inspecting comp for lists of
        // length zero or one. For nontrivial lists the argument must be an
        // actual function (callable tables are not accepted).
        if length <= 1 {
            return Ok(Vec::new());
        }
        let comparator = match args.get(1).copied().unwrap_or(Value::Nil) {
            Value::Nil => None,
            callback @ (Value::Function(_) | Value::Builtin(_)) => Some(callback),
            _ => return Err(FlowError::TypeError),
        };
        // C Lua's array accesses are lua_geti/lua_seti, not raw operations.
        // Pin each value immediately after its __index result: the next
        // callback can allocate and collect before the snapshot is complete.
        self.gc_temporary_roots.push(args.to_vec());
        let result = (|| {
            let mut sorted = Vec::with_capacity(length);
            for index in 1..=length {
                let value = self.lookup_with_metatable(handle, TableKey::Positive(index))?;
                self.gc_temporary_roots
                    .last_mut()
                    .ok_or(FlowError::InvalidBytecode)?
                    .push(value);
                sorted.push(value);
            }
            for index in 1..sorted.len() {
                let mut at = index;
                while at > 0 {
                    if self.steps >= self.budget {
                        return Err(FlowError::StepBudgetExceeded);
                    }
                    self.steps += 1;
                    if !self.table_sort_less(sorted[at], sorted[at - 1], comparator)? {
                        break;
                    }
                    sorted.swap(at, at - 1);
                    at -= 1;
                }
            }
            for (index, value) in sorted.into_iter().enumerate() {
                self.assign_with_metatable(handle, TableKey::Positive(index + 1), value)?;
            }
            Ok(Vec::new())
        })();
        self.gc_temporary_roots.pop();
        result
    }

    /// table.unpack exposes genuine zero-or-more results from raw table
    /// slots, or nils from integer indexing of Lua byte strings.
    fn table_unpack_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let (target, start, count) = self.table_library_range(args, 1, 2, false)?;
        if self.steps.saturating_add(count) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += count;
        // lua_geti observes __index; callbacks may collect between adjacent
        // elements. Root every prior result until all values are returned.
        self.gc_temporary_roots.push(args.to_vec());
        let result = (|| {
            let mut result = Vec::with_capacity(count);
            for offset in 0..count {
                let index = start
                    .checked_add(offset as i64)
                    .ok_or(FlowError::TooLarge)?;
                let value = match target {
                    Value::Table(handle) => {
                        let key = self.table_key(Value::Integer(index))?;
                        self.lookup_with_metatable(handle, key)?
                    }
                    Value::String(_) => Value::Nil,
                    _ => return Err(FlowError::InvalidBytecode),
                };
                self.gc_temporary_roots
                    .last_mut()
                    .ok_or(FlowError::InvalidBytecode)?
                    .push(value);
                result.push(value);
            }
            Ok(result)
        })();
        self.gc_temporary_roots.pop();
        result
    }

    /// table.concat accepts byte strings and numeric entries. Host-side
    /// joining doesn't create Lua values until the explicitly rooted result
    /// is interned, avoiding intermediate GC-root hazards.
    fn table_concat_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        let separator = match args.get(1).copied() {
            Some(value) if value != Value::Nil => self.string_bytes(value)?,
            _ => Vec::new(),
        };
        let (target, start, count) = self.table_library_range(args, 2, 3, true)?;
        // The Lua reference admits strings for sequence length, but their
        // indexed elements are nil. A nonempty join therefore fails.
        if matches!(target, Value::String(_)) && count > 0 {
            return Err(FlowError::TypeError);
        }
        let mut output = Vec::new();
        for offset in 0..count {
            let index = start
                .checked_add(offset as i64)
                .ok_or(FlowError::TooLarge)?;
            let Value::Table(handle) = target else {
                return Err(FlowError::InvalidBytecode);
            };
            let key = self.table_key(Value::Integer(index))?;
            let value = self.lookup_with_metatable(handle, key)?;
            let bytes = self.string_bytes(value)?;
            let extra = bytes.len() + if offset > 0 { separator.len() } else { 0 };
            if output.len().saturating_add(extra) > MAX_STRING_POOL_BYTES {
                return Err(FlowError::TooLarge);
            }
            if self.steps.saturating_add(extra.saturating_add(1)) > self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += extra + 1;
            if offset > 0 {
                output.extend_from_slice(&separator);
            }
            output.extend_from_slice(&bytes);
        }
        self.intern_rooted_string(output, args)
    }

    /// Bounded Lua 5.4 string.find, using the shared byte-pattern matcher
    /// unless explicit plain mode or Lua's literal-search shortcut applies.
    /// Pattern coverage is regression-tested but not full Lua conformance.
    fn string_find_literal_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let source = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        let pattern = self.string_bytes(*args.get(1).ok_or(FlowError::TypeError)?)?;
        let initial = match args.get(2).copied() {
            Some(index) if index != Value::Nil => self.lua_checked_integer_argument(index)?,
            _ => 1,
        };
        // Lua's string.find skips pattern matching when no characters
        // from its SPECIALS set occur anywhere in the pattern. In
        // particular, a lone ')' is literal in find but a capture error
        // in string.match, which always invokes the pattern matcher.
        let plain = args.get(3).copied().unwrap_or(Value::Nil).truthy()
            || !pattern.iter().any(|byte| b"^$*+?.([%-".contains(byte));
        let Some((begin, end, captures)) =
            self.simple_pattern_match(&source, &pattern, initial, plain)?
        else {
            return Ok(vec![Value::Nil]);
        };
        let mut values = vec![Value::Integer(begin as i64 + 1), Value::Integer(end as i64)];
        values.extend(self.simple_pattern_capture_values(&source, &captures, args)?);
        Ok(values)
    }

    /// Bounded Lua string.gsub with textual, table and callback replacement.
    /// Captures, zero-width advances and matching reuse the shared pattern
    /// engine. Callback values remain rooted across nested Lua execution.
    fn string_gsub_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let source = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        let pattern = self.string_bytes(*args.get(1).ok_or(FlowError::TypeError)?)?;
        let replacement_value = *args.get(2).ok_or(FlowError::TypeError)?;
        let replacement = match replacement_value {
            Value::String(_) | Value::Integer(_) | Value::Float(_) => {
                Some(self.string_bytes(replacement_value)?)
            }
            Value::Table(_) | Value::Function(_) | Value::Builtin(_) => None,
            _ => return Err(FlowError::TypeError),
        };
        let limit = match args.get(3).copied() {
            Some(value) if value != Value::Nil => {
                let count = self.lua_checked_integer_argument(value)?;
                if count <= 0 {
                    0
                } else {
                    usize::try_from(count).map_err(|_| FlowError::TooLarge)?
                }
            }
            _ => source.len().saturating_add(1),
        };
        // Table __index callbacks may trigger the collector; the original
        // table, pattern, and replacement must survive all nested calls.
        self.gc_temporary_roots.push(args.to_vec());
        let result = (|| {
            let mut output = Vec::new();
            let mut cursor = 0usize;
            let mut substitutions = 0usize;
            let anchored = pattern.first() == Some(&b'^');
            while substitutions < limit && cursor <= source.len() {
                let Some((begin, end, captures)) =
                    self.simple_pattern_match(&source, &pattern, cursor as i64 + 1, false)?
                else {
                    break;
                };
                if self.steps >= self.budget {
                    return Err(FlowError::StepBudgetExceeded);
                }
                self.steps += 1;
                if begin < cursor || end < begin || end > source.len() {
                    return Err(FlowError::InvalidBytecode);
                }
                // Retain only arguments and values for this match.
                // Callback-local roots must not accumulate per occurrence.
                let match_root_base = self
                    .gc_temporary_roots
                    .last()
                    .ok_or(FlowError::InvalidBytecode)?
                    .len();
                output.extend_from_slice(&source[cursor..begin]);
                if let Some(replacement) = &replacement {
                    let mut index = 0usize;
                    while index < replacement.len() {
                        if replacement[index] == b'%' {
                            let specifier =
                                *replacement.get(index + 1).ok_or(FlowError::TypeError)?;
                            if specifier == b'%' {
                                output.push(b'%');
                            } else if specifier == b'0' {
                                output.extend_from_slice(&source[begin..end]);
                            } else if (b'1'..=b'9').contains(&specifier) {
                                let capture_index = usize::from(specifier - b'1');
                                let &(first, last, position) =
                                    captures.get(capture_index).ok_or(FlowError::TypeError)?;
                                if position {
                                    output.extend_from_slice((first + 1).to_string().as_bytes());
                                } else {
                                    if last == usize::MAX {
                                        return Err(FlowError::TypeError);
                                    }
                                    output.extend_from_slice(&source[first..last]);
                                }
                            } else {
                                return Err(FlowError::TypeError);
                            }
                            index += 2;
                        } else {
                            output.push(replacement[index]);
                            index += 1;
                        }
                        if output.len() > MAX_STRING_POOL_BYTES {
                            return Err(FlowError::TooLarge);
                        }
                        if self.steps >= self.budget {
                            return Err(FlowError::StepBudgetExceeded);
                        }
                        self.steps += 1;
                    }
                } else if let Value::Table(handle) = replacement_value {
                    let key = match captures.first().copied() {
                        Some((start, _, true)) => Value::Integer(start as i64 + 1),
                        Some((start, end, false)) => {
                            if end == usize::MAX {
                                return Err(FlowError::TypeError);
                            }
                            self.intern_rooted_string(source[start..end].to_vec(), args)?
                        }
                        None => self.intern_rooted_string(source[begin..end].to_vec(), args)?,
                    };
                    self.gc_temporary_roots
                        .last_mut()
                        .ok_or(FlowError::InvalidBytecode)?
                        .push(key);
                    let table_key = self.table_key(key)?;
                    let value = self.lookup_with_metatable(handle, table_key)?;
                    let selected = if value == Value::Nil || value == Value::Boolean(false) {
                        source[begin..end].to_vec()
                    } else {
                        self.string_bytes(value)?
                    };
                    if self.steps.saturating_add(selected.len()) > self.budget {
                        return Err(FlowError::StepBudgetExceeded);
                    }
                    self.steps += selected.len();
                    output.extend_from_slice(&selected);
                } else if matches!(replacement_value, Value::Function(_) | Value::Builtin(_)) {
                    // Lua callbacks receive each capture, or the entire
                    // matched substring when the pattern has no captures.
                    // Keep the temporary Lua strings reachable while their
                    // callback executes and may trigger garbage collection.
                    let parameters = if captures.is_empty() {
                        vec![self.intern_rooted_string(source[begin..end].to_vec(), args)?]
                    } else {
                        self.simple_pattern_capture_values(&source, &captures, args)?
                    };
                    self.gc_temporary_roots
                        .last_mut()
                        .ok_or(FlowError::InvalidBytecode)?
                        .extend_from_slice(&parameters);
                    let returned = self.protected_target(replacement_value, &parameters)?;
                    let value = returned.first().copied().unwrap_or(Value::Nil);
                    let selected = if value == Value::Nil || value == Value::Boolean(false) {
                        source[begin..end].to_vec()
                    } else {
                        self.string_bytes(value)?
                    };
                    if self.steps.saturating_add(selected.len()) > self.budget {
                        return Err(FlowError::StepBudgetExceeded);
                    }
                    self.steps += selected.len();
                    output.extend_from_slice(&selected);
                }
                self.gc_temporary_roots
                    .last_mut()
                    .ok_or(FlowError::InvalidBytecode)?
                    .truncate(match_root_base);
                substitutions += 1;
                cursor = end;
                if begin == end {
                    if cursor < source.len() {
                        output.push(source[cursor]);
                        cursor += 1;
                    } else {
                        break;
                    }
                }
                if output.len() > MAX_STRING_POOL_BYTES {
                    return Err(FlowError::TooLarge);
                }
                if anchored {
                    break;
                }
            }
            output.extend_from_slice(&source[cursor..]);
            if output.len() > MAX_STRING_POOL_BYTES {
                return Err(FlowError::TooLarge);
            }
            let output = self.intern_rooted_string(output, args)?;
            Ok(vec![output, Value::Integer(substitutions as i64)])
        })();
        self.gc_temporary_roots.pop();
        result
    }

    /// Root each captured byte string across subsequent GC-capable interns.
    fn simple_pattern_capture_values(
        &mut self,
        source: &[u8],
        captures: &[(usize, usize, bool)],
        args: &[Value],
    ) -> Result<Vec<Value>, FlowError> {
        self.gc_temporary_roots.push(args.to_vec());
        let result = (|| {
            let mut values = Vec::with_capacity(captures.len());
            for &(start, end, is_position) in captures {
                let value = if is_position {
                    Value::Integer(start as i64 + 1)
                } else {
                    if end == usize::MAX {
                        return Err(FlowError::TypeError);
                    }
                    let roots = self
                        .gc_temporary_roots
                        .last()
                        .ok_or(FlowError::InvalidBytecode)?
                        .clone();
                    self.intern_rooted_string(source[start..end].to_vec(), &roots)?
                };
                self.gc_temporary_roots
                    .last_mut()
                    .ok_or(FlowError::InvalidBytecode)?
                    .push(value);
                values.push(value);
            }
            Ok(values)
        })();
        self.gc_temporary_roots.pop();
        result
    }

    /// Match Lua byte patterns with multiple results for each capture.
    fn string_match_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let source = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        let pattern = self.string_bytes(*args.get(1).ok_or(FlowError::TypeError)?)?;
        let initial = match args.get(2).copied() {
            Some(index) if index != Value::Nil => self.lua_checked_integer_argument(index)?,
            _ => 1,
        };
        match self.simple_pattern_match(&source, &pattern, initial, false)? {
            Some((begin, end, captures)) if captures.is_empty() => Ok(vec![
                self.intern_rooted_string(source[begin..end].to_vec(), args)?,
            ]),
            Some((_, _, captures)) => self.simple_pattern_capture_values(&source, &captures, args),
            None => Ok(vec![Value::Nil]),
        }
    }

    /// Parse bounded Lua 5.4 binary formats against Linux x86-64 sizes,
    /// keeping endianness and alignment metadata through runtime layout.
    /// Other host ABIs and exhaustive native diagnostics remain unaccepted.
    fn parse_pack_fields(format: &[u8]) -> Result<Vec<PackField>, FlowError> {
        // Lua 5.4's binary-format functions scan a C string from
        // luaL_checkstring: bytes after the first NUL are not format options.
        let format = &format[..format
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(format.len())];
        let mut fields = Vec::new();
        let mut index = 0;
        let mut little = cfg!(target_endian = "little");
        let mut maxalign = 1usize;
        let mut total = 0usize;
        while index < format.len() {
            let code = format[index];
            index += 1;
            let field =
                match code {
                    b' ' => continue,
                    b'<' => {
                        little = true;
                        continue;
                    }
                    b'>' => {
                        little = false;
                        continue;
                    }
                    b'=' => {
                        little = cfg!(target_endian = "little");
                        continue;
                    }
                    b'!' => {
                        // The Linux x86-64 Lua ABI has a maximum native
                        // alignment of eight bytes; Lua defaults to eight
                        // when the numeric parameter is omitted.
                        let mut limit = 8usize;
                        if index < format.len() && format[index].is_ascii_digit() {
                            limit = 0;
                            while index < format.len() && format[index].is_ascii_digit() {
                                limit = limit
                                    .checked_mul(10)
                                    .and_then(|n| n.checked_add(usize::from(format[index] - b'0')))
                                    .ok_or(FlowError::TooLarge)?;
                                index += 1;
                            }
                        }
                        if !(1..=16).contains(&limit) {
                            return Err(FlowError::TypeError);
                        }
                        maxalign = limit;
                        continue;
                    }
                    b'X' => {
                        // X consumes its following format option solely to
                        // obtain an alignment; it does not consume a Lua
                        // argument nor emit that following field's bytes.
                        let next = *format.get(index).ok_or(FlowError::TypeError)?;
                        index += 1;
                        let mut width: usize = match next {
                            b'b' | b'B' | b'x' => 1,
                            b'h' | b'H' => 2,
                            b'i' | b'I' | b'f' => 4,
                            b'l' | b'L' | b'j' | b'J' | b'T' | b'd' | b'n' | b's' => 8,
                            _ => return Err(FlowError::TypeError),
                        };
                        if matches!(next, b'i' | b'I' | b's')
                            && index < format.len()
                            && format[index].is_ascii_digit()
                        {
                            width = 0;
                            while index < format.len() && format[index].is_ascii_digit() {
                                width = width
                                    .checked_mul(10)
                                    .and_then(|n| n.checked_add(usize::from(format[index] - b'0')))
                                    .ok_or(FlowError::TooLarge)?;
                                index += 1;
                            }
                        }
                        if !(1..=16).contains(&width) {
                            return Err(FlowError::TypeError);
                        }
                        let align = width.min(maxalign);
                        if align > 1 {
                            if !align.is_power_of_two() {
                                return Err(FlowError::TypeError);
                            }
                            if fields.len() >= MAX_STACK {
                                return Err(FlowError::TooLarge);
                            }
                            fields.push(PackField::Align(align));
                        }
                        continue;
                    }
                    b'x' => PackField::Padding,
                    b'b' | b'B' | b'h' | b'H' | b'l' | b'L' | b'j' | b'J' | b'T' | b'i' | b'I' => {
                        let signed = matches!(code, b'b' | b'h' | b'l' | b'j' | b'i');
                        let mut size: usize = match code {
                            b'b' | b'B' => 1,
                            b'h' | b'H' => 2,
                            b'i' | b'I' => 4,
                            _ => 8,
                        };
                        if matches!(code, b'i' | b'I')
                            && index < format.len()
                            && format[index].is_ascii_digit()
                        {
                            size = 0;
                            while index < format.len() && format[index].is_ascii_digit() {
                                size = size
                                    .checked_mul(10)
                                    .and_then(|n| n.checked_add(usize::from(format[index] - b'0')))
                                    .ok_or(FlowError::TooLarge)?;
                                index += 1;
                            }
                        }
                        if !(1..=16).contains(&size) {
                            return Err(FlowError::Unsupported(
                                "pack supports integer widths from 1 to 16 bytes".into(),
                            ));
                        }
                        PackField::Integer {
                            size,
                            signed,
                            little,
                        }
                    }
                    b'f' | b'd' | b'n' => PackField::Float {
                        size: if code == b'f' { 4 } else { 8 },
                        little,
                    },
                    b'z' => PackField::NullTerminated,
                    b's' => {
                        let mut size = 8usize;
                        if index < format.len() && format[index].is_ascii_digit() {
                            size = 0;
                            while index < format.len() && format[index].is_ascii_digit() {
                                size = size
                                    .checked_mul(10)
                                    .and_then(|n| n.checked_add(usize::from(format[index] - b'0')))
                                    .ok_or(FlowError::TooLarge)?;
                                index += 1;
                            }
                        }
                        if !(1..=16).contains(&size) {
                            return Err(FlowError::Unsupported(
                                "pack supports string-length prefixes from 1 to 16 bytes".into(),
                            ));
                        }
                        PackField::LengthPrefixed { size, little }
                    }
                    b'c' => {
                        if index >= format.len() || !format[index].is_ascii_digit() {
                            return Err(FlowError::TypeError);
                        }
                        let mut size = 0usize;
                        while index < format.len() && format[index].is_ascii_digit() {
                            size = size
                                .checked_mul(10)
                                .and_then(|n| n.checked_add(usize::from(format[index] - b'0')))
                                .ok_or(FlowError::TooLarge)?;
                            index += 1;
                        }
                        PackField::FixedString(size)
                    }
                    _ => return Err(FlowError::Unsupported(
                        "Lua pack alignment, variable-length strings or directive not supported"
                            .into(),
                    )),
                };
            total = total.checked_add(field.size()).ok_or(FlowError::TooLarge)?;
            if total > MAX_STRING_POOL_BYTES || fields.len() >= MAX_STACK {
                return Err(FlowError::TooLarge);
            }
            // Lua aligns a field before writing/reading its bytes; the
            // offset is runtime-dependent after variable-length strings.
            // Store alignment markers instead of predicting the padding
            // during static format parsing.
            let align = match field {
                PackField::FixedString(_) | PackField::NullTerminated | PackField::Padding => 1,
                _ => field.size().min(maxalign),
            };
            if align > 1 {
                if !align.is_power_of_two() {
                    return Err(FlowError::TypeError);
                }
                fields.push(PackField::Align(align));
            }
            fields.push(field);
        }
        Ok(fields)
    }

    /// Append an integer in Lua's requested byte order, extending beyond
    /// the native 64-bit integer width with explicit sign/zero bytes.
    fn append_pack_width(
        output: &mut Vec<u8>,
        value: u64,
        size: usize,
        little: bool,
        extension: u8,
    ) {
        for offset in 0..size {
            let significance = if little { offset } else { size - 1 - offset };
            let byte = if significance < 8 {
                (value >> (8 * significance)) as u8
            } else {
                extension
            };
            output.push(byte);
        }
    }

    /// Recover the native low 64 bits and reject high bytes that do not
    /// agree with Lua 5.4's extension/overflow rules. The caller applies
    /// sub-64-bit sign extension only for signed small integer fields.
    fn read_pack_width(chunk: &[u8], little: bool, signed: bool) -> Result<u64, FlowError> {
        let size = chunk.len();
        let mut value = 0u64;
        for significance in 0..size.min(8) {
            let index = if little {
                significance
            } else {
                size - 1 - significance
            };
            value |= u64::from(chunk[index]) << (significance * 8);
        }
        if size > 8 {
            let extension = if signed && (value as i64) < 0 { 255 } else { 0 };
            for significance in 8..size {
                let index = if little {
                    significance
                } else {
                    size - 1 - significance
                };
                if chunk[index] != extension {
                    return Err(FlowError::TypeError);
                }
            }
        }
        Ok(value)
    }

    /// Convert optional/required Lua library integer arguments using
    /// luaL_checkinteger's exact numeric conversion (not token-only
    /// integer parsing). This accepts integral decimal/exponent/hex text,
    /// but not fractional numbers or Unicode whitespace.
    fn lua_checked_integer_argument(&self, value: Value) -> Result<i64, FlowError> {
        let number = self.lua_numeric_operand(value)?;
        self.exact_integer_operand(number)
    }

    /// Lua-style number conversion used by the bounded binary formats
    /// and tonumber. Hexadecimal integers wrap modulo 2^64 and signed
    /// hexadecimal floating strings retain their floating representation.
    /// Arithmetic operators keep their separately tested coercion path.
    fn lua_numeric_operand(&self, value: Value) -> Result<Value, FlowError> {
        if let Value::String(handle) = value {
            let bytes = self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
            let text = std::str::from_utf8(bytes).map_err(|_| FlowError::TypeError)?;
            // Reject Unicode whitespace that Rust would otherwise trim.
            if text.trim() != trim_lua_numeric_space(text) {
                return Err(FlowError::TypeError);
            }
        }
        if let Ok(number) = self.arithmetic_operand(value) {
            return Ok(number);
        }
        let Value::String(handle) = value else {
            return Err(FlowError::TypeError);
        };
        let bytes = self.strings.get(handle).ok_or(FlowError::InvalidBytecode)?;
        let text = std::str::from_utf8(bytes).map_err(|_| FlowError::TypeError)?;
        let text = trim_lua_numeric_space(text);
        let (negative, unsigned) = if let Some(rest) = text.strip_prefix('-') {
            (true, rest)
        } else {
            (false, text.strip_prefix('+').unwrap_or(text))
        };
        if let Some(number) = parse_lua_hex_integer_wrapping(unsigned) {
            return Ok(Value::Integer(if negative {
                number.wrapping_neg()
            } else {
                number
            }));
        }
        let number = parse_lua_hex_float(unsigned).ok_or(FlowError::TypeError)?;
        let number = if negative { -number } else { number };
        Ok(Value::Float(number.to_bits()))
    }

    /// Exact integer conversion allows integral scientific/decimal/hex
    /// numeric strings, but not strings with a fractional numeric value.
    fn pack_integer_operand(&self, value: Value) -> Result<i64, FlowError> {
        self.exact_integer_operand(value).or_else(|_| {
            let number = self.lua_numeric_operand(value)?;
            self.exact_integer_operand(number)
        })
    }

    /// Bounded Lua 5.4 pack/unpack/packsize share one parser and support
    /// 1–16-byte integer and length fields, binary strings, and dynamic
    /// alignment padding. Native ABI portability remains a separate gate.
    fn string_pack_dispatch(
        &mut self,
        builtin: Builtin,
        args: &[Value],
    ) -> Result<Vec<Value>, FlowError> {
        let format = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        let fields = Self::parse_pack_fields(&format)?;
        let total: usize = fields.iter().map(|field| field.size()).sum();
        let cost = format.len().saturating_add(total).max(1);
        if self.steps.saturating_add(cost) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += cost;
        if builtin == Builtin::StringPackSize {
            if fields.iter().any(|field| {
                matches!(
                    field,
                    PackField::NullTerminated | PackField::LengthPrefixed { .. }
                )
            }) {
                return Err(FlowError::TypeError);
            }
            let mut size = 0usize;
            for field in &fields {
                if let PackField::Align(align) = field {
                    size = size
                        .checked_add((align - size % align) % align)
                        .ok_or(FlowError::TooLarge)?;
                } else {
                    size = size.checked_add(field.size()).ok_or(FlowError::TooLarge)?;
                }
                if size > MAX_STRING_POOL_BYTES {
                    return Err(FlowError::TooLarge);
                }
            }
            return Ok(vec![Value::Integer(size as i64)]);
        }
        if builtin == Builtin::StringPack {
            let mut output = Vec::with_capacity(total);
            let mut argument = 1usize;
            for field in fields {
                match field {
                    PackField::Padding => output.push(0),
                    PackField::Align(align) => {
                        let padding = (align - output.len() % align) % align;
                        if output.len().saturating_add(padding) > MAX_STRING_POOL_BYTES {
                            return Err(FlowError::TooLarge);
                        }
                        if self.steps.saturating_add(padding) > self.budget {
                            return Err(FlowError::StepBudgetExceeded);
                        }
                        self.steps += padding;
                        output.resize(output.len() + padding, 0);
                    }
                    PackField::NullTerminated | PackField::LengthPrefixed { .. } => {
                        let bytes =
                            self.string_bytes(*args.get(argument).ok_or(FlowError::TypeError)?)?;
                        argument += 1;
                        let overhead = match field {
                            PackField::NullTerminated => {
                                if bytes.contains(&0) {
                                    return Err(FlowError::TypeError);
                                }
                                output.extend_from_slice(&bytes);
                                output.push(0);
                                bytes.len() + 1
                            }
                            PackField::LengthPrefixed { size, little } => {
                                if size < 8 && bytes.len() as u64 >= (1u64 << (size * 8)) {
                                    return Err(FlowError::TypeError);
                                }
                                let length = bytes.len() as u64;
                                Self::append_pack_width(&mut output, length, size, little, 0);
                                output.extend_from_slice(&bytes);
                                bytes.len()
                            }
                            _ => unreachable!("dynamic field guarded above"),
                        };
                        if self.steps.saturating_add(overhead) > self.budget {
                            return Err(FlowError::StepBudgetExceeded);
                        }
                        self.steps += overhead;
                        if output.len() > MAX_STRING_POOL_BYTES {
                            return Err(FlowError::TooLarge);
                        }
                    }
                    PackField::FixedString(size) => {
                        let bytes =
                            self.string_bytes(*args.get(argument).ok_or(FlowError::TypeError)?)?;
                        argument += 1;
                        if bytes.len() > size {
                            return Err(FlowError::TypeError);
                        }
                        output.extend_from_slice(&bytes);
                        output.resize(output.len() + size - bytes.len(), 0);
                    }
                    PackField::Integer {
                        size,
                        signed,
                        little,
                    } => {
                        let value = *args.get(argument).ok_or(FlowError::TypeError)?;
                        argument += 1;
                        let integer = self.pack_integer_operand(value)?;
                        if size < 8 {
                            let bits = size * 8;
                            let fits = if signed {
                                let limit = 1i64 << (bits - 1);
                                (-limit..limit).contains(&integer)
                            } else {
                                integer >= 0 && (integer as u64) < (1u64 << bits)
                            };
                            if !fits {
                                return Err(FlowError::TypeError);
                            }
                        }
                        let extension = if signed && integer < 0 { 255 } else { 0 };
                        Self::append_pack_width(
                            &mut output,
                            integer as u64,
                            size,
                            little,
                            extension,
                        );
                    }
                    PackField::Float { size, little } => {
                        let value = *args.get(argument).ok_or(FlowError::TypeError)?;
                        argument += 1;
                        let number = match self.lua_numeric_operand(value)? {
                            Value::Integer(number) => number as f64,
                            Value::Float(bits) => f64::from_bits(bits),
                            _ => return Err(FlowError::TypeError),
                        };
                        let bits = if size == 4 {
                            u64::from((number as f32).to_bits())
                        } else {
                            number.to_bits()
                        };
                        if little {
                            output.extend_from_slice(&bits.to_le_bytes()[..size]);
                        } else {
                            output.extend_from_slice(&bits.to_be_bytes()[8 - size..]);
                        }
                    }
                }
            }
            return Ok(vec![self.intern_rooted_string(output, args)?]);
        }
        if builtin != Builtin::StringUnpack {
            return Err(FlowError::InvalidBytecode);
        }
        let bytes = self.string_bytes(*args.get(1).ok_or(FlowError::TypeError)?)?;
        let position = match args.get(2).copied() {
            Some(value) if value != Value::Nil => self.pack_integer_operand(value)?,
            _ => 1,
        };
        // Lua 5.4's posrelatI clamps zero and excessively negative
        // initial positions to 1; one past the end permits empty formats.
        let position = Self::string_offset(position, bytes.len() as i64).max(1);
        if position > bytes.len() as i64 + 1 {
            return Err(FlowError::TypeError);
        }
        let mut cursor = (position - 1) as usize;
        let mut values = Vec::with_capacity(fields.len() + 1);
        self.gc_temporary_roots.push(args.to_vec());
        let outcome = (|| {
            for field in fields {
                if let PackField::Align(align) = field {
                    let padding = (align - cursor % align) % align;
                    cursor = cursor.checked_add(padding).ok_or(FlowError::TooLarge)?;
                    if cursor > bytes.len() {
                        return Err(FlowError::TypeError);
                    }
                    continue;
                }
                let end = cursor
                    .checked_add(field.size())
                    .ok_or(FlowError::TooLarge)?;
                let chunk = bytes.get(cursor..end).ok_or(FlowError::TypeError)?;
                cursor = end;
                let value = match field {
                    PackField::Padding => continue,
                    PackField::Align(_) => unreachable!("alignment consumed before byte read"),
                    PackField::NullTerminated | PackField::LengthPrefixed { .. } => {
                        let (begin, end) = match field {
                            PackField::NullTerminated => {
                                let remaining = bytes.get(cursor..).ok_or(FlowError::TypeError)?;
                                let count = remaining
                                    .iter()
                                    .position(|&byte| byte == 0)
                                    .ok_or(FlowError::TypeError)?;
                                let start = cursor;
                                let end = cursor + count;
                                cursor = end + 1;
                                (start, end)
                            }
                            PackField::LengthPrefixed { little, .. } => {
                                let length = Self::read_pack_width(chunk, little, false)?;
                                let length =
                                    usize::try_from(length).map_err(|_| FlowError::TooLarge)?;
                                if length > MAX_STRING_POOL_BYTES {
                                    return Err(FlowError::TooLarge);
                                }
                                let start = cursor;
                                let end = cursor.checked_add(length).ok_or(FlowError::TooLarge)?;
                                bytes.get(start..end).ok_or(FlowError::TypeError)?;
                                cursor = end;
                                (start, end)
                            }
                            _ => unreachable!("dynamic field guarded above"),
                        };
                        let consumed =
                            end - begin + usize::from(matches!(field, PackField::NullTerminated));
                        if self.steps.saturating_add(consumed) > self.budget {
                            return Err(FlowError::StepBudgetExceeded);
                        }
                        self.steps += consumed;
                        let roots = self
                            .gc_temporary_roots
                            .last()
                            .ok_or(FlowError::InvalidBytecode)?
                            .clone();
                        self.intern_rooted_string(bytes[begin..end].to_vec(), &roots)?
                    }
                    PackField::FixedString(_) => {
                        let roots = self
                            .gc_temporary_roots
                            .last()
                            .ok_or(FlowError::InvalidBytecode)?
                            .clone();
                        self.intern_rooted_string(chunk.to_vec(), &roots)?
                    }
                    PackField::Integer {
                        size,
                        signed,
                        little,
                    } => {
                        let mut integer = Self::read_pack_width(chunk, little, signed)?;
                        if signed && size < 8 && integer & (1u64 << (size * 8 - 1)) != 0 {
                            integer |= !0u64 << (size * 8);
                        }
                        Value::Integer(integer as i64)
                    }
                    PackField::Float { size, little } => {
                        let mut raw = 0u64;
                        for (index, &byte) in chunk.iter().enumerate() {
                            if little {
                                raw |= u64::from(byte) << (index * 8);
                            } else {
                                raw = (raw << 8) | u64::from(byte);
                            }
                        }
                        let number = if size == 4 {
                            f32::from_bits(raw as u32) as f64
                        } else {
                            f64::from_bits(raw)
                        };
                        Value::Float(number.to_bits())
                    }
                };
                self.gc_temporary_roots
                    .last_mut()
                    .ok_or(FlowError::InvalidBytecode)?
                    .push(value);
                values.push(value);
            }
            values.push(Value::Integer(cursor as i64 + 1));
            Ok(values)
        })();
        self.gc_temporary_roots.pop();
        outcome
    }

    /// Create a first-class, independent Lua iterator. The owned engine
    /// limits total live iterator snapshots rather than letting repeated
    /// gmatch calls allocate unbounded Rust-side memory.
    fn string_gmatch_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        let source = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        let mut pattern = self.string_bytes(*args.get(1).ok_or(FlowError::TypeError)?)?;
        // Lua 5.4 gmatch does not interpret a leading '^' as an anchor.
        // It is a literal caret, not a character to discard.
        if pattern.first() == Some(&b'^') {
            pattern.insert(0, b'%');
        }
        let bytes = source.len().saturating_add(pattern.len());
        if self.automatic_gc
            && (self.gmatch_bytes.saturating_add(bytes) > MAX_STRING_POOL_BYTES
                || (self.gmatch_states.len() >= MAX_STRINGS && self.free_gmatch_states.is_empty()))
        {
            let frame = self
                .gc_frame_roots
                .last()
                .ok_or(FlowError::InvalidBytecode)?;
            let slots = frame.slots.clone();
            let prepared = frame.prepared.clone();
            self.collect_root_tables(args, &slots, &prepared)?;
        }
        if (self.gmatch_states.len() >= MAX_STRINGS && self.free_gmatch_states.is_empty())
            || self.gmatch_bytes.saturating_add(bytes) > MAX_STRING_POOL_BYTES
        {
            return Err(FlowError::TooLarge);
        }
        if self.steps.saturating_add(bytes.max(1)) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += bytes.max(1);
        // Lua 5.4 gmatch accepts an optional third argument for its
        // initial byte position. Negative positions count from the end,
        // zero and overly negative values clip to the first byte, and
        // positions past the end yield an exhausted iterator.
        let initial = match args.get(2).copied() {
            Some(value) if value != Value::Nil => self.lua_checked_integer_argument(value)?,
            _ => 1,
        };
        let start = Self::string_offset(initial, source.len() as i64).max(1);
        let cursor = if start > source.len() as i64 + 1 {
            source.len() + 1
        } else {
            (start - 1) as usize
        };
        let state = GmatchState {
            source,
            pattern,
            cursor,
            last_end: None,
            done: false,
        };
        let handle = if let Some(handle) = self.free_gmatch_states.pop() {
            *self
                .gmatch_states
                .get_mut(handle)
                .ok_or(FlowError::InvalidBytecode)? = Some(state);
            handle
        } else {
            let handle = self.gmatch_states.len();
            self.gmatch_states.push(Some(state));
            handle
        };
        self.gmatch_bytes += bytes;
        Ok(Value::Builtin(Builtin::StringGmatchIterator(handle)))
    }

    /// The iterator preserves its own cursor, ignores ordinary Lua call
    /// arguments, skips duplicate empty matches and returns captures (or
    /// the whole match when no captures are present).
    fn string_gmatch_iterator(
        &mut self,
        handle: usize,
        args: &[Value],
    ) -> Result<Vec<Value>, FlowError> {
        let state = self
            .gmatch_states
            .get(handle)
            .and_then(Option::as_ref)
            .ok_or(FlowError::InvalidBytecode)?;
        if state.done {
            return Ok(Vec::new());
        }
        let source = state.source.clone();
        let pattern = state.pattern.clone();
        let mut cursor = state.cursor;
        let previous_end = state.last_end;
        while cursor <= source.len() {
            if self.steps >= self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += 1;
            let result = self.simple_pattern_match(&source, &pattern, cursor as i64 + 1, false)?;
            let Some((begin, end, captures)) = result else {
                break;
            };
            if Some(end) == previous_end {
                cursor = begin.saturating_add(1);
                continue;
            }
            let state = self
                .gmatch_states
                .get_mut(handle)
                .and_then(Option::as_mut)
                .ok_or(FlowError::InvalidBytecode)?;
            state.cursor = end;
            state.last_end = Some(end);
            let mut roots = args.to_vec();
            roots.push(Value::Builtin(Builtin::StringGmatchIterator(handle)));
            return if captures.is_empty() {
                Ok(vec![self.intern_rooted_string(
                    source[begin..end].to_vec(),
                    &roots,
                )?])
            } else {
                self.simple_pattern_capture_values(&source, &captures, &roots)
            };
        }
        self.gmatch_states
            .get_mut(handle)
            .and_then(Option::as_mut)
            .ok_or(FlowError::InvalidBytecode)?
            .done = true;
        Ok(Vec::new())
    }

    /// Search a bounded byte string with Lua-pattern atoms, byte classes,
    /// captures, balanced/frontier assertions, and bounded backtracking.
    /// Invalid constructs are rejected when traversal reaches them.
    /// Returns zero-based [begin,end) bounds and charges the shared budget.
    fn simple_pattern_match(
        &mut self,
        source: &[u8],
        pattern: &[u8],
        initial: i64,
        plain: bool,
    ) -> Result<Option<PatternMatch>, FlowError> {
        // Lua returns no match before parsing pattern atoms when the
        // initial byte position is already beyond the one-past-end sentinel.
        // Even a malformed pattern is unreachable in that case.
        let start = Self::string_offset(initial, source.len() as i64).max(1);
        if start > source.len() as i64 + 1 {
            return Ok(None);
        }
        let mut body = pattern;
        let mut anchored_start = false;
        let mut anchored_end = false;
        if !plain {
            if body.first() == Some(&b'^') {
                body = &body[1..];
                anchored_start = true;
            }
            // A terminal '$' is an anchor only after an even number of
            // consecutive %-escapes (otherwise the dollar is literal).
            if body.last() == Some(&b'$') {
                let escaped = body[..body.len() - 1]
                    .iter()
                    .rev()
                    .take_while(|&&byte| byte == b'%')
                    .count()
                    % 2
                    == 1;
                if !escaped {
                    body = &body[..body.len() - 1];
                    anchored_end = true;
                }
            }
        }
        // A bracket class is stored as its bounded source-byte contents.
        // It may contain ranges, escaped classes, and an initial negation.
        // Backtracking is still iterative and shared across all atom kinds.
        enum Atom {
            Byte(u8, bool),
            Bracket(Vec<u8>),
            Frontier(Vec<u8>),
            Balanced(u8, u8),
            CaptureOpen,
            CaptureClose,
            CapturePosition,
            InvalidClose,
            InvalidBracket,
            InvalidEscape,
            Backreference(u8),
        }
        if self.steps.saturating_add(body.len()) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += body.len();
        let mut atoms = Vec::with_capacity(body.len());
        let mut index = 0;
        let mut opened = 0usize;
        while index < body.len() {
            let atom = if !plain && body[index] == b'(' {
                if body.get(index + 1) == Some(&b')') {
                    index += 2;
                    Atom::CapturePosition
                } else {
                    opened += 1;
                    index += 1;
                    Atom::CaptureOpen
                }
            } else if !plain && body[index] == b')' {
                index += 1;
                if opened == 0 {
                    // Capture errors occur only if Lua's pattern traversal
                    // actually reaches this closing delimiter.
                    Atom::InvalidClose
                } else {
                    opened -= 1;
                    Atom::CaptureClose
                }
            } else if !plain && body[index] == b'[' {
                let start = index + 1;
                let mut cursor = start;
                if body.get(cursor) == Some(&b'^') {
                    cursor += 1;
                }
                // Lua permits a literal ']' as the first class member.
                if body.get(cursor) == Some(&b']') {
                    cursor += 1;
                }
                loop {
                    match body.get(cursor) {
                        None => {
                            // Like upstream Lua's classend(), reject a malformed
                            // bracket only when matching reaches this atom.
                            index = body.len();
                            break Atom::InvalidBracket;
                        }
                        Some(b'%') if cursor + 1 < body.len() => cursor += 2,
                        Some(b']') => {
                            let inside = body[start..cursor].to_vec();
                            index = cursor + 1;
                            break Atom::Bracket(inside);
                        }
                        Some(_) => cursor += 1,
                    }
                }
            } else if !plain && body[index] == b'%' {
                // Upstream matches pattern atoms lazily. A malformed escape
                // after a literal that never matches must not error during
                // pre-parsing; surface it only if the VM reaches that atom.
                match body.get(index + 1).copied() {
                    None => {
                        index = body.len();
                        Atom::InvalidEscape
                    }
                    Some(b'b') => {
                        // %bxy requires two delimiter bytes.
                        match (body.get(index + 2), body.get(index + 3)) {
                            (Some(&opening), Some(&closing)) => {
                                index += 4;
                                Atom::Balanced(opening, closing)
                            }
                            _ => {
                                index = body.len();
                                Atom::InvalidEscape
                            }
                        }
                    }
                    Some(b'f') => {
                        // %f[set] asserts a byte-class frontier without
                        // consuming a source byte.
                        let bracket = index + 2;
                        if body.get(bracket) != Some(&b'[') {
                            index = body.len();
                            Atom::InvalidEscape
                        } else {
                            let start = bracket + 1;
                            let mut cursor = start;
                            if body.get(cursor) == Some(&b'^') {
                                cursor += 1;
                            }
                            if body.get(cursor) == Some(&b']') {
                                cursor += 1;
                            }
                            loop {
                                match body.get(cursor) {
                                    None => {
                                        index = body.len();
                                        break Atom::InvalidEscape;
                                    }
                                    Some(b'%') if cursor + 1 < body.len() => cursor += 2,
                                    Some(b']') => {
                                        let inside = body[start..cursor].to_vec();
                                        index = cursor + 1;
                                        break Atom::Frontier(inside);
                                    }
                                    Some(_) => cursor += 1,
                                }
                            }
                        }
                    }
                    Some(quoted) => {
                        index += 2;
                        if quoted.is_ascii_digit() {
                            Atom::Backreference(quoted)
                        } else {
                            Atom::Byte(quoted, true)
                        }
                    }
                }
            } else {
                let value = body[index];
                // Lua's classend() treats a standalone repetition marker
                // or closing bracket as an ordinary byte. These characters
                // only have operator meaning *after* a preceding atom.
                index += 1;
                Atom::Byte(value, false)
            };
            // Lua interprets repetition suffixes only after ordinary
            // single-byte or bracket-class atoms. After captures, %b,
            // %f, and %1-%9 backreferences, the same punctuation begins
            // a separate literal atom instead of repeating the construct.
            let can_repeat = matches!(&atom, Atom::Byte(_, _) | Atom::Bracket(_));
            let repetition =
                if !plain && can_repeat && index < body.len() && b"*+?-".contains(&body[index]) {
                    let operator = body[index];
                    index += 1;
                    operator
                } else {
                    0
                };
            atoms.push((atom, repetition));
        }
        // An unfinished '(' raises an error only when the matching
        // traversal reaches the end with a still-open capture. A literal
        // prefix that never matches must not fail at parse time.
        let offset = (start - 1) as usize;
        for begin in offset..=source.len() {
            if self.steps >= self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += 1;
            // Explicit DFS replaces recursive Lua pattern backtracking.
            // Branch order is greedy for *, + and ?, lazy for -.
            let mut states: Vec<(usize, usize, Vec<PatternCapture>)> =
                vec![(0usize, begin, Vec::new())];
            while let Some((atom_index, position, captures)) = states.pop() {
                if self.steps >= self.budget {
                    return Err(FlowError::StepBudgetExceeded);
                }
                self.steps += 1;
                if atom_index == atoms.len() {
                    if !anchored_end || position == source.len() {
                        // Capture consumers validate unfinished captures.
                        // Lua gsub with a literal replacement can use a
                        // successful match without retrieving its captures.
                        return Ok(Some((begin, position, captures)));
                    }
                    continue;
                }
                let (atom, repetition) = &atoms[atom_index];
                match atom {
                    Atom::InvalidClose | Atom::InvalidBracket | Atom::InvalidEscape => {
                        return Err(FlowError::TypeError);
                    }
                    Atom::CapturePosition => {
                        let mut next = captures;
                        if next.len() >= 32 {
                            // Lua reports excess captures as a protected
                            // pattern error, not as a fatal host quota.
                            return Err(FlowError::TypeError);
                        }
                        next.push((position, position, true));
                        states.push((atom_index + 1, position, next));
                        continue;
                    }
                    Atom::CaptureOpen => {
                        let mut next = captures;
                        if next.len() >= 32 {
                            // Lua reports excess captures as a protected
                            // pattern error, not as a fatal host quota.
                            return Err(FlowError::TypeError);
                        }
                        next.push((position, usize::MAX, false));
                        states.push((atom_index + 1, position, next));
                        continue;
                    }
                    Atom::CaptureClose => {
                        let mut next = captures;
                        let opened = next
                            .iter()
                            .rposition(|&(_, end, is_position)| end == usize::MAX && !is_position)
                            .ok_or(FlowError::InvalidBytecode)?;
                        next[opened].1 = position;
                        states.push((atom_index + 1, position, next));
                        continue;
                    }
                    _ => {}
                }
                if let Atom::Backreference(digit) = atom {
                    if *digit == b'0' {
                        return Err(FlowError::TypeError);
                    }
                    let referenced = usize::from(*digit - b'1');
                    let &(start, end, is_position) =
                        captures.get(referenced).ok_or(FlowError::TypeError)?;
                    if is_position || end == usize::MAX {
                        return Err(FlowError::TypeError);
                    }
                    let span = end - start;
                    // A backreference comparison must charge its byte
                    // traversal to the same bounded execution budget.
                    if self.steps.saturating_add(span) > self.budget {
                        return Err(FlowError::StepBudgetExceeded);
                    }
                    self.steps += span;
                    if position
                        .checked_add(span)
                        .is_some_and(|last| last <= source.len())
                        && source[position..position + span] == source[start..end]
                    {
                        states.push((atom_index + 1, position + span, captures.clone()));
                    }
                    continue;
                }
                if let Atom::Frontier(contents) = atom {
                    let previous = if position == 0 {
                        0
                    } else {
                        source[position - 1]
                    };
                    let next = source.get(position).copied().unwrap_or(0);
                    if !Self::simple_pattern_bracket_matches(contents, previous)
                        && Self::simple_pattern_bracket_matches(contents, next)
                    {
                        states.push((atom_index + 1, position, captures.clone()));
                    }
                    continue;
                }
                if let Atom::Balanced(opening, closing) = atom {
                    if source.get(position) != Some(opening) {
                        continue;
                    }
                    let mut depth = 1usize;
                    for (cursor, &byte) in source.iter().enumerate().skip(position + 1) {
                        if self.steps >= self.budget {
                            return Err(FlowError::StepBudgetExceeded);
                        }
                        self.steps += 1;
                        if byte == *closing {
                            depth -= 1;
                            if depth == 0 {
                                states.push((atom_index + 1, cursor + 1, captures.clone()));
                                break;
                            }
                        } else if byte == *opening {
                            depth = depth.checked_add(1).ok_or(FlowError::TooLarge)?;
                        }
                    }
                    continue;
                }
                let matches = |at: usize| {
                    at < source.len()
                        && match atom {
                            Atom::Byte(byte, true) => {
                                Self::simple_pattern_escape_matches(*byte, source[at])
                            }
                            Atom::Byte(byte, false) => {
                                source[at] == *byte || (!plain && *byte == b'.')
                            }
                            Atom::Bracket(contents) => {
                                Self::simple_pattern_bracket_matches(contents, source[at])
                            }
                            Atom::Frontier(_)
                            | Atom::Balanced(_, _)
                            | Atom::CaptureOpen
                            | Atom::CaptureClose
                            | Atom::CapturePosition
                            | Atom::InvalidClose
                            | Atom::InvalidBracket
                            | Atom::InvalidEscape
                            | Atom::Backreference(_) => false,
                        }
                };
                match *repetition {
                    0 => {
                        if matches(position) {
                            states.push((atom_index + 1, position + 1, captures.clone()));
                        }
                    }
                    b'?' => {
                        // Stack is LIFO: evaluate consuming before skipping.
                        states.push((atom_index + 1, position, captures.clone()));
                        if matches(position) {
                            states.push((atom_index + 1, position + 1, captures.clone()));
                        }
                    }
                    b'*' | b'+' | b'-' => {
                        let mut maximum = position;
                        while maximum < source.len() {
                            if self.steps >= self.budget {
                                return Err(FlowError::StepBudgetExceeded);
                            }
                            self.steps += 1;
                            if !matches(maximum) {
                                break;
                            }
                            maximum += 1;
                        }
                        let minimum = position + usize::from(*repetition == b'+');
                        if minimum <= maximum {
                            let alternatives = maximum - minimum + 1;
                            if states.len().saturating_add(alternatives) > MAX_STACK {
                                return Err(FlowError::TooLarge);
                            }
                            if *repetition == b'-' {
                                for at in (minimum..=maximum).rev() {
                                    states.push((atom_index + 1, at, captures.clone()));
                                }
                            } else {
                                for at in minimum..=maximum {
                                    states.push((atom_index + 1, at, captures.clone()));
                                }
                            }
                        }
                    }
                    _ => return Err(FlowError::InvalidBytecode),
                }
                if states.len() > MAX_STACK {
                    return Err(FlowError::TooLarge);
                }
            }
            if anchored_start {
                break;
            }
        }
        Ok(None)
    }

    /// Lua bracket-class membership: negation, %-classes, inclusive byte
    /// ranges, and literal punctuation. The surrounding parser already
    /// identified the closing bracket, including an initial literal ']'.
    fn simple_pattern_bracket_matches(contents: &[u8], actual: u8) -> bool {
        let negated = contents.first() == Some(&b'^');
        let mut index = usize::from(negated);
        let mut found = false;
        while index < contents.len() {
            let byte = contents[index];
            if byte == b'%' && index + 1 < contents.len() {
                if Self::simple_pattern_escape_matches(contents[index + 1], actual) {
                    found = true;
                }
                index += 2;
            } else if index + 2 < contents.len() && contents[index + 1] == b'-' {
                if byte <= actual && actual <= contents[index + 2] {
                    found = true;
                }
                index += 3;
            } else {
                if byte == actual {
                    found = true;
                }
                index += 1;
            }
        }
        found != negated
    }

    /// Lua 5.4's %character classes under the project's C/ASCII locale.
    /// Unknown escapes quote the following byte literally.
    fn simple_pattern_escape_matches(class: u8, actual: u8) -> bool {
        let positive = match class.to_ascii_lowercase() {
            b'a' => Some(actual.is_ascii_alphabetic()),
            b'c' => Some(actual.is_ascii_control()),
            b'd' => Some(actual.is_ascii_digit()),
            b'g' => Some((33..=126).contains(&actual)),
            b'l' => Some(actual.is_ascii_lowercase()),
            b'p' => Some(actual.is_ascii_punctuation()),
            // C isspace includes VT (0x0B), which Rust's
            // is_ascii_whitespace deliberately excludes.
            b's' => Some(matches!(actual, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)),
            b'u' => Some(actual.is_ascii_uppercase()),
            b'w' => Some(actual.is_ascii_alphanumeric()),
            b'x' => Some(actual.is_ascii_hexdigit()),
            b'z' => Some(actual == 0),
            _ => None,
        };
        match positive {
            Some(found) if class.is_ascii_uppercase() => !found,
            Some(found) => found,
            None => actual == class,
        }
    }

    /// Lua string offsets are one-based bytes; negatives count from the end.
    fn string_offset(index: i64, length: i64) -> i64 {
        if index < 0 {
            index.saturating_add(length).saturating_add(1)
        } else {
            index
        }
    }

    /// string.byte returns zero or more raw byte values and therefore cannot
    /// be collapsed into the scalar builtin dispatch until after Lua adjusts
    /// the multiple results at the call site.
    fn string_byte_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let bytes = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        let start = match args.get(1).copied() {
            Some(value) if value != Value::Nil => self.lua_checked_integer_argument(value)?,
            _ => 1,
        };
        let end = match args.get(2).copied() {
            Some(value) if value != Value::Nil => self.lua_checked_integer_argument(value)?,
            _ => start,
        };
        let length = bytes.len() as i64;
        let start = Self::string_offset(start, length).max(1);
        let end = Self::string_offset(end, length).min(length);
        if start > end {
            return Ok(Vec::new());
        }
        let count = (end - start + 1) as usize;
        if count > MAX_STACK {
            return Err(FlowError::TooLarge);
        }
        if self.steps.saturating_add(count) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += count;
        Ok(bytes[(start - 1) as usize..end as usize]
            .iter()
            .map(|&byte| Value::Integer(i64::from(byte)))
            .collect())
    }

    fn utf8_optional_integer(
        &mut self,
        args: &[Value],
        index: usize,
        default: i64,
        name: &str,
    ) -> Result<i64, FlowError> {
        if args.get(index).is_none_or(|value| *value == Value::Nil) {
            return Ok(default);
        }
        let Value::Integer(value) = self.checked_math_number(name, args, index, true)? else {
            return Err(FlowError::InvalidBytecode);
        };
        Ok(value)
    }

    fn utf8_argument_error(
        &mut self,
        args: &[Value],
        name: &str,
        index: usize,
        detail: &str,
    ) -> Result<FlowError, FlowError> {
        let message = format!("bad argument #{} to '{name}' ({detail})", index + 1);
        let value = self.intern_rooted_string(message.into_bytes(), args)?;
        Ok(FlowError::Thrown(value))
    }

    fn utf8_char_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        let mut output = Vec::new();
        for (index, _) in args.iter().enumerate() {
            let Value::Integer(code) = self.checked_math_number("utf8.char", args, index, true)?
            else {
                return Err(FlowError::InvalidBytecode);
            };
            if !(0..=0x7FFF_FFFF).contains(&code) {
                return Err(self.utf8_argument_error(
                    args,
                    "utf8.char",
                    index,
                    "value out of range",
                )?);
            }
            lua_utf8_encode(code as u32, &mut output);
            if output.len() > MAX_STRING_POOL_BYTES {
                return Err(FlowError::TooLarge);
            }
        }
        self.intern_rooted_string(output, args)
    }

    /// Byte-oriented Lua 5.4 offset counts lead bytes rather than validating
    /// sequences, and allows the position immediately after the last byte.
    /// Lua codes returns a (iterator, string-state, integer-control) triple.
    /// Keep string byte content intact even when the source was a number.
    fn utf8_codes_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let input = *args.first().ok_or(FlowError::TypeError)?;
        let bytes = self.string_bytes(input)?;
        if bytes.first().is_some_and(|byte| byte & 0xC0 == 0x80) {
            return Err(self.utf8_argument_error(args, "utf8.codes", 0, "invalid UTF-8 code")?);
        }
        let state = if matches!(input, Value::String(_)) {
            input
        } else {
            self.intern_rooted_string(bytes, args)?
        };
        let lax = args.get(1).copied().unwrap_or(Value::Nil).truthy();
        let iterator = if lax {
            Builtin::Utf8CodesIteratorLax
        } else {
            Builtin::Utf8CodesIteratorStrict
        };
        Ok(vec![Value::Builtin(iterator), state, Value::Integer(0)])
    }

    /// Match lutf8lib.c: each control value is the last 1-based byte offset,
    /// and the iterator skips continuation bytes before decoding the next
    /// strict or lax codepoint. Invalid bytes raise "invalid UTF-8 code".
    fn utf8_codes_iterator(
        &mut self,
        args: &[Value],
        strict: bool,
    ) -> Result<Vec<Value>, FlowError> {
        let bytes = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        let control = args
            .get(1)
            .copied()
            .and_then(|value| self.exact_integer_operand(value).ok())
            .unwrap_or(0);
        let Ok(mut cursor) = usize::try_from(control) else {
            return Ok(Vec::new());
        };
        while cursor < bytes.len() && bytes[cursor] & 0xC0 == 0x80 {
            if self.steps >= self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += 1;
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return Ok(Vec::new());
        }
        let decoded = lua_utf8_decode(&bytes[cursor..], strict);
        let (code, width) = match decoded {
            Some((code, width))
                if !bytes
                    .get(cursor + width)
                    .is_some_and(|byte| byte & 0xC0 == 0x80) =>
            {
                (code, width)
            }
            _ => {
                let error = self.intern_rooted_string(b"invalid UTF-8 code".to_vec(), args)?;
                return Err(FlowError::Thrown(error));
            }
        };
        if self.steps.saturating_add(width) > self.budget {
            return Err(FlowError::StepBudgetExceeded);
        }
        self.steps += width;
        Ok(vec![
            Value::Integer(cursor as i64 + 1),
            Value::Integer(code),
        ])
    }

    fn utf8_offset_builtin(&mut self, args: &[Value]) -> Result<Value, FlowError> {
        let bytes = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        let Value::Integer(mut remaining) =
            self.checked_math_number("utf8.offset", args, 1, true)?
        else {
            return Err(FlowError::InvalidBytecode);
        };
        let length = bytes.len();
        let default = if remaining >= 0 { 1 } else { length as i64 + 1 };
        let position = self.utf8_optional_integer(args, 2, default, "utf8.offset")?;
        let position = lua_utf8_pos(position, length);
        if !(1..=length as i64 + 1).contains(&position) {
            return Err(self.utf8_argument_error(
                args,
                "utf8.offset",
                2,
                "position out of bounds",
            )?);
        }
        let mut cursor = (position - 1) as usize;
        let is_continuation =
            |index: usize| bytes.get(index).is_some_and(|byte| byte & 0xC0 == 0x80);
        if remaining == 0 {
            while cursor > 0 && is_continuation(cursor) {
                if self.steps >= self.budget {
                    return Err(FlowError::StepBudgetExceeded);
                }
                self.steps += 1;
                cursor -= 1;
            }
            return Ok(Value::Integer(cursor as i64 + 1));
        }
        if is_continuation(cursor) {
            let value = self
                .intern_rooted_string(b"initial position is a continuation byte".to_vec(), args)?;
            return Err(FlowError::Thrown(value));
        }
        if remaining < 0 {
            while remaining < 0 && cursor > 0 {
                loop {
                    if self.steps >= self.budget {
                        return Err(FlowError::StepBudgetExceeded);
                    }
                    self.steps += 1;
                    cursor -= 1;
                    if cursor == 0 || !is_continuation(cursor) {
                        break;
                    }
                }
                remaining += 1;
            }
        } else {
            remaining -= 1;
            while remaining > 0 && cursor < length {
                loop {
                    if self.steps >= self.budget {
                        return Err(FlowError::StepBudgetExceeded);
                    }
                    self.steps += 1;
                    cursor += 1;
                    if !is_continuation(cursor) {
                        break;
                    }
                }
                remaining -= 1;
            }
        }
        if remaining == 0 {
            Ok(Value::Integer(cursor as i64 + 1))
        } else {
            Ok(Value::Nil)
        }
    }

    fn utf8_len_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let text = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        let initial = self.utf8_optional_integer(args, 1, 1, "utf8.len")?;
        let final_pos = self.utf8_optional_integer(args, 2, -1, "utf8.len")?;
        let lax = args.get(3).copied().unwrap_or(Value::Nil).truthy();
        let start = lua_utf8_pos(initial, text.len());
        let end = lua_utf8_pos(final_pos, text.len());
        if start < 1 || start > text.len() as i64 + 1 {
            return Err(self.utf8_argument_error(
                args,
                "utf8.len",
                1,
                "initial position out of bounds",
            )?);
        }
        if end > text.len() as i64 {
            return Err(self.utf8_argument_error(
                args,
                "utf8.len",
                2,
                "final position out of bounds",
            )?);
        }
        let mut cursor = (start - 1) as usize;
        let mut count = 0_i64;
        let last = end.max(0) as usize;
        while cursor < last {
            if self.steps >= self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += 1;
            let Some((_, width)) = lua_utf8_decode(&text[cursor..], !lax) else {
                return Ok(vec![Value::Nil, Value::Integer(cursor as i64 + 1)]);
            };
            cursor += width;
            count += 1;
        }
        Ok(vec![Value::Integer(count)])
    }

    fn utf8_codepoint_builtin(&mut self, args: &[Value]) -> Result<Vec<Value>, FlowError> {
        let text = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        let initial = self.utf8_optional_integer(args, 1, 1, "utf8.codepoint")?;
        let start = lua_utf8_pos(initial, text.len());
        let final_pos = self.utf8_optional_integer(args, 2, start, "utf8.codepoint")?;
        let end = if args.get(2).is_some_and(|v| *v != Value::Nil) {
            lua_utf8_pos(final_pos, text.len())
        } else {
            final_pos
        };
        let lax = args.get(3).copied().unwrap_or(Value::Nil).truthy();
        if start < 1 {
            return Err(self.utf8_argument_error(args, "utf8.codepoint", 1, "out of bounds")?);
        }
        if end > text.len() as i64 {
            return Err(self.utf8_argument_error(args, "utf8.codepoint", 2, "out of bounds")?);
        }
        if start > end {
            return Ok(Vec::new());
        }
        if (end - start + 1) as usize > MAX_STACK {
            return Err(FlowError::TooLarge);
        }
        let mut cursor = (start - 1) as usize;
        let mut values = Vec::new();
        while cursor < end as usize {
            if self.steps >= self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += 1;
            let Some((code, width)) = lua_utf8_decode(&text[cursor..], !lax) else {
                let error = self.intern_rooted_string(b"invalid UTF-8 code".to_vec(), args)?;
                return Err(FlowError::Thrown(error));
            };
            values.push(Value::Integer(code));
            cursor += width;
        }
        Ok(values)
    }

    /// First-party bounded byte-string functions. Root the complete argument
    /// list if the operation allocates under a string-pool quota.
    fn string_builtin(&mut self, builtin: Builtin, args: &[Value]) -> Result<Value, FlowError> {
        if builtin == Builtin::StringChar {
            let mut bytes = Vec::with_capacity(args.len().min(MAX_STRING_POOL_BYTES + 1));
            for &value in args {
                let number = self
                    .exact_integer_operand(value)
                    .or_else(|_| self.integer_operand(value))?;
                let byte = u8::try_from(number).map_err(|_| FlowError::TypeError)?;
                bytes.push(byte);
                if bytes.len() > MAX_STRING_POOL_BYTES {
                    return Err(FlowError::TooLarge);
                }
            }
            return self.intern_rooted_string(bytes, args);
        }
        let text = self.string_bytes(*args.first().ok_or(FlowError::TypeError)?)?;
        match builtin {
            Builtin::StringFormat => {
                // Deliberately bounded no-width/no-precision directives.
                // Numeric specifiers operate on Lua's signed/unsigned
                // 64-bit integer payload, never on host pointer addresses.
                let mut output = Vec::new();
                let mut index = 0usize;
                let mut argument = 1usize;
                while index < text.len() {
                    if self.steps >= self.budget {
                        return Err(FlowError::StepBudgetExceeded);
                    }
                    self.steps += 1;
                    if text[index] != b'%' {
                        output.push(text[index]);
                        index += 1;
                    } else {
                        let mut cursor = index + 1;
                        let mut left_align = false;
                        let mut zero_pad = false;
                        let mut plus_sign = false;
                        let mut space_sign = false;
                        let mut alternate = false;
                        while let Some(flag) = text.get(cursor).copied() {
                            match flag {
                                b'-' => left_align = true,
                                b'0' => zero_pad = true,
                                b'+' => plus_sign = true,
                                b' ' => space_sign = true,
                                b'#' => alternate = true,
                                _ => break,
                            }
                            cursor += 1;
                        }
                        let mut width = 0usize;
                        while let Some(byte @ b'0'..=b'9') = text.get(cursor).copied() {
                            width = width
                                .checked_mul(10)
                                .and_then(|n| n.checked_add(usize::from(byte - b'0')))
                                .ok_or(FlowError::TooLarge)?;
                            if width > MAX_STRING_POOL_BYTES {
                                return Err(FlowError::TooLarge);
                            }
                            cursor += 1;
                        }
                        let mut precision = None;
                        if text.get(cursor) == Some(&b'.') {
                            cursor += 1;
                            let mut digits = 0usize;
                            while let Some(byte @ b'0'..=b'9') = text.get(cursor).copied() {
                                digits = digits
                                    .checked_mul(10)
                                    .and_then(|n| n.checked_add(usize::from(byte - b'0')))
                                    .ok_or(FlowError::TooLarge)?;
                                if digits > MAX_STRING_POOL_BYTES {
                                    return Err(FlowError::TooLarge);
                                }
                                cursor += 1;
                            }
                            precision = Some(digits);
                        }
                        let specifier = *text.get(cursor).ok_or(FlowError::TypeError)?;
                        index = cursor + 1;
                        if specifier == b'%'
                            && !left_align
                            && !zero_pad
                            && !plus_sign
                            && !space_sign
                            && !alternate
                            && width == 0
                            && precision.is_none()
                        {
                            output.push(b'%');
                        } else {
                            let numeric =
                                matches!(specifier, b'd' | b'i' | b'u' | b'o' | b'x' | b'X');
                            let floating =
                                matches!(specifier, b'f' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A');
                            if (left_align
                                || width > 0
                                || precision.is_some()
                                || zero_pad
                                || plus_sign
                                || space_sign
                                || alternate)
                                && !matches!(
                                    specifier,
                                    b's' | b'd'
                                        | b'i'
                                        | b'u'
                                        | b'o'
                                        | b'x'
                                        | b'X'
                                        | b'c'
                                        | b'f'
                                        | b'e'
                                        | b'E'
                                        | b'g'
                                        | b'G'
                                        | b'a'
                                        | b'A'
                                )
                            {
                                return Err(FlowError::Unsupported(
                                    "Lua width and precision for this format are not implemented"
                                        .into(),
                                ));
                            }
                            // Lua 5.4 format flags vary by directive:
                            // signed d/i: -+0[space]; unsigned u: -0;
                            // octal/hex: -#0; character/string: - only.
                            let invalid_flag = match specifier {
                                b'd' | b'i' => alternate,
                                b'u' => plus_sign || space_sign || alternate,
                                b'o' | b'x' | b'X' => plus_sign || space_sign,
                                b's' | b'c' => zero_pad || plus_sign || space_sign || alternate,
                                b'f' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => false,
                                _ => false,
                            };
                            if invalid_flag || (specifier == b'c' && precision.is_some()) {
                                return Err(FlowError::Unsupported(
                                    "Lua format flags or precision invalid for directive".into(),
                                ));
                            }
                            if matches!(specifier, b'a' | b'A') && precision.is_some_and(|p| p > 99)
                            {
                                return Err(FlowError::TooLarge);
                            }
                            let value = *args.get(argument).ok_or(FlowError::TypeError)?;
                            argument += 1;
                            let mut part = match specifier {
                                b'a' | b'A' => {
                                    let operand = self.arithmetic_operand(value)?;
                                    let number = match operand {
                                        Value::Integer(integer) => integer as f64,
                                        Value::Float(bits) => f64::from_bits(bits),
                                        _ => return Err(FlowError::TypeError),
                                    };
                                    // Plain hexadecimal-float directives share
                                    // Lua %q's exact finite bit representation,
                                    // but C %a/%A spell nonfinite values as
                                    // inf/nan instead of source expressions.
                                    let mut formatted = if number.is_nan() {
                                        b"nan".to_vec()
                                    } else if number.is_infinite() {
                                        if number.is_sign_negative() {
                                            b"-inf".to_vec()
                                        } else {
                                            b"inf".to_vec()
                                        }
                                    } else if let Some(digits) = precision {
                                        Self::formatted_hex_float_precision(
                                            number.to_bits(),
                                            digits,
                                        )?
                                    } else {
                                        Self::quoted_lua_float(number.to_bits())
                                    };
                                    if specifier == b'A' {
                                        formatted.make_ascii_uppercase();
                                    }
                                    if alternate && number.is_finite() && !formatted.contains(&b'.')
                                    {
                                        let exponent = formatted
                                            .iter()
                                            .position(|byte| matches!(*byte, b'p' | b'P'))
                                            .ok_or(FlowError::InvalidBytecode)?;
                                        formatted.insert(exponent, b'.');
                                    }
                                    if !number.is_sign_negative() {
                                        if plus_sign {
                                            formatted.insert(0, b'+');
                                        } else if space_sign {
                                            formatted.insert(0, b' ');
                                        }
                                    }
                                    formatted
                                }
                                b'f' | b'e' | b'E' | b'g' | b'G' => {
                                    let operand = self.arithmetic_operand(value)?;
                                    let number = match operand {
                                        Value::Integer(integer) => integer as f64,
                                        Value::Float(bits) => f64::from_bits(bits),
                                        _ => return Err(FlowError::TypeError),
                                    };
                                    // Lua's scientific conversion uses a
                                    // signed exponent with at least two
                                    // digits; Rust prints a shorter exponent.
                                    let digits = precision.unwrap_or(6);
                                    let mut formatted = match specifier {
                                        b'e' => format!("{:.*e}", digits, number).into_bytes(),
                                        b'E' => format!("{:.*E}", digits, number).into_bytes(),
                                        b'g' | b'G' => Self::lua_general_float(
                                            number,
                                            digits,
                                            specifier == b'G',
                                            alternate,
                                        )?,
                                        _ => format!("{:.*}", digits, number).into_bytes(),
                                    };
                                    if matches!(specifier, b'e' | b'E') {
                                        if let Some(at) = formatted
                                            .iter()
                                            .position(|byte| *byte == b'e' || *byte == b'E')
                                        {
                                            let power = std::str::from_utf8(&formatted[at + 1..])
                                                .map_err(|_| FlowError::InvalidBytecode)?
                                                .parse::<i32>()
                                                .map_err(|_| FlowError::InvalidBytecode)?;
                                            let mut normalized = formatted[..at].to_vec();
                                            if alternate && digits == 0 && number.is_finite() {
                                                normalized.push(b'.');
                                            }
                                            normalized.push(specifier);
                                            normalized.extend_from_slice(
                                                format!("{power:+03}").as_bytes(),
                                            );
                                            formatted = normalized;
                                        }
                                    } else if specifier == b'f'
                                        && alternate
                                        && digits == 0
                                        && number.is_finite()
                                    {
                                        formatted.push(b'.');
                                    }
                                    if !number.is_sign_negative() {
                                        if plus_sign {
                                            formatted.insert(0, b'+');
                                        } else if space_sign {
                                            formatted.insert(0, b' ');
                                        }
                                    }
                                    formatted
                                }
                                b's' => {
                                    // luaL_tolstring performs Lua's tostring
                                    // protocol, including table __tostring
                                    // callbacks, without consulting a global
                                    // tostring override.
                                    let converted = self.tostring_builtin(value, args)?;
                                    self.string_bytes(converted)?
                                }
                                b'q' => match value {
                                    Value::Nil => b"nil".to_vec(),
                                    Value::Boolean(true) => b"true".to_vec(),
                                    Value::Boolean(false) => b"false".to_vec(),
                                    Value::Integer(integer) if integer == i64::MIN => {
                                        // Lua quotes the minimum signed integer in
                                        // hexadecimal, so it can be parsed without
                                        // an out-of-range positive decimal literal.
                                        b"0x8000000000000000".to_vec()
                                    }
                                    Value::Integer(integer) => integer.to_string().into_bytes(),
                                    Value::Float(bits) => Self::quoted_lua_float(bits),
                                    Value::String(_) => {
                                        let bytes = self.string_bytes(value)?;
                                        let mut quoted = vec![b'"'];
                                        for (index, &byte) in bytes.iter().enumerate() {
                                            if byte == b'"' || byte == b'\\' || byte == b'\n' {
                                                quoted.push(b'\\');
                                                quoted.push(byte);
                                            } else if byte.is_ascii_control() {
                                                let decimal = if bytes
                                                    .get(index + 1)
                                                    .is_some_and(u8::is_ascii_digit)
                                                {
                                                    format!("\\{:03}", byte)
                                                } else {
                                                    format!("\\{}", byte)
                                                };
                                                quoted.extend_from_slice(decimal.as_bytes());
                                            } else {
                                                quoted.push(byte);
                                            }
                                        }
                                        quoted.push(b'"');
                                        quoted
                                    }
                                    _ => {
                                        return Err(FlowError::Unsupported(
                                            "Lua float and non-scalar %q quoting is not implemented"
                                                .into(),
                                        ));
                                    }
                                },
                                b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'c' => {
                                    let integer = self
                                        .exact_integer_operand(value)
                                        .or_else(|_| self.integer_operand(value))?;
                                    match specifier {
                                        b'd' | b'i' => integer.to_string().into_bytes(),
                                        b'u' => (integer as u64).to_string().into_bytes(),
                                        b'o' => format!("{:o}", integer as u64).into_bytes(),
                                        b'x' => format!("{:x}", integer as u64).into_bytes(),
                                        b'X' => format!("{:X}", integer as u64).into_bytes(),
                                        b'c' => vec![integer as u8],
                                        _ => unreachable!("numeric specifier prevalidated"),
                                    }
                                }
                                _ => {
                                    return Err(FlowError::Unsupported(
                                        "Lua format width, precision, floats and quoting are not implemented"
                                            .into(),
                                    ));
                                }
                            };
                            if specifier == b's' {
                                // Lua's modified %s uses C's NUL-terminated
                                // formatter and rejects embedded zeros even
                                // if the requested precision would hide one.
                                if (left_align || width > 0 || precision.is_some())
                                    && part.contains(&0)
                                {
                                    return Err(FlowError::TypeError);
                                }
                                if let Some(precision) = precision {
                                    part.truncate(precision);
                                }
                            } else if numeric {
                                // Lua's integer formatting uses printf-style
                                // signed magnitude, unsigned 64-bit payload,
                                // decimal precision and width. Precision
                                // suppresses the zero flag; negative signs
                                // precede any precision/width zero padding.
                                let negative =
                                    matches!(specifier, b'd' | b'i') && part.first() == Some(&b'-');
                                if negative {
                                    part.remove(0);
                                }
                                if let Some(minimum_digits) = precision {
                                    if minimum_digits == 0 && part == b"0" {
                                        part.clear();
                                    } else if part.len() < minimum_digits {
                                        let mut padded = vec![b'0'; minimum_digits - part.len()];
                                        padded.extend_from_slice(&part);
                                        part = padded;
                                    }
                                }
                                if matches!(specifier, b'o' | b'x' | b'X') && alternate {
                                    if specifier == b'o' {
                                        if part.first() != Some(&b'0') {
                                            part.insert(0, b'0');
                                        }
                                    } else if part.iter().any(|byte| *byte != b'0') {
                                        part.insert(0, if specifier == b'X' { b'X' } else { b'x' });
                                        part.insert(0, b'0');
                                    }
                                }
                                if negative {
                                    part.insert(0, b'-');
                                } else if matches!(specifier, b'd' | b'i') {
                                    if plus_sign {
                                        part.insert(0, b'+');
                                    } else if space_sign {
                                        part.insert(0, b' ');
                                    }
                                }
                            }
                            if (specifier == b's' || specifier == b'c' || numeric || floating)
                                && part.len() < width
                            {
                                let padding = width - part.len();
                                if left_align {
                                    part.extend(std::iter::repeat_n(b' ', padding));
                                } else if (numeric && zero_pad && precision.is_none())
                                    || (floating && zero_pad)
                                {
                                    let sign = usize::from(matches!(
                                        part.first(),
                                        Some(b'-' | b'+' | b' ')
                                    ));
                                    let hex_prefix = ((matches!(specifier, b'x' | b'X')
                                        && alternate)
                                        || matches!(specifier, b'a' | b'A'))
                                        && part.get(sign) == Some(&b'0')
                                        && part
                                            .get(sign + 1)
                                            .is_some_and(|byte| matches!(*byte, b'x' | b'X'));
                                    let prefix = sign + if hex_prefix { 2 } else { 0 };
                                    let mut padded = Vec::with_capacity(width);
                                    padded.extend_from_slice(&part[..prefix]);
                                    padded.extend(std::iter::repeat_n(b'0', padding));
                                    padded.extend_from_slice(&part[prefix..]);
                                    part = padded;
                                } else {
                                    let mut padded = vec![b' '; padding];
                                    padded.extend_from_slice(&part);
                                    part = padded;
                                }
                            }
                            if self.steps.saturating_add(part.len()) > self.budget {
                                return Err(FlowError::StepBudgetExceeded);
                            }
                            self.steps += part.len();
                            output.extend_from_slice(&part);
                        }
                    }
                    if output.len() > MAX_STRING_POOL_BYTES {
                        return Err(FlowError::TooLarge);
                    }
                }
                self.intern_rooted_string(output, args)
            }
            Builtin::StringLen => Ok(Value::Integer(text.len() as i64)),
            Builtin::StringLower | Builtin::StringUpper | Builtin::StringReverse => {
                if self.steps.saturating_add(text.len()) > self.budget {
                    return Err(FlowError::StepBudgetExceeded);
                }
                self.steps += text.len();
                let mut result = text;
                match builtin {
                    Builtin::StringLower => result.make_ascii_lowercase(),
                    Builtin::StringUpper => result.make_ascii_uppercase(),
                    Builtin::StringReverse => result.reverse(),
                    _ => unreachable!("byte transformation guarded above"),
                }
                self.intern_rooted_string(result, args)
            }
            Builtin::StringRep => {
                let count = *args.get(1).ok_or(FlowError::TypeError)?;
                let count = self
                    .exact_integer_operand(count)
                    .or_else(|_| self.integer_operand(count))?;
                let separator = match args.get(2).copied() {
                    Some(value) if value != Value::Nil => self.string_bytes(value)?,
                    _ => Vec::new(),
                };
                if count <= 0 || (text.is_empty() && separator.is_empty()) {
                    return self.intern_rooted_string(Vec::new(), args);
                }
                let count = usize::try_from(count).map_err(|_| FlowError::TooLarge)?;
                let total = text
                    .len()
                    .checked_mul(count)
                    .and_then(|bytes| {
                        separator
                            .len()
                            .checked_mul(count - 1)
                            .and_then(|separators| bytes.checked_add(separators))
                    })
                    .ok_or(FlowError::TooLarge)?;
                if total > MAX_STRING_POOL_BYTES {
                    return Err(FlowError::TooLarge);
                }
                if self.steps.saturating_add(total) > self.budget {
                    return Err(FlowError::StepBudgetExceeded);
                }
                self.steps += total;
                let mut result = Vec::with_capacity(total);
                for index in 0..count {
                    if index > 0 {
                        result.extend_from_slice(&separator);
                    }
                    result.extend_from_slice(&text);
                }
                self.intern_rooted_string(result, args)
            }
            Builtin::StringSub => {
                let start = *args.get(1).ok_or(FlowError::TypeError)?;
                let start = self.lua_checked_integer_argument(start)?;
                let end = args.get(2).copied().unwrap_or(Value::Nil);
                let end = if end == Value::Nil {
                    -1
                } else {
                    self.lua_checked_integer_argument(end)?
                };
                let length = text.len() as i64;
                let begin = Self::string_offset(start, length).max(1);
                let finish = Self::string_offset(end, length).min(length);
                let result = if begin > finish {
                    Vec::new()
                } else {
                    text[(begin - 1) as usize..finish as usize].to_vec()
                };
                self.intern_rooted_string(result, args)
            }
            _ => Err(FlowError::InvalidBytecode),
        }
    }

    /// Lua 5.4 %g/%G uses significant digits, switching to scientific
    /// when the rounded exponent is below -4 or >= the precision.
    /// Trailing fractional zeros are removed unless '#' is present.
    fn lua_general_float(
        number: f64,
        precision: usize,
        uppercase: bool,
        alternate: bool,
    ) -> Result<Vec<u8>, FlowError> {
        let digits = precision.max(1);
        // Lua's format precision is limited to two decimal digits.
        if digits > 99 {
            return Err(FlowError::TooLarge);
        }
        if !number.is_finite() {
            let label = if number.is_nan() { "nan" } else { "inf" };
            let mut out = if number.is_sign_negative() {
                format!("-{label}")
            } else {
                label.to_owned()
            };
            if uppercase {
                out.make_ascii_uppercase();
            }
            return Ok(out.into_bytes());
        }
        let scientific = format!("{:.*e}", digits - 1, number);
        let (mantissa, exponent) = scientific
            .split_once('e')
            .ok_or(FlowError::InvalidBytecode)?;
        let exponent: i32 = exponent.parse().map_err(|_| FlowError::InvalidBytecode)?;
        let mut out = if exponent < -4 || exponent >= digits as i32 {
            let mut significance = mantissa.to_owned();
            if !alternate {
                if significance.contains('.') {
                    while significance.ends_with('0') {
                        significance.pop();
                    }
                    if significance.ends_with('.') {
                        significance.pop();
                    }
                }
            } else if !significance.contains('.') {
                significance.push('.');
            }
            let marker = if uppercase { 'E' } else { 'e' };
            format!("{significance}{marker}{exponent:+03}")
        } else {
            let decimals = (digits as i32 - exponent - 1).max(0) as usize;
            let mut fixed = format!("{number:.decimals$}");
            if !alternate {
                if fixed.contains('.') {
                    while fixed.ends_with('0') {
                        fixed.pop();
                    }
                    if fixed.ends_with('.') {
                        fixed.pop();
                    }
                }
            } else if !fixed.contains('.') {
                fixed.push('.');
            }
            fixed
        };
        if uppercase {
            out.make_ascii_uppercase();
        }
        Ok(out.into_bytes())
    }

    /// Render an explicit-precision C/Lua hexadecimal float without libc.
    /// Binary64 has 13 hexadecimal fraction digits. For fewer digits, round
    /// discarded bits to nearest with ties to even, allowing the leading
    /// hex digit to carry from 1 to 2 without changing the printed exponent.
    /// For more digits, append exact trailing zeroes. This is bounded by the
    /// source format parser's two-digit precision cap.
    fn formatted_hex_float_precision(bits: u64, precision: usize) -> Result<Vec<u8>, FlowError> {
        if precision > 99 {
            return Err(FlowError::TooLarge);
        }
        let number = f64::from_bits(bits);
        if !number.is_finite() {
            return Err(FlowError::InvalidBytecode);
        }
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1u64 << 52) - 1);
        let leading = u64::from(exponent_bits != 0);
        let exponent = if exponent_bits == 0 {
            if fraction == 0 { 0 } else { -1022 }
        } else {
            exponent_bits - 1023
        };
        let mut output = if precision <= 13 {
            let kept_bits = precision * 4;
            let shift = 52 - kept_bits;
            let significand = (leading << 52) | fraction;
            let mut rounded = significand >> shift;
            if shift != 0 {
                let remainder = significand & ((1u64 << shift) - 1);
                let half = 1u64 << (shift - 1);
                if remainder > half || (remainder == half && rounded & 1 == 1) {
                    rounded += 1;
                }
            }
            let integer_digit = rounded >> kept_bits;
            if precision == 0 {
                format!("0x{integer_digit:x}p{exponent:+}")
            } else {
                let fractional = rounded & ((1u64 << kept_bits) - 1);
                format!(
                    "0x{integer_digit:x}.{fractional:0width$x}p{exponent:+}",
                    width = precision
                )
            }
        } else {
            format!(
                "0x{leading:x}.{fraction:013x}{}p{exponent:+}",
                "0".repeat(precision - 13)
            )
        };
        if number.is_sign_negative() {
            output.insert(0, '-');
        }
        Ok(output.into_bytes())
    }

    /// Lua 5.4 %q emits a round-trippable float constant. The common
    /// IEEE-754 double build uses C %a's normalized hexadecimal spelling;
    /// nonfinite values use Lua source expressions/literals instead.
    fn quoted_lua_float(bits: u64) -> Vec<u8> {
        let number = f64::from_bits(bits);
        if number.is_nan() {
            return b"(0/0)".to_vec();
        }
        if number.is_infinite() {
            return if number.is_sign_negative() {
                b"-1e9999".to_vec()
            } else {
                b"1e9999".to_vec()
            };
        }
        let negative = number.is_sign_negative();
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1u64 << 52) - 1);
        let mut result = if fraction == 0 {
            if exponent_bits == 0 {
                String::from("0x0p+0")
            } else {
                format!("0x1p{:+}", exponent_bits - 1023)
            }
        } else {
            let digits = format!("{fraction:013x}");
            let trimmed = digits.trim_end_matches('0');
            if exponent_bits == 0 {
                format!("0x0.{trimmed}p-1022")
            } else {
                format!("0x1.{trimmed}p{:+}", exponent_bits - 1023)
            }
        };
        if negative {
            result.insert(0, '-');
        }
        result.into_bytes()
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

    /// Collect on string quota pressure only at boundaries that explicitly
    /// pin all in-flight Lua operands, including builtin arguments held by
    /// host-side Rust vectors. Other interners may still hold unrooted
    /// intermediate values and retain the strict noncollecting quota.
    fn intern_rooted_string(
        &mut self,
        bytes: Vec<u8>,
        roots: &[Value],
    ) -> Result<Value, FlowError> {
        let pool_full = (self.strings.len() >= MAX_STRINGS && self.free_strings.is_empty())
            || self.string_bytes.saturating_add(bytes.len()) > MAX_STRING_POOL_BYTES;
        if self.automatic_gc
            && bytes.len() <= MAX_STRING_POOL_BYTES
            && pool_full
            && !self.string_index.contains_key(bytes.as_slice())
        {
            let frame = self
                .gc_frame_roots
                .last()
                .ok_or(FlowError::InvalidBytecode)?;
            let slots = frame.slots.clone();
            let prepared = frame.prepared.clone();
            self.collect_root_tables(roots, &slots, &prepared)?;
        }
        self.intern_string(bytes)
    }

    fn intern_string(&mut self, bytes: Vec<u8>) -> Result<Value, FlowError> {
        if let Some(&handle) = self.string_index.get(bytes.as_slice()) {
            return Ok(Value::String(handle));
        }
        if (self.strings.len() >= MAX_STRINGS && self.free_strings.is_empty())
            || bytes.len() > MAX_STRING_POOL_BYTES
            || self.string_bytes > MAX_STRING_POOL_BYTES.saturating_sub(bytes.len())
        {
            return Err(FlowError::TooLarge);
        }
        let handle = if let Some(handle) = self.free_strings.pop() {
            if !self.retired_strings.remove(&handle) {
                return Err(FlowError::InvalidBytecode);
            }
            *self
                .strings
                .get_mut(handle)
                .ok_or(FlowError::InvalidBytecode)? = bytes.clone();
            handle
        } else {
            let handle = self.strings.len();
            self.strings.push(bytes.clone());
            handle
        };
        self.string_bytes += bytes.len();
        self.string_index.insert(bytes, handle);
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
        if cell == usize::MAX {
            let fresh = self.allocate_cell()?;
            *slots.get_mut(slot).ok_or(FlowError::InvalidBytecode)? = fresh;
            return Ok(fresh);
        }
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
            .and_then(Option::as_ref)
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

    /// Release one VM call's lexical allocations and GC-root entry.
    /// Captured cells remain live until their owning closure is collected.
    fn release_call_frame(&mut self) -> Result<(), FlowError> {
        self.gc_frame_roots
            .pop()
            .ok_or(FlowError::InvalidBytecode)?;
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
        Ok(())
    }

    fn prepare_call_frame(
        &mut self,
        slot_count: usize,
        initial: &[(usize, Value)],
        captures: &[(usize, usize)],
        varargs: &[Value],
    ) -> Result<OwnedCallFrame, FlowError> {
        if slot_count > MAX_SLOTS {
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
        Ok(OwnedCallFrame {
            pc: 0,
            slots,
            stack: Vec::new(),
            prepared: Vec::new(),
            last_multiple: None,
            varargs: varargs.to_vec(),
        })
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
        self.gc_frame_roots.push(GcFrameRoots::default());
        let result = self.run_inner(instructions, slot_count, initial, captures, varargs);
        self.release_call_frame()?;
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
        if instructions.len() > MAX_CODE {
            return Err(FlowError::TooLarge);
        }
        let mut scheduler = OwnedScheduler {
            instructions: instructions.to_vec(),
            root: self.prepare_call_frame(slot_count, initial, captures, varargs)?,
            active: Vec::new(),
        };
        let result = self.drive_scheduler(&mut scheduler);
        while scheduler.active.pop().is_some() {
            self.release_call_frame()?;
            self.depth -= 1;
        }
        result
    }

    /// Run an owned Lua continuation to return or error. Coroutine support
    /// can later retain this scheduler when execution yields.
    fn drive_scheduler(&mut self, scheduler: &mut OwnedScheduler) -> Result<Vec<Value>, FlowError> {
        let OwnedScheduler {
            instructions,
            root,
            active,
        } = scheduler;
        loop {
            let progress = if let Some(child) = active.last_mut() {
                self.execute_frame(&child.instructions, &mut child.frame, FRAME_STEP_QUANTUM)?
            } else {
                self.execute_frame(instructions, &mut *root, FRAME_STEP_QUANTUM)?
            };
            match progress {
                FrameProgress::Paused => continue,
                FrameProgress::Call { handle, args, many } => {
                    // Child calls execute on an explicit owned frame stack;
                    // native callbacks may still nest a separate scheduler.
                    let closure = self
                        .closures
                        .get(handle)
                        .and_then(Option::as_ref)
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
                    if function.instructions.len() > MAX_CODE {
                        return Err(FlowError::TooLarge);
                    }
                    let initial: Vec<_> = function
                        .parameters
                        .iter()
                        .enumerate()
                        .map(|(i, slot)| (*slot, args.get(i).copied().unwrap_or(Value::Nil)))
                        .collect();
                    let varargs = if function.vararg && args.len() > function.parameters.len() {
                        &args[function.parameters.len()..]
                    } else {
                        &[]
                    };
                    self.depth += 1;
                    self.frame_allocations.push(Vec::new());
                    self.gc_frame_roots.push(GcFrameRoots::default());
                    let child_frame = match self.prepare_call_frame(
                        function.slots,
                        &initial,
                        &closure.captures,
                        varargs,
                    ) {
                        Ok(child) => child,
                        Err(error) => {
                            self.release_call_frame()?;
                            self.depth -= 1;
                            return Err(error);
                        }
                    };
                    active.push(ActiveLuaCall {
                        instructions: function.instructions,
                        frame: child_frame,
                        many,
                    });
                }
                FrameProgress::Returned(values) => {
                    if let Some(child) = active.pop() {
                        self.release_call_frame()?;
                        self.depth -= 1;
                        let parent = if let Some(parent) = active.last_mut() {
                            &mut parent.frame
                        } else {
                            &mut *root
                        };
                        if child.many {
                            parent.last_multiple = Some(values.len());
                            parent.stack.extend(values);
                        } else {
                            parent
                                .stack
                                .push(values.into_iter().next().unwrap_or(Value::Nil));
                        }
                        if parent.stack.len() > MAX_STACK {
                            return Err(FlowError::TooLarge);
                        }
                    } else {
                        return Ok(values);
                    }
                }
            }
        }
    }

    /// Execute up to one instruction quantum against a retained frame.
    /// A paused frame is intact: resumption continues at the next instruction
    /// without replaying a prior bytecode operation or resetting the quota.
    /// Native callback paths still execute synchronously; this is not Lua yield.
    fn execute_frame(
        &mut self,
        instructions: &[Op],
        frame: &mut OwnedCallFrame,
        quantum: usize,
    ) -> Result<FrameProgress, FlowError> {
        let OwnedCallFrame {
            pc,
            slots,
            stack,
            prepared,
            last_multiple,
            varargs,
        } = frame;
        let mut executed = 0usize;
        while *pc < instructions.len() {
            if executed >= quantum {
                return Ok(FrameProgress::Paused);
            }
            executed += 1;
            if self.steps >= self.budget {
                return Err(FlowError::StepBudgetExceeded);
            }
            self.steps += 1;
            // Snapshot *before* the operation. A metamethod or function call
            // can suspend this frame after popping operands from its stack.
            // Unlike borrowed Rust pointers, these owned roots remain valid
            // while nested execution mutates the heap.
            let roots = self
                .gc_frame_roots
                .last_mut()
                .ok_or(FlowError::InvalidBytecode)?;
            roots.stack.clear();
            roots.stack.extend_from_slice(stack);
            roots.slots.clear();
            roots.slots.extend_from_slice(slots);
            roots.prepared.clear();
            roots.prepared.extend_from_slice(prepared);
            let next = *pc + 1;
            match &instructions[*pc] {
                Op::Constant(value) => stack.push(*value),
                Op::ReadGlobal(key) => stack.push(self.read_global(*key)?),
                Op::NewFunction(index) => {
                    if *index >= self.program.functions.len() {
                        return Err(FlowError::InvalidBytecode);
                    }
                    if self.automatic_gc
                        && self.closures.len() >= MAX_CLOSURES
                        && self.free_closures.is_empty()
                    {
                        self.collect_root_tables(stack, slots, prepared)?;
                    }
                    if self.closures.len() >= MAX_CLOSURES && self.free_closures.is_empty() {
                        return Err(FlowError::TooLarge);
                    }
                    let spec = &self.program.functions[*index];
                    let mut captures = Vec::new();
                    for &(parent_slot, child_slot) in &spec.captures {
                        let cell = *slots.get(parent_slot).ok_or(FlowError::InvalidBytecode)?;
                        self.captured_cells.insert(cell);
                        captures.push((child_slot, cell));
                    }
                    let closure = Closure {
                        function: *index,
                        captures,
                    };
                    let handle = if let Some(recycled) = self.free_closures.pop() {
                        let slot = self
                            .closures
                            .get_mut(recycled)
                            .ok_or(FlowError::InvalidBytecode)?;
                        if slot.is_some() {
                            return Err(FlowError::InvalidBytecode);
                        }
                        *slot = Some(closure);
                        recycled
                    } else {
                        let index = self.closures.len();
                        self.closures.push(Some(closure));
                        index
                    };
                    stack.push(Value::Function(handle));
                }
                Op::Call(argc) | Op::CallMany(argc) | Op::CallSpread { prefix: argc, .. } => {
                    let arity = if matches!(instructions[*pc], Op::CallSpread { .. }) {
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
                                    | Builtin::Warn
                                    | Builtin::Pcall
                                    | Builtin::Xpcall
                                    | Builtin::Assert
                                    | Builtin::MathType
                                    | Builtin::MathToInteger
                                    | Builtin::MathSqrt
                                    | Builtin::MathUlt
                                    | Builtin::MathFmod
                                    | Builtin::MathModf
                                    | Builtin::MathSin
                                    | Builtin::MathCos
                                    | Builtin::MathTan
                                    | Builtin::MathRad
                                    | Builtin::MathDeg
                                    | Builtin::MathExp
                                    | Builtin::MathLog
                                    | Builtin::MathAsin
                                    | Builtin::MathAcos
                                    | Builtin::MathAtan
                                    | Builtin::MathRandom
                                    | Builtin::MathRandomSeed
                                    | Builtin::StringChar
                                    | Builtin::StringGmatchIterator(_)
                                    | Builtin::Utf8Char
                                    | Builtin::TablePack
                                    | Builtin::CollectGarbage
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
                                | Builtin::Warn
                                | Builtin::Select
                                | Builtin::Assert
                                | Builtin::Next
                                | Builtin::Pairs
                                | Builtin::Ipairs
                                | Builtin::IpairsIterator
                                | Builtin::Pcall
                                | Builtin::Xpcall
                                | Builtin::StringByte
                                | Builtin::StringFind
                                | Builtin::StringMatch
                                | Builtin::StringGmatch
                                | Builtin::StringGmatchIterator(_)
                                | Builtin::StringGsub
                                | Builtin::StringUnpack
                                | Builtin::Utf8Len
                                | Builtin::Utf8Codepoint
                                | Builtin::Utf8Codes
                                | Builtin::Utf8CodesIteratorStrict
                                | Builtin::Utf8CodesIteratorLax
                                | Builtin::TableUnpack
                                | Builtin::TableInsert
                                | Builtin::TableSort
                                | Builtin::MathModf
                                | Builtin::MathRandomSeed
                        ) {
                            let returned = match builtin {
                                Builtin::Print => self.print_builtin(&args)?,
                                Builtin::Warn => self.warn_builtin(&args)?,
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
                                Builtin::StringByte => self.string_byte_builtin(&args)?,
                                Builtin::StringFind => self.string_find_literal_builtin(&args)?,
                                Builtin::StringMatch => self.string_match_builtin(&args)?,
                                Builtin::StringGmatch => vec![self.string_gmatch_builtin(&args)?],
                                Builtin::StringGmatchIterator(handle) => {
                                    self.string_gmatch_iterator(handle, &args)?
                                }
                                Builtin::StringGsub => self.string_gsub_builtin(&args)?,
                                Builtin::StringUnpack => {
                                    self.string_pack_dispatch(builtin, &args)?
                                }
                                Builtin::Utf8Len => self.utf8_len_builtin(&args)?,
                                Builtin::Utf8Codepoint => self.utf8_codepoint_builtin(&args)?,
                                Builtin::Utf8Codes => self.utf8_codes_builtin(&args)?,
                                Builtin::Utf8CodesIteratorStrict => {
                                    self.utf8_codes_iterator(&args, true)?
                                }
                                Builtin::Utf8CodesIteratorLax => {
                                    self.utf8_codes_iterator(&args, false)?
                                }
                                Builtin::TableUnpack => self.table_unpack_builtin(&args)?,
                                Builtin::TableInsert => self.table_insert_builtin(&args)?,
                                Builtin::TableSort => self.table_sort_builtin(&args)?,
                                Builtin::MathModf => self.math_modf_builtin(&args)?,
                                Builtin::MathRandomSeed => self.math_randomseed_builtin(&args)?,
                                _ => unreachable!("only multi-result builtins enter this path"),
                            };
                            if matches!(
                                instructions[*pc],
                                Op::CallMany(_) | Op::CallSpread { many: true, .. }
                            ) {
                                *last_multiple = Some(returned.len());
                                stack.extend(returned);
                            } else {
                                stack.push(returned.into_iter().next().unwrap_or(Value::Nil));
                            }
                            if stack.len() > MAX_STACK {
                                return Err(FlowError::TooLarge);
                            }
                            *pc = next;
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
                                self.intern_rooted_string(name.to_vec(), &args)?
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
                            | Builtin::Warn
                            | Builtin::Pcall
                            | Builtin::Xpcall
                            | Builtin::Select
                            | Builtin::Assert
                            | Builtin::Next
                            | Builtin::Pairs
                            | Builtin::Ipairs
                            | Builtin::IpairsIterator
                            | Builtin::StringByte
                            | Builtin::StringFind
                            | Builtin::StringMatch
                            | Builtin::StringGmatch
                            | Builtin::StringGmatchIterator(_)
                            | Builtin::StringGsub
                            | Builtin::StringUnpack
                            | Builtin::Utf8Len
                            | Builtin::Utf8Codepoint
                            | Builtin::Utf8Codes
                            | Builtin::Utf8CodesIteratorStrict
                            | Builtin::Utf8CodesIteratorLax
                            | Builtin::TableUnpack
                            | Builtin::TableInsert
                            | Builtin::TableSort
                            | Builtin::MathModf
                            | Builtin::MathRandomSeed => {
                                unreachable!(
                                    "iteration/selection builtins handled as multiple results"
                                )
                            }
                            Builtin::ToString => self.tostring_builtin(value, &args)?,
                            Builtin::CollectGarbage => self.collectgarbage_builtin(&args)?,
                            Builtin::MathAbs
                            | Builtin::MathFloor
                            | Builtin::MathCeil
                            | Builtin::MathMax
                            | Builtin::MathMin
                            | Builtin::MathType
                            | Builtin::MathToInteger
                            | Builtin::MathSqrt
                            | Builtin::MathUlt
                            | Builtin::MathFmod
                            | Builtin::MathSin
                            | Builtin::MathCos
                            | Builtin::MathTan
                            | Builtin::MathRad
                            | Builtin::MathDeg
                            | Builtin::MathExp
                            | Builtin::MathLog
                            | Builtin::MathAsin
                            | Builtin::MathAcos
                            | Builtin::MathAtan
                            | Builtin::MathRandom => self.math_builtin(builtin, &args)?,
                            Builtin::StringLen
                            | Builtin::StringSub
                            | Builtin::StringChar
                            | Builtin::StringLower
                            | Builtin::StringUpper
                            | Builtin::StringReverse
                            | Builtin::StringRep
                            | Builtin::StringFormat => self.string_builtin(builtin, &args)?,
                            Builtin::StringPack | Builtin::StringPackSize => self
                                .string_pack_dispatch(builtin, &args)?
                                .into_iter()
                                .next()
                                .ok_or(FlowError::InvalidBytecode)?,
                            Builtin::Utf8Char => self.utf8_char_builtin(&args)?,
                            Builtin::Utf8Offset => self.utf8_offset_builtin(&args)?,
                            Builtin::TableConcat => self.table_concat_builtin(&args)?,
                            Builtin::TablePack => self.table_pack_builtin(&args)?,
                            Builtin::TableRemove => self.table_remove_builtin(&args)?,
                            Builtin::TableMove => self.table_move_builtin(&args)?,
                        };
                        if matches!(
                            instructions[*pc],
                            Op::CallMany(_) | Op::CallSpread { many: true, .. }
                        ) {
                            *last_multiple = Some(1);
                        }
                        stack.push(result);
                        if stack.len() > MAX_STACK {
                            return Err(FlowError::TooLarge);
                        }
                        *pc = next;
                        continue;
                    }
                    let Value::Function(handle) = callable else {
                        return Err(FlowError::TypeError);
                    };
                    let many = matches!(
                        instructions[*pc],
                        Op::CallMany(_) | Op::CallSpread { many: true, .. }
                    );
                    *pc = next;
                    return Ok(FrameProgress::Call { handle, args, many });
                }
                Op::VarargSingle => {
                    stack.push(varargs.first().copied().unwrap_or(Value::Nil));
                }
                Op::VarargMany => {
                    *last_multiple = Some(varargs.len());
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
                        let cell = self.iteration_cell(slots, *index)?;
                        *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = Some(value);
                    }
                }
                Op::ClearLocals(indices) => {
                    // The old cell can be held by a captured closure after
                    // the lexical scope exits. Keep those captured cells,
                    // but stop ordinary block variables from pinning their
                    // otherwise unreachable table/function/string values.
                    for &slot in indices {
                        let cell = *slots.get(slot).ok_or(FlowError::InvalidBytecode)?;
                        if self.captured_cells.contains(&cell) {
                            // The old lexical cell remains available only
                            // through closures that captured it. A later
                            // iteration rebinds this slot to a fresh cell.
                            *slots.get_mut(slot).ok_or(FlowError::InvalidBytecode)? = usize::MAX;
                        } else {
                            *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = None;
                        }
                    }
                }
                Op::RenewLocal(slot) => {
                    self.iteration_cell(slots, *slot)?;
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
                    let existing = self
                        .tables
                        .get(handle)
                        .ok_or(FlowError::InvalidBytecode)?
                        .array
                        .len();
                    let growth = end.saturating_sub(existing);
                    if self.automatic_gc
                        && self.table_cells.saturating_add(growth) > MAX_TABLE_CELLS_TOTAL
                    {
                        // Tail expansion bypasses ordinary indexed writes.
                        // Preserve all vararg/multi-return objects while the
                        // collector traverses otherwise unreachable tables.
                        let mut roots = Vec::with_capacity(results.len() + 1);
                        roots.push(Value::Table(handle));
                        roots.extend_from_slice(&results);
                        self.collect_root_tables(&roots, slots, prepared)?;
                    }
                    if self.table_cells.saturating_add(growth) > MAX_TABLE_CELLS_TOTAL {
                        return Err(FlowError::TooLarge);
                    }
                    let cells = &mut self
                        .tables
                        .get_mut(handle)
                        .ok_or(FlowError::InvalidBytecode)?
                        .array;
                    if end > cells.len() {
                        self.table_cells += growth;
                        cells.resize(end, None);
                    }
                    for (offset, value) in results.into_iter().enumerate() {
                        cells[first + offset] = (value != Value::Nil).then_some(value);
                    }
                }
                Op::NewTable(count) | Op::NewTableTail(count) => {
                    let arity = if matches!(instructions[*pc], Op::NewTableTail(_)) {
                        let expanded = last_multiple.take().ok_or(FlowError::InvalidBytecode)?;
                        count.checked_add(expanded).ok_or(FlowError::TooLarge)?
                    } else {
                        *count
                    };
                    if arity > MAX_TABLE_FIELDS || stack.len() < arity {
                        return Err(FlowError::TooLarge);
                    }
                    if self.automatic_gc
                        && ((self.tables.len() >= MAX_TABLES && self.free_tables.is_empty())
                            || self.table_cells.saturating_add(arity) > MAX_TABLE_CELLS_TOTAL)
                    {
                        self.collect_root_tables(stack, slots, prepared)?;
                    }
                    if (self.tables.len() >= MAX_TABLES && self.free_tables.is_empty())
                        || self.table_cells.saturating_add(arity) > MAX_TABLE_CELLS_TOTAL
                    {
                        return Err(FlowError::TooLarge);
                    }
                    let values = stack.split_off(stack.len() - arity);
                    let handle = self.insert_table(TableStorage {
                        array: values
                            .into_iter()
                            .map(|value| (value != Value::Nil).then_some(value))
                            .collect(),
                        hashed: HashMap::new(),
                        metatable: None,
                    })?;
                    self.table_cells += arity;
                    stack.push(Value::Table(handle));
                }
                Op::GetTable => {
                    let key = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let receiver = stack.pop().ok_or(FlowError::InvalidBytecode)?;
                    let table_key = self.table_key(key)?;
                    let handle = match receiver {
                        Value::Table(handle) => handle,
                        Value::String(_) => {
                            // Lua strings share the __index table established by
                            // the string library. Keep this original, writable
                            // library identity even if _G.string is reassigned.
                            // Root both operands while lazily materializing it.
                            self.materialize_string_table(&[receiver, key])?
                        }
                        _ => return Err(FlowError::TypeError),
                    };
                    stack.push(self.lookup_with_metatable(handle, table_key)?);
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
                        *pc = *target;
                        continue;
                    }
                }
                Op::JumpIfFalseKeep(target) | Op::JumpIfTrueKeep(target) => {
                    let value = *stack.last().ok_or(FlowError::InvalidBytecode)?;
                    let should_jump = match instructions[*pc] {
                        Op::JumpIfFalseKeep(_) => !value.truthy(),
                        Op::JumpIfTrueKeep(_) => value.truthy(),
                        _ => unreachable!("guarded opcode"),
                    };
                    if should_jump {
                        if *target > instructions.len() {
                            return Err(FlowError::InvalidBytecode);
                        }
                        *pc = *target;
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
                    *pc = *target;
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
                        *pc = *exit;
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
                    let cell = self.iteration_cell(slots, *variable)?;
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
                                *pc = *exit;
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
                        *pc = *exit;
                        continue;
                    }
                    let control_cell = *slots.get(*control).ok_or(FlowError::InvalidBytecode)?;
                    *self
                        .cells
                        .get_mut(control_cell)
                        .ok_or(FlowError::InvalidBytecode)? = Some(first);
                    for (position, slot) in variables.iter().enumerate() {
                        let cell = self.iteration_cell(slots, *slot)?;
                        let value = returned.get(position).copied().unwrap_or(Value::Nil);
                        *self.cells.get_mut(cell).ok_or(FlowError::InvalidBytecode)? = Some(value);
                    }
                }
                Op::Return => {
                    return Ok(FrameProgress::Returned(vec![
                        stack.pop().ok_or(FlowError::InvalidBytecode)?,
                    ]));
                }
                Op::ReturnMany(count) => {
                    if stack.len() < *count {
                        return Err(FlowError::InvalidBytecode);
                    }
                    return Ok(FrameProgress::Returned(
                        stack.split_off(stack.len() - *count),
                    ));
                }
                Op::ReturnTail(prefix) => {
                    let multiple = last_multiple.take().ok_or(FlowError::InvalidBytecode)?;
                    let count = prefix.saturating_add(multiple);
                    if stack.len() < count {
                        return Err(FlowError::InvalidBytecode);
                    }
                    return Ok(FrameProgress::Returned(
                        stack.split_off(stack.len() - count),
                    ));
                }
            }
            if stack.len() > MAX_STACK {
                return Err(FlowError::TooLarge);
            }
            *pc = next;
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
    /// Captured Lua warning stream independent from script errors.
    pub stderr: Vec<u8>,
    /// Error object captured before execution-local string storage is freed.
    pub error_text: Option<Vec<u8>>,
}

/// Run bounded first-party Lua and capture the restricted print builtin.
/// Errors retain output emitted before failure. No production VM fallback.
pub fn execute_flow_capture_with_args<'a>(
    program: &'a FlowProgram,
    budget: usize,
    script_name: &'a str,
    script_args: &'a [String],
) -> FlowOutcome {
    let string_index = program
        .strings
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, value)| (value, index))
        .collect();
    let string_bytes = program.strings.iter().map(Vec::len).sum();
    let mut execution = Execution {
        program,
        budget,
        steps: 0,
        depth: 0,
        automatic_gc: true,
        tables: Vec::new(),
        free_tables: Vec::new(),
        retired_tables: HashSet::new(),
        gc_registrations: Vec::new(),
        gc_finalized: HashSet::new(),
        running_finalizers: false,
        table_cells: 0,
        closures: Vec::new(),
        free_closures: Vec::new(),
        cells: Vec::new(),
        captured_cells: HashSet::new(),
        free_cells: Vec::new(),
        frame_allocations: Vec::new(),
        gc_frame_roots: Vec::new(),
        gc_temporary_roots: Vec::new(),
        strings: program.strings.clone(),
        string_index,
        string_bytes,
        free_strings: Vec::new(),
        retired_strings: HashSet::new(),
        stdout: Vec::new(),
        stderr: Vec::new(),
        warnings_enabled: false,
        script_name,
        script_args,
        arg_handle: None,
        globals: HashMap::new(),
        global_table: None,
        math_table: None,
        random_state: [0, 0xff, 0, 0],
        gmatch_states: Vec::new(),
        free_gmatch_states: Vec::new(),
        gmatch_bytes: 0,
        string_table: None,
        utf8_table: None,
        table_library: None,
        coroutine_table: None,
        coroutines: Vec::new(),
        current_coroutine: None,
        saved_call_contexts: Vec::new(),
        active_coroutine_entries: Vec::new(),
    };
    // Lua's default seed is intentionally weak/noncryptographic; explicit
    // randomseed(x,y) is deterministic, and has upstream Lua 5.4 parity tests.
    let (seed_a, seed_b) = execution.weak_random_seeds();
    lua_random_seed(&mut execution.random_state, seed_a, seed_b);
    let mut result = execution.run(&program.instructions, program.slots, &[], &[], &[]);
    // Lua closes its state after evaluating the chunk, even after errors.
    // Preserve the original script failure if closing also reaches a bound.
    let shutdown = execution.finalize_on_shutdown();
    if result.is_ok()
        && let Err(error) = shutdown
    {
        result = Err(error);
    }
    let error_text = match &result {
        Err(FlowError::Thrown(Value::String(handle))) => execution.strings.get(*handle).cloned(),
        Err(FlowError::Thrown(Value::Integer(value))) => Some(value.to_string().into_bytes()),
        Err(FlowError::Thrown(Value::Float(bits))) => {
            Some(format!("{:?}", f64::from_bits(*bits)).into_bytes())
        }
        Err(FlowError::Thrown(Value::Boolean(_))) => {
            Some(b"error object is a boolean value".to_vec())
        }
        Err(FlowError::Thrown(Value::Nil)) => Some(b"error object is a nil value".to_vec()),
        Err(FlowError::Thrown(Value::Table(_))) => Some(b"error object is a table value".to_vec()),
        Err(FlowError::Thrown(Value::Function(_) | Value::Builtin(_))) => {
            Some(b"error object is a function value".to_vec())
        }
        _ => None,
    };
    FlowOutcome {
        result,
        stdout: execution.stdout,
        stderr: execution.stderr,
        error_text,
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
    fn owned_activation_stack_enforces_lua_call_depth_without_host_recursion() {
        let source =
            "local function sum(n) if n==0 then return 0 end return n+sum(n-1) end return sum(12)";
        let program = compile_flow_chunk(source).unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Ok(vec![Value::Integer(78)])
        );

        let deep = compile_flow_chunk(
            "local function sum(n) if n==0 then return 0 end return sum(n-1) end return sum(30)",
        )
        .unwrap();
        assert_eq!(
            execute_flow_values(&deep, DEFAULT_STEP_BUDGET),
            Err(FlowError::CallDepthExceeded)
        );
    }

    #[test]
    fn owned_call_checkpoint_preserves_multiple_returns_gc_roots_and_tail_adjustment() {
        let script = "local function triple(n) local t={} for i=1,80 do t[i]=i end collectgarbage('collect') return n,n+1,n+2 end local function swap(a,b,c) return c,b,a end return swap(triple(4))";
        let program = compile_flow_chunk(script).unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Ok(vec![
                Value::Integer(6),
                Value::Integer(5),
                Value::Integer(4),
            ])
        );
        assert_eq!(
            execute_flow_values(&program, 60),
            Err(FlowError::StepBudgetExceeded)
        );
    }

    #[test]
    fn instruction_quantum_preserves_nested_frames_varargs_and_shared_budget() {
        // More than 64 instructions execute in both the enclosing frame
        // and the closure. A pause must not restart a loop or lose varargs.
        let script = "local function sum(...) local total=0 for i=1,70 do total=total+i end return total,... end return sum(7,8)";
        let program = compile_flow_chunk(script).unwrap();
        assert_eq!(
            execute_flow_values(&program, DEFAULT_STEP_BUDGET),
            Ok(vec![
                Value::Integer(2485),
                Value::Integer(7),
                Value::Integer(8)
            ])
        );
        assert_eq!(
            execute_flow_values(&program, 50),
            Err(FlowError::StepBudgetExceeded)
        );
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
    fn explicit_collectgarbage_reports_numeric_full_collection_and_count() {
        let source = "local keep={v=42}; for i=1,2000 do local t={}; t.self=t end; print(type(collectgarbage()), type(collectgarbage('collect')),keep.v); local ok,used=pcall(collectgarbage,'count'); print(ok,type(used),used>=0)";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 200_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"number\tnumber\t42\ntrue\tnumber\ttrue\n");
    }

    #[test]
    fn type_and_math_type_preserve_dynamic_strings_under_quota_pressure() {
        let cases = [
            (
                "local hold=tostring(123456789); for i=1,19000 do local x=tostring(i) end; print(type(hold),math.type(7),hold)",
                b"string\tinteger\t123456789\n".as_slice(),
            ),
            (
                "local hold=tostring(123456789); local function stress() for i=1,19000 do local x=tostring(i) end return 17 end; print(type(hold,stress()),math.type(1.5),hold)",
                b"string\tfloat\t123456789\n".as_slice(),
            ),
            (
                "local hold=tostring(123456789); for i=1,19000 do local x=tostring(i) end; local ok1,t=pcall(type,hold); local ok2,n=pcall(math.type,7); print(ok1,t,ok2,n,hold)",
                b"true\tstring\ttrue\tinteger\t123456789\n".as_slice(),
            ),
        ];
        for (source, expected) in cases {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 3_000_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn block_local_gc_lifetime_ends_without_breaking_live_captures() {
        let source = "local w=setmetatable({}, {__mode='v'}); do local t={answer=42}; w[1]=t end; collectgarbage('collect'); print(w[1]==nil); local f; do local x={answer=17}; f=function() return x.answer end end; collectgarbage('collect'); print(f())";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 25_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"true\n17\n");
    }

    #[test]
    fn invalid_lua_gc_methods_warn_with_call_type_diagnostics() {
        for (finalizer, expected) in [
            ("12", "number"),
            ("true", "boolean"),
            ("'not callable'", "string"),
            ("{}", "table"),
        ] {
            let source = format!(
                "warn('@on'); do local t=setmetatable({{}}, {{__gc={finalizer}}}) end; collectgarbage('collect'); print('alive')"
            );
            let program = compile_flow_chunk(&source).unwrap();
            let outcome = execute_flow_capture(&program, 100_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, b"alive\n", "{source}");
            assert_eq!(
                String::from_utf8_lossy(&outcome.stderr),
                format!(
                    "Lua warning: error in __gc (attempt to call a {expected} value (metamethod '__gc'))\n"
                ),
                "{source}"
            );
        }
    }

    #[test]
    fn gc_callable_table_finalizers_follow_lua54_registration_and_shutdown() {
        for (source, expected) in [
            (
                "local count=0; local callback=setmetatable({}, {__call=function(self, v) count=count+v.n end}); do local t=setmetatable({n=7}, {__gc=callback}) end; collectgarbage('collect'); print(count)",
                b"7\n".as_slice(),
            ),
            (
                "local callback=setmetatable({}, {__call=function(self, v) print('closing',v.n) end}); local t=setmetatable({n=9}, {__gc=callback}); print('body')",
                b"body\nclosing\t9\n".as_slice(),
            ),
            (
                "local first=setmetatable({}, {__call=function(self, v) print('old') end}); local mt={__gc=first}; do local t=setmetatable({},mt) end; mt.__gc=function() print('new') end; collectgarbage('collect')",
                b"new\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 100_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn warn_controls_and_finalizer_errors_follow_lua54() {
        for (source, stdout, stderr) in [
            (
                "warn('muted'); warn('@on'); warn('hello',' world'); warn('@off'); warn('muted'); warn('@on'); print(select('#',warn('restored')))",
                b"0\n".as_slice(),
                b"Lua warning: hello world\nLua warning: restored\n".as_slice(),
            ),
            (
                "warn('@on'); do local t=setmetatable({}, {__gc=function() error('gc-failed',0) end}) end; collectgarbage('collect'); print('alive')",
                b"alive\n".as_slice(),
                b"Lua warning: error in __gc (gc-failed)\n".as_slice(),
            ),
            (
                "warn('@on'); local t=setmetatable({}, {__gc=function() error('shutdown-failed',0) end}); print('body')",
                b"body\n".as_slice(),
                b"Lua warning: error in __gc (shutdown-failed)\n".as_slice(),
            ),
            (
                "do local t=setmetatable({}, {__gc=function() error('muted',0) end}) end; collectgarbage('collect'); print('alive')",
                b"alive\n".as_slice(),
                b"".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 100_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, stdout, "{source}");
            assert_eq!(outcome.stderr, stderr, "{source}");
        }
    }

    #[test]
    fn finalizer_collectgarbage_suppression_has_one_nil_return_value() {
        for (source, expected) in [
            (
                "local count=-1; do local t=setmetatable({}, {__gc=function() count=select('#',collectgarbage('collect')) end}) end; collectgarbage('collect'); print(count,select('#',collectgarbage('collect')))",
                b"1\t1\n".as_slice(),
            ),
            (
                "local t=setmetatable({}, {__gc=function() print('close',select('#',collectgarbage('collect'))) end}); print('body')",
                b"body\nclose\t1\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 100_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn nested_collectgarbage_requests_inside_finalizers_do_not_reenter_heap_sweep() {
        for (source, expected) in [
            (
                "local called=false; do local t=setmetatable({}, {__gc=function() local ok,v=pcall(collectgarbage,'collect'); called=ok and v==nil end}) end; collectgarbage('collect'); print(called)",
                b"true\n".as_slice(),
            ),
            (
                "local t=setmetatable({}, {__gc=function() local ok,v=pcall(collectgarbage,'collect'); print('closing',ok,v) end}); print('body')",
                b"body\nclosing\ttrue\tnil\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 100_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn closing_owned_state_finalizes_live_tables_in_reverse_order() {
        for (source, expected) in [
            (
                "local a=setmetatable({id='A'}, {__gc=function(v) print('close',v.id) end}); local b=setmetatable({id='B'}, {__gc=function(v) print('close',v.id) end}); print('body',a.id,b.id)",
                b"body\tA\tB\nclose\tB\nclose\tA\n".as_slice(),
            ),
            (
                "local a=setmetatable({}, {__gc=function() print('once') end}); a=nil; collectgarbage('collect'); print('body')",
                b"once\nbody\n".as_slice(),
            ),
            (
                "local original=setmetatable({}, {__gc=function() print('original'); setmetatable({}, {__gc=function() print('new') end}) end}); print('body')",
                b"body\noriginal\n".as_slice(),
            ),
            (
                "local a=setmetatable({}, {__gc=function() error('finalizer failure') end}); print('body')",
                b"body\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 100_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn finalized_tables_may_be_registered_again_for_a_later_collection() {
        for (source, expected) in [
            (
                "local calls=0; local mt={}; mt.__gc=function(v) calls=calls+1; if calls==1 then setmetatable(v,mt) end end; do local t=setmetatable({},mt) end; collectgarbage('collect'); print(calls); collectgarbage('collect'); print(calls); collectgarbage('collect'); print(calls)",
                b"1\n2\n2\n".as_slice(),
            ),
            (
                "local calls=0; local saved; local mt={}; mt.__gc=function(v) calls=calls+1; if calls==1 then saved=v end end; do local t=setmetatable({},mt) end; collectgarbage('collect'); print(calls,saved~=nil); setmetatable(saved,mt); saved=nil; collectgarbage('collect'); print(calls); collectgarbage('collect'); print(calls)",
                b"1\ttrue\n2\n2\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 200_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn explicit_gc_finalizes_registered_tables_once_in_reverse_order() {
        for (source, expected) in [
            (
                "local log=''; do local a=setmetatable({id='A'},{__gc=function(v) log=log..v.id end}); local b=setmetatable({id='B'},{__gc=function(v) log=log..v.id end}) end; collectgarbage('collect'); print(log); collectgarbage('collect'); print(log)",
                b"BA\nBA\n".as_slice(),
            ),
            (
                "local called=false; local mt={}; do local t=setmetatable({},mt); mt.__gc=function() called=true end end; collectgarbage('collect'); print(called)",
                b"false\n".as_slice(),
            ),
            (
                "local rescued; do local t=setmetatable({answer=42},{__gc=function(v) rescued=v end}) end; collectgarbage('collect'); print(rescued.answer); rescued=nil; collectgarbage('collect'); print('once')",
                b"42\nonce\n".as_slice(),
            ),
            (
                "local weak=setmetatable({}, {__mode='v'}); local cleared=false; do local x=setmetatable({}, {__gc=function(v) cleared=weak[1]==nil end}); weak[1]=x end; collectgarbage('collect'); print(cleared)",
                b"true\n".as_slice(),
            ),
            (
                "local weak=setmetatable({}, {__mode='k'}); local seen=false; do local x=setmetatable({}, {__gc=function(v) seen=weak[v]==42 end}); weak[x]=42 end; collectgarbage('collect'); print(seen); collectgarbage('collect'); print(next(weak)==nil)",
                b"true\ntrue\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 250_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn lazy_standard_tables_root_pending_keys_during_string_collection() {
        let mut lower = 14_000;
        let mut upper = 16_000;
        let mut verified = None;
        while lower + 1 < upper {
            let middle = lower + (upper - lower) / 2;
            let source = format!(
                "local keep=tostring(123456789); collectgarbage('stop'); \
                 for i=1,{middle} do local s=tostring(i) end; \
                 collectgarbage('restart'); \
                 local before=collectgarbage('count'); \
                 local g=_G; local after=collectgarbage('count'); \
                 print(g._VERSION,g.arg[0],type(g.math),keep,before>after)"
            );
            let program = compile_flow_chunk(&source).unwrap();
            let outcome = execute_flow_capture(&program, 3_000_000);
            match outcome.result {
                Ok(_) => {
                    lower = middle;
                    verified = Some(outcome.stdout);
                }
                Err(FlowError::TooLarge) => upper = middle,
                other => panic!("{source}: unexpected result: {other:?}"),
            }
        }
        assert_eq!(
            verified,
            Some(b"Lua 5.4\t=owned\ttable\t123456789\ttrue\n".to_vec())
        );
        let args = vec!["first".to_owned(), "second".to_owned()];
        let source = "for i=1,19000 do local s=tostring(i) end; print(_G.arg[0],_G.arg[1],_G.arg[2],type(_G.math))";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture_with_args(&program, 3_000_000, "script.lua", &args);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"script.lua\tfirst\tsecond\ttable\n");
    }

    #[test]
    fn type_and_math_type_trigger_real_collection_at_string_quota() {
        // Determine the largest string-pool fill that succeeds with GC
        // stopped, using the same compiled program on both branches.
        // The first type name is not pre-interned; math is materialized
        // before the fill to isolate string allocation by these builtins.
        let mut low = 14_000;
        let mut high = 16_000;
        let mut final_output = None;
        while low + 1 < high {
            let middle = low + (high - low) / 2;
            let source = format!(
                "local _=math.abs(7); collectgarbage('stop'); \
                 for i=1,{middle} do local s=tostring(i) end; \
                 collectgarbage('restart'); \
                 local before=collectgarbage('count'); \
                 local first=type({{}}); local second=math.type(7); \
                 local after=collectgarbage('count'); \
                 print(first,second,before>after)"
            );
            let program = compile_flow_chunk(&source).unwrap();
            let outcome = execute_flow_capture(&program, 3_000_000);
            match outcome.result {
                Ok(_) => {
                    low = middle;
                    final_output = Some(outcome.stdout);
                }
                Err(FlowError::TooLarge) => high = middle,
                other => panic!("{source}: unexpected result: {other:?}"),
            }
        }
        assert_eq!(final_output, Some(b"table\tinteger\ttrue\n".to_vec()));
    }

    #[test]
    fn tostring_and_print_recover_string_quota_with_rooted_builtin_arguments() {
        for (source, expected) in [
            (
                "for i=1,19000 do local s=tostring(i) end; print('ok')",
                b"ok\n".as_slice(),
            ),
            (
                "local saved=tostring(99999); for i=1,19000 do local s=tostring(i) end; print(saved)",
                b"99999\n".as_slice(),
            ),
            (
                "local function pressure() for i=1,19000 do local s=tostring(i) end return 7 end; print(tostring(123456),pressure())",
                b"123456\t7\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 3_000_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
        let stopped =
            compile_flow_chunk("collectgarbage('stop'); for i=1,19000 do local s=tostring(i) end")
                .unwrap();
        assert_eq!(
            execute_flow_capture(&stopped, 3_000_000).result,
            Err(FlowError::TooLarge)
        );
    }

    #[test]
    fn concatenation_retries_string_quota_after_rooted_collection() {
        for (source, expected) in [
            (
                "for i=1,19000 do local s='v'..i end; print('ok')",
                b"ok\n".as_slice(),
            ),
            (
                "local saved={'v'..777}; for i=1,19000 do local s='v'..i end; collectgarbage('collect'); print(saved[1])",
                b"v777\n".as_slice(),
            ),
            (
                "local function pressure() for i=1,19000 do local s='v'..i end; return 7 end; local function use(s,n) print(s,n) end; use('sentinel'..42,pressure())",
                b"sentinel42\t7\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 3_000_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
        let stopped =
            compile_flow_chunk("collectgarbage('stop'); for i=1,19000 do local s='v'..i end")
                .unwrap();
        assert_eq!(
            execute_flow_capture(&stopped, 3_000_000).result,
            Err(FlowError::TooLarge)
        );
    }

    #[test]
    fn protected_error_strings_recover_quota_with_rooted_arguments() {
        for (operation, message) in [
            ("local ok,message=pcall(assert,false)", "assertion failed!"),
            (
                "local ok,message=pcall(type)",
                "invalid operand type in owned flow bytecode",
            ),
            (
                "local ok,message=xpcall(type,function(e) return e end)",
                "invalid operand type in owned flow bytecode",
            ),
        ] {
            // Fill with GC stopped to isolate collection by the error path.
            // Recompile each candidate to keep the observed pool boundary exact.
            let mut lower = 14_000;
            let mut upper = 16_000;
            let mut last_success = None;
            while lower + 1 < upper {
                let middle = lower + (upper - lower) / 2;
                let source = format!(
                    "local saved='keeper'..777; collectgarbage('stop'); \
                     for i=1,{middle} do local temp=tostring(i) end; \
                     collectgarbage('restart'); local before=collectgarbage('count'); \
                     {operation}; local after=collectgarbage('count'); \
                     print(ok,message,saved,before>after)"
                );
                let program = compile_flow_chunk(&source).unwrap();
                let outcome = execute_flow_capture(&program, 3_000_000);
                match outcome.result {
                    Ok(_) => {
                        lower = middle;
                        last_success = Some(outcome.stdout);
                    }
                    Err(FlowError::TooLarge) => upper = middle,
                    other => panic!("{operation}: unexpected result: {other:?}"),
                }
            }
            assert_eq!(
                last_success,
                Some(format!("false\t{message}\tkeeper777\ttrue\n").into_bytes()),
                "{operation}"
            );
        }
    }

    #[test]
    fn byte_string_len_and_sub_match_lua_positions_and_library_identity() {
        for (source, expected) in [
            (
                "print(string.len('a\\0b'),string.sub('abcdef',2,4),string.sub('abcdef',-3),string.sub('abcdef',0,2))",
                b"3\tbcd\tdef\tab\n".as_slice(),
            ),
            (
                "print(string.sub('abc',5)=='',string.sub('abc',3,2)=='',string.sub('abc',-100,100),string.sub('',1)=='')",
                b"true\ttrue\tabc\ttrue\n".as_slice(),
            ),
            (
                "print(string.sub('a\\0b',2,2)=='\\0',string.len('é'),string.len(12345),string.sub('abcdef',math.mininteger,math.maxinteger))",
                b"true\t2\t5\tabcdef\n".as_slice(),
            ),
            (
                "local lib=string; assert(_G.string==lib); local saved=lib.len; lib.len=function() return 99 end; print(string.len('abc')); lib.len=saved; print(string.len('abc'))",
                b"99\n3\n".as_slice(),
            ),
            (
                "local good=pcall(string.len,false); local bad=pcall(string.sub,'abc'); print(good,bad)",
                b"false\tfalse\n".as_slice(),
            ),
            (
                "local saved=string.sub('kept'..42,1); for i=1,19000 do local s=tostring(i) end; print(saved,string.sub('abc',-1))",
                b"kept42\tc\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 3_000_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn byte_string_byte_and_char_preserve_binary_data_and_lua_return_arity() {
        for (source, expected) in [
            (
                "print(string.char(65,0,255)=='A\\0\\255',select('#',string.byte('')),string.byte('ABC',1,3))",
                b"true\t0\t65\t66\t67\n".as_slice(),
            ),
            (
                "local a,b=string.byte('é',1,2); local c,d=string.byte('abcdef',-2,-1); print(a,b,c,d,select('#',string.byte('abc',0)))",
                b"195\t169\t101\t102\t0\n".as_slice(),
            ),
            (
                "print(string.char()=='',string.byte('xyz',2),select('#',string.byte('xyz',5)),select('#',string.byte('xyz',3,2)))",
                b"true\t121\t0\t0\n".as_slice(),
            ),
            (
                "local joined=string.char(0,97,255); local a,b,c=string.byte(joined,1,3); print(#joined,a,b,c,string.sub(joined,2,2)=='a')",
                b"3\t0\t97\t255\ttrue\n".as_slice(),
            ),
            (
                "local good=pcall(string.char,65,66); local bad1=pcall(string.char,256); local bad2=pcall(string.char,-1); print(good,bad1,bad2)",
                b"true\tfalse\tfalse\n".as_slice(),
            ),
            (
                "local hold=string.char(65,0,66); for i=1,19000 do local s=tostring(i) end; print(string.len(hold),string.byte(hold,1,3))",
                b"3\t65\t0\t66\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 3_000_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn byte_string_case_reverse_and_repeat_follow_lua54_bounds() {
        for (source, expected) in [
            (
                "print(string.lower('HeLLo\\0WORLD')=='hello\\0world',string.upper('aBc\\0z')=='ABC\\0Z',string.reverse('a\\0b')=='b\\0a')",
                b"true\ttrue\ttrue\n".as_slice(),
            ),
            (
                "print(string.rep('ab',3,'-')=='ab-ab-ab',string.rep('X',3)=='XXX',string.rep('x',0)=='',string.rep('x',-10)=='')",
                b"true\ttrue\ttrue\ttrue\n".as_slice(),
            ),
            (
                "print(string.rep('',1000000)=='',string.rep('',4,'.')=='...',string.reverse('abc')=='cba')",
                b"true\ttrue\ttrue\n".as_slice(),
            ),
            (
                "local s=string.char(0,65,255); local r=string.reverse(s); local a,b,c=string.byte(r,1,3); print(a,b,c,string.upper(string.char(97,0,122))=='A\\0Z')",
                b"255\t65\t0\ttrue\n".as_slice(),
            ),
            (
                "local keep=string.rep('ab',3,'-'); for i=1,19000 do local s=tostring(i) end; print(keep,string.upper('abc'),string.reverse('abc'))",
                b"ab-ab-ab\tABC\tcba\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 3_000_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
        let too_large = compile_flow_chunk("print(string.rep('abcd',20000))").unwrap();
        assert_eq!(
            execute_flow_capture(&too_large, 3_000_000).result,
            Err(FlowError::TooLarge)
        );
    }

    #[test]
    fn owned_table_concat_unpack_cover_binary_sequences_and_result_arity() {
        for (source, expected) in [
            (
                "print(table.concat({'a','b','c'},'-'),table.concat({1,2,3},',',2,3),table.concat({})=='')",
                b"a-b-c\t2,3\ttrue\n".as_slice(),
            ),
            (
                "local t={'a',nil,'c'}; local a,b,c=table.unpack(t,1,3); print(a,b,c,select('#',table.unpack(t,1,3)))",
                b"a\tnil\tc\t3\n".as_slice(),
            ),
            (
                "print(table.concat({'a\\0','b'},':')=='a\\0:b',table.concat({'a','b'},'-',2,1)=='',select('#',table.unpack({})))",
                b"true\ttrue\t0\n".as_slice(),
            ),
            (
                "local lib=table; assert(_G.table==lib); local saved=lib.concat; lib.concat=function() return 99 end; print(table.concat({1,2})); lib.concat=saved; print(table.concat({1,2}))",
                b"99\n12\n".as_slice(),
            ),
            ("print(table.unpack({1,2,3}))", b"1\t2\t3\n".as_slice()),
            (
                "local good=pcall(table.concat,{'a','b'}); local bad=pcall(table.concat,{'a',true}); local ok,a,b,c,d=pcall(table.unpack,'text'); print(good,bad,ok,a,b,c,d)",
                b"true\tfalse\ttrue\tnil\tnil\tnil\tnil\n".as_slice(),
            ),
            (
                "local t={string.char(65,0,66)}; for i=1,19000 do local s=tostring(i) end; print(table.concat(t)==string.char(65,0,66))",
                b"true\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 3_000_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn table_pack_preserves_nil_holes_and_explicit_argument_count() {
        for (source, expected) in [
            (
                "local t=table.pack(1,nil,3); print(t.n,t[1],t[2],t[3],select('#',table.unpack(t,1,t.n)))",
                b"3\t1\tnil\t3\t3\n".as_slice(),
            ),
            (
                "local empty=table.pack(); local nils=table.pack(nil,nil); print(empty.n,nils.n,nils[1]==nil,nils[2]==nil,select('#',table.unpack(nils,1,nils.n)))",
                b"0\t2\ttrue\ttrue\t2\n".as_slice(),
            ),
            (
                "local saved=table.pack('kept'..42,nil,17); for i=1,19000 do local s=tostring(i) end; print(saved.n,saved[1],saved[2]==nil,saved[3])",
                b"3\tkept42\ttrue\t17\n".as_slice(),
            ),
            (
                "local orig=table.pack; table.pack=function(...) return {n=99} end; print(_G.table.pack().n); table.pack=orig; print(table.pack('a').n)",
                b"99\n1\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 3_000_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn table_insert_and_remove_match_lua_sequence_mutations() {
        for (source, expected) in [
            (
                "local t={1,3}; table.insert(t,2,2); table.insert(t,4); print(table.concat(t,',')); print(table.remove(t,2),table.concat(t,',')); print(table.remove(t),table.concat(t,','))",
                b"1,2,3,4\n2\t1,3,4\n4\t1,3\n".as_slice(),
            ),
            (
                "local t={}; print(table.remove(t)==nil); table.insert(t,'a'); print(t[1],select('#',table.insert(t,nil)),#t)",
                b"true\na\t0\t1\n".as_slice(),
            ),
            (
                "local t={1,2}; local a=pcall(table.insert,t,0,3); local b=pcall(table.remove,t,0); local c=pcall(table.insert,t,3,4); print(a,b,c,table.concat(t,','))",
                b"false\tfalse\ttrue\t1,2,4\n".as_slice(),
            ),
            (
                "local t={1,2}; print(table.remove(t,3)==nil,table.concat(t,',')); print(table.remove(t,1),table.concat(t,','))",
                b"true\t1,2\n1\t2\n".as_slice(),
            ),
            (
                "local t=table.pack('a','c'); table.insert(t,2,'b'); print(table.remove(t,2),t[1],t[2],t[3],t.n)",
                b"b\ta\tc\tnil\t2\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 20_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn table_move_handles_overlaps_distinct_tables_and_bounded_ranges() {
        for (source, expected) in [
            (
                "local t={1,2,3}; print(table.move(t,1,3,2)==t,t[1],t[2],t[3],t[4])",
                b"true\t1\t1\t2\t3\n".as_slice(),
            ),
            (
                "local t={1,2,3}; table.move(t,2,3,1); print(t[1],t[2],t[3])",
                b"2\t3\t3\n".as_slice(),
            ),
            (
                "local t={1,2}; local d={9}; print(table.move(t,1,2,2,d)==d,d[1],d[2],d[3],t[1])",
                b"true\t9\t1\t2\t1\n".as_slice(),
            ),
            (
                "local t={1,nil,3}; local d={7,8,9}; table.move(t,1,3,1,d); print(d[1],d[2],d[3])",
                b"1\tnil\t3\n".as_slice(),
            ),
            (
                "local t={}; t[-1]='x'; local d={}; table.move(t,-1,-1,1,d); print(d[1])",
                b"x\n".as_slice(),
            ),
            (
                "local t={1,2}; local d={3}; print(table.move(t,5,4,1,d)==d,d[1])",
                b"true\t3\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 30_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn table_move_large_transfer_preserves_uncatchable_resource_limit() {
        let program =
            compile_flow_chunk("local ok=pcall(table.move,{1},1,math.maxinteger,1); print(ok)")
                .unwrap();
        assert_eq!(
            execute_flow_capture(&program, 30_000).result,
            Err(FlowError::TooLarge)
        );
    }

    #[test]
    fn table_sort_orders_numbers_strings_callbacks_and_table_metamethods() {
        for (source, expected) in [
            (
                "local t={3,1,2}; print(select('#',table.sort(t)),table.concat(t,','))",
                b"0\t1,2,3\n".as_slice(),
            ),
            (
                "local t={'b','a','c'}; table.sort(t); print(table.concat(t,'|'))",
                b"a|b|c\n".as_slice(),
            ),
            (
                "local t={1,4,3,2}; table.sort(t,function(a,b) return a>b end); print(table.concat(t,','))",
                b"4,3,2,1\n".as_slice(),
            ),
            (
                "local t={0,3.5,-1,2}; table.sort(t); print(table.concat(t,','))",
                b"-1,0,2,3.5\n".as_slice(),
            ),
            (
                "local t={}; print(select('#',table.sort(t)),#t); local q={5}; table.sort(q); print(q[1])",
                b"0\t0\n5\n".as_slice(),
            ),
            (
                "local t={3,2,1}; local a=pcall(table.sort,t,42); local b=pcall(table.sort,3); print(a,b,table.concat(t,','))",
                b"false\tfalse\t3,2,1\n".as_slice(),
            ),
            (
                "local mt={__lt=function(a,b) return a.x<b.x end}; local t={setmetatable({x=3},mt),setmetatable({x=1},mt),setmetatable({x=2},mt)}; table.sort(t); print(t[1].x,t[2].x,t[3].x)",
                b"1\t2\t3\n".as_slice(),
            ),
            (
                "local t={3,1,2}; table.sort(t,function(a,b) local temp=tostring(a)..':'..tostring(b); return a<b end); print(table.concat(t,','))",
                b"1,2,3\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 100_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn table_sort_snapshot_roots_values_during_comparator_gc_and_mutation() {
        // Mutating the sorted table from a comparator is not reference-Lua
        // parity territory. This owned-only regression verifies that values
        // retained by the sorter remain rooted after the Lua table is cleared.
        let source = "local t={{n=3},{n=1},{n=2}}; local changed=false; \
                      table.sort(t,function(a,b) \
                        if not changed then t[1]=nil; t[2]=nil; t[3]=nil; changed=true end; \
                        collectgarbage('collect'); return a.n<b.n \
                      end); print(t[1].n,t[2].n,t[3].n)";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 100_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"1\t2\t3\n");
    }

    #[test]
    fn gc_reclaims_runtime_strings_and_reuses_handles_after_explicit_passes() {
        let source = "collectgarbage('stop'); local literal='stable'; for i=1,3200 do local s=tostring(i) end; local high=collectgarbage('count'); collectgarbage('collect'); local low=collectgarbage('count'); for i=3201,6400 do local s=tostring(i) end; collectgarbage('collect'); print(high>low,literal,'stable',tostring(42))";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 450_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"true\tstable\tstable\t42\n");

        // The same runtime string handles must not be swept when reachable
        // through an ordinary table or weak table's primitive string entry.
        let source = "local keep={}; local weak=setmetatable({}, {__mode='kv'}); local key=tostring(11111); keep[1]=tostring(22222); weak[key]=tostring(33333); key=nil; collectgarbage('collect'); print(keep[1],weak['11111'])";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 5_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"22222\t33333\n");
    }

    #[test]
    fn gc_weak_mode_accepts_long_strings_in_pinned_lua54_6() {
        // Lua 5.4.6 traversetable checks ttisstring, not ttisshrstring.
        // A long mode string with flags is therefore still effective.
        for (mode, expected) in [
            (format!("v{}", "x".repeat(64)), b"false\ttrue\n".as_slice()),
            (format!("k{}", "x".repeat(64)), b"true\tfalse\n".as_slice()),
            (format!("x{}kv", "x".repeat(64)), b"true\ttrue\n".as_slice()),
        ] {
            let source = format!(
                "local w=setmetatable({{}}, {{__mode='{mode}'}}); \
                 do local key={{}}; local value={{}}; w[key]=17; w[1]=value end; \
                 collectgarbage('collect'); \
                 local key_gone=true; for k,v in pairs(w) do if type(k)=='table' then key_gone=false end end; \
                 print(key_gone,w[1]==nil)"
            );
            let program = compile_flow_chunk(&source).unwrap();
            let outcome = execute_flow_capture(&program, 30_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn gc_weak_mode_stops_scanning_at_embedded_nul() {
        for (mode, expected) in [
            ("v\\0k", b"false\ttrue\n".as_slice()),
            ("k\\0v", b"true\tfalse\n".as_slice()),
            ("kv\\0x", b"true\ttrue\n".as_slice()),
            ("x\\0kv", b"false\tfalse\n".as_slice()),
        ] {
            let source = format!(
                "local w=setmetatable({{}}, {{__mode='{mode}'}}); \
                 do local key={{}}; local value={{}}; w[key]=17; w[1]=value end; \
                 collectgarbage('collect'); \
                 local key_gone=true; for k,v in pairs(w) do if type(k)=='table' then key_gone=false end end; \
                 print(key_gone,w[1]==nil)"
            );
            let program = compile_flow_chunk(&source).unwrap();
            let outcome = execute_flow_capture(&program, 30_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn gc_weak_mode_recognizes_repeated_flags_and_extra_characters() {
        for (mode, expected) in [
            ("kvv", b"true\ttrue\n".as_slice()),
            ("xxvkx", b"true\ttrue\n".as_slice()),
            ("kkx", b"true\tfalse\n".as_slice()),
            ("vvx", b"false\ttrue\n".as_slice()),
            ("xyz", b"false\tfalse\n".as_slice()),
        ] {
            let source = format!(
                "local w=setmetatable({{}}, {{__mode='{mode}'}}); \
                 do local key={{}}; local value={{}}; w[key]=17; w[1]=value end; \
                 collectgarbage('collect'); \
                 local key_gone=true; for k,v in pairs(w) do if type(k)=='table' then key_gone=false end end; \
                 print(key_gone,w[1]==nil)"
            );
            let program = compile_flow_chunk(&source).unwrap();
            let outcome = execute_flow_capture(&program, 30_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn owned_gc_weak_value_tables_collect_dead_tables_and_closures() {
        let cases = [
            (
                "local w=setmetatable({}, {__mode='v'}); local keep={id=42}; local gone={}; w[1]=keep; w[2]=gone; w.named=function() return 7 end; w.plain=23; w.words='hello'; gone=nil; collectgarbage('collect'); print(w[1].id,w[2]==nil,w.named==nil,w.plain,w.words); keep=nil; collectgarbage('collect'); print(w[1]==nil)",
                b"42\ttrue\ttrue\t23\thello\ntrue\n".as_slice(),
            ),
            (
                "local w=setmetatable({}, {__mode='v'}); do local key={}; w[key]=99 end; collectgarbage('collect'); local key,value=next(w); print(type(key),value)",
                b"table\t99\n".as_slice(),
            ),
            (
                "local meta={__mode='v'}; local w=setmetatable({},meta); meta.__mode=nil; local t={n=7}; w[1]=t; t=nil; collectgarbage('collect'); print(w[1].n)",
                b"7\n".as_slice(),
            ),
        ];
        for (source, expected) in cases {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 20_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn owned_gc_weak_keys_and_ephemeron_fixpoint_preserve_lua_reachability() {
        let cases = [
            (
                "local w=setmetatable({}, {__mode='k'}); local key={}; w[key]={back=key}; key=nil; collectgarbage('collect'); print(next(w)==nil)",
                b"true\n".as_slice(),
            ),
            (
                "local w=setmetatable({}, {__mode='k'}); local key={}; w[key]={n=42}; collectgarbage('collect'); print(w[key].n); key=nil; collectgarbage('collect'); print(next(w)==nil)",
                b"42\ntrue\n".as_slice(),
            ),
            (
                "local w=setmetatable({}, {__mode='k'}); w['name']={n=42}; collectgarbage('collect'); print(w['name'].n)",
                b"42\n".as_slice(),
            ),
            (
                "local w=setmetatable({}, {__mode='k'}); local seed={}; local key2={}; w[seed]={child=key2}; w[key2]={n=42}; key2=nil; collectgarbage('collect'); print(w[w[seed].child].n); seed=nil; collectgarbage('collect'); print(next(w)==nil)",
                b"42\ntrue\n".as_slice(),
            ),
            (
                "local w=setmetatable({}, {__mode='kv'}); local key={}; w[key]=42; key=nil; w[1]={}; w[2]='still'; collectgarbage('collect'); print(w[1]==nil,w[2],next(w)~=nil)",
                b"true\tstill\ttrue\n".as_slice(),
            ),
        ];
        for (source, expected) in cases {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 30_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }
    }

    #[test]
    fn gc_count_reflects_unreachable_table_storage_reclamation() {
        let source = "collectgarbage('stop'); local before=collectgarbage('count'); for i=1,1400 do local t={1,2,3,4,5,6,7,8,9,10,11,12} end; local high=collectgarbage('count'); collectgarbage('collect'); local low=collectgarbage('count'); print(type(low),high>before,low<high,collectgarbage('isrunning'))";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 150_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"number\ttrue\ttrue\tfalse\n");
    }

    #[test]
    fn owned_gc_stop_restart_and_manual_collect_preserve_quota_safety() {
        let program = compile_flow_chunk(
            "print(collectgarbage('isrunning')); collectgarbage('stop'); print(collectgarbage('isrunning')); collectgarbage('collect'); collectgarbage('restart'); print(collectgarbage('isrunning'))",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 4000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"true\nfalse\ntrue\n");

        let stopped =
            compile_flow_chunk("collectgarbage('stop'); for i=1,17000 do local t={} end").unwrap();
        assert_eq!(
            execute_flow_capture(&stopped, 1_000_000).result,
            Err(FlowError::TooLarge)
        );

        let manually_cleared = compile_flow_chunk(
            "collectgarbage('stop'); for i=1,15000 do local t={} end; collectgarbage('collect'); for i=1,15000 do local t={} end; print('ok')",
        )
        .unwrap();
        let outcome = execute_flow_capture(&manually_cleared, 2_000_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"ok\n");

        let restarted = compile_flow_chunk(
            "collectgarbage('stop'); for i=1,1000 do local t={} end; collectgarbage('restart'); for i=1,20000 do local t={} end; print('ok')",
        )
        .unwrap();
        let outcome = execute_flow_capture(&restarted, 2_000_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"ok\n");
    }

    #[test]
    fn collector_charges_sweeps_against_execution_budget() {
        let baseline = compile_flow_chunk(
            "collectgarbage('stop'); for i=1,900 do local t={} end; print('ok')",
        )
        .unwrap();
        let with_collect = compile_flow_chunk(
            "collectgarbage('stop'); for i=1,900 do local t={} end; collectgarbage('collect'); print('ok')",
        )
        .unwrap();

        // Determine the non-collecting execution cost, then leave room for
        // builtin dispatch and marking but not the 900-slot sweep.
        let mut lower = 0;
        let mut upper = 100_000;
        while lower + 1 < upper {
            let middle = lower + (upper - lower) / 2;
            match execute_flow_capture(&baseline, middle).result {
                Ok(_) => upper = middle,
                Err(FlowError::StepBudgetExceeded) => lower = middle,
                other => panic!("unexpected baseline result: {other:?}"),
            }
        }
        assert_eq!(execute_flow_capture(&baseline, upper).result, Ok(vec![]));
        assert_eq!(
            execute_flow_capture(&with_collect, upper + 100).result,
            Err(FlowError::StepBudgetExceeded)
        );
        let enough = execute_flow_capture(&with_collect, upper + 5000);
        assert_eq!(enough.result, Ok(vec![]));
        assert_eq!(enough.stdout, b"ok\n");
    }

    #[test]
    fn gc_reuses_full_table_pool_for_lazy_global_math_and_argument_tables() {
        for (source, expected) in [
            (
                "collectgarbage('stop'); for i=1,16384 do local t={} end; collectgarbage('restart'); print(type(_G),type(math),type(arg))",
                b"table\ttable\ttable\n".as_slice(),
            ),
            (
                "collectgarbage('stop'); for i=1,16384 do local t={} end; collectgarbage('collect'); print(type(math))",
                b"table\n".as_slice(),
            ),
            (
                "collectgarbage('stop'); for i=1,16384 do local t={} end; collectgarbage('collect'); print(type(arg))",
                b"table\n".as_slice(),
            ),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 2_000_000);
            assert_eq!(outcome.result, Ok(vec![]), "{source}");
            assert_eq!(outcome.stdout, expected, "{source}");
        }

        let retained = compile_flow_chunk(
            "collectgarbage('stop'); local keep={}; for i=1,16383 do keep[i]={} end; collectgarbage('restart'); print(type(math))",
        )
        .unwrap();
        assert_eq!(
            execute_flow_capture(&retained, 2_000_000).result,
            Err(FlowError::TooLarge)
        );
    }

    #[test]
    fn gc_reclaims_unreachable_constructor_multi_return_table_tails() {
        let values = (1..=40)
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let source = format!(
            "local function vals() return {values} end; for i=1,1800 do local t={{tag=i,vals()}} end; print('ok')"
        );
        let program = compile_flow_chunk(&source).unwrap();
        let outcome = execute_flow_capture(&program, 3_000_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"ok\n");

        let retained = format!(
            "local function vals() return {values} end; local keep={{}}; for i=1,1800 do keep[i]={{tag=i,vals()}} end"
        );
        let program = compile_flow_chunk(&retained).unwrap();
        assert_eq!(
            execute_flow_capture(&program, 3_000_000).result,
            Err(FlowError::TooLarge)
        );
    }

    #[test]
    fn gc_reclaims_unreachable_table_fields_at_write_pressure() {
        let source = "for i=1,1800 do local t={}; for j=1,40 do t[j]=j end end; print('ok')";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 3_000_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"ok\n");

        let source =
            "local refs={}; for i=1,1800 do local t={}; refs[i]=t; for j=1,40 do t[j]=j end end";
        let program = compile_flow_chunk(source).unwrap();
        assert_eq!(
            execute_flow_capture(&program, 3_000_000).result,
            Err(FlowError::TooLarge)
        );
    }

    #[test]
    fn nested_gc_preserves_suspended_caller_stack_and_closure_values() {
        let source = "local function pressure() for i=1,21000 do local t={}; t.self=t end; return 7 end; local function combine(t,n) return t.value+n end; print(combine({value=35},pressure()))";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 2_000_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"42\n");

        let source = "local function pressure() for i=1,21000 do local f=function() return i end end; return 4 end; local function apply(f,n) return f()+n end; local function make() local x=38; return function() return x end end; print(apply(make(),pressure()))";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 2_000_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"42\n");
    }

    #[test]
    fn root_gc_recycles_short_lived_closures_and_captured_cells() {
        let source = "local keep; do local saved=42; keep=function() return saved end end; for i=1,42000 do local f=function() return i end end; print(keep())";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 2_000_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"42\n");

        let source = "local refs={}; for i=1,17000 do refs[i]=function() return i end end; print('unreachable')";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 1_000_000);
        assert_eq!(outcome.result, Err(FlowError::TooLarge));

        let source = "local f; do local n=3; f=function() if n==0 then return 1 end n=n-1; return f() end end; for i=1,20000 do local temporary=function() return i end end; print(f())";
        let program = compile_flow_chunk(source).unwrap();
        let outcome = execute_flow_capture(&program, 1_000_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"1\n");
    }

    #[test]
    fn root_table_collector_reuses_unreachable_cycles_and_keeps_live_tables() {
        let program = compile_flow_chunk(
            "local keep={value=42}; keep.self=keep; for i=1,17000 do local t={}; t.self=t end; print(keep.value,keep.self==keep)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 1_000_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"42\ttrue\n");

        let program = compile_flow_chunk(
            "local keep={}; for i=1,17000 do keep[i]={} end; print('unexpected')",
        )
        .unwrap();
        assert_eq!(
            execute_flow_capture(&program, 1_000_000).result,
            Err(FlowError::TooLarge)
        );
    }

    #[test]
    fn indexed_string_interning_reuses_compiler_literals_across_functions() {
        let mut source = String::from("local t={}; ");
        for index in 1..=640 {
            source.push_str(&format!("t[{index}]='v{:02}';", index % 64));
        }
        source.push_str("print(t[1],t[640])");
        let program = compile_flow_chunk(&source).unwrap();
        assert!(
            program.strings.len() < 80,
            "identical literals must share stable interned indices"
        );
        let outcome = execute_flow_capture(&program, 20_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"v01\tv00\n");
    }

    #[test]
    fn indexed_runtime_strings_preserve_lua_values_and_limits() {
        let program =
            compile_flow_chunk("local last=''; for i=1,1500 do last=tostring(i) end; print(last)")
                .unwrap();
        let outcome = execute_flow_capture(&program, 100_000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"1500\n");

        let source = format!(
            "print('{}'); print('{}')",
            "z".repeat(33_000),
            "q".repeat(33_000)
        );
        assert!(matches!(
            compile_flow_chunk(&source),
            Err(FlowError::TooLarge)
        ));
    }

    #[test]
    fn math_sqrt_and_unsigned_ordering_handle_lua_numeric_boundaries() {
        let program = compile_flow_chunk(
            "local m=math; assert(_G.math==m); print(math.sqrt(81)==9.0,math.type(math.sqrt(81)),math.sqrt('16')==4.0,math.sqrt(2)^2>1.999,math.sqrt(2)^2<2.001); print(math.sqrt(-1)~=math.sqrt(-1),math.sqrt(math.huge)==math.huge); print(math.ult(1,-1),math.ult(-1,1),math.ult(2.0,3.0),math.ult('2','3')); local a=pcall(math.sqrt,false); local b=pcall(math.ult,1.5,2); local c=pcall(math.ult,2); print(a,b,c)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 10000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(
            outcome.stdout,
            b"true\tfloat\ttrue\ttrue\ttrue\ntrue\ttrue\ntrue\tfalse\ttrue\ttrue\nfalse\tfalse\tfalse\n"
        );
    }

    #[test]
    fn seeded_math_random_uses_lua_54_integer_and_float_semantics() {
        let program = compile_flow_chunk(
            "local a,b=math.randomseed(123,-456); print(a,b,select('#',math.randomseed(123,-456))); local x=math.random(0); local y=math.random(1,9); local z=math.random(); print(math.type(x),y>=1 and y<=9,math.type(z),z>=0 and z<1); math.randomseed(123,-456); print(x==math.random(0),y==math.random(1,9),z==math.random()); local p,q=math.randomseed(1,nil); print(p,q); print(math.random(1,1),math.type(math.random(0))); local ok=pcall(math.random,2,1); print(ok)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 10000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(
            outcome.stdout,
            b"123\t-456\t2\ninteger\ttrue\tfloat\ttrue\ntrue\ttrue\ttrue\n1\t0\n1\tinteger\nfalse\n"
        );
    }

    #[test]
    fn utf8_codes_iterates_byte_positions_with_strict_and_lax_decoding() {
        let program = compile_flow_chunk(
            "local s='Aé💩'; local iter,state,control=utf8.codes(s); print(type(iter),state==s,control,select('#',utf8.codes(s))); for p,c in utf8.codes(s) do print(p,c) end; local ok,p,c=pcall(iter,state,0); print(ok,p,c); print(select('#',iter(state,4))); for p,c in utf8.codes(utf8.char(0x110000),true) do print(p,c) end; local valid=pcall(function() for p,c in utf8.codes(utf8.char(0x110000)) do end end); print(valid)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 10000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(
            outcome.stdout,
            b"function\ttrue\t0\t3\n1\t65\n2\t233\n4\t128169\ntrue\t1\t65\n0\n1\t1114112\nfalse\n"
        );
    }

    #[test]
    fn utf8_offset_counts_byte_beginnings_and_reports_continuation_errors() {
        let program = compile_flow_chunk(
            "local s='Aé💩Z'; print(utf8.offset(s,1),utf8.offset(s,2),utf8.offset(s,3),utf8.offset(s,4),utf8.offset(s,5),utf8.offset(s,6)==nil); print(utf8.offset(s,-1),utf8.offset(s,-2),utf8.offset(s,0,5),utf8.offset(s,1,-1)); print(utf8.offset(s,-4),utf8.offset(s,-5)==nil,utf8.offset(s,0,9),utf8.offset(s,2,9)==nil); local ok,msg=pcall(utf8.offset,s,1,5); print(ok,msg); print(utf8.offset(s,0,5),utf8.offset(s,-1,1)==nil)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 10000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(
            outcome.stdout,
            b"1\t2\t4\t8\t9\ttrue\n8\t4\t4\t8\n1\ttrue\t9\ttrue\nfalse\tinitial position is a continuation byte\n4\ttrue\n"
        );
    }

    #[test]
    fn utf8_char_len_and_codepoint_support_strict_and_lax_bytes() {
        let program = compile_flow_chunk(
            "local u=utf8; assert(_G.utf8==u); local s=utf8.char(65,0,0x1F600); print(string.len(s),utf8.len(s),utf8.codepoint(s,1,6)); local extended=utf8.char(0x110000); local count,bad=utf8.len(extended); print(count==nil,bad,utf8.len(extended,1,-1,true),utf8.codepoint(extended,1,1,true)); local overlong=string.char(0xC0,0x80); local n,p=utf8.len(overlong,1,-1,true); print(n==nil,p); local a,b=utf8.len('é',2,2); print(a==nil,b); print(select('#',utf8.codepoint('abc',2,1)),utf8.len('abc',4)); local ok=pcall(utf8.char,0x80000000); print(ok)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 10000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(
            outcome.stdout,
            b"6\t3\t65\t0\t128512\ntrue\t1\t1\t1114112\ntrue\t1\ntrue\t2\n0\t0\nfalse\n"
        );
    }

    #[test]
    fn math_inverse_trig_exponents_and_optional_base_log_obey_lua_values() {
        let program = compile_flow_chunk(
            "local e=1e-10; print(math.type(math.exp(1)),math.type(math.log(8,2)),math.type(math.atan(1)),math.type(math.asin(0)),math.type(math.acos(0))); print(math.abs(math.exp(0)-1)<e,math.abs(math.log(math.exp(1))-1)<e,math.abs(math.log(100,10)-2)<e,math.abs(math.log(8,2)-3)<e); print(math.abs(math.asin(1)-math.pi/2)<e,math.abs(math.acos(0)-math.pi/2)<e,math.abs(math.atan(1)-math.pi/4)<e,math.abs(math.atan(0,-1)-math.pi)<e); print(math.log(-1)~=math.log(-1),math.asin(2)~=math.asin(2),math.acos(2)~=math.acos(2),math.exp(math.huge)==math.huge); local ok,value=pcall(math.log,'100',10); print(ok,math.abs(value-2)<e); print(pcall(math.exp,false)); print(pcall(math.atan,0,false))",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 10000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(
            outcome.stdout,
            b"float\tfloat\tfloat\tfloat\tfloat\ntrue\ttrue\ttrue\ttrue\ntrue\ttrue\ttrue\ttrue\ntrue\ttrue\ttrue\ttrue\ntrue\ttrue\nfalse\tbad argument #1 to 'math.exp' (number expected, got boolean)\nfalse\tbad argument #2 to 'math.atan' (number expected, got boolean)\n"
        );
    }

    #[test]
    fn math_trigonometry_and_unit_conversions_are_float_and_protected() {
        let program = compile_flow_chunk(
            "local e=1e-10; print(math.type(math.sin(0)),math.type(math.cos(0)),math.type(math.tan(0)),math.type(math.rad(180)),math.type(math.deg(math.pi))); print(math.abs(math.sin(math.rad(30))-0.5)<e,math.abs(math.cos(math.rad(60))-0.5)<e,math.abs(math.tan(math.rad(45))-1)<e); print(math.abs(math.deg(math.rad(180))-180)<e,math.abs(math.rad(math.deg(math.pi))-math.pi)<e); local ok,x=pcall(math.rad,'180'); print(ok,math.abs(x-math.pi)<e); local p,msg=pcall(math.sin,false); print(p,msg); local q,err=pcall(math.cos); print(q,err)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 10000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(
            outcome.stdout,
            b"float\tfloat\tfloat\tfloat\tfloat\ntrue\ttrue\ttrue\ntrue\ttrue\ntrue\ttrue\nfalse\tbad argument #1 to 'math.sin' (number expected, got boolean)\nfalse\tbad argument #1 to 'math.cos' (number expected, got no value)\n"
        );
    }

    #[test]
    fn math_fmod_and_modf_preserve_signed_remainder_and_result_arity() {
        let program = compile_flow_chunk(
            "print(math.fmod(-7,3),math.fmod(7,-3),math.fmod(math.mininteger,-1),math.type(math.fmod(7,3)),math.type(math.fmod(7.0,3)),math.fmod(-7.0,3.0)==-1.0); local x,y=math.modf(-3.75); print(x,y,math.type(x),math.type(y)); local a,b=math.modf(3.25); print(a,b); local i,f=math.modf(math.huge); print(i==math.huge,f==0,math.type(i)); local ok,c,d=pcall(math.modf,1.25); print(ok,c,d); local bad,msg=pcall(math.fmod,2,0); print(bad,msg)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 10000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(
            outcome.stdout,
            b"-1\t1\t0\tinteger\tfloat\ttrue\n-3\t-0.75\tinteger\tfloat\n3\t0.25\ntrue\ttrue\tfloat\ntrue\t1\t0.25\nfalse\tbad argument #2 to 'math.fmod' (zero)\n"
        );
    }

    #[test]
    fn partial_numeric_math_library_has_lua_number_types_and_shared_identity() {
        let program = compile_flow_chunk(
            "local m=math; assert(_G.math==m); assert(math==m); print(math.abs(-7),math.floor(3.75),math.ceil(-3.75),math.max(2,9,5),math.min(2,9,5),math.type(3),math.type(3.0),math.tointeger('42')); print(math.pi>3 and math.pi<4,math.huge>1e100,math.maxinteger>0,math.mininteger<0)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 5000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(
            outcome.stdout,
            b"7\t3\t-3\t9\t2\tinteger\tfloat\t42\ntrue\ttrue\ttrue\ttrue\n"
        );

        let program = compile_flow_chunk(
            "local original=math.abs; math.abs=function(v) return v end; print(math.abs(-5)); math.abs=original; print(math.abs(-5)); local ok=pcall(math.abs,'wrong'); assert(not ok)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 2000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"-5\n5\n");
    }

    #[test]
    fn capture_preserves_explicit_lua_errors_and_prior_prints() {
        for (source, expected, prior_stdout) in [
            (
                "print('before'); error('failure detail')",
                "failure detail",
                "before\n",
            ),
            ("assert(false, 'assertion detail')", "assertion detail", ""),
            ("error(42)", "42", ""),
            ("error(nil)", "error object is a nil value", ""),
        ] {
            let program = compile_flow_chunk(source).unwrap();
            let outcome = execute_flow_capture(&program, 1000);
            assert!(
                matches!(outcome.result, Err(FlowError::Thrown(_))),
                "{source}"
            );
            assert_eq!(outcome.error_text.as_deref(), Some(expected.as_bytes()));
            assert_eq!(outcome.stdout, prior_stdout.as_bytes());
        }
        let program = compile_flow_chunk("return 1 // 0").unwrap();
        let outcome = execute_flow_capture(&program, 100);
        assert_eq!(outcome.result, Err(FlowError::DivisionByZero));
        assert_eq!(outcome.error_text, None);
    }

    #[test]
    fn print_ignores_global_tostring_overrides_but_uses_metamethods() {
        let program = compile_flow_chunk(
            "tostring=function() return 'override' end; print(7,true); _G.tostring=nil; print('hello'); local t=setmetatable({}, {__tostring=function() return 'tag' end}); print(t)",
        )
        .unwrap();
        let outcome = execute_flow_capture(&program, 2000);
        assert_eq!(outcome.result, Ok(vec![]));
        assert_eq!(outcome.stdout, b"7\ttrue\nhello\ntag\n");
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
