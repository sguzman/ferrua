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
