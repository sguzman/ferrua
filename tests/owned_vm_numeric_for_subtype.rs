//! Lua 5.4 numeric-for subtype selection and numeric control coercions.
//! A float limit alone must not turn integer start/step controls into floats.
use ferrua::owned_vm::{RegisterValue, compile_integer_chunk, execute_register_program};
use std::process::Command;

fn owned(chunk: &str) -> RegisterValue {
    execute_register_program(&compile_integer_chunk(chunk).unwrap()).unwrap()
}

#[test]
fn numeric_for_control_types_match_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    let integer = RegisterValue::Integer;
    let float = |n: f64| RegisterValue::Float(n.to_bits());
    for (chunk, expected) in [
        ("local last=0;for i=1,3.5 do last=i end return last", integer(3)),
        ("local last=0;for i=1,3.0 do last=i end return last", integer(3)),
        ("local last=0;for i=4,1.5,-1 do last=i end return last", integer(2)),
        ("local last=0;for i=-1,-2.5,-1 do last=i end return last", integer(-2)),
        ("local last=0;for i=1,3.5,0.5 do last=i end return last", float(3.5)),
        ("local last=0;for i=1.0,3 do last=i end return last", float(3.0)),
        (r#"local last=0;for i="1","3" do last=i end return last"#, float(3.0)),
        (r#"local last=0;for i=1,"3" do last=i end return last"#, integer(3)),
        (r#"local last=0;for i=1,3,"1" do last=i end return last"#, float(3.0)),
    ] {
        assert_eq!(owned(chunk), expected, "{chunk}");
        let rendered = match expected {
            RegisterValue::Integer(n) => n.to_string(),
            RegisterValue::Float(bits) => format!("{:?}", f64::from_bits(bits)),
            _ => unreachable!(),
        };
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let script = format!("local function f() {chunk} end; io.write(tostring(f()))");
            let output = command.args(["-e", &script]).output().unwrap();
            assert!(
                output.status.success(),
                "{executable} rejected {chunk}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, rendered.as_bytes(), "{executable}: {chunk}");
        }
    }
}
