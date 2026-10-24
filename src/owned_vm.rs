//! Experimental Ferrua-owned register bytecode for integer expressions.
//! Not used by production fer; not a full Lua VM.

use crate::expr::{Expr, LocalAttribute, Statement, parse_chunk, parse_expression};
use std::collections::HashMap;
use std::fmt;
const MAX_REGISTERS: usize = 16384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instruction {
    Load {
        dest: usize,
        value: i64,
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
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(s) => write!(f, "unsupported arithmetic subset: {s}"),
            Self::TooLarge => write!(f, "experimental program is too large"),
            Self::InvalidBytecode => write!(f, "invalid experimental register bytecode"),
            Self::DivisionByZero => write!(f, "integer division by zero"),
        }
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy)]
struct RegisterBinding {
    register: usize,
    constant: bool,
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
    program.code.push(instruction);
    Ok(output)
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
        Expr::Atom(value) if matches!(*value, "true" | "false") => {
            let dest = program.registers;
            program.registers += 1;
            program.code.push(Instruction::Load {
                dest,
                value: i64::from(*value == "true"),
            });
            Ok(dest)
        }
        Expr::Unary { op, right } if *op == "not" => {
            let source = emit_condition(program, right, locals, depth + 1)?;
            if program.registers >= MAX_REGISTERS {
                return Err(Error::TooLarge);
            }
            let dest = program.registers;
            program.registers += 1;
            program.code.push(Instruction::BooleanNot {
                dest,
                input: source,
            });
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
            let lhs = emit(program, left, depth + 1, locals)?;
            let rhs = emit(program, right, depth + 1, locals)?;
            if program.registers >= MAX_REGISTERS {
                return Err(Error::TooLarge);
            }
            let dest = program.registers;
            program.registers += 1;
            program.code.push(Instruction::Compare {
                dest,
                lhs,
                rhs,
                operation,
            });
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
            program.code.push(Instruction::NumericTruthy { dest, input });
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
/// nested do/end blocks, bounded integer/Boolean if/elseif/else branches in
/// which every path returns, and a single integer return from an active block.
/// Calls, general truthiness, loops, globals, nil and closures remain unsupported.
pub fn compile_integer_chunk(source: &str) -> Result<Program, Error> {
    let statements = parse_chunk(source).map_err(|e| Error::Unsupported(e.to_string()))?;
    let mut program = Program {
        code: Vec::new(),
        registers: 0,
        result: 0,
    };
    let mut scopes = vec![HashMap::new()];
    let value = compile_statements(&statements, &mut program, &mut scopes, 0)?
        .ok_or_else(|| Error::Unsupported("chunk requires one final return value".into()))?;
    program.result = value;
    Ok(program)
}

fn compile_statements<'a>(
    statements: &[Statement<'a>],
    program: &mut Program,
    scopes: &mut Vec<HashMap<&'a str, RegisterBinding>>,
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
                let child = compile_statements(body, program, scopes, depth + 1)?;
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
                else_branch: Some(else_body),
            } => {
                // Every arm must terminate with an integer return, so there
                // is no register-state merge or fallthrough across arms.
                for (condition, body) in branches {
                    let guard = emit_condition(program, condition, scopes, 0)?;
                    let branch_at = program.code.len();
                    program.code.push(Instruction::BranchIfFalse {
                        condition: guard,
                        target: usize::MAX,
                    });
                    scopes.push(HashMap::new());
                    let returned = compile_statements(body, program, scopes, depth + 1)?;
                    scopes.pop();
                    if returned.is_none() {
                        return Err(Error::Unsupported(
                            "integer if arms must all return a value".into(),
                        ));
                    }
                    let target = program.code.len();
                    program.code[branch_at] = Instruction::BranchIfFalse {
                        condition: guard,
                        target,
                    };
                }
                scopes.push(HashMap::new());
                let final_return = compile_statements(else_body, program, scopes, depth + 1)?;
                scopes.pop();
                if final_return.is_none() {
                    return Err(Error::Unsupported(
                        "integer if arms must all return a value".into(),
                    ));
                }
                result = Some(0);
                break;
            }
            Statement::Return(values) if values.len() == 1 => {
                let value = emit(program, &values[0], 0, scopes)?;
                program.code.push(Instruction::Return { source: value });
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

/// Run the tiny integer register IR with initialized-register validation.
pub fn execute_integer_program(program: &Program) -> Result<i64, Error> {
    if program.registers == 0
        || program.registers > MAX_REGISTERS
        || program.code.len() > MAX_REGISTERS
    {
        return Err(Error::InvalidBytecode);
    }
    let mut registers: Vec<Option<i64>> = vec![None; program.registers];
    fn read(registers: &[Option<i64>], at: usize) -> Result<i64, Error> {
        registers
            .get(at)
            .copied()
            .flatten()
            .ok_or(Error::InvalidBytecode)
    }
    let mut pc = 0;
    while pc < program.code.len() {
        let (dest, value) = match program.code[pc] {
            Instruction::BranchIfFalse { condition, target } => {
                if target <= pc || target >= program.code.len() {
                    return Err(Error::InvalidBytecode);
                }
                pc = if read(&registers, condition)? == 0 {
                    target
                } else {
                    pc + 1
                };
                continue;
            }
            Instruction::Return { source } => return read(&registers, source),
            Instruction::NumericTruthy { dest, input } => {
                read(&registers, input)?;
                (dest, 1)
            }
            Instruction::BooleanNot { dest, input } => {
                (dest, i64::from(read(&registers, input)? == 0))
            }
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
                    CompareOperation::Less => left < right,
                    CompareOperation::LessEqual => left <= right,
                    CompareOperation::Greater => left > right,
                    CompareOperation::GreaterEqual => left >= right,
                };
                (dest, i64::from(flag))
            }
            Instruction::Load { dest, value } => (dest, value),
            Instruction::Neg { dest, input } => (dest, read(&registers, input)?.wrapping_neg()),
            Instruction::Add { dest, lhs, rhs } => (
                dest,
                read(&registers, lhs)?.wrapping_add(read(&registers, rhs)?),
            ),
            Instruction::Sub { dest, lhs, rhs } => (
                dest,
                read(&registers, lhs)?.wrapping_sub(read(&registers, rhs)?),
            ),
            Instruction::Mul { dest, lhs, rhs } => (
                dest,
                read(&registers, lhs)?.wrapping_mul(read(&registers, rhs)?),
            ),
            Instruction::FloorDiv { dest, lhs, rhs } => (
                dest,
                floor_div(read(&registers, lhs)?, read(&registers, rhs)?)?,
            ),
            Instruction::Mod { dest, lhs, rhs } => (
                dest,
                lua_mod(read(&registers, lhs)?, read(&registers, rhs)?)?,
            ),
            Instruction::BitAnd { dest, lhs, rhs } => {
                (dest, read(&registers, lhs)? & read(&registers, rhs)?)
            }
            Instruction::BitOr { dest, lhs, rhs } => {
                (dest, read(&registers, lhs)? | read(&registers, rhs)?)
            }
            Instruction::BitXor { dest, lhs, rhs } => {
                (dest, read(&registers, lhs)? ^ read(&registers, rhs)?)
            }
            Instruction::BitNot { dest, input } => (dest, !read(&registers, input)?),
            Instruction::Shl { dest, lhs, rhs } => (
                dest,
                shift(read(&registers, lhs)?, read(&registers, rhs)?, true),
            ),
            Instruction::Shr { dest, lhs, rhs } => (
                dest,
                shift(read(&registers, lhs)?, read(&registers, rhs)?, false),
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
            "do return nil end",
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
            "if true then return nil else return 2 end",
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
        let program = compile_integer_chunk(
            "if 1//0 then return 1 else return 2 end",
        )
        .unwrap();
        assert_eq!(
            execute_integer_program(&program),
            Err(Error::DivisionByZero)
        );
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
    fn rejects_unsupported_dynamic_chunk_features() {
        for source in [
            "local x; return x",
            "return x",
            "x=1; return x",
            "local x=1; print(x); return x",
            "local x=1; if x>0 then x=2 end; return x",
            "local x=1; while x<3 do x=x+1 end; return x",
            "local x=1; return 1.5",
            "local x=1; x.y=2; return x",
        ] {
            assert!(compile_integer_chunk(source).is_err(), "{source}");
        }
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
