//! Experimental Ferrua-owned typed register bytecode for bounded Lua expressions.
//! Not used by production fer; not a full Lua VM.

use crate::expr::{Expr, LocalAttribute, Statement, TableField, parse_chunk, parse_expression};
use crate::owned_flow::{decode_lua_string, parse_lua_hex_float, parse_lua_hex_integer_wrapping};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;
const MAX_REGISTERS: usize = 16384;
const MAX_REGISTER_STEPS: usize = 100_000;
const MAX_REGISTER_STRING_BYTES: usize = 65_536;
const MAX_CALL_ARGUMENTS: usize = 128;
const MAX_REGISTER_TABLES: usize = 512;
const MAX_REGISTER_TABLE_ENTRIES: usize = 4096;

/// Bounded first-class builtins in the independent register dispatcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterBuiltin {
    Type,
    ToString,
    Select,
    RawGet,
    RawSet,
    RawLen,
    RawEqual,
    Assert,
    ToNumber,
    Next,
    Pairs,
    Ipairs,
    IpairsIterator,
}

/// Bounded scalar values, execution-owned tables, byte strings and first-class
/// builtin identities. Lua closures, userdata and arbitrary metamethods remain
/// unsupported by this independent register VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterValue {
    Nil,
    Boolean(bool),
    Integer(i64),
    /// Raw IEEE-754 bits preserve Copy/Eq and signed-zero representation.
    Float(u64),
    /// Index into this program's immutable Lua byte-string pool.
    String(usize),
    /// First-class builtin function identity, not a Lua closure.
    Builtin(RegisterBuiltin),
    /// Execution-owned Lua table identity.
    Table(usize),
}

impl RegisterValue {
    fn truthy(self) -> bool {
        !matches!(self, Self::Nil | Self::Boolean(false))
    }

    fn integer(self) -> Result<i64, Error> {
        match self {
            Self::Integer(value) => Ok(value),
            _ => Err(Error::TypeError),
        }
    }

    /// Bitwise operators convert integral finite floats exactly to i64.
    /// Arithmetic and numeric-for controls retain their distinct coercions.
    fn bitwise_integer(self) -> Result<i64, Error> {
        match self {
            Self::Integer(value) => Ok(value),
            Self::Float(bits) => {
                let value = f64::from_bits(bits);
                if value.is_finite()
                    && value.fract() == 0.0
                    && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value)
                {
                    Ok(value as i64)
                } else {
                    Err(Error::TypeError)
                }
            }
            _ => Err(Error::TypeError),
        }
    }

    fn number(self) -> Result<f64, Error> {
        match self {
            Self::Integer(value) => Ok(value as f64),
            Self::Float(bits) => Ok(f64::from_bits(bits)),
            _ => Err(Error::TypeError),
        }
    }
}

/// Compare an integer and a float without first rounding the integer to
/// f64. In particular 2^53+1 must not compare equal to floating 2^53.
fn compare_integer_float(integer: i64, float: f64) -> Option<Ordering> {
    if float.is_nan() {
        return None;
    }
    if float < -9_223_372_036_854_775_808.0 {
        return Some(Ordering::Greater);
    }
    if float >= 9_223_372_036_854_775_808.0 {
        return Some(Ordering::Less);
    }
    let trunc = float.trunc() as i64;
    match integer.cmp(&trunc) {
        Ordering::Equal if float.fract() > 0.0 => Some(Ordering::Less),
        Ordering::Equal if float.fract() < 0.0 => Some(Ordering::Greater),
        other => Some(other),
    }
}

// Lua's float modulus uses fmod, then adjusts a nonzero remainder
// whose sign disagrees with the divisor. This preserves signed zero and
// avoids the precision loss of computing a - floor(a/b) * b.
fn lua_float_modulus(a: f64, b: f64) -> f64 {
    let mut remainder = a % b;
    if (remainder > 0.0 && b < 0.0) || (remainder < 0.0 && b > 0.0) {
        remainder += b;
    }
    remainder
}

fn numeric_comparison(
    left: RegisterValue,
    right: RegisterValue,
) -> Result<Option<Ordering>, Error> {
    match (left, right) {
        (RegisterValue::Integer(a), RegisterValue::Integer(b)) => Ok(Some(a.cmp(&b))),
        (RegisterValue::Integer(a), RegisterValue::Float(b)) => {
            Ok(compare_integer_float(a, f64::from_bits(b)))
        }
        (RegisterValue::Float(a), RegisterValue::Integer(b)) => {
            Ok(compare_integer_float(b, f64::from_bits(a)).map(Ordering::reverse))
        }
        (RegisterValue::Float(a), RegisterValue::Float(b)) => {
            Ok(f64::from_bits(a).partial_cmp(&f64::from_bits(b)))
        }
        _ => Err(Error::TypeError),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instruction {
    Load {
        dest: usize,
        value: i64,
    },
    LoadNil {
        dest: usize,
    },
    LoadBoolean {
        dest: usize,
        value: bool,
    },
    LoadFloat {
        dest: usize,
        bits: u64,
    },
    LoadString {
        dest: usize,
        index: usize,
    },
    LoadBuiltin {
        dest: usize,
        builtin: RegisterBuiltin,
    },
    /// Read a global environment binding, defaulting to nil when absent.
    LoadGlobal {
        dest: usize,
        name: usize,
    },
    NewTable {
        dest: usize,
    },
    GetTable {
        dest: usize,
        table: usize,
        key: usize,
    },
    SetTable {
        table: usize,
        key: usize,
        value: usize,
    },
    /// Expand a final unparenthesized call into consecutive array fields.
    ExpandTableResults {
        table: usize,
        first: i64,
    },
    /// Write a global after every right-hand side has been evaluated.
    StoreGlobal {
        name: usize,
        source: usize,
    },
    /// Arguments are evaluated first and placed on the call stack in Lua order.
    PushArgument {
        source: usize,
    },
    Call {
        dest: usize,
        callee: usize,
        argc: usize,
        /// Expand the previous call's complete result vector into this
        /// call's final argument position, as required by Lua.
        expand_tail: bool,
    },
    /// Read one value from the most recent call, padding with nil.
    ReadCallResult {
        dest: usize,
        index: usize,
    },
    /// Expand all results from the immediately preceding final expression call.
    AppendReturnExpansion,
    Length {
        dest: usize,
        input: usize,
    },
    /// Lua string concatenation of byte strings or numeric scalars.
    Concat {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    Neg {
        dest: usize,
        input: usize,
    },
    Add {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    Sub {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    Mul {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    /// Lua's ordinary division always yields a floating-point number.
    Div {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    /// Lua exponentiation also produces a floating-point result.
    Pow {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    FloorDiv {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    Mod {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    BitAnd {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    BitOr {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    BitXor {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    BitNot {
        dest: usize,
        input: usize,
    },
    Shl {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    Shr {
        dest: usize,
        lhs: usize,
        rhs: usize,
    },
    /// Conditional comparison values are internal boolean guard registers,
    /// not general integer-valued Lua expressions.
    Compare {
        dest: usize,
        lhs: usize,
        rhs: usize,
        operation: CompareOperation,
    },
    BranchIfFalse {
        condition: usize,
        target: usize,
    },
    /// Generic Lua iteration ends on nil, not false.
    BranchIfNil {
        value: usize,
        target: usize,
    },
    Return {
        source: usize,
    },
    /// Append one evaluated scalar return value, retaining its position.
    /// Executed return paths have exact Lua zero-versus-nil arity.
    AppendReturn {
        source: usize,
    },
    NumericTruthy {
        dest: usize,
        input: usize,
    },
    BooleanNot {
        dest: usize,
        input: usize,
    },
    BooleanCopy {
        dest: usize,
        input: usize,
    },
    /// Copy the selected predecessor's integer into one merge register.
    /// Exactly one predecessor executes on a valid forward-only path.
    MergeCopy {
        dest: usize,
        input: usize,
    },
    Jump {
        target: usize,
    },
    /// Copy a predecessor value into a loop-carried register. Unlike an
    /// SSA definition, the target is updated once per completed iteration.
    LoopCopy {
        dest: usize,
        input: usize,
    },
    /// Clear temporary SSA registers before a new loop iteration. This
    /// includes the condition and body temporaries, but not carried locals.
    LoopReset {
        start: usize,
        end: usize,
    },
    /// Only validated backedges to a LoopReset instruction are permitted.
    LoopJump {
        target: usize,
    },
    /// Test the repeat-until condition after executing its body, branching
    /// backward only when false.
    LoopBranchIfFalse {
        condition: usize,
        target: usize,
    },
    /// Integer numeric-for controls are evaluated once before the loop
    /// variable enters scope. The induction value is copied into the
    /// read-only visible register on each iteration.
    NumericForCheck {
        counter: usize,
        limit: usize,
        step: usize,
        visible: usize,
        exit: usize,
    },
    /// Advance without wrapping: an integer loop ends on overflow.
    NumericForStep {
        counter: usize,
        step: usize,
        exit: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareOperation {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    code: Vec<Instruction>,
    registers: usize,
    result: usize,
    strings: Vec<Vec<u8>>,
}
/// The result and all immutable byte strings created during one bounded
/// register execution. A dynamic string handle belongs to this execution,
/// not to the compiled Program's static literal pool.
#[derive(Clone, Debug, PartialEq, Eq)]
enum TableKey {
    Boolean(bool),
    Integer(i64),
    Float(u64),
    /// Interned index into the execution-owned byte-string pool.
    String(usize),
    Table(usize),
    Builtin(RegisterBuiltin),
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct RegisterTable {
    entries: Vec<(TableKey, RegisterValue)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterExecution {
    /// First scalar result, adjusted to nil for zero-result returns.
    pub value: RegisterValue,
    /// Exact arity and values for explicitly returned scalar expressions.
    values: Vec<RegisterValue>,
    strings: Vec<Vec<u8>>,
    tables: Vec<RegisterTable>,
}

impl RegisterExecution {
    pub fn values(&self) -> &[RegisterValue] {
        &self.values
    }

    /// Inspect a key in a returned table from this execution. A missing
    /// key yields Lua nil; the heap remains owned by the result.
    pub fn table_value(&self, table: RegisterValue, key: RegisterValue) -> Option<RegisterValue> {
        let RegisterValue::Table(index) = table else {
            return None;
        };
        let key = register_table_key(key, &self.strings).ok()?;
        let entries = &self.tables.get(index)?.entries;
        Some(
            entries
                .iter()
                .find(|(candidate, _)| *candidate == key)
                .map_or(RegisterValue::Nil, |(_, value)| *value),
        )
    }

    /// Resolve any returned string against the execution-owned string pool.
    pub fn value_string_bytes(&self, value: RegisterValue) -> Option<&[u8]> {
        match value {
            RegisterValue::String(index) => self.strings.get(index).map(Vec::as_slice),
            _ => None,
        }
    }

    pub fn string_bytes(&self) -> Option<&[u8]> {
        match self.value {
            RegisterValue::String(index) => self.strings.get(index).map(Vec::as_slice),
            _ => None,
        }
    }
}

impl Program {
    pub fn instructions(&self) -> &[Instruction] {
        &self.code
    }
    pub fn register_count(&self) -> usize {
        self.registers
    }

    /// Decode a returned string handle against its originating program.
    pub fn string_bytes(&self, value: RegisterValue) -> Option<&[u8]> {
        match value {
            RegisterValue::String(index) => self.strings.get(index).map(Vec::as_slice),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Unsupported(String),
    TooLarge,
    InvalidBytecode,
    DivisionByZero,
    TypeError,
    ZeroForStep,
    StepBudgetExceeded,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(s) => write!(f, "unsupported arithmetic subset: {s}"),
            Self::TooLarge => write!(f, "experimental program is too large"),
            Self::InvalidBytecode => write!(f, "invalid experimental register bytecode"),
            Self::DivisionByZero => write!(f, "integer division by zero"),
            Self::TypeError => write!(f, "noninteger operand in register arithmetic"),
            Self::ZeroForStep => write!(f, "numeric for step cannot be zero"),
            Self::StepBudgetExceeded => write!(f, "bounded register VM step budget exhausted"),
        }
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy)]
struct RegisterBinding {
    register: usize,
    constant: bool,
}

enum AssignmentDestination<'a> {
    Name(&'a str),
    Table { table: usize, key: usize },
}

// Pending exits belong to the innermost active loop. Each exit preserves
// the loop-carried bindings visible along that particular lexical path.
struct LoopExit<'a> {
    carried: Vec<(usize, &'a str, usize)>,
    jumps: Vec<usize>,
}

/// Bound instruction emission itself, not only the completed chunk size.
fn emit_instruction(program: &mut Program, instruction: Instruction) -> Result<(), Error> {
    if program.code.len() >= MAX_REGISTERS {
        return Err(Error::TooLarge);
    }
    program.code.push(instruction);
    Ok(())
}

/// Emit a bounded literal key as a typed register value.
fn emit_table_key_string(program: &mut Program, name: &str) -> Result<usize, Error> {
    if program.registers >= MAX_REGISTERS {
        return Err(Error::TooLarge);
    }
    let index = intern_global_name(program, name)?;
    let dest = program.registers;
    program.registers += 1;
    emit_instruction(program, Instruction::LoadString { dest, index })?;
    Ok(dest)
}

fn emit_table_key_integer(program: &mut Program, number: i64) -> Result<usize, Error> {
    if program.registers >= MAX_REGISTERS {
        return Err(Error::TooLarge);
    }
    let dest = program.registers;
    program.registers += 1;
    emit_instruction(
        program,
        Instruction::Load {
            dest,
            value: number,
        },
    )?;
    Ok(dest)
}

/// Bind an ASCII Lua global name to an immutable byte-string pool slot.
/// Global bindings themselves remain per-execution, never process-wide.
fn intern_global_name(program: &mut Program, name: &str) -> Result<usize, Error> {
    if let Some(index) = program
        .strings
        .iter()
        .position(|item| item.as_slice() == name.as_bytes())
    {
        return Ok(index);
    }
    let used = program.strings.iter().map(Vec::len).sum::<usize>();
    if program.strings.len() >= MAX_REGISTERS
        || used.saturating_add(name.len()) > MAX_REGISTER_STRING_BYTES
    {
        return Err(Error::TooLarge);
    }
    let index = program.strings.len();
    program.strings.push(name.as_bytes().to_vec());
    Ok(index)
}

fn emit<'a>(
    program: &mut Program,
    expression: &Expr<'a>,
    depth: usize,
    locals: &[HashMap<&'a str, RegisterBinding>],
) -> Result<usize, Error> {
    if depth > 128 || program.registers >= MAX_REGISTERS {
        return Err(Error::TooLarge);
    }
    let instruction = match expression {
        Expr::Grouped(inner) => return emit(program, inner, depth + 1, locals),
        Expr::Atom("nil") => Instruction::LoadNil {
            dest: program.registers,
        },
        Expr::Atom("false") | Expr::Atom("true") => Instruction::LoadBoolean {
            dest: program.registers,
            value: matches!(expression, Expr::Atom("true")),
        },
        Expr::Atom(text)
            if text.starts_with('\'') || text.starts_with('"') || text.starts_with('[') =>
        {
            let bytes =
                decode_lua_string(text).map_err(|err| Error::Unsupported(err.to_string()))?;
            let index = if let Some(existing) = program.strings.iter().position(|s| *s == bytes) {
                existing
            } else {
                let used = program.strings.iter().map(Vec::len).sum::<usize>();
                if program.strings.len() >= MAX_REGISTERS
                    || used.saturating_add(bytes.len()) > MAX_REGISTER_STRING_BYTES
                {
                    return Err(Error::TooLarge);
                }
                let index = program.strings.len();
                program.strings.push(bytes);
                index
            };
            Instruction::LoadString {
                dest: program.registers,
                index,
            }
        }
        Expr::Atom(text) => {
            let hex = text.starts_with("0x") || text.starts_with("0X");
            let integer = if hex {
                parse_lua_hex_integer_wrapping(text)
            } else {
                text.parse::<i64>().ok()
            };
            match integer {
                Some(value) => Instruction::Load {
                    dest: program.registers,
                    value,
                },
                None => {
                    // Numeric tokens starting with a digit or decimal point
                    // may be Lua floats; never interpret the name of a
                    // local like "inf" or "NaN" as a Rust float constant.
                    let numeric_token = text.as_bytes().first().is_some_and(u8::is_ascii_digit)
                        || (text.starts_with('.')
                            && text.as_bytes().get(1).is_some_and(u8::is_ascii_digit));
                    let floating = if hex {
                        parse_lua_hex_float(text)
                    } else {
                        text.parse::<f64>().ok()
                    };
                    if numeric_token && let Some(number) = floating {
                        let output = program.registers;
                        program.registers += 1;
                        emit_instruction(
                            program,
                            Instruction::LoadFloat {
                                dest: output,
                                bits: number.to_bits(),
                            },
                        )?;
                        return Ok(output);
                    }
                    if let Some(register) = locals
                        .iter()
                        .rev()
                        .find_map(|scope| scope.get(*text).map(|binding| binding.register))
                    {
                        return Ok(register);
                    }
                    // Full Lua's _ENV is a table with arbitrary field
                    // operations. Until that feature is implemented, do
                    // not silently represent it as nil.
                    if *text == "_ENV" {
                        return Err(Error::Unsupported(
                            "dynamic _ENV tables are not available in register VM".into(),
                        ));
                    }
                    let name = intern_global_name(program, text)?;
                    let dest = program.registers;
                    program.registers += 1;
                    emit_instruction(program, Instruction::LoadGlobal { dest, name })?;
                    return Ok(dest);
                }
            }
        }
        Expr::Table(fields) => {
            if fields.len() > MAX_REGISTER_TABLE_ENTRIES {
                return Err(Error::TooLarge);
            }
            let dest = program.registers;
            program.registers += 1;
            emit_instruction(program, Instruction::NewTable { dest })?;
            let mut sequence = 0_i64;
            // Lua expands the final array-style expression even if named or
            // explicit-key fields follow it in the constructor.
            let last_array_field = fields
                .iter()
                .rposition(|field| matches!(field, TableField::Value(_)));
            for (position, field) in fields.iter().enumerate() {
                if let TableField::Value(value) = field
                    && Some(position) == last_array_field
                    && matches!(value, Expr::Call { .. })
                {
                    sequence = sequence.checked_add(1).ok_or(Error::TooLarge)?;
                    let _ = emit(program, value, depth + 1, locals)?;
                    emit_instruction(
                        program,
                        Instruction::ExpandTableResults {
                            table: dest,
                            first: sequence,
                        },
                    )?;
                    continue;
                }
                let (key, value) = match field {
                    TableField::Named { name, value } => {
                        (emit_table_key_string(program, name)?, value)
                    }
                    TableField::Indexed { key, value } => {
                        (emit(program, key, depth + 1, locals)?, value)
                    }
                    TableField::Value(value) => {
                        sequence = sequence.checked_add(1).ok_or(Error::TooLarge)?;
                        (emit_table_key_integer(program, sequence)?, value)
                    }
                };
                let source = emit(program, value, depth + 1, locals)?;
                emit_instruction(
                    program,
                    Instruction::SetTable {
                        table: dest,
                        key,
                        value: source,
                    },
                )?;
            }
            return Ok(dest);
        }
        Expr::Field { object, name } => {
            let table = emit(program, object, depth + 1, locals)?;
            let key = emit_table_key_string(program, name)?;
            Instruction::GetTable {
                dest: program.registers,
                table,
                key,
            }
        }
        Expr::Index { object, key } => {
            let table = emit(program, object, depth + 1, locals)?;
            let key = emit(program, key, depth + 1, locals)?;
            Instruction::GetTable {
                dest: program.registers,
                table,
                key,
            }
        }
        Expr::MethodCall {
            object,
            method,
            arguments,
        } => {
            // Evaluate the receiver exactly once, fetch its method, and pass
            // the original receiver as Lua's implicit first argument.
            if arguments.len() >= MAX_CALL_ARGUMENTS {
                return Err(Error::TooLarge);
            }
            let receiver = emit(program, object, depth + 1, locals)?;
            let key = emit_table_key_string(program, method)?;
            if program.registers >= MAX_REGISTERS {
                return Err(Error::TooLarge);
            }
            let callee = program.registers;
            program.registers += 1;
            emit_instruction(
                program,
                Instruction::GetTable {
                    dest: callee,
                    table: receiver,
                    key,
                },
            )?;
            emit_instruction(program, Instruction::PushArgument { source: receiver })?;
            let expand_tail = matches!(arguments.last(), Some(Expr::Call { .. }));
            for (index, argument) in arguments.iter().enumerate() {
                let source = emit(program, argument, depth + 1, locals)?;
                if !(expand_tail && index + 1 == arguments.len()) {
                    emit_instruction(program, Instruction::PushArgument { source })?;
                }
            }
            Instruction::Call {
                dest: program.registers,
                callee,
                argc: arguments.len() + 1 - usize::from(expand_tail),
                expand_tail,
            }
        }
        Expr::Call { callee, arguments } => {
            // A bounded register-call subset with actual function values.
            // Nested calls consume only their own pushed arguments.
            if arguments.len() > MAX_CALL_ARGUMENTS {
                return Err(Error::TooLarge);
            }
            let callee = emit(program, callee, depth + 1, locals)?;
            let expand_tail = matches!(arguments.last(), Some(Expr::Call { .. }));
            for (index, argument) in arguments.iter().enumerate() {
                let source = emit(program, argument, depth + 1, locals)?;
                if !(expand_tail && index + 1 == arguments.len()) {
                    emit_instruction(program, Instruction::PushArgument { source })?;
                }
            }
            Instruction::Call {
                dest: program.registers,
                callee,
                argc: arguments.len() - usize::from(expand_tail),
                expand_tail,
            }
        }
        Expr::Unary { op: "#", right } => {
            let input = emit(program, right, depth + 1, locals)?;
            Instruction::Length {
                dest: program.registers,
                input,
            }
        }
        Expr::Unary { op, right } if matches!(*op, "-" | "~") => {
            let input = emit(program, right, depth + 1, locals)?;
            if *op == "-" {
                Instruction::Neg {
                    dest: program.registers,
                    input,
                }
            } else {
                Instruction::BitNot {
                    dest: program.registers,
                    input,
                }
            }
        }
        Expr::Unary { op: "not", right } => {
            let input = emit(program, right, depth + 1, locals)?;
            Instruction::BooleanNot {
                dest: program.registers,
                input,
            }
        }
        Expr::Binary { op, left, right } if matches!(*op, "and" | "or") => {
            // Lua returns the selected operand, not a coerced Boolean.
            // Only one predecessor executes per evaluation.
            let lhs = emit(program, left, depth + 1, locals)?;
            if program.registers >= MAX_REGISTERS {
                return Err(Error::TooLarge);
            }
            let dest = program.registers;
            program.registers += 1;
            let branch_at = program.code.len();
            emit_instruction(
                program,
                Instruction::BranchIfFalse {
                    condition: lhs,
                    target: usize::MAX,
                },
            )?;
            if *op == "and" {
                let rhs = emit(program, right, depth + 1, locals)?;
                emit_instruction(program, Instruction::MergeCopy { dest, input: rhs })?;
                let jump_at = program.code.len();
                emit_instruction(program, Instruction::Jump { target: usize::MAX })?;
                let false_at = program.code.len();
                emit_instruction(program, Instruction::MergeCopy { dest, input: lhs })?;
                let after = program.code.len();
                program.code[branch_at] = Instruction::BranchIfFalse {
                    condition: lhs,
                    target: false_at,
                };
                program.code[jump_at] = Instruction::Jump { target: after };
            } else {
                emit_instruction(program, Instruction::MergeCopy { dest, input: lhs })?;
                let jump_at = program.code.len();
                emit_instruction(program, Instruction::Jump { target: usize::MAX })?;
                let rhs_at = program.code.len();
                let rhs = emit(program, right, depth + 1, locals)?;
                emit_instruction(program, Instruction::MergeCopy { dest, input: rhs })?;
                let after = program.code.len();
                program.code[branch_at] = Instruction::BranchIfFalse {
                    condition: lhs,
                    target: rhs_at,
                };
                program.code[jump_at] = Instruction::Jump { target: after };
            }
            return Ok(dest);
        }
        Expr::Binary { op, left, right }
            if matches!(*op, "==" | "~=" | "<" | "<=" | ">" | ">=") =>
        {
            let lhs = emit(program, left, depth + 1, locals)?;
            let rhs = emit(program, right, depth + 1, locals)?;
            let operation = match *op {
                "==" => CompareOperation::Equal,
                "~=" => CompareOperation::NotEqual,
                "<" => CompareOperation::Less,
                "<=" => CompareOperation::LessEqual,
                ">" => CompareOperation::Greater,
                ">=" => CompareOperation::GreaterEqual,
                _ => unreachable!("matched comparison operator"),
            };
            Instruction::Compare {
                dest: program.registers,
                lhs,
                rhs,
                operation,
            }
        }
        Expr::Binary { op, left, right }
            if matches!(
                *op,
                "+" | "-" | "*" | "/" | "^" | "//" | "%" | "&" | "|" | "~" | "<<" | ">>" | ".."
            ) =>
        {
            let lhs = emit(program, left, depth + 1, locals)?;
            let rhs = emit(program, right, depth + 1, locals)?;
            let dest = program.registers;
            match *op {
                "+" => Instruction::Add { dest, lhs, rhs },
                ".." => Instruction::Concat { dest, lhs, rhs },
                "-" => Instruction::Sub { dest, lhs, rhs },
                "*" => Instruction::Mul { dest, lhs, rhs },
                "/" => Instruction::Div { dest, lhs, rhs },
                "^" => Instruction::Pow { dest, lhs, rhs },
                "//" => Instruction::FloorDiv { dest, lhs, rhs },
                "%" => Instruction::Mod { dest, lhs, rhs },
                "&" => Instruction::BitAnd { dest, lhs, rhs },
                "|" => Instruction::BitOr { dest, lhs, rhs },
                "~" => Instruction::BitXor { dest, lhs, rhs },
                "<<" => Instruction::Shl { dest, lhs, rhs },
                ">>" => Instruction::Shr { dest, lhs, rhs },
                _ => unreachable!("matched operators only"),
            }
        }
        _ => {
            return Err(Error::Unsupported(
                "only integer arithmetic and bitwise expressions".into(),
            ));
        }
    };
    let output = program.registers;
    program.registers += 1;
    emit_instruction(program, instruction)?;
    Ok(output)
}

/// Nil and boolean literal identity for condition-only comparisons.
/// An outer Some distinguishes the nil value from an unsupported expression.
fn boolean_nil_literal(expression: &Expr<'_>) -> Option<Option<bool>> {
    match expression {
        Expr::Atom("nil") => Some(None),
        Expr::Atom("false") => Some(Some(false)),
        Expr::Atom("true") => Some(Some(true)),
        Expr::Grouped(inner) => boolean_nil_literal(inner),
        _ => None,
    }
}

/// Compile Boolean literals, integer comparisons, unary not and pure integer
/// expressions into internal guards. Unlike some host languages, every Lua
/// integer, including zero, is truthy. These Boolean guard registers are never
/// made available as general Lua integer expression values.
fn emit_condition<'a>(
    program: &mut Program,
    expression: &Expr<'a>,
    locals: &[HashMap<&'a str, RegisterBinding>],
    depth: usize,
) -> Result<usize, Error> {
    if depth > 64 || program.registers >= MAX_REGISTERS {
        return Err(Error::TooLarge);
    }
    match expression {
        Expr::Grouped(inner) => emit_condition(program, inner, locals, depth + 1),
        Expr::Atom(value)
            if matches!(*value, "true" | "false" | "nil")
                || matches!(value.as_bytes().first(), Some(b'\'' | b'"' | b'['))
                || value
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| byte.is_ascii_digit())
                || (value.starts_with('.')
                    && value
                        .as_bytes()
                        .get(1)
                        .is_some_and(|byte| byte.is_ascii_digit())) =>
        {
            // This register VM only carries integers. Lua literal truthiness
            // can nonetheless be lowered exactly as a condition-only guard:
            // nil and false are false; every string and number is true,
            // including empty strings and floating-point zero.
            let dest = program.registers;
            program.registers += 1;
            emit_instruction(
                program,
                Instruction::LoadBoolean {
                    dest,
                    value: !matches!(*value, "false" | "nil"),
                },
            )?;
            Ok(dest)
        }
        Expr::Unary { op, right } if *op == "not" => {
            let source = emit_condition(program, right, locals, depth + 1)?;
            if program.registers >= MAX_REGISTERS {
                return Err(Error::TooLarge);
            }
            let dest = program.registers;
            program.registers += 1;
            emit_instruction(
                program,
                Instruction::BooleanNot {
                    dest,
                    input: source,
                },
            )?;
            Ok(dest)
        }
        Expr::Binary { op, left, right } if matches!(*op, "and" | "or") => {
            let lhs = emit_condition(program, left, locals, depth + 1)?;
            if program.registers >= MAX_REGISTERS {
                return Err(Error::TooLarge);
            }
            let dest = program.registers;
            program.registers += 1;
            let branch_at = program.code.len();
            emit_instruction(
                program,
                Instruction::BranchIfFalse {
                    condition: lhs,
                    target: usize::MAX,
                },
            )?;
            if *op == "and" {
                // False lhs skips evaluation of the rhs completely.
                let rhs = emit_condition(program, right, locals, depth + 1)?;
                emit_instruction(program, Instruction::BooleanCopy { dest, input: rhs })?;
                let jump_at = program.code.len();
                emit_instruction(program, Instruction::Jump { target: usize::MAX })?;
                let false_at = program.code.len();
                emit_instruction(program, Instruction::LoadBoolean { dest, value: false })?;
                let after = program.code.len();
                program.code[branch_at] = Instruction::BranchIfFalse {
                    condition: lhs,
                    target: false_at,
                };
                program.code[jump_at] = Instruction::Jump { target: after };
            } else {
                // True lhs skips evaluation of the rhs completely.
                emit_instruction(program, Instruction::LoadBoolean { dest, value: true })?;
                let jump_at = program.code.len();
                emit_instruction(program, Instruction::Jump { target: usize::MAX })?;
                let rhs_at = program.code.len();
                let rhs = emit_condition(program, right, locals, depth + 1)?;
                emit_instruction(program, Instruction::BooleanCopy { dest, input: rhs })?;
                let after = program.code.len();
                program.code[branch_at] = Instruction::BranchIfFalse {
                    condition: lhs,
                    target: rhs_at,
                };
                program.code[jump_at] = Instruction::Jump { target: after };
            }
            Ok(dest)
        }
        Expr::Binary { op, left, right }
            if matches!(*op, "==" | "~=" | "<" | "<=" | ">" | ">=") =>
        {
            let operation = match *op {
                "==" => CompareOperation::Equal,
                "~=" => CompareOperation::NotEqual,
                "<" => CompareOperation::Less,
                "<=" => CompareOperation::LessEqual,
                ">" => CompareOperation::Greater,
                ">=" => CompareOperation::GreaterEqual,
                _ => {
                    return Err(Error::Unsupported(
                        "integer conditional needs a comparison or Boolean literal".into(),
                    ));
                }
            };
            // Lua compares nil and boolean values by type and identity.
            // These are condition-only constants; the integer register
            // representation is deliberately not widened to hold Lua values.
            if matches!(
                operation,
                CompareOperation::Equal | CompareOperation::NotEqual
            ) && let (Some(lhs), Some(rhs)) =
                (boolean_nil_literal(left), boolean_nil_literal(right))
            {
                let same = lhs == rhs;
                let result = if operation == CompareOperation::Equal {
                    same
                } else {
                    !same
                };
                let dest = program.registers;
                program.registers += 1;
                emit_instruction(
                    program,
                    Instruction::LoadBoolean {
                        dest,
                        value: result,
                    },
                )?;
                return Ok(dest);
            }
            let lhs = emit(program, left, depth + 1, locals)?;
            let rhs = emit(program, right, depth + 1, locals)?;
            if program.registers >= MAX_REGISTERS {
                return Err(Error::TooLarge);
            }
            let dest = program.registers;
            program.registers += 1;
            emit_instruction(
                program,
                Instruction::Compare {
                    dest,
                    lhs,
                    rhs,
                    operation,
                },
            )?;
            Ok(dest)
        }
        _ => {
            // All currently lowerable arithmetic expressions yield integers.
            // Evaluate them to retain runtime errors, then encode Lua's
            // unconditional numeric truthiness in a separate guard.
            let input = emit(program, expression, depth + 1, locals)?;
            if program.registers >= MAX_REGISTERS {
                return Err(Error::TooLarge);
            }
            let dest = program.registers;
            program.registers += 1;
            emit_instruction(program, Instruction::NumericTruthy { dest, input })?;
            Ok(dest)
        }
    }
}

/// Compile a bounded Lua scalar-expression subset into typed register IR.
/// Lua globals, calls, dynamic strings, tables and side effects remain unsupported.
pub fn compile_integer_expression(source: &str) -> Result<Program, Error> {
    let ast = parse_expression(source).map_err(|err| Error::Unsupported(err.to_string()))?;
    let mut program = Program {
        code: Vec::new(),
        registers: 0,
        result: 0,
        strings: Vec::new(),
    };
    let result = emit(&mut program, &ast, 0, &[])?;
    program.result = result;
    // Branch targets in a standalone short-circuit expression must
    // point at a real terminal instruction, not one-past-bytecode.
    if program
        .code
        .iter()
        .any(|op| matches!(op, Instruction::BranchIfFalse { .. }))
    {
        emit_instruction(&mut program, Instruction::Return { source: result })?;
    }
    Ok(program)
}

/// Compile a static subset of Lua chunks to Ferrua-owned register bytecode.
///
/// Supports typed locals, limited execution-owned tables and globals, Lua
/// nil-padding, parallel assignments, branches and bounded numeric loops.
/// Bounded builtins can transport variable result counts in final-call
/// expression positions. Arbitrary closures, general iteration, varargs,
/// metamethods and full Lua library dispatch are not yet supported.
pub fn compile_integer_chunk(source: &str) -> Result<Program, Error> {
    let statements = parse_chunk(source).map_err(|e| Error::Unsupported(e.to_string()))?;
    let mut program = Program {
        code: Vec::new(),
        registers: 0,
        result: 0,
        strings: Vec::new(),
    };
    let mut scopes = vec![HashMap::new()];
    let mut loops = Vec::new();
    let value = match compile_statements(&statements, &mut program, &mut scopes, &mut loops, 0)? {
        Some(value) => value,
        None => {
            // A Lua chunk or function may run off the end without returning
            // values. In this scalar register API, the observable result is
            // nil; the epilogue also anchors all fallthrough branch targets.
            if program.registers >= MAX_REGISTERS {
                return Err(Error::TooLarge);
            }
            let register = program.registers;
            program.registers += 1;
            emit_instruction(&mut program, Instruction::LoadNil { dest: register })?;
            emit_instruction(&mut program, Instruction::Return { source: register })?;
            register
        }
    };
    if program.code.len() > MAX_REGISTERS {
        return Err(Error::TooLarge);
    }
    program.result = value;
    Ok(program)
}

/// Conservatively prove a block always returns; false only causes
/// extra merge-register reservation, never a missing register assignment.
fn statements_definitely_return(statements: &[Statement<'_>]) -> bool {
    for statement in statements {
        match statement {
            Statement::Return(_) => return true,
            Statement::Block(body) if statements_definitely_return(body) => return true,
            Statement::If {
                branches,
                else_branch: Some(else_body),
            } if branches
                .iter()
                .all(|(_, body)| statements_definitely_return(body))
                && statements_definitely_return(else_body) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Emit one unique integer-register definition on the selected predecessor.
/// Parent scopes are identified by lexical depth, so shadowing is preserved.
fn emit_merge_copies<'a>(
    program: &mut Program,
    merge: &[(usize, &'a str, usize)],
    path: &[HashMap<&'a str, RegisterBinding>],
) -> Result<(), Error> {
    for &(level, name, dest) in merge {
        let source = path
            .get(level)
            .and_then(|scope| scope.get(name))
            .ok_or(Error::InvalidBytecode)?
            .register;
        emit_instruction(
            program,
            Instruction::MergeCopy {
                dest,
                input: source,
            },
        )?;
    }
    Ok(())
}

/// Conservative set of names assigned by this branch, including nested
/// blocks. Shadowed names may cause a harmless extra join, but missing a
/// mutated enclosing binding would create an uninitialized merge register.
fn assigned_names<'a>(body: &[Statement<'a>], names: &mut HashSet<&'a str>) {
    let mut pending = vec![body];
    while let Some(statements) = pending.pop() {
        for node in statements {
            match node {
                Statement::Assign { targets, .. } => {
                    for target in targets {
                        if let Expr::Atom(name) = target {
                            names.insert(*name);
                        }
                    }
                }
                Statement::Block(inner)
                | Statement::While { body: inner, .. }
                | Statement::Repeat { body: inner, .. }
                | Statement::NumericFor { body: inner, .. }
                | Statement::GenericFor { body: inner, .. } => pending.push(inner),
                Statement::If {
                    branches,
                    else_branch,
                } => {
                    for (_, inner) in branches {
                        pending.push(inner);
                    }
                    if let Some(inner) = else_branch {
                        pending.push(inner);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Evaluate RHS values before changing bindings. Lua expands a final
/// unparenthesized call into its available results; every other expression
/// contributes exactly one scalar. Extra results are discarded and missing
/// results receive nil.
fn adjusted_rhs<'a>(
    program: &mut Program,
    expressions: &[Expr<'a>],
    targets: usize,
    scopes: &[HashMap<&'a str, RegisterBinding>],
) -> Result<Vec<usize>, Error> {
    let mut sources = Vec::with_capacity(targets);
    for (position, expression) in expressions.iter().enumerate() {
        let register = emit(program, expression, 0, scopes)?;
        if sources.len() < targets {
            sources.push(register);
            if position + 1 == expressions.len() && matches!(expression, Expr::Call { .. }) {
                let remaining = targets - sources.len();
                for index in 1..=remaining {
                    if program.registers >= MAX_REGISTERS {
                        return Err(Error::TooLarge);
                    }
                    let dest = program.registers;
                    program.registers += 1;
                    emit_instruction(program, Instruction::ReadCallResult { dest, index })?;
                    sources.push(dest);
                }
            }
        }
    }
    while sources.len() < targets {
        if program.registers >= MAX_REGISTERS {
            return Err(Error::TooLarge);
        }
        let register = program.registers;
        program.registers += 1;
        emit_instruction(program, Instruction::LoadNil { dest: register })?;
        sources.push(register);
    }
    Ok(sources)
}

fn compile_statements<'a>(
    statements: &[Statement<'a>],
    program: &mut Program,
    scopes: &mut Vec<HashMap<&'a str, RegisterBinding>>,
    loops: &mut Vec<LoopExit<'a>>,
    depth: usize,
) -> Result<Option<usize>, Error> {
    if depth > 64 {
        return Err(Error::TooLarge);
    }
    let mut result = None;
    for node in statements {
        if result.is_some() {
            return Err(Error::Unsupported("statement follows return".into()));
        }
        match node {
            Statement::Local {
                names,
                attributes,
                initializers,
            } => {
                if attributes
                    .iter()
                    .any(|attribute| matches!(attribute, Some(LocalAttribute::Close)))
                {
                    return Err(Error::Unsupported(
                        "to-be-closed local variables require unavailable finalizers".into(),
                    ));
                }
                if names.is_empty() || names.len() != attributes.len() {
                    return Err(Error::InvalidBytecode);
                }
                // All initializers use the preceding lexical scope.
                // Missing values are nil; surplus scalar results are
                // evaluated and discarded, matching Lua assignment order.
                let rhs = adjusted_rhs(program, initializers, names.len(), scopes)?;
                for ((name, attribute), register) in names.iter().zip(attributes).zip(rhs) {
                    scopes.last_mut().expect("root scope").insert(
                        *name,
                        RegisterBinding {
                            register,
                            constant: *attribute == Some(LocalAttribute::Const),
                        },
                    );
                }
            }
            Statement::Assign { targets, values } => {
                if targets.is_empty() {
                    return Err(Error::InvalidBytecode);
                }
                // Resolve table bases and keys before computing any RHS.
                // Store operations still occur only after every RHS evaluates,
                // so aliasing and parallel assignments retain Lua order.
                let mut destinations = Vec::with_capacity(targets.len());
                for target in targets {
                    let destination = match target {
                        Expr::Atom(name) => AssignmentDestination::Name(name),
                        Expr::Field { object, name } => {
                            let table = emit(program, object, 0, scopes)?;
                            let key = emit_table_key_string(program, name)?;
                            AssignmentDestination::Table { table, key }
                        }
                        Expr::Index { object, key } => {
                            let table = emit(program, object, 0, scopes)?;
                            let key = emit(program, key, 0, scopes)?;
                            AssignmentDestination::Table { table, key }
                        }
                        _ => {
                            return Err(Error::Unsupported(
                                "unsupported assignment target in register VM".into(),
                            ));
                        }
                    };
                    destinations.push(destination);
                }
                let rhs = adjusted_rhs(program, values, targets.len(), scopes)?;
                // Lua assigns competing destinations right-to-left after all
                // targets and RHS values have been evaluated. The leftmost
                // occurrence wins if an lvalue appears more than once.
                for (target, register) in destinations.into_iter().zip(rhs).rev() {
                    match target {
                        AssignmentDestination::Table { table, key } => {
                            emit_instruction(
                                program,
                                Instruction::SetTable {
                                    table,
                                    key,
                                    value: register,
                                },
                            )?;
                        }
                        AssignmentDestination::Name(name) => {
                            if let Some(binding) = scopes
                                .iter_mut()
                                .rev()
                                .find_map(|scope| scope.get_mut(name))
                            {
                                if binding.constant {
                                    return Err(Error::Unsupported(format!(
                                        "cannot assign to constant local: {name}"
                                    )));
                                }
                                binding.register = register;
                            } else {
                                if name == "_ENV" {
                                    return Err(Error::Unsupported(
                                        "dynamic _ENV tables are not available in register VM"
                                            .into(),
                                    ));
                                }
                                let name = intern_global_name(program, name)?;
                                emit_instruction(
                                    program,
                                    Instruction::StoreGlobal {
                                        name,
                                        source: register,
                                    },
                                )?;
                            }
                        }
                    }
                }
            }
            Statement::Block(body) => {
                scopes.push(HashMap::new());
                let child = compile_statements(body, program, scopes, loops, depth + 1)?;
                scopes.pop();
                // A do/end block does not create a separate return context:
                // returning inside it exits the entire integer chunk.
                // Do not emit instructions for unreachable later statements.
                if let Some(value) = child {
                    result = Some(value);
                    break;
                }
            }
            Statement::If {
                branches,
                else_branch,
            } => {
                // Reserve shared destinations only when an arm might
                // continue. Each branch computes its own predecessor
                // values; the executed path defines each merge once.
                let may_fall_through = branches
                    .iter()
                    .any(|(_, body)| !statements_definitely_return(body))
                    || else_branch
                        .as_ref()
                        .is_none_or(|body| !statements_definitely_return(body));
                let mut assigned = HashSet::new();
                for (_, body) in branches {
                    assigned_names(body, &mut assigned);
                }
                if let Some(body) = else_branch {
                    assigned_names(body, &mut assigned);
                }
                let mut merge = Vec::new();
                if may_fall_through {
                    // Only names a reachable arm might write need phi-like
                    // destination registers; read-only and const locals keep
                    // their original registers across the join.
                    let mut live = scopes
                        .iter()
                        .enumerate()
                        .flat_map(|(level, scope)| {
                            scope
                                .iter()
                                .filter(|(name, binding)| {
                                    !binding.constant && assigned.contains(*name)
                                })
                                .map(move |(name, _)| (level, *name))
                        })
                        .collect::<Vec<_>>();
                    live.sort_unstable();
                    for (level, name) in live {
                        if program.registers >= MAX_REGISTERS {
                            return Err(Error::TooLarge);
                        }
                        let dest = program.registers;
                        program.registers += 1;
                        merge.push((level, name, dest));
                    }
                }

                let mut exit_jumps = Vec::new();
                let mut every_arm_returns = true;
                for (condition, body) in branches {
                    let guard = emit_condition(program, condition, scopes, 0)?;
                    let branch_at = program.code.len();
                    emit_instruction(
                        program,
                        Instruction::BranchIfFalse {
                            condition: guard,
                            target: usize::MAX,
                        },
                    )?;
                    let mut branch_scopes = scopes.clone();
                    branch_scopes.push(HashMap::new());
                    let returned =
                        compile_statements(body, program, &mut branch_scopes, loops, depth + 1)?;
                    if returned.is_none() {
                        if !may_fall_through {
                            return Err(Error::InvalidBytecode);
                        }
                        every_arm_returns = false;
                        emit_merge_copies(program, &merge, &branch_scopes)?;
                        exit_jumps.push(program.code.len());
                        emit_instruction(program, Instruction::Jump { target: usize::MAX })?;
                    }
                    let target = program.code.len();
                    program.code[branch_at] = Instruction::BranchIfFalse {
                        condition: guard,
                        target,
                    };
                }
                let else_returns = if let Some(else_body) = else_branch {
                    let mut branch_scopes = scopes.clone();
                    branch_scopes.push(HashMap::new());
                    let returned = compile_statements(
                        else_body,
                        program,
                        &mut branch_scopes,
                        loops,
                        depth + 1,
                    )?;
                    if returned.is_none() {
                        if !may_fall_through {
                            return Err(Error::InvalidBytecode);
                        }
                        emit_merge_copies(program, &merge, &branch_scopes)?;
                    }
                    returned.is_some()
                } else {
                    emit_merge_copies(program, &merge, scopes)?;
                    false
                };
                let after = program.code.len();
                for at in exit_jumps {
                    program.code[at] = Instruction::Jump { target: after };
                }
                if every_arm_returns && else_returns {
                    result = Some(0);
                    break;
                }
                // All continuing predecessors have initialized their merge
                // destinations. Later statements read only those registers.
                for (level, name, dest) in merge {
                    let binding = scopes
                        .get_mut(level)
                        .and_then(|scope| scope.get_mut(name))
                        .ok_or(Error::InvalidBytecode)?;
                    binding.register = dest;
                }
            }
            Statement::While { condition, body } => {
                // Resolve loop-carried locals at their lexical levels. The
                // initialization copies execute once before the loop header;
                // backedges update only those registers that might change.
                let mut assigned = HashSet::new();
                assigned_names(body, &mut assigned);
                let mut carried = Vec::new();
                let mut candidates = scopes
                    .iter()
                    .enumerate()
                    .flat_map(|(level, scope)| {
                        scope
                            .iter()
                            .filter(|(name, binding)| !binding.constant && assigned.contains(*name))
                            .map(move |(name, _)| (level, *name))
                    })
                    .collect::<Vec<_>>();
                candidates.sort_unstable();
                for (level, name) in candidates {
                    if program.registers >= MAX_REGISTERS {
                        return Err(Error::TooLarge);
                    }
                    let source = scopes
                        .get(level)
                        .and_then(|scope| scope.get(name))
                        .ok_or(Error::InvalidBytecode)?
                        .register;
                    let dest = program.registers;
                    program.registers += 1;
                    emit_instruction(
                        program,
                        Instruction::LoopCopy {
                            dest,
                            input: source,
                        },
                    )?;
                    scopes
                        .get_mut(level)
                        .and_then(|scope| scope.get_mut(name))
                        .ok_or(Error::InvalidBytecode)?
                        .register = dest;
                    carried.push((level, name, dest));
                }
                let start = program.code.len();
                let reset_start = program.registers;
                emit_instruction(
                    program,
                    Instruction::LoopReset {
                        start: reset_start,
                        end: usize::MAX,
                    },
                )?;
                let guard = emit_condition(program, condition, scopes, 0)?;
                let exit_at = program.code.len();
                emit_instruction(
                    program,
                    Instruction::BranchIfFalse {
                        condition: guard,
                        target: usize::MAX,
                    },
                )?;
                let mut inner = scopes.clone();
                inner.push(HashMap::new());
                loops.push(LoopExit {
                    carried: carried.clone(),
                    jumps: Vec::new(),
                });
                let compiled = compile_statements(body, program, &mut inner, loops, depth + 1);
                let exits = loops.pop().ok_or(Error::InvalidBytecode)?;
                let returned = compiled?;
                if returned.is_none() {
                    for &(level, name, dest) in &carried {
                        let source = inner
                            .get(level)
                            .and_then(|scope| scope.get(name))
                            .ok_or(Error::InvalidBytecode)?
                            .register;
                        emit_instruction(
                            program,
                            Instruction::LoopCopy {
                                dest,
                                input: source,
                            },
                        )?;
                    }
                    emit_instruction(program, Instruction::LoopJump { target: start })?;
                }
                program.code[start] = Instruction::LoopReset {
                    start: reset_start,
                    end: program.registers,
                };
                let after = program.code.len();
                program.code[exit_at] = Instruction::BranchIfFalse {
                    condition: guard,
                    target: after,
                };
                for at in exits.jumps {
                    program.code[at] = Instruction::Jump { target: after };
                }
                // An unconditional true loop whose body definitely returns
                // also terminates the chunk. A body that merely breaks or
                // conditionally returns does not establish that guarantee.
                if matches!(condition, Expr::Atom("true"))
                    && statements_definitely_return(body)
                    && let Some(value) = returned
                {
                    // Even an unreachable loop exit must target a valid
                    // instruction; the validator checks every branch.
                    // Materialize a nil epilogue as a defensive exit path.
                    if program.registers >= MAX_REGISTERS {
                        return Err(Error::TooLarge);
                    }
                    let fallback = program.registers;
                    program.registers += 1;
                    emit_instruction(program, Instruction::LoadNil { dest: fallback })?;
                    emit_instruction(program, Instruction::Return { source: fallback })?;
                    result = Some(value);
                    break;
                }
            }
            Statement::Repeat { body, condition } => {
                // The until-condition sees locals declared in the repeat
                // body. Enclosing assignments use mutable carried registers,
                // while each iteration's temporary definitions remain SSA.
                let mut assigned = HashSet::new();
                assigned_names(body, &mut assigned);
                let mut carried = Vec::new();
                let mut candidates = scopes
                    .iter()
                    .enumerate()
                    .flat_map(|(level, scope)| {
                        scope
                            .iter()
                            .filter(|(name, binding)| !binding.constant && assigned.contains(*name))
                            .map(move |(name, _)| (level, *name))
                    })
                    .collect::<Vec<_>>();
                candidates.sort_unstable();
                for (level, name) in candidates {
                    if program.registers >= MAX_REGISTERS {
                        return Err(Error::TooLarge);
                    }
                    let source = scopes
                        .get(level)
                        .and_then(|scope| scope.get(name))
                        .ok_or(Error::InvalidBytecode)?
                        .register;
                    let dest = program.registers;
                    program.registers += 1;
                    emit_instruction(
                        program,
                        Instruction::LoopCopy {
                            dest,
                            input: source,
                        },
                    )?;
                    scopes
                        .get_mut(level)
                        .and_then(|scope| scope.get_mut(name))
                        .ok_or(Error::InvalidBytecode)?
                        .register = dest;
                    carried.push((level, name, dest));
                }
                let start = program.code.len();
                let reset_start = program.registers;
                emit_instruction(
                    program,
                    Instruction::LoopReset {
                        start: reset_start,
                        end: usize::MAX,
                    },
                )?;
                let mut inner = scopes.clone();
                inner.push(HashMap::new());
                loops.push(LoopExit {
                    carried: carried.clone(),
                    jumps: Vec::new(),
                });
                let compiled = compile_statements(body, program, &mut inner, loops, depth + 1);
                let exits = loops.pop().ok_or(Error::InvalidBytecode)?;
                let returned = compiled?;
                if returned.is_none() {
                    let guard = emit_condition(program, condition, &inner, 0)?;
                    for &(level, name, dest) in &carried {
                        let source = inner
                            .get(level)
                            .and_then(|scope| scope.get(name))
                            .ok_or(Error::InvalidBytecode)?
                            .register;
                        emit_instruction(
                            program,
                            Instruction::LoopCopy {
                                dest,
                                input: source,
                            },
                        )?;
                    }
                    emit_instruction(
                        program,
                        Instruction::LoopBranchIfFalse {
                            condition: guard,
                            target: start,
                        },
                    )?;
                }
                program.code[start] = Instruction::LoopReset {
                    start: reset_start,
                    end: program.registers,
                };
                let after = program.code.len();
                for at in exits.jumps {
                    program.code[at] = Instruction::Jump { target: after };
                }
            }
            Statement::GenericFor {
                variables,
                iterators,
                body,
            } => {
                // Evaluate Lua's iterator/state/control triple once, before
                // introducing the loop's lexical variables.
                if variables.is_empty() || variables.len() > MAX_CALL_ARGUMENTS {
                    return Err(Error::TooLarge);
                }
                let triple = adjusted_rhs(program, iterators, 3, scopes)?;
                let iterator = triple[0];
                let state = triple[1];
                if program.registers >= MAX_REGISTERS {
                    return Err(Error::TooLarge);
                }
                let control = program.registers;
                program.registers += 1;
                emit_instruction(
                    program,
                    Instruction::LoopCopy {
                        dest: control,
                        input: triple[2],
                    },
                )?;
                let mut assigned = HashSet::new();
                assigned_names(body, &mut assigned);
                let mut candidates = scopes
                    .iter()
                    .enumerate()
                    .flat_map(|(level, scope)| {
                        scope
                            .iter()
                            .filter(|(name, binding)| !binding.constant && assigned.contains(*name))
                            .map(move |(name, _)| (level, *name))
                    })
                    .collect::<Vec<_>>();
                candidates.sort_unstable();
                let mut carried = Vec::new();
                for (level, name) in candidates {
                    if program.registers >= MAX_REGISTERS {
                        return Err(Error::TooLarge);
                    }
                    let source = scopes
                        .get(level)
                        .and_then(|scope| scope.get(name))
                        .ok_or(Error::InvalidBytecode)?
                        .register;
                    let dest = program.registers;
                    program.registers += 1;
                    emit_instruction(
                        program,
                        Instruction::LoopCopy {
                            dest,
                            input: source,
                        },
                    )?;
                    scopes
                        .get_mut(level)
                        .and_then(|scope| scope.get_mut(name))
                        .ok_or(Error::InvalidBytecode)?
                        .register = dest;
                    carried.push((level, name, dest));
                }
                let header = program.code.len();
                let reset_start = program.registers;
                emit_instruction(
                    program,
                    Instruction::LoopReset {
                        start: reset_start,
                        end: usize::MAX,
                    },
                )?;
                emit_instruction(program, Instruction::PushArgument { source: state })?;
                emit_instruction(program, Instruction::PushArgument { source: control })?;
                if program.registers >= MAX_REGISTERS {
                    return Err(Error::TooLarge);
                }
                let first = program.registers;
                program.registers += 1;
                emit_instruction(
                    program,
                    Instruction::Call {
                        dest: first,
                        callee: iterator,
                        argc: 2,
                        expand_tail: false,
                    },
                )?;
                let exit_at = program.code.len();
                emit_instruction(
                    program,
                    Instruction::BranchIfNil {
                        value: first,
                        target: usize::MAX,
                    },
                )?;
                emit_instruction(
                    program,
                    Instruction::LoopCopy {
                        dest: control,
                        input: first,
                    },
                )?;
                let mut loop_scope = HashMap::new();
                for (index, variable) in variables.iter().enumerate() {
                    let visible = if index == 0 {
                        first
                    } else {
                        if program.registers >= MAX_REGISTERS {
                            return Err(Error::TooLarge);
                        }
                        let dest = program.registers;
                        program.registers += 1;
                        emit_instruction(program, Instruction::ReadCallResult { dest, index })?;
                        dest
                    };
                    loop_scope.insert(
                        *variable,
                        RegisterBinding {
                            register: visible,
                            constant: true,
                        },
                    );
                }
                let mut inner = scopes.clone();
                inner.push(loop_scope);
                loops.push(LoopExit {
                    carried: carried.clone(),
                    jumps: Vec::new(),
                });
                let compiled = compile_statements(body, program, &mut inner, loops, depth + 1);
                let exits = loops.pop().ok_or(Error::InvalidBytecode)?;
                let returned = compiled?;
                if returned.is_none() {
                    for &(level, name, dest) in &carried {
                        let source = inner
                            .get(level)
                            .and_then(|scope| scope.get(name))
                            .ok_or(Error::InvalidBytecode)?
                            .register;
                        emit_instruction(
                            program,
                            Instruction::LoopCopy {
                                dest,
                                input: source,
                            },
                        )?;
                    }
                    emit_instruction(program, Instruction::LoopJump { target: header })?;
                }
                program.code[header] = Instruction::LoopReset {
                    start: reset_start,
                    end: program.registers,
                };
                let after = program.code.len();
                program.code[exit_at] = Instruction::BranchIfNil {
                    value: first,
                    target: after,
                };
                for at in exits.jumps {
                    program.code[at] = Instruction::Jump { target: after };
                }
            }
            Statement::NumericFor {
                variable,
                start,
                limit,
                step,
                body,
            } => {
                // Evaluate all controls before declaring the read-only
                // induction variable. Bounds remain fixed while body runs.
                let first = emit(program, start, 0, scopes)?;
                let bound = emit(program, limit, 0, scopes)?;
                let stride = if let Some(step) = step {
                    emit(program, step, 0, scopes)?
                } else {
                    if program.registers >= MAX_REGISTERS {
                        return Err(Error::TooLarge);
                    }
                    let reg = program.registers;
                    program.registers += 1;
                    emit_instruction(
                        program,
                        Instruction::Load {
                            dest: reg,
                            value: 1,
                        },
                    )?;
                    reg
                };
                if program.registers >= MAX_REGISTERS {
                    return Err(Error::TooLarge);
                }
                let counter = program.registers;
                program.registers += 1;
                emit_instruction(
                    program,
                    Instruction::LoopCopy {
                        dest: counter,
                        input: first,
                    },
                )?;

                let mut assigned = HashSet::new();
                assigned_names(body, &mut assigned);
                let mut candidates = scopes
                    .iter()
                    .enumerate()
                    .flat_map(|(level, scope)| {
                        scope
                            .iter()
                            .filter(|(name, binding)| !binding.constant && assigned.contains(*name))
                            .map(move |(name, _)| (level, *name))
                    })
                    .collect::<Vec<_>>();
                candidates.sort_unstable();
                let mut carried = Vec::new();
                for (level, name) in candidates {
                    if program.registers >= MAX_REGISTERS {
                        return Err(Error::TooLarge);
                    }
                    let source = scopes
                        .get(level)
                        .and_then(|scope| scope.get(name))
                        .ok_or(Error::InvalidBytecode)?
                        .register;
                    let dest = program.registers;
                    program.registers += 1;
                    emit_instruction(
                        program,
                        Instruction::LoopCopy {
                            dest,
                            input: source,
                        },
                    )?;
                    scopes
                        .get_mut(level)
                        .and_then(|scope| scope.get_mut(name))
                        .ok_or(Error::InvalidBytecode)?
                        .register = dest;
                    carried.push((level, name, dest));
                }

                let header = program.code.len();
                let reset_start = program.registers;
                emit_instruction(
                    program,
                    Instruction::LoopReset {
                        start: reset_start,
                        end: usize::MAX,
                    },
                )?;
                if program.registers >= MAX_REGISTERS {
                    return Err(Error::TooLarge);
                }
                let visible = program.registers;
                program.registers += 1;
                let check = program.code.len();
                emit_instruction(
                    program,
                    Instruction::NumericForCheck {
                        counter,
                        limit: bound,
                        step: stride,
                        visible,
                        exit: usize::MAX,
                    },
                )?;
                let mut inner = scopes.clone();
                let mut loop_scope = HashMap::new();
                loop_scope.insert(
                    *variable,
                    RegisterBinding {
                        register: visible,
                        constant: true,
                    },
                );
                inner.push(loop_scope);
                loops.push(LoopExit {
                    carried: carried.clone(),
                    jumps: Vec::new(),
                });
                let compiled = compile_statements(body, program, &mut inner, loops, depth + 1);
                let exits = loops.pop().ok_or(Error::InvalidBytecode)?;
                let returned = compiled?;
                let mut advance = None;
                if returned.is_none() {
                    for &(level, name, dest) in &carried {
                        let source = inner
                            .get(level)
                            .and_then(|scope| scope.get(name))
                            .ok_or(Error::InvalidBytecode)?
                            .register;
                        emit_instruction(
                            program,
                            Instruction::LoopCopy {
                                dest,
                                input: source,
                            },
                        )?;
                    }
                    advance = Some(program.code.len());
                    emit_instruction(
                        program,
                        Instruction::NumericForStep {
                            counter,
                            step: stride,
                            exit: usize::MAX,
                        },
                    )?;
                    emit_instruction(program, Instruction::LoopJump { target: header })?;
                }
                program.code[header] = Instruction::LoopReset {
                    start: reset_start,
                    end: program.registers,
                };
                let after = program.code.len();
                program.code[check] = Instruction::NumericForCheck {
                    counter,
                    limit: bound,
                    step: stride,
                    visible,
                    exit: after,
                };
                if let Some(at) = advance {
                    program.code[at] = Instruction::NumericForStep {
                        counter,
                        step: stride,
                        exit: after,
                    };
                }
                for at in exits.jumps {
                    program.code[at] = Instruction::Jump { target: after };
                }
            }
            Statement::Break => {
                let carried = loops
                    .last()
                    .ok_or_else(|| Error::Unsupported("break outside integer loop".into()))?
                    .carried
                    .clone();
                for (level, name, dest) in carried {
                    let source = scopes
                        .get(level)
                        .and_then(|scope| scope.get(name))
                        .ok_or(Error::InvalidBytecode)?
                        .register;
                    emit_instruction(
                        program,
                        Instruction::LoopCopy {
                            dest,
                            input: source,
                        },
                    )?;
                }
                let at = program.code.len();
                emit_instruction(program, Instruction::Jump { target: usize::MAX })?;
                loops
                    .last_mut()
                    .ok_or(Error::InvalidBytecode)?
                    .jumps
                    .push(at);
                // Break terminates this control path, like Return, but the
                // surrounding loop itself still falls through after exit.
                result = Some(0);
                break;
            }
            Statement::Call(expression) => {
                // Lua discards every result of a call used as a statement.
                // The call still executes, including argument evaluation
                // and any runtime errors, under the shared step budget.
                let _ = emit(program, expression, 0, scopes)?;
            }
            Statement::Return(values) => {
                // Every expression is scalar except an unparenthesized
                // final call, which expands its complete result vector.
                let mut first = None;
                for (position, expression) in values.iter().enumerate() {
                    let register = emit(program, expression, 0, scopes)?;
                    if position + 1 == values.len() && matches!(expression, Expr::Call { .. }) {
                        emit_instruction(program, Instruction::AppendReturnExpansion)?;
                    } else {
                        emit_instruction(program, Instruction::AppendReturn { source: register })?;
                    }
                    if first.is_none() {
                        first = Some(register);
                    }
                }
                let value = if let Some(first) = first {
                    first
                } else {
                    // This scalar register interface adjusts zero Lua
                    // results to nil. Exact multiple-result transport is a
                    // separate, still-unaccepted register VM capability.
                    if program.registers >= MAX_REGISTERS {
                        return Err(Error::TooLarge);
                    }
                    let register = program.registers;
                    program.registers += 1;
                    emit_instruction(program, Instruction::LoadNil { dest: register })?;
                    register
                };
                emit_instruction(program, Instruction::Return { source: value })?;
                result = Some(value);
            }
            _ => {
                return Err(Error::Unsupported(
                    "only integer locals, assignments, do/end blocks, and final return".into(),
                ));
            }
        }
    }
    Ok(result)
}

// Lua integer floor division rounds toward minus infinity, unlike Rust /.
pub(crate) fn floor_div(a: i64, b: i64) -> Result<i64, Error> {
    if b == 0 {
        return Err(Error::DivisionByZero);
    }
    if b == -1 {
        return Ok(a.wrapping_neg());
    }
    let q = a / b;
    let r = a % b;
    if r != 0 && ((r ^ b) < 0) {
        Ok(q.wrapping_sub(1))
    } else {
        Ok(q)
    }
}

pub(crate) fn lua_mod(a: i64, b: i64) -> Result<i64, Error> {
    if b == 0 {
        return Err(Error::DivisionByZero);
    }
    if b == -1 {
        return Ok(0);
    }
    let r = a % b;
    if r != 0 && ((r ^ b) < 0) {
        Ok(r.wrapping_add(b))
    } else {
        Ok(r)
    }
}

// Lua shifts operate on an unsigned bit pattern and negative shift counts
// reverse direction. A count >= 64 clears all bits.
pub(crate) fn shift(value: i64, distance: i64, left: bool) -> i64 {
    let count = distance.unsigned_abs();
    if count >= i64::BITS as u64 {
        return 0;
    }
    if left != (distance < 0) {
        (value as u64).wrapping_shl(count as u32) as i64
    } else {
        ((value as u64) >> count) as i64
    }
}

/// Validate every instruction before executing the bounded register VM.
/// An untaken branch or early return must not conceal malformed bytecode.
fn validate_integer_program(program: &Program) -> Result<(), Error> {
    let size = program.code.len();
    if program.strings.iter().map(Vec::len).sum::<usize>() > MAX_REGISTER_STRING_BYTES {
        return Err(Error::InvalidBytecode);
    }
    if program.registers == 0
        || program.registers > MAX_REGISTERS
        || size == 0
        || size > MAX_REGISTERS
        || program.result >= program.registers
    {
        return Err(Error::InvalidBytecode);
    }
    let register = |index: usize| index < program.registers;
    for (pc, instruction) in program.code.iter().enumerate() {
        let valid = match *instruction {
            Instruction::LoadString { dest, index } => {
                register(dest) && index < program.strings.len()
            }
            Instruction::LoadBuiltin { dest, .. } | Instruction::NewTable { dest } => {
                register(dest)
            }
            Instruction::GetTable { dest, table, key } => {
                register(dest) && register(table) && register(key)
            }
            Instruction::SetTable { table, key, value } => {
                register(table) && register(key) && register(value)
            }
            Instruction::ExpandTableResults { table, first } => register(table) && first >= 1,
            Instruction::LoadGlobal { dest, name } => {
                register(dest) && name < program.strings.len()
            }
            Instruction::StoreGlobal { name, source } => {
                name < program.strings.len() && register(source)
            }
            Instruction::PushArgument { source } => register(source),
            Instruction::ReadCallResult { dest, .. } => register(dest),
            Instruction::AppendReturnExpansion => true,
            Instruction::Call {
                dest, callee, argc, ..
            } => register(dest) && register(callee) && argc <= MAX_CALL_ARGUMENTS,
            Instruction::Load { dest, .. }
            | Instruction::LoadBoolean { dest, .. }
            | Instruction::LoadFloat { dest, .. }
            | Instruction::LoadNil { dest } => register(dest),
            Instruction::Neg { dest, input }
            | Instruction::Length { dest, input }
            | Instruction::LoopCopy { dest, input }
            | Instruction::BitNot { dest, input }
            | Instruction::NumericTruthy { dest, input }
            | Instruction::BooleanNot { dest, input }
            | Instruction::BooleanCopy { dest, input }
            | Instruction::MergeCopy { dest, input } => register(dest) && register(input),
            Instruction::AppendReturn { source } => register(source),
            Instruction::Add { dest, lhs, rhs }
            | Instruction::Sub { dest, lhs, rhs }
            | Instruction::Mul { dest, lhs, rhs }
            | Instruction::Div { dest, lhs, rhs }
            | Instruction::Pow { dest, lhs, rhs }
            | Instruction::FloorDiv { dest, lhs, rhs }
            | Instruction::Mod { dest, lhs, rhs }
            | Instruction::BitAnd { dest, lhs, rhs }
            | Instruction::BitOr { dest, lhs, rhs }
            | Instruction::BitXor { dest, lhs, rhs }
            | Instruction::Shl { dest, lhs, rhs }
            | Instruction::Shr { dest, lhs, rhs }
            | Instruction::Compare { dest, lhs, rhs, .. }
            | Instruction::Concat { dest, lhs, rhs } => {
                register(dest) && register(lhs) && register(rhs)
            }
            Instruction::BranchIfFalse { condition, target } => {
                register(condition) && target > pc && target < size
            }
            Instruction::BranchIfNil { value, target } => {
                register(value) && target > pc && target < size
            }
            Instruction::Jump { target } => target > pc && target < size,
            Instruction::LoopReset { start, end } => start < end && end <= program.registers,
            Instruction::NumericForCheck {
                counter,
                limit,
                step,
                visible,
                exit,
            } => {
                register(counter)
                    && register(limit)
                    && register(step)
                    && register(visible)
                    && exit > pc
                    && exit < size
            }
            Instruction::NumericForStep {
                counter,
                step,
                exit,
            } => register(counter) && register(step) && exit > pc && exit < size,
            Instruction::LoopJump { target }
            | Instruction::LoopBranchIfFalse {
                target,
                condition: _,
            } => {
                target < pc
                    && matches!(
                        program.code.get(target),
                        Some(Instruction::LoopReset { .. })
                    )
                    && match *instruction {
                        Instruction::LoopBranchIfFalse { condition, .. } => register(condition),
                        _ => true,
                    }
            }
            Instruction::Return { source } => register(source),
        };
        if !valid {
            return Err(Error::InvalidBytecode);
        }
    }
    Ok(())
}

/// Run the tiny integer register IR with initialized-register validation.
/// Statically reject malformed instructions, including unreachable ones.
pub fn execute_integer_program(program: &Program) -> Result<i64, Error> {
    execute_integer_program_with_budget(program, MAX_REGISTER_STEPS)
}

/// Run the experimental register dispatcher with an explicit instruction
/// budget. Callers can lower but never increase the hard process-safe cap.
/// Validation always precedes budget execution, including when budget is 0.
pub fn execute_integer_program_with_budget(program: &Program, budget: usize) -> Result<i64, Error> {
    execute_register_program_with_budget(program, budget)?.integer()
}

/// Execute the experimental register IR with Lua scalar value tags.
/// Literal string results are resolved with Program::string_bytes. Dynamic
/// strings, functions, tables and full register compilation remain unaccepted.
pub fn execute_register_program(program: &Program) -> Result<RegisterValue, Error> {
    execute_register_program_with_budget(program, MAX_REGISTER_STEPS)
}

/// All register variants use the same bounded execution and validation path.
pub fn execute_register_program_with_budget(
    program: &Program,
    budget: usize,
) -> Result<RegisterValue, Error> {
    let result = execute_register_program_with_strings_and_budget(program, budget)?;
    if matches!(result.value, RegisterValue::String(index) if index >= program.strings.len()) {
        return Err(Error::Unsupported(
            "dynamic string result requires execute_register_program_with_strings".into(),
        ));
    }
    Ok(result.value)
}

/// Execute the typed register VM and retain any dynamically produced byte
/// strings. The returned string handle must be decoded through this value.
pub fn execute_register_program_with_strings(
    program: &Program,
) -> Result<RegisterExecution, Error> {
    execute_register_program_with_strings_and_budget(program, MAX_REGISTER_STEPS)
}

/// The combined literal and dynamic string pool is bounded by the same
/// static byte limit; every instruction is validated before execution.
pub fn execute_register_program_with_strings_and_budget(
    program: &Program,
    budget: usize,
) -> Result<RegisterExecution, Error> {
    validate_integer_program(program)?;
    let budget = budget.min(MAX_REGISTER_STEPS);
    let mut strings = program.strings.clone();
    let mut string_bytes_used: usize = strings.iter().map(Vec::len).sum();
    let mut return_values = Vec::new();
    let mut last_call_results = Vec::new();
    let mut call_arguments = Vec::new();
    let mut globals: HashMap<usize, RegisterValue> = HashMap::new();
    let mut tables: Vec<RegisterTable> = Vec::new();
    let mut table_entries_used: usize = 0;
    let mut registers: Vec<Option<RegisterValue>> = vec![None; program.registers];
    fn read(registers: &[Option<RegisterValue>], at: usize) -> Result<RegisterValue, Error> {
        registers
            .get(at)
            .copied()
            .flatten()
            .ok_or(Error::InvalidBytecode)
    }
    fn read_bitwise_integer(registers: &[Option<RegisterValue>], at: usize) -> Result<i64, Error> {
        read(registers, at)?.bitwise_integer()
    }
    let mut pc = 0;
    let mut steps = 0usize;
    while pc < program.code.len() {
        if steps >= budget {
            return Err(Error::StepBudgetExceeded);
        }
        steps += 1;
        let (dest, value) = match program.code[pc] {
            Instruction::BranchIfFalse { condition, target } => {
                if target <= pc || target >= program.code.len() {
                    return Err(Error::InvalidBytecode);
                }
                pc = if !read(&registers, condition)?.truthy() {
                    target
                } else {
                    pc + 1
                };
                continue;
            }
            Instruction::BranchIfNil { value, target } => {
                if target <= pc || target >= program.code.len() {
                    return Err(Error::InvalidBytecode);
                }
                pc = if read(&registers, value)? == RegisterValue::Nil {
                    target
                } else {
                    pc + 1
                };
                continue;
            }
            Instruction::Return { source } => {
                return Ok(RegisterExecution {
                    value: read(&registers, source)?,
                    values: return_values,
                    strings,
                    tables,
                });
            }
            Instruction::AppendReturn { source } => {
                if return_values.len() >= MAX_REGISTERS {
                    return Err(Error::TooLarge);
                }
                return_values.push(read(&registers, source)?);
                pc += 1;
                continue;
            }
            Instruction::AppendReturnExpansion => {
                if return_values.len().saturating_add(last_call_results.len()) > MAX_REGISTERS {
                    return Err(Error::TooLarge);
                }
                return_values.extend_from_slice(&last_call_results);
                pc += 1;
                continue;
            }
            Instruction::PushArgument { source } => {
                if call_arguments.len() >= MAX_CALL_ARGUMENTS * 2 {
                    return Err(Error::TooLarge);
                }
                call_arguments.push(read(&registers, source)?);
                pc += 1;
                continue;
            }
            Instruction::StoreGlobal { name, source } => {
                globals.insert(name, read(&registers, source)?);
                pc += 1;
                continue;
            }
            Instruction::NumericForCheck {
                counter,
                limit,
                step,
                visible,
                exit,
            } => {
                let current = read(&registers, counter)?;
                let bound = read(&registers, limit)?;
                let stride = read(&registers, step)?;
                let (finished, exposed) = match (current, bound, stride) {
                    (
                        RegisterValue::Integer(value),
                        RegisterValue::Integer(bound),
                        RegisterValue::Integer(step),
                    ) => {
                        if step == 0 {
                            return Err(Error::ZeroForStep);
                        }
                        (
                            (step > 0 && value > bound) || (step < 0 && value < bound),
                            RegisterValue::Integer(value),
                        )
                    }
                    (value, bound, step) => {
                        let value = value.number()?;
                        let bound = bound.number()?;
                        let step = step.number()?;
                        if step == 0.0 {
                            return Err(Error::ZeroForStep);
                        }
                        // Materialize floating induction in the private
                        // counter; limit/step registers may alias outer locals
                        // and must retain their original Lua numeric subtype.
                        let float = RegisterValue::Float(value.to_bits());
                        *registers.get_mut(counter).ok_or(Error::InvalidBytecode)? = Some(float);
                        (
                            (step > 0.0 && value > bound) || (step < 0.0 && value < bound),
                            float,
                        )
                    }
                };
                if finished {
                    pc = exit;
                } else {
                    let slot = registers.get_mut(visible).ok_or(Error::InvalidBytecode)?;
                    if slot.replace(exposed).is_some() {
                        return Err(Error::InvalidBytecode);
                    }
                    pc += 1;
                }
                continue;
            }
            Instruction::NumericForStep {
                counter,
                step,
                exit,
            } => {
                let value = read(&registers, counter)?;
                let stride = read(&registers, step)?;
                match (value, stride) {
                    (RegisterValue::Integer(value), RegisterValue::Integer(stride)) => {
                        if let Some(next) = value.checked_add(stride) {
                            *registers.get_mut(counter).ok_or(Error::InvalidBytecode)? =
                                Some(RegisterValue::Integer(next));
                            pc += 1;
                        } else {
                            pc = exit;
                        }
                    }
                    (value, stride) => {
                        let next = value.number()? + stride.number()?;
                        *registers.get_mut(counter).ok_or(Error::InvalidBytecode)? =
                            Some(RegisterValue::Float(next.to_bits()));
                        pc += 1;
                    }
                }
                continue;
            }
            Instruction::LoopReset { start, end } => {
                for value in registers
                    .get_mut(start..end)
                    .ok_or(Error::InvalidBytecode)?
                {
                    *value = None;
                }
                pc += 1;
                continue;
            }
            Instruction::LoopJump { target }
            | Instruction::LoopBranchIfFalse {
                condition: _,
                target,
            } => {
                if target >= pc
                    || !matches!(
                        program.code.get(target),
                        Some(Instruction::LoopReset { .. })
                    )
                {
                    return Err(Error::InvalidBytecode);
                }
                let jump = match program.code[pc] {
                    Instruction::LoopBranchIfFalse { condition, .. } => {
                        !read(&registers, condition)?.truthy()
                    }
                    _ => true,
                };
                pc = if jump { target } else { pc + 1 };
                continue;
            }
            Instruction::LoopCopy { dest, input } => {
                let value = read(&registers, input)?;
                *registers.get_mut(dest).ok_or(Error::InvalidBytecode)? = Some(value);
                pc += 1;
                continue;
            }
            Instruction::Jump { target } => {
                if target <= pc || target >= program.code.len() {
                    return Err(Error::InvalidBytecode);
                }
                pc = target;
                continue;
            }
            Instruction::BooleanCopy { dest, input } | Instruction::MergeCopy { dest, input } => {
                (dest, read(&registers, input)?)
            }
            Instruction::NumericTruthy { dest, input } => (
                dest,
                RegisterValue::Boolean(read(&registers, input)?.truthy()),
            ),
            Instruction::BooleanNot { dest, input } => (
                dest,
                RegisterValue::Boolean(!read(&registers, input)?.truthy()),
            ),
            Instruction::Compare {
                dest,
                lhs,
                rhs,
                operation,
            } => {
                let left = read(&registers, lhs)?;
                let right = read(&registers, rhs)?;
                let both_numbers =
                    matches!(left, RegisterValue::Integer(_) | RegisterValue::Float(_))
                        && matches!(right, RegisterValue::Integer(_) | RegisterValue::Float(_));
                let flag = if matches!(
                    operation,
                    CompareOperation::Equal | CompareOperation::NotEqual
                ) {
                    let equal = if both_numbers {
                        numeric_comparison(left, right)? == Some(Ordering::Equal)
                    } else if let (RegisterValue::String(a), RegisterValue::String(b)) =
                        (left, right)
                    {
                        strings.get(a).ok_or(Error::InvalidBytecode)?
                            == strings.get(b).ok_or(Error::InvalidBytecode)?
                    } else {
                        left == right
                    };
                    if operation == CompareOperation::Equal {
                        equal
                    } else {
                        !equal
                    }
                } else {
                    let ordering = match (left, right) {
                        (RegisterValue::String(a), RegisterValue::String(b)) => Some(
                            strings
                                .get(a)
                                .ok_or(Error::InvalidBytecode)?
                                .cmp(strings.get(b).ok_or(Error::InvalidBytecode)?),
                        ),
                        _ => numeric_comparison(left, right)?,
                    };
                    match operation {
                        CompareOperation::Less => ordering == Some(Ordering::Less),
                        CompareOperation::LessEqual => {
                            matches!(ordering, Some(Ordering::Less | Ordering::Equal))
                        }
                        CompareOperation::Greater => ordering == Some(Ordering::Greater),
                        CompareOperation::GreaterEqual => {
                            matches!(ordering, Some(Ordering::Greater | Ordering::Equal))
                        }
                        _ => unreachable!("equality handled above"),
                    }
                };
                (dest, RegisterValue::Boolean(flag))
            }
            Instruction::Load { dest, value } => (dest, RegisterValue::Integer(value)),
            Instruction::LoadNil { dest } => (dest, RegisterValue::Nil),
            Instruction::LoadBoolean { dest, value } => (dest, RegisterValue::Boolean(value)),
            Instruction::LoadFloat { dest, bits } => (dest, RegisterValue::Float(bits)),
            Instruction::LoadString { dest, index } => (dest, RegisterValue::String(index)),
            Instruction::LoadBuiltin { dest, builtin } => (dest, RegisterValue::Builtin(builtin)),
            Instruction::NewTable { dest } => {
                if tables.len() >= MAX_REGISTER_TABLES {
                    return Err(Error::TooLarge);
                }
                let index = tables.len();
                tables.push(RegisterTable::default());
                (dest, RegisterValue::Table(index))
            }
            Instruction::GetTable { dest, table, key } => {
                let RegisterValue::Table(index) = read(&registers, table)? else {
                    return Err(Error::TypeError);
                };
                let key = read(&registers, key)?;
                // Unlike insertion, reading an absent nil or NaN index
                // returns nil without creating a table slot.
                let value = if matches!(key, RegisterValue::Nil)
                    || matches!(key, RegisterValue::Float(bits) if f64::from_bits(bits).is_nan())
                {
                    RegisterValue::Nil
                } else {
                    let key = register_table_key(key, &strings)?;
                    let entries = &tables.get(index).ok_or(Error::InvalidBytecode)?.entries;
                    entries
                        .iter()
                        .find(|(candidate, _)| *candidate == key)
                        .map_or(RegisterValue::Nil, |(_, value)| *value)
                };
                (dest, value)
            }
            Instruction::SetTable { table, key, value } => {
                let RegisterValue::Table(index) = read(&registers, table)? else {
                    return Err(Error::TypeError);
                };
                let key = register_table_key(read(&registers, key)?, &strings)?;
                let value = read(&registers, value)?;
                let entries = &mut tables.get_mut(index).ok_or(Error::InvalidBytecode)?.entries;
                if let Some(position) = entries.iter().position(|(candidate, _)| *candidate == key)
                {
                    if value == RegisterValue::Nil {
                        entries.swap_remove(position);
                        table_entries_used -= 1;
                    } else {
                        entries[position].1 = value;
                    }
                } else if value != RegisterValue::Nil {
                    if table_entries_used >= MAX_REGISTER_TABLE_ENTRIES {
                        return Err(Error::TooLarge);
                    }
                    entries.push((key, value));
                    table_entries_used += 1;
                }
                pc += 1;
                continue;
            }
            Instruction::ExpandTableResults { table, first } => {
                let RegisterValue::Table(index) = read(&registers, table)? else {
                    return Err(Error::TypeError);
                };
                let entries = &mut tables.get_mut(index).ok_or(Error::InvalidBytecode)?.entries;
                for (offset, &value) in last_call_results.iter().enumerate() {
                    let number = first
                        .checked_add(i64::try_from(offset).map_err(|_| Error::TooLarge)?)
                        .ok_or(Error::TooLarge)?;
                    let key = TableKey::Integer(number);
                    if let Some(position) =
                        entries.iter().position(|(candidate, _)| *candidate == key)
                    {
                        if value == RegisterValue::Nil {
                            entries.swap_remove(position);
                            table_entries_used -= 1;
                        } else {
                            entries[position].1 = value;
                        }
                    } else if value != RegisterValue::Nil {
                        if table_entries_used >= MAX_REGISTER_TABLE_ENTRIES {
                            return Err(Error::TooLarge);
                        }
                        entries.push((key, value));
                        table_entries_used += 1;
                    }
                }
                pc += 1;
                continue;
            }
            Instruction::LoadGlobal { dest, name } => {
                let value = if let Some(value) = globals.get(&name) {
                    *value
                } else {
                    match program
                        .strings
                        .get(name)
                        .ok_or(Error::InvalidBytecode)?
                        .as_slice()
                    {
                        b"type" => RegisterValue::Builtin(RegisterBuiltin::Type),
                        b"tostring" => RegisterValue::Builtin(RegisterBuiltin::ToString),
                        b"select" => RegisterValue::Builtin(RegisterBuiltin::Select),
                        b"rawget" => RegisterValue::Builtin(RegisterBuiltin::RawGet),
                        b"rawset" => RegisterValue::Builtin(RegisterBuiltin::RawSet),
                        b"rawlen" => RegisterValue::Builtin(RegisterBuiltin::RawLen),
                        b"rawequal" => RegisterValue::Builtin(RegisterBuiltin::RawEqual),
                        b"assert" => RegisterValue::Builtin(RegisterBuiltin::Assert),
                        b"tonumber" => RegisterValue::Builtin(RegisterBuiltin::ToNumber),
                        b"next" => RegisterValue::Builtin(RegisterBuiltin::Next),
                        b"pairs" => RegisterValue::Builtin(RegisterBuiltin::Pairs),
                        b"ipairs" => RegisterValue::Builtin(RegisterBuiltin::Ipairs),
                        _ => RegisterValue::Nil,
                    }
                };
                (dest, value)
            }
            Instruction::Call {
                dest,
                callee,
                argc,
                expand_tail,
            } => {
                let function = read(&registers, callee)?;
                let start = call_arguments
                    .len()
                    .checked_sub(argc)
                    .ok_or(Error::InvalidBytecode)?;
                let mut evaluated = call_arguments[start..].to_vec();
                if expand_tail {
                    if evaluated.len().saturating_add(last_call_results.len()) > MAX_CALL_ARGUMENTS
                    {
                        return Err(Error::TooLarge);
                    }
                    evaluated.extend_from_slice(&last_call_results);
                }
                let arguments = evaluated.as_slice();
                let results = match function {
                    RegisterValue::Builtin(RegisterBuiltin::Type) => {
                        let first = arguments.first().ok_or(Error::TypeError)?;
                        let bytes = match first {
                            RegisterValue::Nil => b"nil".as_slice(),
                            RegisterValue::Boolean(_) => b"boolean",
                            RegisterValue::Integer(_) | RegisterValue::Float(_) => b"number",
                            RegisterValue::String(_) => b"string",
                            RegisterValue::Builtin(_) => b"function",
                            RegisterValue::Table(_) => b"table",
                        };
                        vec![intern_register_string(
                            bytes.to_vec(),
                            &mut strings,
                            &mut string_bytes_used,
                        )?]
                    }
                    RegisterValue::Builtin(RegisterBuiltin::ToString) => {
                        let first = arguments.first().ok_or(Error::TypeError)?;
                        let bytes = match first {
                            RegisterValue::Nil => b"nil".to_vec(),
                            RegisterValue::Boolean(value) => value.to_string().into_bytes(),
                            RegisterValue::Integer(value) => value.to_string().into_bytes(),
                            RegisterValue::Float(bits) => {
                                let value = f64::from_bits(*bits);
                                if value.is_subnormal() {
                                    format!("{value:.13e}").into_bytes()
                                } else {
                                    format!("{value:?}").into_bytes()
                                }
                            }
                            RegisterValue::String(index) => {
                                strings.get(*index).cloned().ok_or(Error::InvalidBytecode)?
                            }
                            RegisterValue::Builtin(_) => b"function: builtin".to_vec(),
                            RegisterValue::Table(index) => {
                                format!("table: 0x{index:x}").into_bytes()
                            }
                        };
                        vec![intern_register_string(
                            bytes,
                            &mut strings,
                            &mut string_bytes_used,
                        )?]
                    }
                    RegisterValue::Builtin(RegisterBuiltin::Select) => {
                        let selector = arguments.first().ok_or(Error::TypeError)?;
                        let available = arguments.len() - 1;
                        match selector {
                            RegisterValue::String(index)
                                if strings.get(*index).is_some_and(|bytes| bytes == b"#") =>
                            {
                                vec![RegisterValue::Integer(available as i64)]
                            }
                            _ => {
                                let position = register_select_integer(*selector, &strings)?;
                                if position == 0 {
                                    return Err(Error::TypeError);
                                }
                                let offset = if position > 0 {
                                    i128::from(position) - 1
                                } else {
                                    available as i128 + i128::from(position)
                                };
                                if offset < 0 {
                                    return Err(Error::TypeError);
                                }
                                let offset =
                                    usize::try_from(offset).map_err(|_| Error::TooLarge)?;
                                arguments[1 + offset.min(available)..].to_vec()
                            }
                        }
                    }
                    RegisterValue::Builtin(RegisterBuiltin::ToNumber) => {
                        let input = *arguments.first().ok_or(Error::TypeError)?;
                        let base = arguments.get(1).copied().unwrap_or(RegisterValue::Nil);
                        vec![register_tonumber(input, base, &strings)?]
                    }
                    RegisterValue::Builtin(RegisterBuiltin::Assert) => {
                        let condition = arguments.first().ok_or(Error::TypeError)?;
                        if !condition.truthy() {
                            return Err(Error::TypeError);
                        }
                        arguments.to_vec()
                    }
                    RegisterValue::Builtin(RegisterBuiltin::RawGet) => {
                        let Some(RegisterValue::Table(index)) = arguments.first() else {
                            return Err(Error::TypeError);
                        };
                        let key = *arguments.get(1).ok_or(Error::TypeError)?;
                        // Reading a nil or NaN key is an absent-key read.
                        let value = if matches!(key, RegisterValue::Nil)
                            || matches!(key, RegisterValue::Float(bits) if f64::from_bits(bits).is_nan())
                        {
                            RegisterValue::Nil
                        } else {
                            let key = register_table_key(key, &strings)?;
                            tables
                                .get(*index)
                                .ok_or(Error::InvalidBytecode)?
                                .entries
                                .iter()
                                .find(|(candidate, _)| *candidate == key)
                                .map_or(RegisterValue::Nil, |(_, value)| *value)
                        };
                        vec![value]
                    }
                    RegisterValue::Builtin(RegisterBuiltin::RawLen) => {
                        let value = *arguments.first().ok_or(Error::TypeError)?;
                        let length = register_raw_length(value, &strings, &tables)?;
                        vec![RegisterValue::Integer(length as i64)]
                    }
                    RegisterValue::Builtin(RegisterBuiltin::RawEqual) => {
                        let left = *arguments.first().ok_or(Error::TypeError)?;
                        let right = *arguments.get(1).ok_or(Error::TypeError)?;
                        let both_numbers =
                            matches!(left, RegisterValue::Integer(_) | RegisterValue::Float(_))
                                && matches!(
                                    right,
                                    RegisterValue::Integer(_) | RegisterValue::Float(_)
                                );
                        let equal = if both_numbers {
                            numeric_comparison(left, right)? == Some(Ordering::Equal)
                        } else if let (RegisterValue::String(a), RegisterValue::String(b)) =
                            (left, right)
                        {
                            strings.get(a).ok_or(Error::InvalidBytecode)?
                                == strings.get(b).ok_or(Error::InvalidBytecode)?
                        } else {
                            left == right
                        };
                        vec![RegisterValue::Boolean(equal)]
                    }
                    RegisterValue::Builtin(RegisterBuiltin::Next) => {
                        let Some(RegisterValue::Table(index)) = arguments.first() else {
                            return Err(Error::TypeError);
                        };
                        let table = tables.get(*index).ok_or(Error::InvalidBytecode)?;
                        let previous = arguments.get(1).copied().unwrap_or(RegisterValue::Nil);
                        let following = if previous == RegisterValue::Nil {
                            table.entries.first()
                        } else {
                            let key = register_table_key(previous, &strings)?;
                            let offset = table
                                .entries
                                .iter()
                                .position(|(candidate, _)| *candidate == key)
                                .ok_or(Error::TypeError)?;
                            table.entries.get(offset + 1)
                        };
                        if let Some((key, value)) = following {
                            let key = register_table_key_value(key);
                            vec![key, *value]
                        } else {
                            vec![RegisterValue::Nil]
                        }
                    }
                    RegisterValue::Builtin(RegisterBuiltin::Pairs) => {
                        let Some(RegisterValue::Table(_)) = arguments.first() else {
                            return Err(Error::TypeError);
                        };
                        vec![
                            RegisterValue::Builtin(RegisterBuiltin::Next),
                            arguments[0],
                            RegisterValue::Nil,
                        ]
                    }
                    RegisterValue::Builtin(RegisterBuiltin::Ipairs) => {
                        let Some(RegisterValue::Table(_)) = arguments.first() else {
                            return Err(Error::TypeError);
                        };
                        vec![
                            RegisterValue::Builtin(RegisterBuiltin::IpairsIterator),
                            arguments[0],
                            RegisterValue::Integer(0),
                        ]
                    }
                    RegisterValue::Builtin(RegisterBuiltin::IpairsIterator) => {
                        let Some(RegisterValue::Table(index)) = arguments.first() else {
                            return Err(Error::TypeError);
                        };
                        let previous = *arguments.get(1).ok_or(Error::TypeError)?;
                        let counter = register_select_integer(previous, &strings)?;
                        let next_index = counter.checked_add(1).ok_or(Error::TooLarge)?;
                        let table = tables.get(*index).ok_or(Error::InvalidBytecode)?;
                        let value = table
                            .entries
                            .iter()
                            .find(|(key, _)| *key == TableKey::Integer(next_index))
                            .map_or(RegisterValue::Nil, |(_, value)| *value);
                        if value == RegisterValue::Nil {
                            vec![RegisterValue::Nil]
                        } else {
                            vec![RegisterValue::Integer(next_index), value]
                        }
                    }
                    RegisterValue::Builtin(RegisterBuiltin::RawSet) => {
                        let Some(RegisterValue::Table(index)) = arguments.first() else {
                            return Err(Error::TypeError);
                        };
                        let key = register_table_key(
                            *arguments.get(1).ok_or(Error::TypeError)?,
                            &strings,
                        )?;
                        let value = *arguments.get(2).ok_or(Error::TypeError)?;
                        let entries = &mut tables
                            .get_mut(*index)
                            .ok_or(Error::InvalidBytecode)?
                            .entries;
                        if let Some(position) =
                            entries.iter().position(|(candidate, _)| *candidate == key)
                        {
                            if value == RegisterValue::Nil {
                                entries.swap_remove(position);
                                table_entries_used -= 1;
                            } else {
                                entries[position].1 = value;
                            }
                        } else if value != RegisterValue::Nil {
                            if table_entries_used >= MAX_REGISTER_TABLE_ENTRIES {
                                return Err(Error::TooLarge);
                            }
                            entries.push((key, value));
                            table_entries_used += 1;
                        }
                        vec![RegisterValue::Table(*index)]
                    }
                    _ => return Err(Error::TypeError),
                };
                last_call_results = results;
                let output = last_call_results
                    .first()
                    .copied()
                    .unwrap_or(RegisterValue::Nil);
                call_arguments.truncate(start);
                (dest, output)
            }
            Instruction::ReadCallResult { dest, index } => (
                dest,
                last_call_results
                    .get(index)
                    .copied()
                    .unwrap_or(RegisterValue::Nil),
            ),
            Instruction::Length { dest, input } => {
                let length = register_raw_length(read(&registers, input)?, &strings, &tables)?;
                (dest, RegisterValue::Integer(length as i64))
            }
            Instruction::Concat { dest, lhs, rhs } => {
                let left = concat_operand_bytes(read(&registers, lhs)?, &strings)?;
                let right = concat_operand_bytes(read(&registers, rhs)?, &strings)?;
                let length = left.len().checked_add(right.len()).ok_or(Error::TooLarge)?;
                if length > MAX_REGISTER_STRING_BYTES {
                    return Err(Error::TooLarge);
                }
                let mut bytes = left;
                bytes.extend_from_slice(&right);
                let index = if let Some(index) = strings.iter().position(|item| item == &bytes) {
                    index
                } else {
                    if strings.len() >= MAX_REGISTERS
                        || string_bytes_used.saturating_add(length) > MAX_REGISTER_STRING_BYTES
                    {
                        return Err(Error::TooLarge);
                    }
                    string_bytes_used += length;
                    let index = strings.len();
                    strings.push(bytes);
                    index
                };
                (dest, RegisterValue::String(index))
            }
            Instruction::Neg { dest, input } => {
                let value = read(&registers, input)?;
                (
                    dest,
                    match value {
                        RegisterValue::Integer(value) => {
                            RegisterValue::Integer(value.wrapping_neg())
                        }
                        RegisterValue::Float(bits) => RegisterValue::Float(bits ^ (1_u64 << 63)),
                        _ => return Err(Error::TypeError),
                    },
                )
            }
            Instruction::Add { dest, lhs, rhs } => {
                let left = read(&registers, lhs)?;
                let right = read(&registers, rhs)?;
                let result = match (left, right) {
                    (RegisterValue::Integer(a), RegisterValue::Integer(b)) => {
                        RegisterValue::Integer(a.wrapping_add(b))
                    }
                    (a, b) => RegisterValue::Float((a.number()? + b.number()?).to_bits()),
                };
                (dest, result)
            }
            Instruction::Sub { dest, lhs, rhs } => {
                let left = read(&registers, lhs)?;
                let right = read(&registers, rhs)?;
                let result = match (left, right) {
                    (RegisterValue::Integer(a), RegisterValue::Integer(b)) => {
                        RegisterValue::Integer(a.wrapping_sub(b))
                    }
                    (a, b) => RegisterValue::Float((a.number()? - b.number()?).to_bits()),
                };
                (dest, result)
            }
            Instruction::Mul { dest, lhs, rhs } => {
                let left = read(&registers, lhs)?;
                let right = read(&registers, rhs)?;
                let result = match (left, right) {
                    (RegisterValue::Integer(a), RegisterValue::Integer(b)) => {
                        RegisterValue::Integer(a.wrapping_mul(b))
                    }
                    (a, b) => RegisterValue::Float((a.number()? * b.number()?).to_bits()),
                };
                (dest, result)
            }
            Instruction::Div { dest, lhs, rhs } => (
                dest,
                RegisterValue::Float(
                    (read(&registers, lhs)?.number()? / read(&registers, rhs)?.number()?).to_bits(),
                ),
            ),
            Instruction::Pow { dest, lhs, rhs } => (
                dest,
                RegisterValue::Float(
                    read(&registers, lhs)?
                        .number()?
                        .powf(read(&registers, rhs)?.number()?)
                        .to_bits(),
                ),
            ),
            Instruction::FloorDiv { dest, lhs, rhs } => {
                let left = read(&registers, lhs)?;
                let right = read(&registers, rhs)?;
                let result = match (left, right) {
                    (RegisterValue::Integer(a), RegisterValue::Integer(b)) => {
                        RegisterValue::Integer(floor_div(a, b)?)
                    }
                    (a, b) => RegisterValue::Float((a.number()? / b.number()?).floor().to_bits()),
                };
                (dest, result)
            }
            Instruction::Mod { dest, lhs, rhs } => {
                let left = read(&registers, lhs)?;
                let right = read(&registers, rhs)?;
                let result = match (left, right) {
                    (RegisterValue::Integer(a), RegisterValue::Integer(b)) => {
                        RegisterValue::Integer(lua_mod(a, b)?)
                    }
                    (a, b) => {
                        RegisterValue::Float(lua_float_modulus(a.number()?, b.number()?).to_bits())
                    }
                };
                (dest, result)
            }
            Instruction::BitAnd { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(
                    read_bitwise_integer(&registers, lhs)? & read_bitwise_integer(&registers, rhs)?,
                ),
            ),
            Instruction::BitOr { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(
                    read_bitwise_integer(&registers, lhs)? | read_bitwise_integer(&registers, rhs)?,
                ),
            ),
            Instruction::BitXor { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(
                    read_bitwise_integer(&registers, lhs)? ^ read_bitwise_integer(&registers, rhs)?,
                ),
            ),
            Instruction::BitNot { dest, input } => (
                dest,
                RegisterValue::Integer(!read_bitwise_integer(&registers, input)?),
            ),
            Instruction::Shl { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(shift(
                    read_bitwise_integer(&registers, lhs)?,
                    read_bitwise_integer(&registers, rhs)?,
                    true,
                )),
            ),
            Instruction::Shr { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(shift(
                    read_bitwise_integer(&registers, lhs)?,
                    read_bitwise_integer(&registers, rhs)?,
                    false,
                )),
            ),
        };
        let target = registers.get_mut(dest).ok_or(Error::InvalidBytecode)?;
        if target.is_some() {
            return Err(Error::InvalidBytecode);
        }
        *target = Some(value);
        pc += 1;
    }
    Ok(RegisterExecution {
        value: read(&registers, program.result)?,
        values: return_values,
        strings,
        tables,
    })
}

/// Length for Lua byte strings and the bounded raw table array border.
/// No __len metamethod is consulted by this independent register subset.
fn register_raw_length(
    value: RegisterValue,
    strings: &[Vec<u8>],
    tables: &[RegisterTable],
) -> Result<usize, Error> {
    match value {
        RegisterValue::String(index) => Ok(strings.get(index).ok_or(Error::InvalidBytecode)?.len()),
        RegisterValue::Table(index) => {
            let entries = &tables.get(index).ok_or(Error::InvalidBytecode)?.entries;
            let mut border = 0_usize;
            while border < entries.len()
                && entries
                    .iter()
                    .any(|(key, _)| *key == TableKey::Integer((border + 1) as i64))
            {
                border += 1;
            }
            Ok(border)
        }
        _ => Err(Error::TypeError),
    }
}

/// Normalize Lua table keys for the bounded register heap. Integral float
/// indices share slots with integer indices, strings compare by bytes, and
/// identity-bearing objects compare by their execution-local handles.
fn register_table_key(value: RegisterValue, strings: &[Vec<u8>]) -> Result<TableKey, Error> {
    match value {
        RegisterValue::Nil => Err(Error::TypeError),
        RegisterValue::Boolean(value) => Ok(TableKey::Boolean(value)),
        RegisterValue::Integer(value) => Ok(TableKey::Integer(value)),
        RegisterValue::Float(bits) => {
            let number = f64::from_bits(bits);
            if number.is_nan() {
                return Err(Error::TypeError);
            }
            if number.is_finite()
                && number.fract() == 0.0
                && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&number)
            {
                Ok(TableKey::Integer(number as i64))
            } else {
                Ok(TableKey::Float(if number == 0.0 {
                    0.0_f64.to_bits()
                } else {
                    bits
                }))
            }
        }
        RegisterValue::String(index) => {
            if index >= strings.len() {
                return Err(Error::InvalidBytecode);
            }
            Ok(TableKey::String(index))
        }
        RegisterValue::Table(index) => Ok(TableKey::Table(index)),
        RegisterValue::Builtin(builtin) => Ok(TableKey::Builtin(builtin)),
    }
}

/// Convert execution-internal table keys back to Lua values for `next`.
fn register_table_key_value(key: &TableKey) -> RegisterValue {
    match *key {
        TableKey::Boolean(value) => RegisterValue::Boolean(value),
        TableKey::Integer(value) => RegisterValue::Integer(value),
        TableKey::Float(bits) => RegisterValue::Float(bits),
        TableKey::String(index) => RegisterValue::String(index),
        TableKey::Table(index) => RegisterValue::Table(index),
        TableKey::Builtin(value) => RegisterValue::Builtin(value),
    }
}

/// Bounded Lua-style tonumber conversion. Numeric scalars retain their tag.
/// An explicit base accepts integral numeric strings in 2..=36; a missing
/// base accepts decimal and hexadecimal Lua numeric spellings.
fn register_tonumber(
    value: RegisterValue,
    base: RegisterValue,
    strings: &[Vec<u8>],
) -> Result<RegisterValue, Error> {
    if base == RegisterValue::Nil {
        match value {
            RegisterValue::Integer(_) | RegisterValue::Float(_) => return Ok(value),
            RegisterValue::String(index) => {
                let raw = strings.get(index).ok_or(Error::InvalidBytecode)?;
                let Ok(raw) = std::str::from_utf8(raw) else {
                    return Ok(RegisterValue::Nil);
                };
                let text = raw.trim_ascii();
                if text.is_empty() {
                    return Ok(RegisterValue::Nil);
                }
                let (sign, unsigned) = if let Some(rest) = text.strip_prefix('-') {
                    (-1.0, rest)
                } else if let Some(rest) = text.strip_prefix('+') {
                    (1.0, rest)
                } else {
                    (1.0, text)
                };
                if unsigned.starts_with("0x") || unsigned.starts_with("0X") {
                    if let Some(integer) = parse_lua_hex_integer_wrapping(unsigned) {
                        return Ok(RegisterValue::Integer(if sign < 0.0 {
                            integer.wrapping_neg()
                        } else {
                            integer
                        }));
                    }
                    if let Some(number) = parse_lua_hex_float(unsigned) {
                        return Ok(RegisterValue::Float((sign * number).to_bits()));
                    }
                    return Ok(RegisterValue::Nil);
                }
                if let Ok(integer) = text.parse::<i64>() {
                    return Ok(RegisterValue::Integer(integer));
                }
                let valid_decimal = text.bytes().any(|byte| byte.is_ascii_digit())
                    && text.bytes().all(|byte| {
                        byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.' | b'e' | b'E')
                    });
                if valid_decimal && let Ok(number) = text.parse::<f64>() {
                    return Ok(RegisterValue::Float(number.to_bits()));
                }
                return Ok(RegisterValue::Nil);
            }
            _ => return Ok(RegisterValue::Nil),
        }
    }

    let radix = register_select_integer(base, strings)?;
    if !(2..=36).contains(&radix) {
        return Err(Error::TypeError);
    }
    let RegisterValue::String(index) = value else {
        return Err(Error::TypeError);
    };
    let raw = strings.get(index).ok_or(Error::InvalidBytecode)?;
    let Ok(raw) = std::str::from_utf8(raw) else {
        return Ok(RegisterValue::Nil);
    };
    let trimmed = raw.trim_ascii();
    let (negative, digits) = if let Some(rest) = trimmed.strip_prefix('-') {
        (true, rest)
    } else {
        (false, trimmed.strip_prefix('+').unwrap_or(trimmed))
    };
    // An explicit base accepts digits only; Lua does not strip 0x when
    // a base argument is supplied, even when that base is hexadecimal.
    if digits.is_empty() {
        return Ok(RegisterValue::Nil);
    }
    let mut result = 0_u64;
    for byte in digits.bytes() {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'a'..=b'z' => u32::from(byte - b'a') + 10,
            b'A'..=b'Z' => u32::from(byte - b'A') + 10,
            _ => return Ok(RegisterValue::Nil),
        };
        if i64::from(digit) >= radix {
            return Ok(RegisterValue::Nil);
        }
        result = result
            .wrapping_mul(radix as u64)
            .wrapping_add(u64::from(digit));
    }
    let integer = result as i64;
    Ok(RegisterValue::Integer(if negative {
        integer.wrapping_neg()
    } else {
        integer
    }))
}

/// Lua's integer argument coercion for the bounded `select` builtin.
/// Numeric strings and exactly integral finite floats are accepted; fractional,
/// overflowing, nonnumeric and non-scalar values are rejected.
fn register_select_integer(value: RegisterValue, strings: &[Vec<u8>]) -> Result<i64, Error> {
    fn exact_integer(number: f64) -> Option<i64> {
        if !number.is_finite()
            || number.fract() != 0.0
            || !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&number)
        {
            return None;
        }
        Some(number as i64)
    }

    match value {
        RegisterValue::Integer(number) => Ok(number),
        RegisterValue::Float(bits) => exact_integer(f64::from_bits(bits)).ok_or(Error::TypeError),
        RegisterValue::String(index) => {
            let bytes = strings.get(index).ok_or(Error::InvalidBytecode)?;
            let source = std::str::from_utf8(bytes)
                .map_err(|_| Error::TypeError)?
                .trim_ascii();
            if source.is_empty() {
                return Err(Error::TypeError);
            }
            if let Ok(integer) = source.parse::<i64>() {
                return Ok(integer);
            }
            let (negative, unsigned) = if let Some(unsigned) = source.strip_prefix('-') {
                (true, unsigned)
            } else {
                (false, source.strip_prefix('+').unwrap_or(source))
            };
            if unsigned.starts_with("0x") || unsigned.starts_with("0X") {
                if let Some(integer) = parse_lua_hex_integer_wrapping(unsigned) {
                    return Ok(if negative {
                        integer.wrapping_neg()
                    } else {
                        integer
                    });
                }
                if let Some(float) = parse_lua_hex_float(unsigned) {
                    return exact_integer(if negative { -float } else { float })
                        .ok_or(Error::TypeError);
                }
            }
            exact_integer(source.parse::<f64>().map_err(|_| Error::TypeError)?)
                .ok_or(Error::TypeError)
        }
        _ => Err(Error::TypeError),
    }
}

/// Intern execution-owned byte strings, preserving equality and quota
/// accounting for both dynamic concatenation and first-class builtin calls.
fn intern_register_string(
    bytes: Vec<u8>,
    strings: &mut Vec<Vec<u8>>,
    used_bytes: &mut usize,
) -> Result<RegisterValue, Error> {
    if let Some(index) = strings.iter().position(|value| *value == bytes) {
        return Ok(RegisterValue::String(index));
    }
    if strings.len() >= MAX_REGISTERS
        || used_bytes.saturating_add(bytes.len()) > MAX_REGISTER_STRING_BYTES
    {
        return Err(Error::TooLarge);
    }
    *used_bytes += bytes.len();
    let index = strings.len();
    strings.push(bytes);
    Ok(RegisterValue::String(index))
}

/// Byte-correct concatenation input. Numbers are formatted without routing
/// through a host shell or foreign Lua runtime; other Lua types are rejected.
fn concat_operand_bytes(value: RegisterValue, strings: &[Vec<u8>]) -> Result<Vec<u8>, Error> {
    match value {
        RegisterValue::String(index) => strings.get(index).cloned().ok_or(Error::InvalidBytecode),
        RegisterValue::Integer(value) => Ok(value.to_string().into_bytes()),
        RegisterValue::Float(bits) => Ok(format!("{:?}", f64::from_bits(bits)).into_bytes()),
        _ => Err(Error::TypeError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn eval(source: &str) -> i64 {
        let p = compile_integer_expression(source).unwrap();
        execute_integer_program(&p).unwrap()
    }
    #[test]
    fn supports_arithmetic_and_precedence() {
        assert_eq!(eval("1+2*3"), 7);
        assert_eq!(eval("(4+6)*2"), 20);
        assert_eq!(eval("10- -3"), 13);
        assert_eq!(eval("0xF+1"), 16);
        assert_eq!(eval("0xffffffffffffffff"), -1);
        assert_eq!(eval("0x8000000000000000"), i64::MIN);
        assert_eq!(eval("0xffffffffffffffff&0xff"), 255);
        assert_eq!(eval("0x8000000000000000>>63"), 1);
        assert_eq!(eval("-(7-4)"), -3);
    }
    #[test]
    fn wraps_i64_integer_overflow() {
        assert_eq!(eval("9223372036854775807+1"), i64::MIN);
        assert_eq!(eval("0-9223372036854775807-2"), i64::MAX);
    }
    #[test]
    fn matches_lua_floor_division_and_modulo_signs() {
        assert_eq!(eval("-7//3"), -3);
        assert_eq!(eval("-7%3"), 2);
        assert_eq!(eval("7//-3"), -3);
        assert_eq!(eval("7%-3"), -2);
        assert_eq!(eval("7//3"), 2);
        assert_eq!(eval("7%3"), 1);
        assert_eq!(eval("(0-9223372036854775807-1)//-1"), i64::MIN);
        assert_eq!(eval("(0-9223372036854775807-1)%-1"), 0);
    }
    #[test]
    fn division_by_zero_returns_error() {
        for source in ["1//0", "1%0", "1//(2-2)"] {
            let program = compile_integer_expression(source).unwrap();
            assert_eq!(
                execute_integer_program(&program),
                Err(Error::DivisionByZero)
            );
        }
    }
    #[test]
    fn integer_bitwise_and_shift_semantics() {
        assert_eq!(eval("5&3"), 1);
        assert_eq!(eval("5|2"), 7);
        assert_eq!(eval("5~3"), 6);
        assert_eq!(eval("~0"), -1);
        assert_eq!(eval("1<<63"), i64::MIN);
        assert_eq!(eval("8<<-1"), 4);
        assert_eq!(eval("-1>>1"), i64::MAX);
        assert_eq!(eval("1<<64"), 0);
        assert_eq!(eval("1>>-64"), 0);
        assert_eq!(eval("1<<(-9223372036854775807-1)"), 0);
    }
    #[test]
    fn rejects_noninteger_or_side_effectful_expressions() {
        assert!(matches!(
            compile_integer_expression("_ENV"),
            Err(Error::Unsupported(_))
        ));
        for source in ["math.abs(1)", "t[1]"] {
            let compiled = compile_integer_expression(source).unwrap();
            assert_eq!(
                execute_register_program(&compiled),
                Err(Error::TypeError),
                "{source}"
            );
        }
    }
    #[test]
    fn typed_literal_register_expressions_do_not_coerce_to_integers() {
        for (source, expected) in [
            ("true", RegisterValue::Boolean(true)),
            ("false", RegisterValue::Boolean(false)),
            ("nil", RegisterValue::Nil),
            ("0", RegisterValue::Integer(0)),
        ] {
            let program = compile_integer_expression(source).unwrap();
            assert_eq!(execute_register_program(&program), Ok(expected));
            if !matches!(expected, RegisterValue::Integer(_)) {
                assert_eq!(execute_integer_program(&program), Err(Error::TypeError));
            }
        }
    }

    #[test]
    fn compiles_integer_chunks_with_local_scopes_and_parallel_assignment() {
        let examples = [
            ("local a,b=3,4; return a*b", 12),
            ("local x=4; local y=x*2; x=y-3; return x", 5),
            ("local a=1; local b=2; a,b=b,a; return a*10+b", 21),
            ("local x=10; do local x=x+5; x=x*2 end; return x", 10),
            ("local x=10; do x=x+5 end; return x", 15),
            ("local x=1; local x=x+1; return x", 2),
            ("local a,b=2,3; a,b=b,a; return (a<<4)|b", 50),
        ];
        for (source, expected) in examples {
            let bytecode = compile_integer_chunk(source).unwrap();
            assert_eq!(
                execute_integer_program(&bytecode).unwrap(),
                expected,
                "{source}"
            );
        }
    }
    #[test]
    fn nested_do_blocks_propagate_integer_returns_and_skip_unreachable_code() {
        for (source, expected) in [
            ("do return 7 end", 7),
            ("local x=3; do return x+4 end", 7),
            ("do do return 11 end end", 11),
            ("local a=2; do local b=a*3; return b+1 end", 7),
            ("local n=1; do n=n+9; do return n end end", 10),
            ("do return (7<<2)|3 end", 31),
            ("do return 8 end; local unused=9; return unused", 8),
            ("local x=5; do return x end; x=0; return x", 5),
            ("do local x=4 end; return 6", 6),
            ("do local x=3; do local x=x+9; return x end end", 12),
            ("do do local x=2 end end; return 7", 7),
        ] {
            let bytecode = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_integer_program(&bytecode), Ok(expected), "{source}");
        }
        let float_return = compile_integer_chunk("do return 1.5 end").unwrap();
        assert_eq!(
            execute_register_program(&float_return),
            Ok(RegisterValue::Float(1.5_f64.to_bits()))
        );
        assert_eq!(
            execute_integer_program(&float_return),
            Err(Error::TypeError)
        );
        let scalar = compile_integer_chunk("do return 1, 2 end").unwrap();
        assert_eq!(execute_integer_program(&scalar), Ok(1));
        let missing_library = compile_integer_chunk("do return math.max(1,2) end").unwrap();
        assert_eq!(
            execute_register_program(&missing_library),
            Err(Error::TypeError)
        );
    }

    #[test]
    fn register_integer_conditional_return_arms_and_comparisons() {
        for (source, expected) in [
            ("if true then return 7 else return 11 end", 7),
            ("if false then return 7 else return 11 end", 11),
            ("if 4<5 then return 9 else return 2 end", 9),
            ("if 4>5 then return 9 else return 2 end", 2),
            ("if 4<=4 then return 8 else return 1 end", 8),
            ("if 4>=5 then return 8 else return 1 end", 1),
            ("if 4==4 then return 8 else return 1 end", 8),
            ("if 4~=4 then return 8 else return 1 end", 1),
            (
                "if 1>2 then return 1 elseif 3<4 then return 17 else return 9 end",
                17,
            ),
            (
                "if 1>2 then return 1 elseif 4<3 then return 17 else return 9 end",
                9,
            ),
            (
                "local x=5; if x>3 then local y=x+2; return y else return 0 end",
                7,
            ),
            (
                "local x=5; if x<3 then return 0 else do return x+9 end end",
                14,
            ),
            ("if true then do return 8 end else return 9 end", 8),
            ("if (4*2)==8 then return (5<<2)|3 else return 0 end", 23),
            (
                "if 0==0 then if 3>=2 then return 19 else return 2 end else return 7 end",
                19,
            ),
        ] {
            let bytecode = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_integer_program(&bytecode), Ok(expected), "{source}");
        }
        for (source, expected) in [
            ("if true then return 1 end", RegisterValue::Integer(1)),
            (
                "if true then local a=2 else return 3 end",
                RegisterValue::Nil,
            ),
            (
                "if false then return 2 else local a=3 end",
                RegisterValue::Nil,
            ),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_register_program(&program), Ok(expected), "{source}");
        }
        let table_truth = compile_integer_chunk("if {} then return 1 else return 2 end").unwrap();
        assert_eq!(execute_integer_program(&table_truth), Ok(1));
        let float_return =
            compile_integer_chunk("if true then return 1.5 else return 2 end").unwrap();
        assert_eq!(
            execute_register_program(&float_return),
            Ok(RegisterValue::Float(1.5_f64.to_bits()))
        );
        assert_eq!(
            execute_integer_program(&float_return),
            Err(Error::TypeError)
        );
        let invalid = Program {
            code: vec![
                Instruction::Load { dest: 0, value: 1 },
                Instruction::BranchIfFalse {
                    condition: 0,
                    target: 0,
                },
            ],
            registers: 1,
            result: 0,
            strings: Vec::new(),
        };
        assert_eq!(
            execute_integer_program(&invalid),
            Err(Error::InvalidBytecode)
        );
    }

    #[test]
    fn integer_guard_clauses_fall_through_to_a_later_return() {
        for (source, expected) in [
            ("if true then return 7 end; return 11", 7),
            ("if false then return 7 end; return 11", 11),
            ("if 0 then return 8 end; return 4", 8),
            ("if not 0 then return 8 end; return 4", 4),
            ("local x=3; if x>2 then return x+5 end; return 0", 8),
            ("local x=3; if x<2 then return x+5 end; return x*3", 9),
            (
                "if false then return 1 elseif true then return 13 end; return 9",
                13,
            ),
            (
                "if false then return 1 elseif false then return 13 end; return 9",
                9,
            ),
            (
                "local x=2; if x>3 then return 1 elseif x==2 then return 17 end; return 0",
                17,
            ),
            ("if false and (1//0>0) then return 6 end; return 12", 12),
            ("if true or (1//0>0) then return 15 end; return 0", 15),
            ("if not (2<1) then do return 21 end end; return 9", 21),
            ("do if false then return 2 end end; return 19", 19),
            ("local x=7; do if x<5 then return 2 end end; return x+4", 11),
            ("local x=0; if x==1 then return 3 end; x=6; return x+2", 8),
            (
                "if false then return 3 end; if true then return 24 end; return 5",
                24,
            ),
            (
                "if false then return 3 end; if false then return 24 end; return 5",
                5,
            ),
            (
                "local x=5; if x==5 then local y=3; return x*y end; return 0",
                15,
            ),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        }
        let source = "if true then return 7 end";
        let program = compile_integer_chunk(source).unwrap();
        assert_eq!(execute_integer_program(&program), Ok(7));
    }

    #[test]
    fn register_returning_branches_do_not_leak_bindings_into_other_paths() {
        for (source, expected) in [
            (
                "local x=4; if false then x=9; return x else return x end",
                4,
            ),
            ("local x=4; if true then x=9; return x else return x end", 9),
            ("local x=4; if false then x=9; return x end; return x", 4),
            ("local x=4; if true then x=9; return x end; return x", 9),
            (
                "local x=4; if false then x=9; return x elseif true then return x end; return 1",
                4,
            ),
            (
                "local x=4; if true then x=9; return x elseif true then return x end; return 1",
                9,
            ),
            (
                "local x=2; if false then x=5; return x elseif true then x=7; return x else return x end",
                7,
            ),
            (
                "local x=2; if false then x=5; return x elseif false then x=7; return x else return x end",
                2,
            ),
            (
                "local x=2; if true then x=5; return x elseif true then x=7; return x else return x end",
                5,
            ),
            (
                "local x=2; if false then x=5; return x else do local x=9; return x end end",
                9,
            ),
            (
                "local x=2; if false then x=5; return x else return x+6 end",
                8,
            ),
            (
                "local x=2; if false then x=5; return x end; do x=x+3 end; return x",
                5,
            ),
            (
                "local x=2; do if false then x=10; return x end end; return x",
                2,
            ),
            (
                "local x=2; do if true then x=10; return x end end; return x",
                10,
            ),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        }
    }

    #[test]
    fn integer_nonreturning_conditional_arms_merge_only_unmodified_bindings() {
        for (source, expected) in [
            ("if true then local tmp=3 end; return 9", 9),
            ("if false then local tmp=1 end; return 8", 8),
            ("local x=4; if x>2 then local y=x*3 end; return x+1", 5),
            (
                "if true then local t=1 elseif true then return 99 else return 1 end; return 4",
                4,
            ),
            (
                "if false then local t=1 elseif true then return 19 else return 1 end; return 4",
                19,
            ),
            ("if false then local t=1 else local q=2 end; return 4", 4),
            ("if true then return 2 else local q=3 end; return 4", 2),
            ("if false then return 2 else local q=3 end; return 4", 4),
            ("if true then local q=8 else return 9 end; return 4", 4),
            ("if false then local q=8 else return 9 end; return 4", 9),
            (
                "local x=2; if false then local y=x+1 elseif true then local z=x*2 end; return x+5",
                7,
            ),
            (
                "local x=1; if true then local y=x+2 end; do x=x+7 end; return x",
                8,
            ),
            ("if false and (1//0>0) then local x=8 end; return 3", 3),
            ("if true or (1//0>0) then local y=3 end; return 12", 12),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        }
        let source = "if true then local a=1 end";
        let program = compile_integer_chunk(source).unwrap();
        assert_eq!(execute_register_program(&program), Ok(RegisterValue::Nil));
        let program = compile_integer_chunk("if true then local y=1//0 end; return 1").unwrap();
        assert_eq!(
            execute_integer_program(&program),
            Err(Error::DivisionByZero)
        );
        let program = compile_integer_chunk("if false then local y=1//0 end; return 1").unwrap();
        assert_eq!(execute_integer_program(&program), Ok(1));
    }

    #[test]
    fn integer_emission_rejects_overflow_without_growing_the_code_vector() {
        let mut program = Program {
            code: vec![Instruction::Load { dest: 0, value: 0 }; MAX_REGISTERS],
            registers: 1,
            result: 0,
            strings: Vec::new(),
        };
        assert_eq!(
            emit_instruction(&mut program, Instruction::Return { source: 0 }),
            Err(Error::TooLarge)
        );
        assert_eq!(program.code.len(), MAX_REGISTERS);
        program.code.pop();
        assert_eq!(
            emit_instruction(&mut program, Instruction::Return { source: 0 }),
            Ok(())
        );
        assert_eq!(program.code.len(), MAX_REGISTERS);
    }

    #[test]
    fn integer_chunk_rejects_oversized_instruction_stream_before_execution() {
        // A no-op conditional allocates only two registers but lowers to
        // four instructions. Without a separate instruction budget, chunks
        // like this compiled successfully and failed only during execution.
        let source = format!(
            "{}return 1",
            "if false then local unused=1 end; ".repeat(MAX_REGISTERS / 4 + 8)
        );
        assert_eq!(compile_integer_chunk(&source), Err(Error::TooLarge));
    }

    #[test]
    fn merged_integer_conditional_locals_preserve_selected_path_values() {
        for (source, expected) in [
            ("local x=1; if true then x=2 end; return x", 2),
            ("local x=1; if false then x=2 end; return x", 1),
            ("local x=1; if true then x=2 else x=3 end; return x", 2),
            ("local x=1; if false then x=2 else x=3 end; return x", 3),
            (
                "local x=1; if false then x=2 elseif true then x=7 end; return x",
                7,
            ),
            (
                "local x=1; if false then x=2 elseif false then x=7 end; return x",
                1,
            ),
            (
                "local x=1; if true then x=2 elseif true then x=7 end; return x",
                2,
            ),
            (
                "local x=1; if false then return 5 else x=2 end; return x",
                2,
            ),
            ("local x=1; if true then return 5 else x=2 end; return x", 5),
            ("local x=1; if true then x=2; do x=x+3 end end; return x", 5),
            ("local a,b=1,2; if true then a,b=b,a end; return a*10+b", 21),
            (
                "local a,b=1,2; if false then a,b=b,a end; return a*10+b",
                12,
            ),
            ("local x=1; if true then local x=5; x=6 end; return x", 1),
            ("local x=1; if false then x=2 else x=x+3 end; return x", 4),
            (
                "local x=1; if true then x=2 end; if false then x=4 end; return x",
                2,
            ),
            (
                "local x=1; if true then x=2 end; if true then x=x+5 end; return x",
                7,
            ),
            (
                "local x=1; if false then x=2 end; if true then x=x+5 end; return x",
                6,
            ),
            ("local x=1; if false then x=1//0 end; return x", 1),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        }
        let program =
            compile_integer_chunk("local x=1; if true then x=1//0 end; return x").unwrap();
        assert_eq!(
            execute_integer_program(&program),
            Err(Error::DivisionByZero)
        );
    }

    #[test]
    fn integer_guard_truthiness_and_boolean_not_match_lua() {
        for (source, expected) in [
            ("if 0 then return 7 else return 11 end", 7),
            ("if -1 then return 9 else return 3 end", 9),
            ("local x=0; if x then return 5 else return 6 end", 5),
            ("local x=9; if x-x then return 4 else return 8 end", 4),
            ("if not 0 then return 4 else return 8 end", 8),
            ("if not false then return 3 else return 6 end", 3),
            ("if not true then return 3 else return 6 end", 6),
            ("if not not 0 then return 15 else return 3 end", 15),
            ("if not (2>1) then return 0 else return 11 end", 11),
            ("if not (2<1) then return 9 else return 0 end", 9),
            ("if (4-4) then return 22 else return 0 end", 22),
            (
                "if false then return 3 elseif not (5<=4) then return 13 else return 2 end",
                13,
            ),
        ] {
            let bytecode = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_integer_program(&bytecode), Ok(expected), "{source}");
        }
        let program = compile_integer_chunk("if 1//0 then return 1 else return 2 end").unwrap();
        assert_eq!(
            execute_integer_program(&program),
            Err(Error::DivisionByZero)
        );
    }

    #[test]
    fn integer_conditional_short_circuit_guards_preserve_lua_control_flow() {
        for (source, expected) in [
            ("if false and (1//0>0) then return 4 else return 9 end", 9),
            ("if true or (1//0>0) then return 5 else return 9 end", 5),
            ("if true and 0 then return 6 else return 9 end", 6),
            ("if false or 0 then return 7 else return 9 end", 7),
            ("if false or false then return 7 else return 9 end", 9),
            ("if true and false then return 7 else return 9 end", 9),
            (
                "if false or true and true then return 8 else return 0 end",
                8,
            ),
            (
                "if true and false or true then return 13 else return 0 end",
                13,
            ),
            ("if (1<2) and (3>2) then return 12 else return 0 end", 12),
            ("if (1>2) or (3<2) then return 12 else return 2 end", 2),
            (
                "if not (false or false) then return 15 else return 0 end",
                15,
            ),
            (
                "local x=0; if (x or false) and not false then return 22 else return 0 end",
                22,
            ),
            (
                "if (false and (1//0>0)) or true then return 17 else return 0 end",
                17,
            ),
            (
                "if true or false and false then return 19 else return 0 end",
                19,
            ),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        }
        for source in [
            "if true and (1//0>0) then return 1 else return 2 end",
            "if false or (1//0>0) then return 1 else return 2 end",
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(
                execute_integer_program(&program),
                Err(Error::DivisionByZero)
            );
        }
    }

    #[test]
    fn integer_register_locals_track_lua_const_attributes_and_shadowing() {
        for (source, expected) in [
            ("local x<const> =3; return x+4", 7),
            ("local a<const>,b=3,4; b=7; return a+b", 10),
            ("local a,b<const> =1,2; a=7; return a+b", 9),
            ("local x<const> =3; local x=x+2; x=9; return x", 9),
            ("local x<const> =3; do local x=5; x=6 end; return x", 3),
            ("local x<const> =3; do local x=4; return x end", 4),
            ("local x=2; do local x<const> =x*3; return x+1 end", 7),
            ("local x<const> =3; local y=x+4; return y", 7),
            ("local x<const> =3; do local y=x+5; return y end", 8),
            ("local x<const> =3; local y=2; y=x+y; return y", 5),
        ] {
            let bytecode = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_integer_program(&bytecode), Ok(expected), "{source}");
        }
        for source in [
            "local x<const> =3; x=4; return x",
            "local x<const> =3; do x=4 end; return x",
            "local a,b<const> =1,2; b=4; return a+b",
            "local x<close> =3; return x",
            "local x<const>; x=1; return x",
        ] {
            assert!(compile_integer_chunk(source).is_err(), "{source}");
        }
    }

    #[test]
    fn register_while_loops_preserve_loop_carried_values_and_lexical_scopes() {
        for (source, result) in [
            ("local n=0; while n<5 do n=n+1 end; return n", 5),
            (
                "local n=1; local i=0; while i<4 do n=n*2; i=i+1 end; return n",
                16,
            ),
            ("local n=0; while false do n=n+1 end; return n", 0),
            (
                "local x=0; while x<5 do if x<2 then x=x+2 else x=x+1 end end; return x",
                5,
            ),
            (
                "local n=0; while n<3 do local i=0; while i<2 do n=n+1; i=i+1 end end; return n",
                4,
            ),
            (
                "local i=0; while i<3 do local temporary=i+7; i=i+1 end; return i",
                3,
            ),
            ("local i=0; while i<3 do i=i+1 end; return i+2", 5),
            (
                "local i=0; while i<3 do i=i+1 end; while i<6 do i=i+1 end; return i",
                6,
            ),
            (
                "local n=9223372036854775807; while n>0 do n=n+1 end; return n",
                i64::MIN,
            ),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(
                execute_integer_program(&program),
                Ok(result),
                "owned register loop: {source}"
            );
        }
    }

    #[test]
    fn register_repeat_until_evaluates_body_before_condition_and_sees_local_scope() {
        for (source, expected) in [
            ("local n=0; repeat n=n+1 until n>=4; return n", 4),
            ("local n=0; repeat n=n+1 until true; return n", 1),
            (
                "local n=0; repeat local next=n+1; n=next until next>=4; return n",
                4,
            ),
            (
                "local n=0; repeat if n<2 then n=n+1 else n=n+2 end until n>=4; return n",
                4,
            ),
            (
                "local n=0; repeat local i=0; while i<2 do n=n+1; i=i+1 end until n>=6; return n",
                6,
            ),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(
                execute_integer_program(&program),
                Ok(expected),
                "repeat-until: {source}"
            );
        }
        let never = compile_integer_chunk("local n=0; repeat n=n+1 until false; return n").unwrap();
        assert_eq!(
            execute_integer_program(&never),
            Err(Error::StepBudgetExceeded)
        );
    }

    #[test]
    fn register_loop_break_exits_preserve_nested_carried_values() {
        for (source, expected) in [
            (
                "local x=1; while true do x=x+3; break; x=100 end; return x",
                4,
            ),
            (
                "local x=0; while x<9 do x=x+1; if x==4 then break end end; return x",
                4,
            ),
            (
                "local x=0; repeat x=x+1; if x==3 then break end until x>99; return x",
                3,
            ),
            (
                "local x=0; repeat local i=x+1; x=i; if i>2 then break end until x>99; return x",
                3,
            ),
            (
                "local a=0; local b=0; while a<5 do a=a+1; while true do b=b+1; break end; if a==3 then break end end; return a*10+b",
                33,
            ),
            (
                "local x=1; while true do if true then x=x+4; break else x=90 end end; return x",
                5,
            ),
            (
                "local x=0; while true do do x=x+7; break end; x=99 end; return x",
                7,
            ),
            ("local x=0; while false do break end; return x", 0),
        ] {
            let program = compile_integer_chunk(source).unwrap_or_else(|e| panic!("{source}: {e}"));
            assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        }
        assert!(compile_integer_chunk("break; return 0").is_err());
        assert!(compile_integer_chunk("do break end; return 0").is_err());
    }

    #[test]
    fn register_integer_numeric_for_controls_scope_and_overflow() {
        for (source, expected) in [
            ("local s=0; for i=1,5 do s=s+i end; return s", 15),
            (
                "local n=0; for i=0x8000000000000000,0x8000000000000000,-1 do n=n+1 end; return n",
                1,
            ),
            (
                "local n=0; for i=0x7ffffffffffffffe,0x7fffffffffffffff,2 do n=n+1 end; return n",
                1,
            ),
            (
                "local n=0; for i=0x8000000000000001,0x8000000000000000,-2 do n=n+1 end; return n",
                1,
            ),
            ("local n=0; for i=4,1,-1 do n=n+i end; return n", 10),
            ("local x=1; for i=1,3 do x=x+i end; return x", 7),
            ("local s=0; for i=5,1,-2 do s=s+i end; return s", 9),
            ("local s=0; for i=1,0 do s=s+i end; return s", 0),
            ("local s=0; for i=0,3,-1 do s=s+1 end; return s", 0),
            (
                "local i=100; local s=0; for i=1,3 do s=s+i end; return i+s",
                106,
            ),
            (
                "local limit=2; local s=0; for i=1,limit do limit=10; s=s+i end; return s",
                3,
            ),
            ("local i=2; local s=0; for i=i,4 do s=s+i end; return s", 9),
            (
                "local s=0; for i=1,5 do if i==3 then break end; s=s+i end; return s",
                3,
            ),
            (
                "local s=0; for i=9223372036854775807,9223372036854775807 do s=s+1 end; return s",
                1,
            ),
            (
                "local x=0; local k=0; while k<2 do for i=1,2 do x=x+1 end; k=k+1 end; return x",
                4,
            ),
            (
                "local s=0; for i=1,2 do for j=1,3 do s=s+i*j end end; return s",
                18,
            ),
            (
                "local s=0; for i=1,5 do if i>2 then break else s=s+i end end; return s",
                3,
            ),
        ] {
            let program = compile_integer_chunk(source).unwrap_or_else(|e| panic!("{source}: {e}"));
            assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        }
        assert!(compile_integer_chunk("for i=1,3 do i=2 end; return 0").is_err());
        let zero = compile_integer_chunk("for i=1,2,0 do end; return 5").unwrap();
        assert_eq!(execute_integer_program(&zero), Err(Error::ZeroForStep));
        let endless =
            compile_integer_chunk("local s=0; for i=1,999999999 do s=s+i end; return s").unwrap();
        assert_eq!(
            execute_integer_program(&endless),
            Err(Error::StepBudgetExceeded)
        );
    }

    #[test]
    fn register_while_loop_step_quota_prevents_unbounded_execution() {
        let loop_program =
            compile_integer_chunk("local n=0; while true do n=n+1 end; return n").unwrap();
        assert_eq!(
            execute_integer_program(&loop_program),
            Err(Error::StepBudgetExceeded)
        );
    }

    #[test]
    fn register_explicit_instruction_budget_is_exact_and_cannot_bypass_hard_cap() {
        let chunk = compile_integer_chunk("return 7").unwrap();
        // Loading, recording the result, and returning each spend a step.
        assert_eq!(chunk.code.len(), 3);
        assert_eq!(
            execute_integer_program_with_budget(&chunk, 0),
            Err(Error::StepBudgetExceeded)
        );
        assert_eq!(
            execute_integer_program_with_budget(&chunk, 1),
            Err(Error::StepBudgetExceeded)
        );
        assert_eq!(
            execute_integer_program_with_budget(&chunk, 2),
            Err(Error::StepBudgetExceeded)
        );
        assert_eq!(execute_integer_program_with_budget(&chunk, 3), Ok(7));
        assert_eq!(
            execute_integer_program_with_budget(&chunk, usize::MAX),
            Ok(7)
        );

        let expression = compile_integer_expression("1+2").unwrap();
        assert_eq!(expression.code.len(), 3);
        assert_eq!(
            execute_integer_program_with_budget(&expression, 2),
            Err(Error::StepBudgetExceeded)
        );
        assert_eq!(execute_integer_program_with_budget(&expression, 3), Ok(3));

        let infinite = compile_integer_chunk("while true do end; return 1").unwrap();
        assert_eq!(
            execute_integer_program_with_budget(&infinite, 19),
            Err(Error::StepBudgetExceeded)
        );
        assert_eq!(
            execute_integer_program_with_budget(&infinite, usize::MAX),
            Err(Error::StepBudgetExceeded)
        );

        let invalid = Program {
            code: vec![Instruction::Load { dest: 10, value: 7 }],
            registers: 1,
            result: 0,
            strings: Vec::new(),
        };
        assert_eq!(
            execute_integer_program_with_budget(&invalid, 0),
            Err(Error::InvalidBytecode)
        );
    }

    #[test]
    fn register_implicit_fallthrough_returns_nil_in_scalar_context() {
        for (source, expected) in [
            ("", RegisterValue::Nil),
            ("local x=3", RegisterValue::Nil),
            ("do local x=3 end", RegisterValue::Nil),
            ("if true then return 7 end", RegisterValue::Integer(7)),
            ("if false then return 7 end", RegisterValue::Nil),
            (
                "if true then local x=3 else return 7 end",
                RegisterValue::Nil,
            ),
            (
                "if false then return 7 else local x=3 end",
                RegisterValue::Nil,
            ),
            ("local n=0; while n<3 do n=n+1 end", RegisterValue::Nil),
            ("local n=0; repeat n=n+1 until n==3", RegisterValue::Nil),
            ("for i=1,3 do end", RegisterValue::Nil),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_register_program(&program), Ok(expected), "{source}");
        }
    }

    #[test]
    fn register_zero_result_returns_coerce_to_nil_in_scalar_context() {
        for (source, expected) in [
            ("return", RegisterValue::Nil),
            ("do return end", RegisterValue::Nil),
            ("if true then return else return 7 end", RegisterValue::Nil),
            (
                "if false then return else return 7 end",
                RegisterValue::Integer(7),
            ),
            (
                "local x=3; if x==3 then return end; return x",
                RegisterValue::Nil,
            ),
            ("local x=1; while true do return end", RegisterValue::Nil),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_register_program(&program), Ok(expected), "{source}");
        }
    }

    #[test]
    fn register_scalar_rhs_adjustment_preserves_nil_and_discarded_expressions() {
        for (source, expected) in [
            ("local x; return x", RegisterValue::Nil),
            ("local x,y; return y", RegisterValue::Nil),
            ("local x,y=7; return y", RegisterValue::Nil),
            ("local x<const>; return x", RegisterValue::Nil),
            ("local x,y<const> =4; return y", RegisterValue::Nil),
            ("local x,y=7; return x", RegisterValue::Integer(7)),
            ("local x=2,3; return x", RegisterValue::Integer(2)),
            ("local x,y=3,4; x=9; return y", RegisterValue::Integer(4)),
            ("local x,y=3,4; x,y=8; return y", RegisterValue::Nil),
            ("local x,y=3,4; x,y=8; return x", RegisterValue::Integer(8)),
            ("local x,y=3,4; x=9,10; return x", RegisterValue::Integer(9)),
            ("local x; x=5; return x", RegisterValue::Integer(5)),
            (
                "local x=1; local x,y=x+1; return x",
                RegisterValue::Integer(2),
            ),
            ("local x=1; local x,y=x+1; return y", RegisterValue::Nil),
            (
                "local x,y; if x==nil then y=7 end; return y",
                RegisterValue::Integer(7),
            ),
            (
                "local x,y=0; while x<3 do x=x+1 end; return y",
                RegisterValue::Nil,
            ),
            (
                "local x,y=3,4; x,y=y,x; return x",
                RegisterValue::Integer(4),
            ),
        ] {
            let program = compile_integer_chunk(source).unwrap();
            assert_eq!(execute_register_program(&program), Ok(expected), "{source}");
        }
        let nil = compile_integer_chunk("local x; return x").unwrap();
        assert_eq!(execute_integer_program(&nil), Err(Error::TypeError));
    }

    #[test]
    fn rejects_unsupported_dynamic_chunk_features() {
        for source in ["return _ENV", "_ENV=1; return _ENV"] {
            assert!(compile_integer_chunk(source).is_err(), "{source}");
        }
        let scalar_indexed = compile_integer_chunk("local x=1; x.y=2; return x").unwrap();
        assert_eq!(
            execute_register_program(&scalar_indexed),
            Err(Error::TypeError)
        );
        let float_return = compile_integer_chunk("local x=1; return 1.5").unwrap();
        assert_eq!(
            execute_register_program(&float_return),
            Ok(RegisterValue::Float(1.5_f64.to_bits()))
        );
    }
    #[test]
    fn validates_unreachable_register_string_pool_indices() {
        let valid = Program {
            code: vec![
                Instruction::LoadString { dest: 0, index: 0 },
                Instruction::Return { source: 0 },
            ],
            registers: 1,
            result: 0,
            strings: vec![b"ok".to_vec()],
        };
        let result = execute_register_program(&valid).unwrap();
        assert_eq!(valid.string_bytes(result), Some(b"ok".as_slice()));
        let mut invalid = valid.clone();
        invalid.code.push(Instruction::LoadString {
            dest: 0,
            index: usize::MAX,
        });
        assert_eq!(
            execute_register_program_with_budget(&invalid, 0),
            Err(Error::InvalidBytecode)
        );
        let mut oversized = valid;
        oversized.strings = vec![vec![b'x'; MAX_REGISTER_STRING_BYTES + 1]];
        assert_eq!(
            execute_register_program_with_budget(&oversized, 0),
            Err(Error::InvalidBytecode)
        );
    }

    #[test]
    fn validates_unreachable_integer_bytecode_and_register_bounds() {
        let valid = Program {
            code: vec![
                Instruction::Load { dest: 0, value: 7 },
                Instruction::Return { source: 0 },
            ],
            registers: 1,
            result: 0,
            strings: Vec::new(),
        };
        assert_eq!(execute_integer_program(&valid), Ok(7));

        let malformed = [
            Program {
                code: vec![
                    Instruction::Load { dest: 0, value: 7 },
                    Instruction::Return { source: 0 },
                    Instruction::Jump { target: 0 },
                ],
                ..valid.clone()
            },
            Program {
                code: vec![
                    Instruction::Load { dest: 0, value: 7 },
                    Instruction::Return { source: 0 },
                    Instruction::Add {
                        dest: 2,
                        lhs: 0,
                        rhs: 0,
                    },
                ],
                ..valid.clone()
            },
            Program {
                code: vec![
                    Instruction::Load { dest: 0, value: 7 },
                    Instruction::Return { source: 0 },
                    Instruction::BranchIfFalse {
                        condition: 5,
                        target: 3,
                    },
                    Instruction::Return { source: 0 },
                ],
                ..valid.clone()
            },
            Program {
                result: usize::MAX,
                ..valid.clone()
            },
            Program {
                code: vec![
                    Instruction::Load { dest: 0, value: 7 },
                    Instruction::Return { source: 0 },
                    Instruction::MergeCopy { dest: 1, input: 0 },
                ],
                ..valid.clone()
            },
            Program {
                code: vec![
                    Instruction::Load { dest: 0, value: 7 },
                    Instruction::Return { source: 0 },
                    Instruction::Compare {
                        dest: 0,
                        lhs: usize::MAX,
                        rhs: 0,
                        operation: CompareOperation::Equal,
                    },
                ],
                ..valid.clone()
            },
        ];
        for (index, program) in malformed.iter().enumerate() {
            assert_eq!(
                execute_integer_program(program),
                Err(Error::InvalidBytecode),
                "malformed case {index}"
            );
        }
    }

    #[test]
    fn rejects_malformed_numeric_for_dispatch_even_on_unreachable_paths() {
        let original =
            compile_integer_chunk("local x=0; for i=1,3 do x=x+i end; return x").unwrap();
        let check_at = original
            .code
            .iter()
            .position(|op| matches!(op, Instruction::NumericForCheck { .. }))
            .unwrap();
        let step_at = original
            .code
            .iter()
            .position(|op| matches!(op, Instruction::NumericForStep { .. }))
            .unwrap();
        let mut bad_register = original.clone();
        if let Instruction::NumericForCheck { visible, .. } = &mut bad_register.code[check_at] {
            *visible = original.registers;
        }
        assert_eq!(
            execute_integer_program(&bad_register),
            Err(Error::InvalidBytecode)
        );
        let mut bad_exit = original.clone();
        if let Instruction::NumericForCheck { exit, .. } = &mut bad_exit.code[check_at] {
            *exit = check_at;
        }
        assert_eq!(
            execute_integer_program(&bad_exit),
            Err(Error::InvalidBytecode)
        );
        let mut bad_step = original;
        if let Instruction::NumericForStep { exit, .. } = &mut bad_step.code[step_at] {
            *exit = 0;
        }
        assert_eq!(
            execute_integer_program(&bad_step),
            Err(Error::InvalidBytecode)
        );
    }

    #[test]
    fn tagged_register_values_obey_exact_budgets_and_unreachable_validation() {
        let program = compile_integer_chunk("return nil").unwrap();
        assert_eq!(program.code.len(), 3);
        assert_eq!(
            execute_register_program_with_budget(&program, 0),
            Err(Error::StepBudgetExceeded)
        );
        assert_eq!(
            execute_register_program_with_budget(&program, 1),
            Err(Error::StepBudgetExceeded)
        );
        assert_eq!(
            execute_register_program_with_budget(&program, 2),
            Err(Error::StepBudgetExceeded)
        );
        assert_eq!(
            execute_register_program_with_budget(&program, 3),
            Ok(RegisterValue::Nil)
        );
        assert_eq!(
            execute_register_program_with_budget(&program, usize::MAX),
            Ok(RegisterValue::Nil)
        );

        let boolean =
            compile_integer_chunk("local n=0; if n then return true else return false end")
                .unwrap();
        assert_eq!(
            execute_register_program(&boolean),
            Ok(RegisterValue::Boolean(true))
        );

        let bad_control = compile_integer_chunk("for i=true,3 do end; return 7").unwrap();
        assert_eq!(
            execute_register_program(&bad_control),
            Err(Error::TypeError)
        );

        let mut unreachable = program;
        unreachable.code.push(Instruction::LoadNil {
            dest: unreachable.registers,
        });
        assert_eq!(
            execute_register_program_with_budget(&unreachable, 0),
            Err(Error::InvalidBytecode)
        );
    }

    #[test]
    fn rejects_invalid_register_bytecode() {
        let p = Program {
            code: vec![Instruction::Add {
                dest: 0,
                lhs: 1,
                rhs: 2,
            }],
            registers: 3,
            result: 0,
            strings: Vec::new(),
        };
        assert_eq!(execute_integer_program(&p), Err(Error::InvalidBytecode));
        let p = Program {
            code: vec![
                Instruction::Load { dest: 0, value: 1 },
                Instruction::Load { dest: 0, value: 2 },
            ],
            registers: 1,
            result: 0,
            strings: Vec::new(),
        };
        assert_eq!(execute_integer_program(&p), Err(Error::InvalidBytecode));
    }
}
