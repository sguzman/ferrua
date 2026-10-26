//! Independent register VM Lua assert acceptance. Failure status is tested;
//! this bounded subset does not yet claim Lua traceback or custom error parity.
use ferrua::owned_vm::{
    Error, RegisterValue, compile_integer_chunk, execute_register_program_with_strings,
};
use std::process::Command;

fn owned_result(chunk: &str) -> Result<String, Error> {
    let program = compile_integer_chunk(chunk)?;
    let output = execute_register_program_with_strings(&program)?;
    let mut rendered = output.values().len().to_string();
    for &value in output.values() {
        rendered.push('|');
        match value {
            RegisterValue::Nil => rendered.push_str("nil"),
            RegisterValue::Boolean(v) => rendered.push_str(&v.to_string()),
            RegisterValue::Integer(v) => rendered.push_str(&v.to_string()),
            RegisterValue::Float(bits) => {
                rendered.push_str(&format!("{:?}", f64::from_bits(bits)));
            }
            RegisterValue::String(_) => {
                rendered.push_str(&String::from_utf8_lossy(
                    output.value_string_bytes(value).expect("valid string handle"),
                ));
            }
            RegisterValue::Table(_) => rendered.push_str("table"),
            RegisterValue::Builtin(_) => rendered.push_str("function"),
        }
    }
    Ok(rendered)
}

#[test]
fn register_assert_preserves_lua_truthiness_and_return_arity() {
    let reference = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (chunk, expected) in [
        ("return assert(true)", "1|true"),
        ("return assert(0)", "1|0"),
        ("return assert('')", "1|"),
        ("return assert(3,4,5)", "3|3|4|5"),
        ("return assert(true,nil,8)", "3|true|nil|8"),
        ("return assert(true,select(1,7,8))", "3|true|7|8"),
        ("return assert(true,(select(1,7,8)))", "2|true|7"),
        ("return 5,assert(2,3,4)", "4|5|2|3|4"),
        ("return (assert(3,4))", "1|3"),
        ("local x,y=assert(true,4);return x,y", "2|true|4"),
        ("local f=assert;return f('ok',9)", "2|ok|9"),
        ("return type(assert)", "1|function"),
        ("local assert=type;return assert(3)", "1|number"),
    ] {
        assert_eq!(owned_result(chunk), Ok(expected.to_string()), "{chunk}");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !reference {
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
fn register_assert_rejects_false_and_nil_after_evaluating_arguments() {
    for chunk in [
        "return assert()",
        "return assert(nil)",
        "return assert(false)",
        "return assert(false, 'message')",
        "return assert(false, 8)",
    ] {
        assert_eq!(owned_result(chunk), Err(Error::TypeError), "{chunk}");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4"
                && std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
                    .ok()
                    .as_deref()
                    != Some("1")
            {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command.args(["-e", chunk]).output().unwrap();
            assert!(!output.status.success(), "{executable}: {chunk}");
        }
    }
    assert_eq!(
        owned_result("return assert(false,1//0)"),
        Err(Error::DivisionByZero)
    );
}
