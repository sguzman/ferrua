//! Nested-path acceptance matrix for Ferrua's experimental integer register VM.
//! Production fer remains pinned to its separate backend.

use ferrua::owned_vm::{Error, compile_integer_chunk, execute_integer_program};
use std::process::Command;

#[test]
fn nested_mutable_register_merges_match_lua54_and_production() {
    let compare_upstream = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        (
            "local x=1; if true then if false then x=4 else x=9 end end; return x",
            9,
        ),
        (
            "local x=1; if false then if true then x=6 end else x=x+8 end; return x",
            9,
        ),
        (
            "local x=1; if true then do local x=5; x=x+1 end; x=x+2 end; return x",
            3,
        ),
        (
            "local x=1; if false then x=6 elseif true then do x=x+5 end end; return x",
            6,
        ),
        (
            "local a,b=2,3; if true then a,b=b,a; if false then a=9 else b=b+4 end end; return a*10+b",
            36,
        ),
        (
            "local x=1; if true then x=3; if x==3 then x=x+4 end end; return x",
            7,
        ),
        (
            "local x=1; if false then x=9 elseif x==1 then x=4 end; return x",
            4,
        ),
        (
            "local x=1; if true then x=9 elseif (1//0>0) then x=3 end; return x",
            9,
        ),
        (
            "local x=1; do local x=10; if false then x=19 else x=x+2 end end; return x",
            1,
        ),
        (
            "local x=1; do local x=10; if true then x=19 end end; return x",
            1,
        ),
        (
            "local x=1; if true then x=2; if false then return 99 end else return 7 end; return x",
            2,
        ),
        (
            "local x=1; if false then x=8; if true then return 99 end elseif true then x=3 end; return x",
            3,
        ),
        (
            "local x=1; if true then if false then return 9 else x=8 end else x=7 end; return x",
            8,
        ),
        (
            "local x=1; if not false then if 0 then x=x+4 end end; return x",
            5,
        ),
        (
            "local x=1; if false then x=9 else if true then x=x+2 else x=8 end end; return x",
            3,
        ),
        (
            "local x=1; if false then x=4 elseif false then x=5 end; return x",
            1,
        ),
        (
            "local x=1; if true then x=2 elseif true then x=3 else x=4 end; return x",
            2,
        ),
        ("local x=1; do if true then x=5 end end; return x", 5),
        (
            "local a,b=1,2; if true then local a=9; b=a+b end; return a*10+b",
            21,
        ),
        (
            "local a,b=2,3; if false then a,b=b,a else a=a+b; b=a-b end; return a*10+b",
            52,
        ),
        (
            "local x=1; if true then local x=5; if false then x=9 end end; return x",
            1,
        ),
        (
            "local x=2; if true and not false then x=x*4 end; return x",
            8,
        ),
        ("local x=2; if false or (3<4) then x=x+6 end; return x", 8),
        (
            "local x=5; if true then x=6 end; if false then x=8 elseif x==6 then x=x+2 end; return x",
            8,
        ),
    ] {
        let owned = compile_integer_chunk(source).expect(source);
        assert_eq!(execute_integer_program(&owned), Ok(expected), "{source}");
        let code = format!("print((function() {source} end)())");
        for binary in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if binary == "lua5.4" && !compare_upstream {
                continue;
            }
            let mut command = Command::new(binary);
            if binary != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command.args(["-e", &code]).output().expect(binary);
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
fn sparse_conditional_updates_do_not_exhaust_registers() {
    let declarations = (0..70)
        .map(|index| format!("local v{index}=1; "))
        .collect::<String>();
    let no_updates = format!(
        "{declarations}{}return v0",
        "if true then local temporary=2 end; ".repeat(300)
    );
    let program = compile_integer_chunk(&no_updates).unwrap();
    assert_eq!(execute_integer_program(&program), Ok(1));

    let sparse_updates = format!(
        "{declarations}{}return v0",
        "if false then v0=3 end; ".repeat(300)
    );
    let program = compile_integer_chunk(&sparse_updates).unwrap();
    assert_eq!(execute_integer_program(&program), Ok(1));
}

#[test]
fn branch_merges_preserve_errors_and_constant_local_restrictions() {
    for source in [
        "local x<const>=2; if true then x=3 end; return x",
        "local x<const>=2; if false then x=3 end; return x",
        "local x<const>=2; if true then do x=3 end end; return x",
        "local x<const>=2; if true then local y=3; x=y end; return x",
    ] {
        assert!(compile_integer_chunk(source).is_err(), "{source}");
    }
    let skipped =
        compile_integer_chunk("local x=1; if false then x=1//0 else x=x+2 end; return x").unwrap();
    assert_eq!(execute_integer_program(&skipped), Ok(3));
    let selected =
        compile_integer_chunk("local x=1; if true then x=1//0 else x=x+2 end; return x").unwrap();
    assert_eq!(
        execute_integer_program(&selected),
        Err(Error::DivisionByZero)
    );
}

#[test]
fn generated_nested_branch_joins_match_reference_interpreters() {
    let compare_upstream = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    // Fixed arithmetic seeds make the corpus reproducible without rand.
    // Every case forces a different combination of taken/skipped sibling
    // branches, shadowed locals, and mutable enclosing register joins.
    for seed in 0_i64..48 {
        let a = seed % 11 - 4;
        let b = (seed * 3) % 13 - 5;
        let c = (seed * 7) % 17 - 8;
        let limit_a = seed % 9 - 3;
        let limit_b = (seed * 2) % 11 - 4;
        let plus = seed % 5 + 1;
        let minus = seed % 4 + 1;
        let threshold = seed % 7 - 3;
        let source = format!(
            "local a={a}; local b={b}; local c={c}; \
             if a<{limit_a} then \
                if b>{limit_b} then a=a+{plus}; b=b+a else b=b-{minus}; a=a+b end; \
                if c%2==0 then c=c+a else c=c-b end \
             elseif b<{limit_b} then \
                local b=a+1; c=c+b; \
                if a>0 then a=a-b else c=c+a end \
             else \
                a=a-{minus}; b=b+{plus} \
             end; \
             if not (a<b) or c>{threshold} then a=a+b else b=b+c end; \
             return a*10000+b*100+c"
        );
        let program = compile_integer_chunk(&source)
            .unwrap_or_else(|error| panic!("seed {seed}: compile error {error}: {source}"));
        let actual = execute_integer_program(&program)
            .unwrap_or_else(|error| panic!("seed {seed}: execution error {error}: {source}"));
        let lua_code = format!("print((function() {source} end)())");
        for binary in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if binary == "lua5.4" && !compare_upstream {
                continue;
            }
            let mut command = Command::new(binary);
            if binary != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command
                .args(["-e", &lua_code])
                .output()
                .unwrap_or_else(|error| panic!("seed {seed}: {binary}: {error}"));
            assert!(
                output.status.success(),
                "seed {seed} ({binary}): {source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let expected = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<i64>()
                .unwrap_or_else(|error| panic!("seed {seed} ({binary}): {error}: {source}"));
            assert_eq!(
                actual, expected,
                "seed {seed}: register VM vs {binary}: {source}"
            );
        }
    }
}

#[test]
fn generated_nested_guard_failures_match_reference_interpreters() {
    let compare_upstream = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    // Each fixed seed changes which potentially failing expression is
    // selected. Skipped Lua arms must never evaluate integer division by zero.
    let mut observed_success = 0;
    let mut observed_failure = 0;
    for seed in 0_i64..48 {
        let x = (seed * 3) % 15 - 6;
        let y = (seed * 7) % 13 - 6;
        let cutoff = seed % 9 - 4;
        let add = seed % 5 + 1;
        let source = format!(
            "local x={x}; local y={y}; \
             if x<{cutoff} then \
                if y%2==0 or (1//0)>0 then x=x+{add} else x=x-1 end \
             elseif x>{cutoff} then \
                if y%3==0 then x=1//0 else x=x+y end \
             else \
                if false and (1//0)>0 then x=0 else x=x*2 end \
             end; \
             return x"
        );
        let program = compile_integer_chunk(&source)
            .unwrap_or_else(|error| panic!("seed {seed}: compilation error {error}: {source}"));
        let owned = execute_integer_program(&program);
        if owned.is_ok() {
            observed_success += 1;
        } else {
            observed_failure += 1;
            assert_eq!(owned, Err(Error::DivisionByZero), "seed {seed}: {source}");
        }

        let lua_code = format!("print((function() {source} end)())");
        for binary in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if binary == "lua5.4" && !compare_upstream {
                continue;
            }
            let mut command = Command::new(binary);
            if binary != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command
                .args(["-e", &lua_code])
                .output()
                .unwrap_or_else(|error| panic!("seed {seed}: {binary}: {error}"));
            if output.status.success() {
                let reference = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<i64>()
                    .unwrap_or_else(|error| panic!("seed {seed}: {binary}: {error}"));
                assert_eq!(owned, Ok(reference), "seed {seed}: {binary}: {source}");
            } else {
                let error = String::from_utf8_lossy(&output.stderr);
                assert!(
                    error.to_lowercase().contains("zero"),
                    "seed {seed}: unexpected {binary} failure for {source}: {error}"
                );
                assert_eq!(
                    owned,
                    Err(Error::DivisionByZero),
                    "seed {seed}: {binary}: {source}"
                );
            }
        }
    }
    assert!(observed_success > 0, "all generated paths failed");
    assert!(observed_failure > 0, "no selected guard raised its error");
}
