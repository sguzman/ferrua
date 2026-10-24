//! Differential smoke for the experimental owned integer register subset.
//! Reference is the existing production fer CLI, not upstream Lua directly.
use ferrua::owned_vm::{
    compile_integer_chunk, compile_integer_expression, execute_integer_program,
};
use std::process::Command;

#[test]
fn integer_register_subset_matches_production_interpreter() {
    let samples = [
        "1+2*3",
        "(4+6)*2",
        "10- -3",
        "0xF+1",
        "0xffffffffffffffff",
        "0x8000000000000000",
        "0x8000000000000000>>63",
        "-(7-4)",
        "9223372036854775807+1",
        "-7//3",
        "-7%3",
        "7%-3",
        "5&3",
        "5|2",
        "5~3",
        "~0",
        "1<<63",
        "8<<-1",
        "-1>>1",
        "1<<64",
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
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        != Some("1")
    {
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
        "1+2*3",
        "(4+6)*2",
        "10- -3",
        "0xF+1",
        "0xffffffffffffffff",
        "0x8000000000000000",
        "0x8000000000000000>>63",
        "-(7-4)",
        "9223372036854775807+1",
        "-7//3",
        "-7%3",
        "7%-3",
        "5&3",
        "5|2",
        "5~3",
        "~0",
        "1<<63",
        "8<<-1",
        "-1>>1",
        "1<<64",
        "(5*7)-3",
        "-(2+3)*6",
    ] {
        let program = compile_integer_expression(source).unwrap();
        let owned = execute_integer_program(&program).unwrap();
        let code = format!("print({source})");
        let reference = Command::new("lua5.4").args(["-e", &code]).output().unwrap();
        assert!(reference.status.success(), "{source}");
        let official: i64 = String::from_utf8_lossy(&reference.stdout)
            .trim()
            .parse()
            .unwrap();
        assert_eq!(owned, official, "official Lua 5.4.6 disagrees on {source}");
    }
}

#[test]
fn static_integer_chunk_matches_production_fer() {
    let samples = [
        "local a,b=3,4; return a*b",
        "local x=4; local y=x*2; x=y-3; return x",
        "local a=1; local b=2; a,b=b,a; return a*10+b",
        "local x=10; do local x=x+5; x=x*2 end; return x",
        "local x=10; do x=x+5 end; return x",
        "local x=1; local x=x+1; return x",
    ];
    for source in samples {
        let bytecode = compile_integer_chunk(source).unwrap();
        let actual = execute_integer_program(&bytecode).unwrap();
        let lua = format!("print((function() {source} end)())");
        let result = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--lua=5.4", "-e", &lua])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let expected: i64 = String::from_utf8_lossy(&result.stdout)
            .trim()
            .parse()
            .unwrap();
        assert_eq!(actual, expected, "chunk mismatch: {source}");
    }
}

#[test]
fn integer_conditional_branches_match_fer_and_upstream_lua() {
    let compare_upstream = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("if true then return 7 else return 11 end", 7),
        ("if false then return 7 else return 11 end", 11),
        ("if 4<5 then return 9 else return 2 end", 9),
        ("if 4>5 then return 9 else return 2 end", 2),
        ("if 4<=4 then return 8 else return 1 end", 8),
        ("if 4>=5 then return 8 else return 1 end", 1),
        ("if 4==4 then return 8 else return 1 end", 8),
        ("if 4~=4 then return 8 else return 1 end", 1),
        (
            "if 1>2 then return 1 elseif 3<4 then return 17 else return 9 end",
            17,
        ),
        (
            "if 1>2 then return 1 elseif 4<3 then return 17 else return 9 end",
            9,
        ),
        (
            "local x=5; if x>3 then local y=x+2; return y else return 0 end",
            7,
        ),
        (
            "local x=5; if x<3 then return 0 else do return x+9 end end",
            14,
        ),
        ("if true then do return 8 end else return 9 end", 8),
        ("if (4*2)==8 then return (5<<2)|3 else return 0 end", 23),
        (
            "if 0==0 then if 3>=2 then return 19 else return 2 end else return 7 end",
            19,
        ),
    ] {
        let bytecode = compile_integer_chunk(source).unwrap();
        assert_eq!(execute_integer_program(&bytecode), Ok(expected), "{source}");
        let code = format!("print((function() {source} end)())");
        let production = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--lua=5.4", "-e", &code])
            .output()
            .unwrap();
        assert!(
            production.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&production.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&production.stdout).trim(),
            expected.to_string(),
            "{source}"
        );
        if compare_upstream {
            let upstream = Command::new("lua5.4").args(["-e", &code]).output().unwrap();
            assert!(
                upstream.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&upstream.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&upstream.stdout).trim(),
                expected.to_string(),
                "{source}"
            );
        }
    }
}

#[test]
fn integer_register_guard_clauses_match_fer_and_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("if true then return 7 end; return 11", 7),
        ("if false then return 7 end; return 11", 11),
        ("if 0 then return 8 end; return 4", 8),
        ("if not 0 then return 8 end; return 4", 4),
        ("local x=3; if x>2 then return x+5 end; return 0", 8),
        ("local x=3; if x<2 then return x+5 end; return x*3", 9),
        ("if false then return 1 elseif true then return 13 end; return 9", 13),
        ("if false then return 1 elseif false then return 13 end; return 9", 9),
        ("local x=2; if x>3 then return 1 elseif x==2 then return 17 end; return 0", 17),
        ("if false and (1//0>0) then return 6 end; return 12", 12),
        ("if true or (1//0>0) then return 15 end; return 0", 15),
        ("if not (2<1) then do return 21 end end; return 9", 21),
        ("do if false then return 2 end end; return 19", 19),
        ("local x=7; do if x<5 then return 2 end end; return x+4", 11),
        ("local x=0; if x==1 then return 3 end; x=6; return x+2", 8),
        ("if false then return 3 end; if true then return 24 end; return 5", 24),
        ("if false then return 3 end; if false then return 24 end; return 5", 5),
        ("local x=5; if x==5 then local y=3; return x*y end; return 0", 15),
    ] {
        let program = compile_integer_chunk(source).unwrap();
        assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        let code = format!("print((function() {source} end)())");
        for binary in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if binary == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(binary);
            if binary != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command.args(["-e", &code]).output().unwrap();
            assert!(
                output.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                expected.to_string(),
                "{source}"
            );
        }
    }
}

#[test]
fn numeric_truthiness_guard_matrix_matches_production_and_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("if 0 then return 7 else return 11 end", 7),
        ("if -1 then return 9 else return 3 end", 9),
        ("local x=0; if x then return 5 else return 6 end", 5),
        ("local x=9; if x-x then return 4 else return 8 end", 4),
        ("if not 0 then return 4 else return 8 end", 8),
        ("if not false then return 3 else return 6 end", 3),
        ("if not true then return 3 else return 6 end", 6),
        ("if not not 0 then return 15 else return 3 end", 15),
        ("if not (2>1) then return 0 else return 11 end", 11),
        ("if not (2<1) then return 9 else return 0 end", 9),
        ("if (4-4) then return 22 else return 0 end", 22),
        (
            "if false then return 3 elseif not (5<=4) then return 13 else return 2 end",
            13,
        ),
    ] {
        let program = compile_integer_chunk(source).unwrap();
        assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        let code = format!("print((function() {source} end)())");
        for binary in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if binary == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(binary);
            if binary != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command.args(["-e", &code]).output().unwrap();
            assert!(
                output.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                expected.to_string(),
                "{source}"
            );
        }
    }
}

#[test]
fn register_short_circuit_conditions_match_lua_reference() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("if false and (1//0>0) then return 4 else return 9 end", 9),
        ("if true or (1//0>0) then return 5 else return 9 end", 5),
        ("if true and 0 then return 6 else return 9 end", 6),
        ("if false or 0 then return 7 else return 9 end", 7),
        ("if false or false then return 7 else return 9 end", 9),
        ("if true and false then return 7 else return 9 end", 9),
        (
            "if false or true and true then return 8 else return 0 end",
            8,
        ),
        (
            "if true and false or true then return 13 else return 0 end",
            13,
        ),
        ("if (1<2) and (3>2) then return 12 else return 0 end", 12),
        ("if (1>2) or (3<2) then return 12 else return 2 end", 2),
        (
            "if not (false or false) then return 15 else return 0 end",
            15,
        ),
        (
            "local x=0; if (x or false) and not false then return 22 else return 0 end",
            22,
        ),
        (
            "if (false and (1//0>0)) or true then return 17 else return 0 end",
            17,
        ),
        (
            "if true or false and false then return 19 else return 0 end",
            19,
        ),
    ] {
        let program = compile_integer_chunk(source).unwrap();
        assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        let code = format!("print((function() {source} end)())");
        for interpreter in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if interpreter == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(interpreter);
            if interpreter != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command.args(["-e", &code]).output().unwrap();
            assert!(
                output.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                expected.to_string(),
                "{source}"
            );
        }
    }
}

#[test]
fn static_integer_chunk_matches_upstream_lua_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    for source in [
        "local a,b=3,4; return a*b",
        "local x=4; local y=x*2; x=y-3; return x",
        "local a=1; local b=2; a,b=b,a; return a*10+b",
        "local x=10; do local x=x+5; x=x*2 end; return x",
        "local x=10; do x=x+5 end; return x",
        "local x=1; local x=x+1; return x",
    ] {
        let bytecode = compile_integer_chunk(source).unwrap();
        let actual = execute_integer_program(&bytecode).unwrap();
        let lua = format!("print((function() {source} end)())");
        let result = Command::new("lua5.4").args(["-e", &lua]).output().unwrap();
        assert!(
            result.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let expected: i64 = String::from_utf8_lossy(&result.stdout)
            .trim()
            .parse()
            .unwrap();
        assert_eq!(actual, expected, "upstream chunk mismatch: {source}");
    }
}

#[test]
fn nested_integer_returns_match_production_and_upstream_lua() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("do return 7 end", 7),
        ("local x=3; do return x+4 end", 7),
        ("do do return 11 end end", 11),
        ("local a=2; do local b=a*3; return b+1 end", 7),
        ("local n=1; do n=n+9; do return n end end", 10),
        ("do return (7<<2)|3 end", 31),
        ("do return 8 end; local unused=9; return unused", 8),
        ("local x=5; do return x end; x=0; return x", 5),
        ("do local x=4 end; return 6", 6),
        ("do local x=3; do local x=x+9; return x end end", 12),
    ] {
        let program = compile_integer_chunk(source).unwrap();
        assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        let lua = format!("print((function() {source} end)())");
        for (interpreter, args) in [
            (
                env!("CARGO_BIN_EXE_fer"),
                vec!["--lua=5.4", "-e", lua.as_str()],
            ),
            ("lua5.4", vec!["-e", lua.as_str()]),
        ] {
            if interpreter == "lua5.4" && !compare_lua {
                continue;
            }
            let output = Command::new(interpreter).args(args).output().unwrap();
            assert!(
                output.status.success(),
                "{interpreter} rejected {source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let actual: i64 = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse()
                .unwrap();
            assert_eq!(actual, expected, "{interpreter} disagrees: {source}");
        }
    }
}

#[test]
fn deterministic_integer_operator_matrix_matches_lua_54_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    // Stable generated corpus with both signs, negative shifts, zero, and
    // out-of-range shift counts. All cases are integer-only; never floats.
    let mut seed = 0x1234_5678_9abc_def0_u64;
    let mut sources = Vec::new();
    let mut expected = Vec::new();
    let mut script = String::new();
    for _ in 0..128 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let a = (seed % 2001) as i64 - 1000;
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let b = (seed % 2001) as i64 - 1000;
        for operator in ["+", "-", "*", "//", "%", "&", "|", "~", "<<", ">>"] {
            if b == 0 && matches!(operator, "//" | "%") {
                continue;
            }
            let source = format!("({a}){operator}({b})");
            let bytecode = compile_integer_expression(&source).unwrap();
            let actual = execute_integer_program(&bytecode).unwrap();
            script.push_str(&format!("print({source})\n"));
            sources.push(source);
            expected.push(actual);
        }
    }
    let output = Command::new("lua5.4")
        .args(["-e", &script])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Lua reference failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        expected.len(),
        "reference result count mismatch"
    );
    for (index, line) in lines.iter().enumerate() {
        let official: i64 = line.parse().unwrap();
        assert_eq!(
            expected[index], official,
            "Lua 5.4 diverged: {}",
            sources[index]
        );
    }
}

#[test]
fn division_by_zero_matches_upstream_error_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    for source in ["1//0", "1%0"] {
        let bytecode = compile_integer_expression(source).unwrap();
        assert!(execute_integer_program(&bytecode).is_err(), "{source}");
        let code = format!("print({source})");
        let reference = Command::new("lua5.4").args(["-e", &code]).output().unwrap();
        assert!(
            !reference.status.success(),
            "upstream Lua accepted {source}"
        );
    }
}
