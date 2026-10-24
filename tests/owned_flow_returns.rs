//! Differential coverage for Lua's implicit zero-value chunk fallthrough.
//! The owned engine is still experimental and never silently uses luna-core.

use ferrua::owned_flow::{Value, compile_flow_chunk, execute_flow_values};
use std::process::Command;

#[test]
fn implicit_chunk_returns_match_production_and_upstream_lua() {
    let compare_upstream = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for source in [
        "",
        "local n=1",
        "if false then return 99 end",
        "if true then local n=1 end",
        "local n=1; while n<3 do n=n+1 end",
        "goto finish; ::finish::",
    ] {
        let compiled = compile_flow_chunk(source).unwrap();
        assert_eq!(execute_flow_values(&compiled, 1000), Ok(vec![]), "{source}");
        let lua = format!("print(select('#', (function() {source} end)()))");
        for binary in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if binary == "lua5.4" && !compare_upstream {
                continue;
            }
            let mut command = Command::new(binary);
            if binary != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command.args(["-e", &lua]).output().unwrap();
            assert!(
                output.status.success(),
                "{binary} rejected {source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                "0",
                "{binary} returned the wrong arity: {source}"
            );
        }
    }
    let source = "if true then return 9 end";
    let compiled = compile_flow_chunk(source).unwrap();
    assert_eq!(
        execute_flow_values(&compiled, 100),
        Ok(vec![Value::Integer(9)])
    );
}
