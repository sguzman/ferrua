//! Differential acceptance for experimental integer+boolean control-flow VM.
use ferrua::owned_flow::{compile_flow_chunk, execute_flow_chunk, DEFAULT_STEP_BUDGET};
use std::process::Command;

const FIXTURES: &[&str] = &[
    "local n=0; while n<5 do n=n+1 end; return n",
    "local a=1; local b=1; local n=0; while n<6 do a,b=b,a+b; n=n+1 end; return b",
    "local x=0; if x==0 then x=9 else x=1 end; return x",
    "if 0 then return 7 else return 9 end",
    "if false then return 7 elseif true then return 8 else return 9 end",
    "if not false then return 12 else return 13 end",
    "local x=2; if x<0 then return 1 elseif x<=2 then return 8 else return 5 end",
    "local x=0; repeat local next=x+1; x=next until next>=4; return x",
    "local x=3; repeat x=x-1 until x==0; return x",
];

fn check(reference: &str) {
    for source in FIXTURES {
        let compiled = compile_flow_chunk(source).unwrap();
        let owned = execute_flow_chunk(&compiled, DEFAULT_STEP_BUDGET).unwrap();
        let code = format!("print((function() {source} end)())");
        let output = Command::new(reference).args(["-e", &code]).output().unwrap();
        assert!(output.status.success(), "{source}: {}", String::from_utf8_lossy(&output.stderr));
        let expected: i64 = String::from_utf8_lossy(&output.stdout).trim().parse().unwrap();
        assert_eq!(owned, expected, "{reference} differs on {source}");
    }
}

#[test]
fn production_fer_agrees_with_owned_control_flow_subset() {
    for source in FIXTURES {
        let compiled = compile_flow_chunk(source).unwrap();
        let owned = execute_flow_chunk(&compiled, DEFAULT_STEP_BUDGET).unwrap();
        let code = format!("print((function() {source} end)())");
        let output = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--lua=5.4", "-e", &code]).output().unwrap();
        assert!(output.status.success(), "{source}: {}", String::from_utf8_lossy(&output.stderr));
        let expected: i64 = String::from_utf8_lossy(&output.stdout).trim().parse().unwrap();
        assert_eq!(owned, expected, "production fer differs on {source}");
    }
}

#[test]
fn upstream_lua_agrees_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA").ok().as_deref() == Some("1") {
        check("lua5.4");
    }
}
