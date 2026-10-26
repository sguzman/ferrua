//! Lua 5.4 reference comparison for the bounded register generic-for
//! iterator/state/control protocol, not unrestricted custom closures.
use ferrua::owned_vm::{
    Error, RegisterValue, compile_integer_chunk, execute_register_program,
    execute_register_program_with_strings, execute_register_program_with_strings_and_budget,
};
use std::process::Command;

fn run_scalar(chunk: &str) -> Result<RegisterValue, Error> {
    execute_register_program(&compile_integer_chunk(chunk)?)
}

#[test]
fn bounded_generic_for_pairs_and_next_match_lua54() {
    let reference = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (chunk, expected) in [
        ("local t={};local n=0;for k,v in pairs(t) do n=n+1 end return n", 0),
        ("local t={x=7};local n=0;for k,v in pairs(t) do n=n+v end return n", 7),
        ("local t={x=7,y=5};local n=0;for k,v in pairs(t) do n=n+v end return n", 12),
        ("local t={x=7,y=5};local n=0;for k in pairs(t) do n=n+1 end return n", 2),
        ("local t={[false]=7};local n=0;for k,v in pairs(t) do n=n+v end return n", 7),
        ("local t={x=3};local n=0;for k,v in next,t,nil do n=n+v end return n", 3),
        ("local t={x=4};local n=0;for k,v,w in pairs(t) do if w==nil then n=n+v end end return n", 4),
        ("local t={x=4};local n=0;for k,v in pairs(t) do n=n+v;break end return n", 4),
        ("local t={x=4};for k,v in pairs(t) do return v end return 0", 4),
        ("local t={x=3};local a=0;local b=0;for k,v in pairs(t) do a=a+v;b=b+1 end return a+b", 4),
        ("local t={x=2};local n=0;for k,v in pairs(t) do for p,w in pairs(t) do n=n+v*w end end return n", 4),
        ("local t={x=3};local n=0;for k,v in pairs(t) do if false then n=n+100 else n=n+v end end return n", 3),
        ("local t={x=2};local n=0;for k,v in pairs(t) do n=n+v end;for k,v in pairs(t) do n=n+v end return n", 4),
        ("local t={x=2};local n=0;for k,v in pairs(t) do if v==2 then break end n=n+100 end return n", 0),
        ("local t={x=2};local n=0;for k,v in pairs(t) do do n=n+v end end return n", 2),
        ("local t={x=2};local n=0;for k,v in pairs(t) do for i=1,2 do n=n+v end end return n", 4),
    ] {
        assert_eq!(
            run_scalar(chunk),
            Ok(RegisterValue::Integer(expected)),
            "{chunk}"
        );
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !reference {
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
            assert_eq!(
                output.stdout,
                expected.to_string().as_bytes(),
                "{executable}: {chunk}"
            );
        }
    }
}

#[test]
fn bounded_generic_for_rejects_bad_iterators_and_enforces_budget() {
    for chunk in [
        "for k in 3,nil,nil do return 1 end",
        "for k in nil,nil,nil do return 1 end",
        "for k in next,8,nil do return 1 end",
    ] {
        let program = compile_integer_chunk(chunk).unwrap();
        assert_eq!(
            execute_register_program_with_strings(&program),
            Err(Error::TypeError),
            "{chunk}"
        );
    }
    let program = compile_integer_chunk(
        "local t={};for i=1,60 do t[i]=i end local n=0;for k,v in pairs(t) do n=n+v end return n",
    )
    .unwrap();
    assert_eq!(
        execute_register_program_with_strings_and_budget(&program, 50),
        Err(Error::StepBudgetExceeded)
    );
}
