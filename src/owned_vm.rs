//! Experimental Ferrua-owned register bytecode for integer expressions.
//! Not used by production fer; not a full Lua VM.

use crate::expr::{Expr, Statement, parse_chunk, parse_expression};
use std::collections::HashMap;
use std::fmt;
const MAX_REGISTERS: usize = 16384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instruction {
    Load { dest: usize, value: i64 },
    Neg { dest: usize, input: usize },
    Add { dest: usize, lhs: usize, rhs: usize },
    Sub { dest: usize, lhs: usize, rhs: usize },
    Mul { dest: usize, lhs: usize, rhs: usize },
    FloorDiv { dest: usize, lhs: usize, rhs: usize },
    Mod { dest: usize, lhs: usize, rhs: usize },
    BitAnd { dest: usize, lhs: usize, rhs: usize },
    BitOr { dest: usize, lhs: usize, rhs: usize },
    BitXor { dest: usize, lhs: usize, rhs: usize },
    BitNot { dest: usize, input: usize },
    Shl { dest: usize, lhs: usize, rhs: usize },
    Shr { dest: usize, lhs: usize, rhs: usize },
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

fn emit<'a>(
    program: &mut Program,
    expression: &Expr<'a>,
    depth: usize,
    locals: &[HashMap<&'a str, usize>],
) -> Result<usize, Error> {
    if depth > 128 || program.registers >= MAX_REGISTERS {
        return Err(Error::TooLarge);
    }
    let instruction = match expression {
        Expr::Atom(text) => {
            let integer =
                if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                    i64::from_str_radix(hex, 16)
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
                        .find_map(|scope| scope.get(*text).copied())
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
/// nested do/end blocks and a single final integer return. Calls, conditionals,
/// loops, globals, nil and closures remain explicitly unsupported.
pub fn compile_integer_chunk(source: &str) -> Result<Program, Error> {
    let statements = parse_chunk(source).map_err(|e| Error::Unsupported(e.to_string()))?;
    let mut program = Program {
        code: Vec::new(),
        registers: 0,
        result: 0,
    };
    let mut scopes = vec![HashMap::new()];
    let value = compile_statements(&statements, &mut program, &mut scopes, false, 0)?
        .ok_or_else(|| Error::Unsupported("chunk requires one final return value".into()))?;
    program.result = value;
    Ok(program)
}

fn compile_statements<'a>(
    statements: &[Statement<'a>],
    program: &mut Program,
    scopes: &mut Vec<HashMap<&'a str, usize>>,
    nested: bool,
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
                initializers,
            } => {
                if names.is_empty() || names.len() != initializers.len() {
                    return Err(Error::Unsupported(
                        "all locals need one integer initializer".into(),
                    ));
                }
                // Evaluate RHS before adding newly declared bindings.
                let rhs = initializers
                    .iter()
                    .map(|e| emit(program, e, 0, scopes))
                    .collect::<Result<Vec<_>, _>>()?;
                for (name, register) in names.iter().zip(rhs) {
                    scopes
                        .last_mut()
                        .expect("root scope")
                        .insert(*name, register);
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
                    let slot = scopes
                        .iter_mut()
                        .rev()
                        .find_map(|scope| scope.get_mut(*name))
                        .ok_or_else(|| Error::Unsupported(format!("global assignment: {name}")))?;
                    *slot = register;
                }
            }
            Statement::Block(body) => {
                scopes.push(HashMap::new());
                let child = compile_statements(body, program, scopes, true, depth + 1)?;
                scopes.pop();
                if child.is_some() {
                    return Err(Error::Unsupported("nested return is not supported".into()));
                }
            }
            Statement::Return(values) if !nested && values.len() == 1 => {
                result = Some(emit(program, &values[0], 0, scopes)?);
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
fn floor_div(a: i64, b: i64) -> Result<i64, Error> {
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

fn lua_mod(a: i64, b: i64) -> Result<i64, Error> {
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
fn shift(value: i64, distance: i64, left: bool) -> i64 {
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
    for instruction in &program.code {
        let (dest, value) = match *instruction {
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
    fn rejects_unsupported_dynamic_chunk_features() {
        for source in [
            "local x; return x",
            "return x",
            "x=1; return x",
            "local x=1; print(x); return x",
            "local x=1; if x>0 then x=2 end; return x",
            "local x=1; while x<3 do x=x+1 end; return x",
            "local x=1; return 1.5",
            "local x=1; do return x end",
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
