//! Lua 5.4 table-valued __call dispatch to first-party register builtins.
//! User-defined function metamethods remain outside this bounded subset.
use ferrua::owned_vm::{
    Error, RegisterValue, compile_integer_chunk, execute_register_program_with_strings,
};
use std::process::Command;

fn owned(chunk: &str) -> Result<String, Error> {
    let result = execute_register_program_with_strings(&compile_integer_chunk(chunk)?)?;
    Ok(match result.value {
        RegisterValue::Nil => "nil".to_string(),
        RegisterValue::Boolean(value) => value.to_string(),
        RegisterValue::Integer(value) => value.to_string(),
        RegisterValue::Float(bits) => format!("{:?}", f64::from_bits(bits)),
        RegisterValue::String(_) => String::from_utf8_lossy(
            result
                .value_string_bytes(result.value)
                .ok_or(Error::InvalidBytecode)?,
        )
        .into_owned(),
        RegisterValue::Table(_) => "table".to_string(),
        RegisterValue::Builtin(_) => "function".to_string(),
    })
}

fn reference(executable: &str, chunk: &str) -> std::process::Output {
    let mut command = Command::new(executable);
    if executable != "lua5.4" {
        command.arg("--lua=5.4");
    }
    command
        .args([
            "-e",
            &format!("local function f() {chunk} end; io.write(tostring(f()))"),
        ])
        .output()
        .unwrap()
}

#[test]
fn callable_table_builtins_match_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (chunk, expected) in [
        (
            "local t=setmetatable({}, {__call=type});return t()",
            "table",
        ),
        (
            "local t=setmetatable({}, {__call=assert});return select('#',t(7,8))",
            "3",
        ),
        (
            "local t=setmetatable({}, {__call=rawset});t('x',9);return t.x",
            "9",
        ),
        (
            "local t=setmetatable({x=4},{__call=rawget});return t('x')",
            "4",
        ),
        (
            "local t=setmetatable({}, {__call=rawequal});return t(t)",
            "true",
        ),
        (
            "local inner=setmetatable({}, {__call=assert});local outer=setmetatable({}, {__call=inner});return select('#',outer(7,8))",
            "4",
        ),
        (
            "local t=setmetatable({}, {__call=type});local f=t;return f()",
            "table",
        ),
        (
            "local t={x=3};setmetatable(t,{__call=rawget});return select('#',t('x'))",
            "1",
        ),
    ] {
        assert_eq!(owned(chunk), Ok(expected.to_string()), "{chunk}");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let output = reference(executable, chunk);
            assert!(
                output.status.success(),
                "{executable} rejected {chunk}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, expected.as_bytes(), "{executable}: {chunk}");
        }
    }
}

#[test]
fn cyclic_and_invalid_callable_tables_reject() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for chunk in [
        "local t={};return t()",
        "local t=setmetatable({}, {__call=7});return t()",
        "local t={};setmetatable(t,{__call=t});return t()",
        "local t=setmetatable({}, {__call={}});return t()",
    ] {
        assert_eq!(owned(chunk), Err(Error::TypeError), "{chunk}");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let output = reference(executable, chunk);
            assert!(!output.status.success(), "{executable}: {chunk}");
        }
    }
}
