//! Bounded register VM tonumber conversions compared to production fer and Lua 5.4.
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
fn tonumber_numeric_strings_and_bases_match_lua54() {
    let reference = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (chunk, expected) in [
        ("return tonumber(42)", "1|42"),
        ("return tonumber(3.5)", "1|3.5"),
        ("return tonumber('42')", "1|42"),
        ("return tonumber('  -12  ')", "1|-12"),
        ("return tonumber('.5')", "1|0.5"),
        ("return tonumber('2.5')", "1|2.5"),
        ("return tonumber('0x10')", "1|16"),
        ("return tonumber('bad')", "1|nil"),
        ("return tonumber(nil)", "1|nil"),
        ("return tonumber(false)", "1|nil"),
        ("return tonumber({})", "1|nil"),
        ("return tonumber('ff',16)", "1|255"),
        ("return tonumber('z',36)", "1|35"),
        ("return tonumber('-101',2)", "1|-5"),
        ("return tonumber('0x10',16)", "1|16"),
        ("return tonumber('20',8)", "1|16"),
        ("return tonumber('10',2)", "1|2"),
        ("return tonumber('foo',10)", "1|nil"),
        ("return tonumber('1.5',10)", "1|nil"),
        ("return tonumber('3',3)", "1|nil"),
        ("return tonumber('  +1f  ',16)", "1|31"),
        ("return tonumber('nan')", "1|nil"),
        ("return tonumber('inf')", "1|nil"),
        ("return tonumber('2',2.0)", "1|nil"),
        ("return tonumber('2','10')", "1|2"),
        ("return type(tonumber)", "1|function"),
        ("return tonumber(' 0x1 '),tonumber('0.25')", "2|1|0.25"),
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
fn tonumber_rejects_bad_argument_and_base_range() {
    for chunk in [
        "return tonumber()",
        "return tonumber('10',1)",
        "return tonumber('10',37)",
        "return tonumber(10,16)",
        "return tonumber('10',false)",
    ] {
        assert_eq!(owned_result(chunk), Err(Error::TypeError), "{chunk}");
    }
}
