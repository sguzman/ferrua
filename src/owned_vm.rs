//! Experimental Ferrua-owned register bytecode for integer expressions.
//! Not used by production fer; not a full Lua VM.

use crate::expr::{Expr, parse_expression};
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

fn emit(program: &mut Program, expression: &Expr<'_>, depth: usize) -> Result<usize, Error> {
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
                }
                .map_err(|_| Error::Unsupported(format!("not an integer literal: {text}")))?;
            Instruction::Load {
                dest: program.registers,
                value: integer,
            }
        }
        Expr::Unary { op, right } if matches!(*op, "-" | "~") => {
            let input = emit(program, right, depth + 1)?;
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
            let lhs = emit(program, left, depth + 1)?;
            let rhs = emit(program, right, depth + 1)?;
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
    let result = emit(&mut program, &ast, 0)?;
    program.result = result;
    Ok(program)
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
