//! Differential smoke for the experimental owned integer register subset.
//! Reference is the existing production fer CLI, not upstream Lua directly.
use ferrua::owned_vm::{compile_integer_expression, execute_integer_program};
use std::process::Command;

#[test]
fn integer_register_subset_matches_production_interpreter() {
    let samples = [
        "1+2*3",
        "(4+6)*2",
        "10- -3",
        "0xF+1",
        "-(7-4)",
        "9223372036854775807+1",
    ];
    for source in samples {
        let program = compile_integer_expression(source).unwrap();
        let owned = execute_integer_program(&program).unwrap();
        let code = format!("print({source})");
        let output = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--lua=5.4", "-e", &code])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected: i64 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap();
        assert_eq!(owned, expected, "integer subset diverged: {source}");
    }
}

#[test]
fn integer_register_subset_matches_official_lua_54_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA").ok().as_deref() != Some("1") {
        return;
    }
    let version = Command::new("lua5.4").arg("-v").output().unwrap();
    assert!(version.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&version.stderr)
    );
    assert!(text.contains("Lua 5.4.6"), "unexpected reference: {text}");
    for source in [
        "1+2*3", "(4+6)*2", "10- -3", "0xF+1", "-(7-4)",
        "9223372036854775807+1", "(5*7)-3", "-(2+3)*6",
    ] {
        let program = compile_integer_expression(source).unwrap();
        let owned = execute_integer_program(&program).unwrap();
        let code = format!("print({source})");
        let reference = Command::new("lua5.4").args(["-e", &code]).output().unwrap();
        assert!(reference.status.success(), "{source}");
        let official: i64 = String::from_utf8_lossy(&reference.stdout).trim().parse().unwrap();
        assert_eq!(owned, official, "official Lua 5.4.6 disagrees on {source}");
    }
}
