//! Bounded rawget/rawset acceptance for the independent register VM.
//! These tests do not imply metatable or full standard-library support.
use ferrua::owned_vm::{
    Error, RegisterValue, compile_integer_chunk, execute_register_program_with_strings,
};
use std::process::Command;

fn run_owned(source: &str) -> Result<String, Error> {
    let program = compile_integer_chunk(source)?;
    let execution = execute_register_program_with_strings(&program)?;
    let mut output = execution.values().len().to_string();
    for &value in execution.values() {
        output.push('|');
        match value {
            RegisterValue::Nil => output.push_str("nil"),
            RegisterValue::Boolean(value) => output.push_str(&value.to_string()),
            RegisterValue::Integer(value) => output.push_str(&value.to_string()),
            RegisterValue::Float(bits) => {
                output.push_str(&format!("{:?}", f64::from_bits(bits)));
            }
            RegisterValue::String(_) => {
                output.push_str(&String::from_utf8_lossy(
                    execution
                        .value_string_bytes(value)
                        .expect("valid result string"),
                ));
            }
            RegisterValue::Builtin(_) => output.push_str("function"),
            RegisterValue::Table(_) => output.push_str("table"),
        }
    }
    Ok(output)
}

#[test]
fn register_raw_table_builtins_match_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (chunk, expected) in [
        ("local t={x=4}; return rawget(t,'x')", "1|4"),
        ("local t={x=4}; return rawget(t,'missing')", "1|nil"),
        ("local t={}; rawset(t,'x',8); return t.x", "1|8"),
        ("local t={}; rawset(t,2,8); return t[2]", "1|8"),
        ("local t={}; rawset(t,2.0,8); return rawget(t,2)", "1|8"),
        (
            "local t={x=1}; rawset(t,'x',5); return rawget(t,'x')",
            "1|5",
        ),
        ("local t={x=4}; rawset(t,'x',nil); return t.x", "1|nil"),
        (
            "local t={}; return rawset(t,'x',9)==t,rawget(t,'x')",
            "2|true|9",
        ),
        ("local t={}; local f=rawset;f(t,'x',7);return t.x", "1|7"),
        ("local t={x=3};local f=rawget;return f(t,'x')", "1|3"),
        ("local t={}; rawset(t,1,10);rawset(t,2,20);return #t", "1|2"),
        ("local t={}; rawset(t,true,9);return rawget(t,true)", "1|9"),
        (
            "local t={};return type(rawget),type(rawset)",
            "2|function|function",
        ),
        ("local t={};return rawget(t,nil)", "1|nil"),
        ("local t={};return rawget(t,0/0)", "1|nil"),
        (
            "local t={};rawset(t,'a'..'b',7);return rawget(t,'ab')",
            "1|7",
        ),
    ] {
        assert_eq!(run_owned(chunk), Ok(expected.to_string()), "{chunk}");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let script = format!(
                "local function f() {chunk} end; local t=table.pack(f()); io.write(t.n); for i=1,t.n do io.write('|',tostring(t[i])) end"
            );
            let output = command.args(["-e", &script]).output().unwrap();
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
fn register_raw_table_builtins_reject_invalid_operations() {
    for chunk in [
        "return rawget()",
        "return rawget(8,'a')",
        "return rawset()",
        "return rawset({},'a')",
        "return rawset(8,'a',1)",
        "return rawset({},nil,1)",
        "return rawset({},0/0,1)",
        "local rawset=3; return rawset({},'a',1)",
    ] {
        assert_eq!(run_owned(chunk), Err(Error::TypeError), "{chunk}");
    }
}

#[test]
fn register_rawset_obeys_table_entry_quota() {
    let program =
        compile_integer_chunk("local t={}; for i=1,5000 do rawset(t,i,i) end return #t").unwrap();
    assert_eq!(
        execute_register_program_with_strings(&program),
        Err(Error::TooLarge)
    );
}
