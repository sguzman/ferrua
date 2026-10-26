//! Lua 5.4 differential acceptance for the register VM's bounded table
//! iterator protocol. Generic-for compilation is tested separately.
use ferrua::owned_vm::{
    Error, RegisterValue, compile_integer_chunk, execute_register_program_with_strings,
};
use std::process::Command;

fn render(chunk: &str) -> Result<String, Error> {
    let program = compile_integer_chunk(chunk)?;
    let execution = execute_register_program_with_strings(&program)?;
    let mut output = execution.values().len().to_string();
    for &value in execution.values() {
        output.push('|');
        match value {
            RegisterValue::Nil => output.push_str("nil"),
            RegisterValue::Boolean(v) => output.push_str(&v.to_string()),
            RegisterValue::Integer(v) => output.push_str(&v.to_string()),
            RegisterValue::Float(bits) => {
                output.push_str(&format!("{:?}", f64::from_bits(bits)));
            }
            RegisterValue::String(_) => {
                output.push_str(&String::from_utf8_lossy(
                    execution.value_string_bytes(value).expect("string handle"),
                ));
            }
            RegisterValue::Table(_) => output.push_str("table"),
            RegisterValue::Builtin(_) => output.push_str("function"),
        }
    }
    Ok(output)
}

#[test]
fn register_next_and_pairs_builtin_protocol_match_lua54() {
    let reference = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (chunk, expected) in [
        ("return type(next),type(pairs)", "2|function|function"),
        ("local t={}; return next(t)", "1|nil"),
        ("local t={}; return next(t,nil)", "1|nil"),
        ("local t={x=9}; return next(t)", "2|x|9"),
        ("local t={[2]=7};return next(t)", "2|2|7"),
        ("local t={[true]=5};return next(t)", "2|true|5"),
        ("local t={x=9};return next(t,'x')", "1|nil"),
        ("local t={x=9};local f=next;return f(t)", "2|x|9"),
        ("local t={x=9};return (next(t))", "1|x"),
        ("local t={x=9};return pairs(t)", "3|function|table|nil"),
        ("local t={};return pairs(t)", "3|function|table|nil"),
        ("local t={x=9};local i,s,v=pairs(t);return type(i),s==t,v", "3|function|true|nil"),
        ("local t={x=9};local i,s,v=pairs(t);return i(s,v)", "2|x|9"),
        ("local t={x=9};local i,s,v=pairs(t);return i(s,'x')", "1|nil"),
    ] {
        assert_eq!(render(chunk), Ok(expected.to_string()), "{chunk}");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !reference {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let script = format!(
                "local function f() {chunk} end; local t=table.pack(f()); io.write(t.n); for i=1,t.n do io.write('|',type(t[i])=='table' and 'table' or type(t[i])=='function' and 'function' or tostring(t[i])) end"
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
fn register_next_and_pairs_validate_table_and_previous_key() {
    for chunk in [
        "return pairs()",
        "return pairs(1)",
        "return next()",
        "return next(false)",
        "return next({},'missing')",
        "local t={x=1};return next(t,'missing')",
        "local next=8;return next({})",
    ] {
        assert_eq!(render(chunk), Err(Error::TypeError), "{chunk}");
    }
}
