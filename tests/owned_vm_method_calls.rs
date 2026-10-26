//! Bounded Lua colon-method lowering in the independent register dispatcher.
//! Metatables and user-defined closures remain separate T302 acceptance work.
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
                    output
                        .value_string_bytes(value)
                        .expect("valid string handle"),
                ));
            }
            RegisterValue::Table(_) => rendered.push_str("table"),
            RegisterValue::Builtin(_) => rendered.push_str("function"),
        }
    }
    Ok(rendered)
}

#[test]
fn register_method_call_implicit_self_matches_lua54() {
    let reference = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (chunk, expected) in [
        ("local t={f=type};return t:f()", "1|table"),
        ("local t={f=type};return t:f(2)", "1|table"),
        ("local t={f=rawget,x=3};return t:f('x')", "1|3"),
        ("local t={f=rawget};return t:f('missing')", "1|nil"),
        ("local t={f=rawset};t:f('x',8);return t.x", "1|8"),
        ("local t={f=rawset};return t:f('x',9)==t,t.x", "2|true|9"),
        ("local t={f=rawset};local u=t;u:f('x',7);return t.x", "1|7"),
        (
            "local t={f=rawset};t:f('x',select(1,10,20));return t.x",
            "1|10",
        ),
        (
            "local t={f=rawset};t:f('x',(select(1,10,20)));return t.x",
            "1|10",
        ),
        (
            "local t={f=rawget,x=2};return t:f('x'),t.f==rawget",
            "2|2|true",
        ),
        ("local t={f=type};return type(t:f())", "1|string"),
        ("local t={f=rawget,x=12};return tostring(t:f('x'))", "1|12"),
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
            let source = format!(
                "local function f() {chunk} end; local t=table.pack(f()); io.write(t.n); for i=1,t.n do io.write('|',tostring(t[i])) end"
            );
            let output = command.args(["-e", &source]).output().unwrap();
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
fn register_method_call_rejects_noncallable_methods() {
    for chunk in [
        "local t={f=9};return t:f()",
        "local t={};return t:missing()",
        "local t={f=nil};return t:f()",
        "local t={f=rawset};return t:f('x')",
        "local t={f=rawget};return t:f()",
    ] {
        assert_eq!(owned_result(chunk), Err(Error::TypeError), "{chunk}");
    }
}
