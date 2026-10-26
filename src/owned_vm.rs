//! Experimental Ferrua-owned register bytecode for integer expressions.
//! Not used by production fer; not a full Lua VM.

use crate::expr::{Expr, LocalAttribute, Statement, parse_chunk, parse_expression};
use std::collections::{HashMap, HashSet};
use std::fmt;
const MAX_REGISTERS: usize = 16384;
const MAX_REGISTER_STEPS: usize = 100_000;

/// Bounded register values; strings, tables and functions remain future work.
/// Tagged booleans and nil no longer masquerade as Lua integers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterValue {
    Nil,
    Boolean(bool),
    Integer(i64),
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
    Return {
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
}
impl Program {
    pub fn instructions(&self) -> &[Instruction] {
        &self.code
    }
    pub fn register_count(&self) -> usize {
        self.registers
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
        Expr::Atom(text) => {
            let integer =
                if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                    u64::from_str_radix(hex, 16).map(|bits| bits as i64)
                } else {
                    text.parse::<i64>()
                };
            match integer {
                Ok(value) => Instruction::Load {
                    dest: program.registers,
                    value,
                },
                Err(_) => {
                    return locals
                        .iter()
                        .rev()
                        .find_map(|scope| scope.get(*text).map(|binding| binding.register))
                        .ok_or_else(|| {
                            Error::Unsupported(format!("unbound or noninteger name: {text}"))
                        });
                }
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
        Expr::Binary { op, left, right }
            if matches!(
                *op,
                "+" | "-" | "*" | "//" | "%" | "&" | "|" | "~" | "<<" | ">>"
            ) =>
        {
            let lhs = emit(program, left, depth + 1, locals)?;
            let rhs = emit(program, right, depth + 1, locals)?;
            let dest = program.registers;
            match *op {
                "+" => Instruction::Add { dest, lhs, rhs },
                "-" => Instruction::Sub { dest, lhs, rhs },
                "*" => Instruction::Mul { dest, lhs, rhs },
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

/// Compile only standalone integer arithmetic and bitwise expressions into owned IR.
/// Variables, functions, floats, tables and side effects are unsupported.
pub fn compile_integer_expression(source: &str) -> Result<Program, Error> {
    let ast = parse_expression(source).map_err(|err| Error::Unsupported(err.to_string()))?;
    let mut program = Program {
        code: Vec::new(),
        registers: 0,
        result: 0,
    };
    let result = emit(&mut program, &ast, 0, &[])?;
    program.result = result;
    Ok(program)
}

/// Compile a static subset of Lua chunks to Ferrua-owned register bytecode.
///
/// Supports integer locals with initializers, parallel assignments to locals,
/// nested do/end blocks, bounded integer/Boolean if/elseif/else branches
/// with returning or continuing branches, SSA-like single-assignment
/// integer merge copies, bounded while, repeat-until and integer numeric-for
/// loops with loop-carried register values and lexical break targets, and a
/// single integer return from an active block. Calls, general truthiness,
/// generic iteration, globals, nil and closures remain unsupported.
pub fn compile_integer_chunk(source: &str) -> Result<Program, Error> {
    let statements = parse_chunk(source).map_err(|e| Error::Unsupported(e.to_string()))?;
    let mut program = Program {
        code: Vec::new(),
        registers: 0,
        result: 0,
    };
    let mut scopes = vec![HashMap::new()];
    let mut loops = Vec::new();
    let value = compile_statements(&statements, &mut program, &mut scopes, &mut loops, 0)?
        .ok_or_else(|| Error::Unsupported("chunk requires one final return value".into()))?;
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
            Statement::Return(values) if values.len() == 1 => return true,
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
                | Statement::NumericFor { body: inner, .. } => pending.push(inner),
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
                if names.is_empty()
                    || names.len() != initializers.len()
                    || names.len() != attributes.len()
                {
                    return Err(Error::Unsupported(
                        "all integer locals need one initializer".into(),
                    ));
                }
                // Evaluate RHS before adding newly declared bindings.
                let rhs = initializers
                    .iter()
                    .map(|e| emit(program, e, 0, scopes))
                    .collect::<Result<Vec<_>, _>>()?;
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
                if targets.is_empty() || targets.len() != values.len() {
                    return Err(Error::Unsupported("assignment RHS count mismatch".into()));
                }
                // All RHS expressions see the original bindings, as in Lua.
                let rhs = values
                    .iter()
                    .map(|e| emit(program, e, 0, scopes))
                    .collect::<Result<Vec<_>, _>>()?;
                for (target, register) in targets.iter().zip(rhs) {
                    let Expr::Atom(name) = target else {
                        return Err(Error::Unsupported("only local variable targets".into()));
                    };
                    let binding = scopes
                        .iter_mut()
                        .rev()
                        .find_map(|scope| scope.get_mut(*name))
                        .ok_or_else(|| Error::Unsupported(format!("global assignment: {name}")))?;
                    if binding.constant {
                        return Err(Error::Unsupported(format!(
                            "cannot assign to constant local: {name}"
                        )));
                    }
                    binding.register = register;
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
            Statement::Return(values) if values.len() == 1 => {
                let value = emit(program, &values[0], 0, scopes)?;
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
            Instruction::Load { dest, .. }
            | Instruction::LoadBoolean { dest, .. }
            | Instruction::LoadNil { dest } => register(dest),
            Instruction::Neg { dest, input }
            | Instruction::LoopCopy { dest, input }
            | Instruction::BitNot { dest, input }
            | Instruction::NumericTruthy { dest, input }
            | Instruction::BooleanNot { dest, input }
            | Instruction::BooleanCopy { dest, input }
            | Instruction::MergeCopy { dest, input } => register(dest) && register(input),
            Instruction::Add { dest, lhs, rhs }
            | Instruction::Sub { dest, lhs, rhs }
            | Instruction::Mul { dest, lhs, rhs }
            | Instruction::FloorDiv { dest, lhs, rhs }
            | Instruction::Mod { dest, lhs, rhs }
            | Instruction::BitAnd { dest, lhs, rhs }
            | Instruction::BitOr { dest, lhs, rhs }
            | Instruction::BitXor { dest, lhs, rhs }
            | Instruction::Shl { dest, lhs, rhs }
            | Instruction::Shr { dest, lhs, rhs }
            | Instruction::Compare { dest, lhs, rhs, .. } => {
                register(dest) && register(lhs) && register(rhs)
            }
            Instruction::BranchIfFalse { condition, target } => {
                register(condition) && target > pc && target < size
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

/// Execute the experimental register IR and preserve Lua nil/boolean/integer
/// return types. Strings, functions, tables, floats and a complete register
/// compiler remain separate unaccepted T302 work.
pub fn execute_register_program(program: &Program) -> Result<RegisterValue, Error> {
    execute_register_program_with_budget(program, MAX_REGISTER_STEPS)
}

/// All register variants use the same bounded execution and validation path.
pub fn execute_register_program_with_budget(
    program: &Program,
    budget: usize,
) -> Result<RegisterValue, Error> {
    validate_integer_program(program)?;
    let budget = budget.min(MAX_REGISTER_STEPS);
    let mut registers: Vec<Option<RegisterValue>> = vec![None; program.registers];
    fn read(registers: &[Option<RegisterValue>], at: usize) -> Result<RegisterValue, Error> {
        registers
            .get(at)
            .copied()
            .flatten()
            .ok_or(Error::InvalidBytecode)
    }
    fn read_integer(registers: &[Option<RegisterValue>], at: usize) -> Result<i64, Error> {
        read(registers, at)?.integer()
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
            Instruction::Return { source } => return read(&registers, source),
            Instruction::NumericForCheck {
                counter,
                limit,
                step,
                visible,
                exit,
            } => {
                let current = read_integer(&registers, counter)?;
                let bound = read_integer(&registers, limit)?;
                let stride = read_integer(&registers, step)?;
                if stride == 0 {
                    return Err(Error::ZeroForStep);
                }
                if (stride > 0 && current > bound) || (stride < 0 && current < bound) {
                    pc = exit;
                } else {
                    let slot = registers.get_mut(visible).ok_or(Error::InvalidBytecode)?;
                    if slot.replace(RegisterValue::Integer(current)).is_some() {
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
                let value = read_integer(&registers, counter)?;
                let stride = read_integer(&registers, step)?;
                if let Some(next) = value.checked_add(stride) {
                    *registers.get_mut(counter).ok_or(Error::InvalidBytecode)? =
                        Some(RegisterValue::Integer(next));
                    pc += 1;
                } else {
                    pc = exit;
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
                let flag = match operation {
                    CompareOperation::Equal => left == right,
                    CompareOperation::NotEqual => left != right,
                    CompareOperation::Less => left.integer()? < right.integer()?,
                    CompareOperation::LessEqual => left.integer()? <= right.integer()?,
                    CompareOperation::Greater => left.integer()? > right.integer()?,
                    CompareOperation::GreaterEqual => left.integer()? >= right.integer()?,
                };
                (dest, RegisterValue::Boolean(flag))
            }
            Instruction::Load { dest, value } => (dest, RegisterValue::Integer(value)),
            Instruction::LoadNil { dest } => (dest, RegisterValue::Nil),
            Instruction::LoadBoolean { dest, value } => (dest, RegisterValue::Boolean(value)),
            Instruction::Neg { dest, input } => (
                dest,
                RegisterValue::Integer(read_integer(&registers, input)?.wrapping_neg()),
            ),
            Instruction::Add { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(
                    read_integer(&registers, lhs)?.wrapping_add(read_integer(&registers, rhs)?),
                ),
            ),
            Instruction::Sub { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(
                    read_integer(&registers, lhs)?.wrapping_sub(read_integer(&registers, rhs)?),
                ),
            ),
            Instruction::Mul { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(
                    read_integer(&registers, lhs)?.wrapping_mul(read_integer(&registers, rhs)?),
                ),
            ),
            Instruction::FloorDiv { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(floor_div(
                    read_integer(&registers, lhs)?,
                    read_integer(&registers, rhs)?,
                )?),
            ),
            Instruction::Mod { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(lua_mod(
                    read_integer(&registers, lhs)?,
                    read_integer(&registers, rhs)?,
                )?),
            ),
            Instruction::BitAnd { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(
                    read_integer(&registers, lhs)? & read_integer(&registers, rhs)?,
                ),
            ),
            Instruction::BitOr { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(
                    read_integer(&registers, lhs)? | read_integer(&registers, rhs)?,
                ),
            ),
            Instruction::BitXor { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(
                    read_integer(&registers, lhs)? ^ read_integer(&registers, rhs)?,
                ),
            ),
            Instruction::BitNot { dest, input } => (
                dest,
                RegisterValue::Integer(!read_integer(&registers, input)?),
            ),
            Instruction::Shl { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(shift(
                    read_integer(&registers, lhs)?,
                    read_integer(&registers, rhs)?,
                    true,
                )),
            ),
            Instruction::Shr { dest, lhs, rhs } => (
                dest,
                RegisterValue::Integer(shift(
                    read_integer(&registers, lhs)?,
                    read_integer(&registers, rhs)?,
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
    read(&registers, program.result)
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
        for source in ["1.5+2", "1/2", "math.abs(1)", "x+1", "true", "2^3", "t[1]"] {
            assert!(
                matches!(
                    compile_integer_expression(source),
                    Err(Error::Unsupported(_))
                ),
                "{source}"
            );
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
        for source in [
            "do return 1.5 end",
            "do return 1, 2 end",
            "do return math.max(1,2) end",
        ] {
            assert!(compile_integer_chunk(source).is_err(), "{source}");
        }
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
        for source in [
            "if true then return 1 end",
            "if true then local a=2 else return 3 end",
            "if {} then return 1 else return 2 end",
            "if false then return 2 else local a=3 end",
            "if true then return 1.5 else return 2 end",
        ] {
            assert!(compile_integer_chunk(source).is_err(), "{source}");
        }
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
        assert!(compile_integer_chunk(source).is_err(), "{source}");
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
        assert!(compile_integer_chunk(source).is_err(), "{source}");
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
            "local x<const>; return 1",
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
        assert_eq!(chunk.code.len(), 2);
        assert_eq!(
            execute_integer_program_with_budget(&chunk, 0),
            Err(Error::StepBudgetExceeded)
        );
        assert_eq!(
            execute_integer_program_with_budget(&chunk, 1),
            Err(Error::StepBudgetExceeded)
        );
        assert_eq!(execute_integer_program_with_budget(&chunk, 2), Ok(7));
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
        };
        assert_eq!(
            execute_integer_program_with_budget(&invalid, 0),
            Err(Error::InvalidBytecode)
        );
    }

    #[test]
    fn rejects_unsupported_dynamic_chunk_features() {
        for source in [
            "local x; return x",
            "return x",
            "x=1; return x",
            "local x=1; print(x); return x",
            "local x=1; return 1.5",
            "local x=1; x.y=2; return x",
        ] {
            assert!(compile_integer_chunk(source).is_err(), "{source}");
        }
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
    fn rejects_invalid_register_bytecode() {
        let p = Program {
            code: vec![Instruction::Add {
                dest: 0,
                lhs: 1,
                rhs: 2,
            }],
            registers: 3,
            result: 0,
        };
        assert_eq!(execute_integer_program(&p), Err(Error::InvalidBytecode));
        let p = Program {
            code: vec![
                Instruction::Load { dest: 0, value: 1 },
                Instruction::Load { dest: 0, value: 2 },
            ],
            registers: 1,
            result: 0,
        };
        assert_eq!(execute_integer_program(&p), Err(Error::InvalidBytecode));
    }
}
