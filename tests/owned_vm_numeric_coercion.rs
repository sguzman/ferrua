//! Lua 5.4 numeric-string coercion in the independent register arithmetic
//! dispatcher. Bitwise operations and comparisons intentionally do not coerce.
use ferrua::owned_vm::{Error, RegisterValue, compile_integer_chunk, execute_register_program};
use std::process::Command;

fn owned(chunk: &str) -> Result<RegisterValue, Error> {
    execute_register_program(&compile_integer_chunk(chunk)?)
}

fn reference_output(executable: &str, chunk: &str) -> std::process::Output {
    let mut command = Command::new(executable);
    if executable != "lua5.4" {
        command.arg("--lua=5.4");
    }
    command
        .args([
            "-e",
            &format!(
                "local function f() {chunk} end; local n=f(); io.write(type(n),'|',tostring(n))"
            ),
        ])
        .output()
        .unwrap()
}

#[test]
fn numeric_string_arithmetic_matches_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    let integer = RegisterValue::Integer;
    let float = |n: f64| RegisterValue::Float(n.to_bits());
    for (chunk, expected) in [
        (r#"return "12" + 3"#, integer(15)),
        (r#"return "12" + "3""#, integer(15)),
        (r#"return "3.5" + 2"#, float(5.5)),
        (r#"return "9" - 5"#, integer(4)),
        (r#"return "6" * "7""#, integer(42)),
        (r#"return "9" // 2"#, integer(4)),
        (r#"return "10" % 4"#, integer(2)),
        (r#"return "3" / 2"#, float(1.5)),
        (r#"return "2" ^ 3"#, float(8.0)),
        (r#"return -"7""#, integer(-7)),
        (r#"return "  -9 " + 2"#, integer(-7)),
        (r#"return "0x10" + 2"#, integer(18)),
        (r#"return "0x1p+2" + 1"#, float(5.0)),
        (r#"local n="3";return n*4"#, integer(12)),
        (r#"return "4.0" + 1"#, float(5.0)),
    ] {
        assert_eq!(owned(chunk), Ok(expected), "{chunk}");
        let rendered = match expected {
            RegisterValue::Integer(n) => format!("number|{n}"),
            RegisterValue::Float(bits) => format!("number|{:?}", f64::from_bits(bits)),
            _ => unreachable!(),
        };
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let output = reference_output(executable, chunk);
            assert!(
                output.status.success(),
                "{executable} rejected {chunk}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, rendered.as_bytes(), "{executable}: {chunk}");
        }
    }
}

#[test]
fn invalid_coercions_and_non_arithmetic_strings_still_reject() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for chunk in [
        r#"return "not a number" + 2"#,
        r#"return true * 2"#,
        r#"return "3" & 1"#,
        r#"return "3" < 4"#,
    ] {
        assert_eq!(owned(chunk), Err(Error::TypeError), "{chunk}");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let output = reference_output(executable, chunk);
            assert!(!output.status.success(), "{executable}: {chunk}");
        }
    }
}
