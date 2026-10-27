//! Lua 5.4 multiple-result adjustment for final colon-method calls.
//! Only the bounded first-class callable subset is accepted here.
use ferrua::owned_vm::{
    Error, RegisterValue, compile_integer_chunk, execute_register_program_with_strings,
};
use std::process::Command;

fn owned(chunk: &str) -> Result<String, Error> {
    let program = compile_integer_chunk(chunk)?;
    let result = execute_register_program_with_strings(&program)?;
    let mut output = result.values().len().to_string();
    for &value in result.values() {
        output.push('|');
        match value {
            RegisterValue::Nil => output.push_str("nil"),
            RegisterValue::Boolean(value) => output.push_str(&value.to_string()),
            RegisterValue::Integer(value) => output.push_str(&value.to_string()),
            RegisterValue::Float(bits) => {
                output.push_str(&format!("{:?}", f64::from_bits(bits)));
            }
            RegisterValue::String(_) => output.push_str(&String::from_utf8_lossy(
                result
                    .value_string_bytes(value)
                    .expect("valid string handle"),
            )),
            RegisterValue::Builtin(_) => output.push_str("function"),
            RegisterValue::Table(_) => output.push_str("table"),
        }
    }
    Ok(output)
}

#[test]
fn final_method_call_expansion_matches_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (chunk, expected) in [
        ("local t={m=pairs};return t:m()", "3|function|table|nil"),
        ("local t={m=pairs};return (t:m())", "1|function"),
        ("local t={m=pairs};return 7,t:m()", "4|7|function|table|nil"),
        ("local t={m=pairs};return t:m(),7", "2|function|7"),
        ("local t={m=pairs};return select('#',t:m())", "1|3"),
        ("local t={m=pairs};return select('#',(t:m()))", "1|1"),
        (
            "local t={m=pairs};local f,s,k=t:m();return type(f),rawequal(s,t),k",
            "3|function|true|nil",
        ),
        (
            "local t={m=pairs};local a={t:m()};return type(a[1]),rawequal(a[2],t),a[3]",
            "3|function|true|nil",
        ),
        (
            "local t={m=pairs};local a={t:m(),99};return type(a[1]),a[2]",
            "2|function|99",
        ),
        (
            "local t={m=pairs};local a={t:m(),x=1};return type(a[1]),rawequal(a[2],t),a.x",
            "3|function|true|1",
        ),
        ("local t={m=assert};return select('#',t:m(5,6))", "1|3"),
        ("local t={m=assert};return select('#',t:m(1,t:m(2,3)))", "1|5"),
        (
            "local t={m=assert};local a,b,c=t:m(5,6);return rawequal(a,t),b,c",
            "3|true|5|6",
        ),
        (
            "local t={m=ipairs};local f,s,k=t:m();return type(f),rawequal(s,t),k",
            "3|function|true|0",
        ),
    ] {
        assert_eq!(owned(chunk), Ok(expected.to_string()), "{chunk}");
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
            let rendered = String::from_utf8_lossy(&output.stdout);
            // The Lua reference formats table addresses differently from the
            // owned execution; normalize only their known identity display.
            let result = if expected.contains("|table|") {
                let fields = rendered.split('|').collect::<Vec<_>>();
                fields
                    .iter()
                    .map(|field| {
                        if field.starts_with("table: ") {
                            "table"
                        } else {
                            *field
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            } else {
                rendered.into_owned()
            };
            assert_eq!(result, expected, "{executable}: {chunk}");
        }
    }
}
