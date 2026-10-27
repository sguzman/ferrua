//! Bounded Lua 5.4 table-valued metatable fallback semantics. No callbacks.
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
fn table_valued_metamethods_match_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (chunk, expected) in [
        ("local t=setmetatable({}, {__index={x=7}});return t.x", "7"),
        (
            "local t=setmetatable({x=3}, {__index={x=7}});return t.x",
            "3",
        ),
        (
            "local fallback=setmetatable({}, {__index={x=11}});local t=setmetatable({}, {__index=fallback});return t.x",
            "11",
        ),
        (
            "local sink={};local t=setmetatable({}, {__newindex=sink});t.x=9;return sink.x",
            "9",
        ),
        (
            "local sink={};local t=setmetatable({x=3}, {__newindex=sink});t.x=5;return t.x",
            "5",
        ),
        (
            "local sink=setmetatable({}, {__newindex={x=0}});local t=setmetatable({}, {__newindex=sink});t.x=9;return rawget(sink,'x')",
            "nil",
        ),
        (
            "local sink={};local t=setmetatable({}, {__newindex=sink});rawset(t,'x',4);return t.x",
            "4",
        ),
        (
            "local fallback={x=4};local t=setmetatable({}, {__index=fallback});return rawget(t,'x')",
            "nil",
        ),
        (
            "local mt={};local t=setmetatable({},mt);return rawequal(getmetatable(t),mt)",
            "true",
        ),
        (
            "local t=setmetatable({},{});setmetatable(t,nil);return getmetatable(t)",
            "nil",
        ),
        (
            "local t=setmetatable({}, {__metatable='hidden'});return getmetatable(t)",
            "hidden",
        ),
        (
            "local t=setmetatable({}, {__metatable=false});return getmetatable(t)",
            "false",
        ),
        ("return getmetatable(5)", "nil"),
        (
            "local t={};local mt={__index={x=8}};setmetatable(t,mt);return type(t.x)",
            "number",
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
fn protected_and_cyclic_metatable_operations_reject() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for chunk in [
        "setmetatable({}, 5)",
        "setmetatable(5, {})",
        "local t=setmetatable({}, {__metatable='locked'});setmetatable(t, {})",
        "local t={};setmetatable(t, {__index=t});return t.missing",
        "local t={};setmetatable(t, {__newindex=t});t.missing=1",
        "local t=setmetatable({}, {__index=2});return t.x",
        "local t=setmetatable({}, {__newindex=2});t.x=1",
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
