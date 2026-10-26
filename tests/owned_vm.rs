//! Differential smoke for the experimental owned integer register subset.
//! Reference is the existing production fer CLI, not upstream Lua directly.
use ferrua::owned_vm::{
    RegisterValue, compile_integer_chunk, compile_integer_expression, execute_integer_program,
    execute_register_program,
};
use std::process::Command;

#[test]
fn tagged_register_decimal_floats_and_mixed_number_order_match_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("1.5", RegisterValue::Float(1.5_f64.to_bits())),
        ("1e2", RegisterValue::Float(100.0_f64.to_bits())),
        ("-0.0", RegisterValue::Float((-0.0_f64).to_bits())),
        ("1.25+2.5", RegisterValue::Float(3.75_f64.to_bits())),
        ("1/2", RegisterValue::Float(0.5_f64.to_bits())),
        ("4/2", RegisterValue::Float(2.0_f64.to_bits())),
        ("2.0/4", RegisterValue::Float(0.5_f64.to_bits())),
        ("2^3", RegisterValue::Float(8.0_f64.to_bits())),
        ("7.0//2", RegisterValue::Float(3.0_f64.to_bits())),
        ("7//2.0", RegisterValue::Float(3.0_f64.to_bits())),
        ("-7.0//2", RegisterValue::Float((-4.0_f64).to_bits())),
        ("7.5//-2", RegisterValue::Float((-4.0_f64).to_bits())),
        ("-0.0//3", RegisterValue::Float((-0.0_f64).to_bits())),
        ("7.5%2", RegisterValue::Float(1.5_f64.to_bits())),
        ("-7.5%2", RegisterValue::Float(0.5_f64.to_bits())),
        ("7.5%-2", RegisterValue::Float((-0.5_f64).to_bits())),
        ("-7.5%-2", RegisterValue::Float((-1.5_f64).to_bits())),
        ("-0.0%3", RegisterValue::Float((-0.0_f64).to_bits())),
        ("9%2.0", RegisterValue::Float(1.0_f64.to_bits())),
        ("2^-2", RegisterValue::Float(0.25_f64.to_bits())),
        ("9.0^0.5", RegisterValue::Float(3.0_f64.to_bits())),
        ("1+2.5", RegisterValue::Float(3.5_f64.to_bits())),
        ("7.5-2", RegisterValue::Float(5.5_f64.to_bits())),
        ("1.25*4", RegisterValue::Float(5.0_f64.to_bits())),
        ("0.0 or 8", RegisterValue::Float(0.0_f64.to_bits())),
        ("1.0==1", RegisterValue::Boolean(true)),
        ("-0.0==0.0", RegisterValue::Boolean(true)),
        ("1.5<2", RegisterValue::Boolean(true)),
        ("2<=2.5", RegisterValue::Boolean(true)),
        ("2.5>2", RegisterValue::Boolean(true)),
        ("2.0>=2", RegisterValue::Boolean(true)),
        ("not 0.0", RegisterValue::Boolean(false)),
        (
            "9007199254740993==9007199254740992.0",
            RegisterValue::Boolean(false),
        ),
        (
            "9223372036854775807<9223372036854775808.0",
            RegisterValue::Boolean(true),
        ),
    ] {
        let expression = compile_integer_expression(source).unwrap();
        assert_eq!(
            execute_register_program(&expression),
            Ok(expected),
            "{source}"
        );
        let chunk = compile_integer_chunk(&format!("return {source}")).unwrap();
        assert_eq!(execute_register_program(&chunk), Ok(expected), "{source}");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let mut cmd = Command::new(executable);
            if executable != "lua5.4" {
                cmd.arg("--lua=5.4");
            }
            let code = format!("local x=({source}); print(type(x),x)");
            let result = cmd.args(["-e", &code]).output().unwrap();
            assert!(
                result.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let output = String::from_utf8_lossy(&result.stdout);
            let fields = output.trim().split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 2, "{source}: {output}");
            match expected {
                RegisterValue::Float(bits) => {
                    assert_eq!(fields[0], "number", "{source}");
                    let actual: f64 = fields[1].parse().unwrap();
                    assert_eq!(actual.to_bits(), bits, "{source}");
                }
                RegisterValue::Boolean(value) => {
                    assert_eq!(fields[0], "boolean", "{source}");
                    assert_eq!(fields[1], value.to_string(), "{source}");
                }
                _ => unreachable!("float/boolean reference only"),
            }
        }
    }
    for source in [
        "1.5+true",
        "nil*0.5",
        "false<1.5",
        "1.0//true",
        "nil%2.0",
    ] {
        let program = compile_integer_expression(source).unwrap();
        assert_eq!(
            execute_register_program(&program),
            Err(ferrua::owned_vm::Error::TypeError),
            "{source}"
        );
    }
}

#[test]
fn tagged_register_and_or_return_selected_operands_like_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected, rendered) in [
        ("true and 7", RegisterValue::Integer(7), "7"),
        ("false and 7", RegisterValue::Boolean(false), "false"),
        ("nil and 5", RegisterValue::Nil, "nil"),
        ("0 and false", RegisterValue::Boolean(false), "false"),
        ("false or 3", RegisterValue::Integer(3), "3"),
        ("nil or 4", RegisterValue::Integer(4), "4"),
        ("0 or 7", RegisterValue::Integer(0), "0"),
        ("true or 12", RegisterValue::Boolean(true), "true"),
        ("false and (1//0)", RegisterValue::Boolean(false), "false"),
        ("true or (1//0)", RegisterValue::Boolean(true), "true"),
        ("false or nil", RegisterValue::Nil, "nil"),
        ("true and false or 9", RegisterValue::Integer(9), "9"),
        ("(3<4) and (9>=7)", RegisterValue::Boolean(true), "true"),
        ("nil or false or 7", RegisterValue::Integer(7), "7"),
        ("(5==5) and 42", RegisterValue::Integer(42), "42"),
    ] {
        let compiled = compile_integer_expression(source).unwrap();
        assert_eq!(
            execute_register_program(&compiled),
            Ok(expected),
            "{source}"
        );
        let chunk = compile_integer_chunk(&format!("return {source}")).unwrap();
        assert_eq!(execute_register_program(&chunk), Ok(expected), "{source}");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command
                .args(["-e", &format!("print({source})")])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                rendered,
                "{source}"
            );
        }
    }
    for source in ["true and (1//0)", "false or (1//0)"] {
        let program = compile_integer_expression(source).unwrap();
        assert_eq!(
            execute_register_program(&program),
            Err(ferrua::owned_vm::Error::DivisionByZero),
            "{source}"
        );
    }
}

#[test]
fn tagged_register_boolean_expressions_match_production_and_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("not false", RegisterValue::Boolean(true)),
        ("not true", RegisterValue::Boolean(false)),
        ("not nil", RegisterValue::Boolean(true)),
        ("not 0", RegisterValue::Boolean(false)),
        ("not (2-2)", RegisterValue::Boolean(false)),
        ("true==true", RegisterValue::Boolean(true)),
        ("true==false", RegisterValue::Boolean(false)),
        ("nil==nil", RegisterValue::Boolean(true)),
        ("nil==false", RegisterValue::Boolean(false)),
        ("nil~=false", RegisterValue::Boolean(true)),
        ("0==false", RegisterValue::Boolean(false)),
        ("0~=false", RegisterValue::Boolean(true)),
        ("4<5", RegisterValue::Boolean(true)),
        ("5<=5", RegisterValue::Boolean(true)),
        ("5>4", RegisterValue::Boolean(true)),
        ("4>=5", RegisterValue::Boolean(false)),
        ("(1+2)==3", RegisterValue::Boolean(true)),
        ("not (1==2)", RegisterValue::Boolean(true)),
        ("not (4<5)", RegisterValue::Boolean(false)),
    ] {
        let program = compile_integer_expression(source).unwrap();
        assert_eq!(execute_register_program(&program), Ok(expected), "{source}");
        let code = format!("print({source})");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command.args(["-e", &code]).output().unwrap();
            assert!(
                output.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let reference = match expected {
                RegisterValue::Boolean(true) => "true",
                RegisterValue::Boolean(false) => "false",
                _ => unreachable!("Boolean cases only"),
            };
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                reference,
                "{source}"
            );
        }
    }
    for source in ["true<false", "nil<0", "1<true"] {
        let program = compile_integer_expression(source).unwrap();
        assert_eq!(
            execute_register_program(&program),
            Err(ferrua::owned_vm::Error::TypeError),
            "{source}"
        );
    }
}

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
fn register_literal_truthiness_matches_lua54_without_general_float_or_string_values() {
    let compare_upstream = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("if nil then return 1 else return 7 end", 7),
        ("if not nil then return 9 else return 1 end", 9),
        ("if '' then return 4 else return 1 end", 4),
        ("if [[hello]] then return 6 else return 1 end", 6),
        ("if 0.0 then return 5 else return 1 end", 5),
        ("if .5 then return 8 else return 1 end", 8),
        ("if 0x1p+1 then return 11 else return 1 end", 11),
        ("if nil or 'hello' then return 13 else return 1 end", 13),
        ("if false or 0.0 then return 15 else return 1 end", 15),
        ("if nil and 1//0==1 then return 1 else return 17 end", 17),
        ("if not (nil or false) then return 19 else return 1 end", 19),
        ("local x=0; while nil do x=x+1 end; return x", 0),
        ("local x=0; repeat x=x+1 until 'nonempty'; return x", 1),
        ("local x=3; if not '' then x=99 end; return x", 3),
    ] {
        let owned = compile_integer_chunk(source)
            .unwrap_or_else(|error| panic!("register guard rejected {source}: {error}"));
        assert_eq!(execute_integer_program(&owned), Ok(expected), "{source}");
        let code = format!("print((function() {source} end)())");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_upstream {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
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
    // Strings still require a real Lua string value representation.
    for source in ["return 'hello'", "local x='hello'; return x"] {
        assert!(compile_integer_chunk(source).is_err(), "{source}");
    }
    // Floats are now first-class tagged results, but the legacy integer
    // compatibility API must still reject every noninteger return.
    for (source, expected) in [("return 0.0", 0.0_f64), ("return 4 + 0.5", 4.5_f64)] {
        let chunk = compile_integer_chunk(source).unwrap();
        assert_eq!(
            ferrua::owned_vm::execute_register_program(&chunk),
            Ok(ferrua::owned_vm::RegisterValue::Float(expected.to_bits())),
            "{source}"
        );
        assert_eq!(
            execute_integer_program(&chunk),
            Err(ferrua::owned_vm::Error::TypeError),
            "{source}"
        );
    }
}

#[test]
fn register_boolean_nil_literal_equality_matches_lua54_in_control_flow() {
    let compare_upstream = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("if nil==nil then return 7 else return 0 end", 7),
        ("if nil~=nil then return 7 else return 9 end", 9),
        ("if true==true then return 11 else return 0 end", 11),
        ("if true==false then return 0 else return 13 end", 13),
        ("if false~=nil then return 17 else return 0 end", 17),
        ("if true~=nil then return 19 else return 0 end", 19),
        ("if (nil)==(nil) then return 23 else return 0 end", 23),
        (
            "if (true==false) or (nil==nil) then return 29 else return 0 end",
            29,
        ),
        (
            "if (nil~=false) and 4<5 then return 31 else return 0 end",
            31,
        ),
        (
            "local n=0; while (false==false) and n<3 do n=n+1 end; return n",
            3,
        ),
        ("local n=0; repeat n=n+1 until (nil~=false); return n", 1),
    ] {
        let program = compile_integer_chunk(source)
            .unwrap_or_else(|error| panic!("register guard rejected {source}: {error}"));
        assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        let code = format!("print((function() {source} end)())");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_upstream {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
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
    // Ordering and arithmetic remain type-checked at execution time.
    for source in [
        "if nil<false then return 1 else return 2 end",
        "local x=true; return x+1",
        "local x=1; if x<false then return 1 else return 2 end",
    ] {
        let program = compile_integer_chunk(source).unwrap();
        assert_eq!(
            ferrua::owned_vm::execute_register_program(&program),
            Err(ferrua::owned_vm::Error::TypeError),
            "{source}"
        );
    }
    let equality = compile_integer_chunk("return nil==nil").unwrap();
    assert_eq!(
        ferrua::owned_vm::execute_register_program(&equality),
        Ok(ferrua::owned_vm::RegisterValue::Boolean(true))
    );
}

#[test]
fn tagged_nil_boolean_integer_register_returns_match_lua54() {
    use ferrua::owned_vm::{RegisterValue, execute_register_program};
    let compare_upstream = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected, reference) in [
        ("return nil", RegisterValue::Nil, "nil\tnil"),
        ("return true", RegisterValue::Boolean(true), "boolean\ttrue"),
        (
            "return false",
            RegisterValue::Boolean(false),
            "boolean\tfalse",
        ),
        ("return 0", RegisterValue::Integer(0), "number\t0"),
        (
            "local x=nil; if x then return 1 else return 7 end",
            RegisterValue::Integer(7),
            "number\t7",
        ),
        (
            "local x=false; if not x then return 9 else return 1 end",
            RegisterValue::Integer(9),
            "number\t9",
        ),
        (
            "local x=0; if x then return 11 else return 0 end",
            RegisterValue::Integer(11),
            "number\t11",
        ),
        (
            "local x=true; if x==true then return 13 else return 0 end",
            RegisterValue::Integer(13),
            "number\t13",
        ),
        (
            "local x=nil; if x==nil then return 17 else return 0 end",
            RegisterValue::Integer(17),
            "number\t17",
        ),
        (
            "local x=true; x=false; return x",
            RegisterValue::Boolean(false),
            "boolean\tfalse",
        ),
        (
            "local x=3; if x==true then return 0 else return 19 end",
            RegisterValue::Integer(19),
            "number\t19",
        ),
        (
            "local x=false; if x==0 then return 0 else return 23 end",
            RegisterValue::Integer(23),
            "number\t23",
        ),
        (
            "local x=nil; if x==false then return 0 else return 29 end",
            RegisterValue::Integer(29),
            "number\t29",
        ),
        (
            "local x=1; local y=true; x,y=2,false; return y",
            RegisterValue::Boolean(false),
            "boolean\tfalse",
        ),
        (
            "local x=true; do local x=nil; if x then return 0 else return 31 end end; return 0",
            RegisterValue::Integer(31),
            "number\t31",
        ),
        (
            "local x=false; if x then return 0 else x=true end; if x then return 37 else return 0 end",
            RegisterValue::Integer(37),
            "number\t37",
        ),
        (
            "local again=true; local n=0; while again do n=n+1; if n==3 then again=false end end; return n",
            RegisterValue::Integer(3),
            "number\t3",
        ),
    ] {
        let program = compile_integer_chunk(source)
            .unwrap_or_else(|error| panic!("tagged register compile rejected {source}: {error}"));
        assert_eq!(execute_register_program(&program), Ok(expected), "{source}");
        let code = format!(
            "local result=(function() {source} end)(); print(type(result), tostring(result))"
        );
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_upstream {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
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
                reference,
                "{source}"
            );
        }
    }

    let boolean_return = compile_integer_chunk("return true").unwrap();
    assert_eq!(
        execute_integer_program(&boolean_return),
        Err(ferrua::owned_vm::Error::TypeError)
    );
}

#[test]
fn register_while_loops_match_production_and_upstream_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("local n=0; while n<5 do n=n+1 end; return n", 5),
        (
            "local n=1; local i=0; while i<4 do n=n*2; i=i+1 end; return n",
            16,
        ),
        (
            "local n=0; while n<3 do local i=0; while i<2 do n=n+1; i=i+1 end end; return n",
            4,
        ),
        (
            "local x=0; while x<5 do if x<2 then x=x+2 else x=x+1 end end; return x",
            5,
        ),
        ("local n=0; while false do n=n+1 end; return n", 0),
        (
            "local n=9223372036854775807; while n>0 do n=n+1 end; return n",
            i64::MIN,
        ),
    ] {
        let program = compile_integer_chunk(source).unwrap();
        assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        let code = format!("print((function() {source} end)())");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let result = command.args(["-e", &code]).output().unwrap();
            assert!(
                result.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&result.stdout).trim(),
                expected.to_string(),
                "{source}"
            );
        }
    }
}

#[test]
fn register_break_exits_match_production_and_upstream_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        (
            "local x=1; while true do x=x+3; break; x=100 end; return x",
            4,
        ),
        (
            "local x=0; while x<9 do x=x+1; if x==4 then break end end; return x",
            4,
        ),
        (
            "local x=0; repeat x=x+1; if x==3 then break end until x>99; return x",
            3,
        ),
        (
            "local x=0; repeat local i=x+1; x=i; if i>2 then break end until x>99; return x",
            3,
        ),
        (
            "local a=0; local b=0; while a<5 do a=a+1; while true do b=b+1; break end; if a==3 then break end end; return a*10+b",
            33,
        ),
        (
            "local x=1; while true do if true then x=x+4; break else x=90 end end; return x",
            5,
        ),
        (
            "local x=0; while true do do x=x+7; break end; x=99 end; return x",
            7,
        ),
        ("local x=0; while false do break end; return x", 0),
    ] {
        let program = compile_integer_chunk(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        let lua = format!("print((function() {source} end)())");
        for binary in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if binary == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(binary);
            if binary != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command.args(["-e", &lua]).output().unwrap();
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
fn register_numeric_for_matches_production_and_upstream_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("local s=0; for i=1,5 do s=s+i end; return s", 15),
        (
            "local n=0; for i=0x8000000000000000,0x8000000000000000,-1 do n=n+1 end; return n",
            1,
        ),
        (
            "local n=0; for i=0x7ffffffffffffffe,0x7fffffffffffffff,2 do n=n+1 end; return n",
            1,
        ),
        (
            "local n=0; for i=0x8000000000000001,0x8000000000000000,-2 do n=n+1 end; return n",
            1,
        ),
        ("local n=0; for i=4,1,-1 do n=n+i end; return n", 10),
        ("local x=1; for i=1,3 do x=x+i end; return x", 7),
        ("local s=0; for i=5,1,-2 do s=s+i end; return s", 9),
        ("local s=0; for i=1,0 do s=s+i end; return s", 0),
        ("local s=0; for i=0,3,-1 do s=s+1 end; return s", 0),
        (
            "local i=100; local s=0; for i=1,3 do s=s+i end; return i+s",
            106,
        ),
        (
            "local limit=2; local s=0; for i=1,limit do limit=10; s=s+i end; return s",
            3,
        ),
        ("local i=2; local s=0; for i=i,4 do s=s+i end; return s", 9),
        (
            "local s=0; for i=1,5 do if i==3 then break end; s=s+i end; return s",
            3,
        ),
        (
            "local s=0; for i=9223372036854775807,9223372036854775807 do s=s+1 end; return s",
            1,
        ),
        (
            "local x=0; local k=0; while k<2 do for i=1,2 do x=x+1 end; k=k+1 end; return x",
            4,
        ),
        (
            "local s=0; for i=1,2 do for j=1,3 do s=s+i*j end end; return s",
            18,
        ),
        (
            "local s=0; for i=1,5 do if i>2 then break else s=s+i end end; return s",
            3,
        ),
    ] {
        let program = compile_integer_chunk(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        let lua = format!("print((function() {source} end)())");
        for binary in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if binary == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(binary);
            if binary != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let output = command.args(["-e", &lua]).output().unwrap();
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
    let source = "for i=1,3,0 do end";
    let program = compile_integer_chunk(&format!("{source}; return 0")).unwrap();
    assert_eq!(
        execute_integer_program(&program),
        Err(ferrua::owned_vm::Error::ZeroForStep)
    );
    for binary in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
        if binary == "lua5.4" && !compare_lua {
            continue;
        }
        let mut command = Command::new(binary);
        if binary != "lua5.4" {
            command.arg("--lua=5.4");
        }
        let output = command.args(["-e", source]).output().unwrap();
        assert!(
            !output.status.success(),
            "zero-step loop should fail: {binary}"
        );
    }
}

#[test]
fn generated_numeric_for_steps_branches_and_breaks_match_reference_lua() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for seed in 0_i64..48 {
        let start = seed % 11 - 5;
        let limit = (seed * 3) % 13 - 6;
        let raw_step = seed % 7 - 3;
        let step = if raw_step == 0 { 2 } else { raw_step };
        let offset = seed % 9 - 4;
        let source = format!(
            "local total={offset}; for i={start},{limit},{step} do \
             if i%2==0 then total=total+i else total=total-i end; \
             if total>20 then break end \
             end; return total"
        );
        let code = format!("print((function() {source} end)())");
        let program = compile_integer_chunk(&source)
            .unwrap_or_else(|error| panic!("seed {seed}: compile {error}: {source}"));
        let actual = execute_integer_program(&program)
            .unwrap_or_else(|error| panic!("seed {seed}: execute {error}: {source}"));
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
                "seed {seed} {binary}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let expected: i64 = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse()
                .unwrap();
            assert_eq!(actual, expected, "seed {seed} {binary}: {source}");
        }
    }
}

#[test]
fn register_repeat_until_matches_production_and_lua_54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("local n=0; repeat n=n+1 until n>=4; return n", 4),
        ("local n=0; repeat n=n+1 until true; return n", 1),
        (
            "local n=0; repeat local next=n+1; n=next until next>=4; return n",
            4,
        ),
        (
            "local n=0; repeat if n<2 then n=n+1 else n=n+2 end until n>=4; return n",
            4,
        ),
        (
            "local n=0; repeat local i=0; while i<2 do n=n+1; i=i+1 end until n>=6; return n",
            6,
        ),
    ] {
        let program = compile_integer_chunk(source).unwrap();
        assert_eq!(execute_integer_program(&program), Ok(expected), "{source}");
        let code = format!("print((function() {source} end)())");
        for executable in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if executable == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(executable);
            if executable != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let result = command.args(["-e", &code]).output().unwrap();
            assert!(
                result.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&result.stdout).trim(),
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
        (
            "if false then return 1 elseif true then return 13 end; return 9",
            13,
        ),
        (
            "if false then return 1 elseif false then return 13 end; return 9",
            9,
        ),
        (
            "local x=2; if x>3 then return 1 elseif x==2 then return 17 end; return 0",
            17,
        ),
        ("if false and (1//0>0) then return 6 end; return 12", 12),
        ("if true or (1//0>0) then return 15 end; return 0", 15),
        ("if not (2<1) then do return 21 end end; return 9", 21),
        ("do if false then return 2 end end; return 19", 19),
        ("local x=7; do if x<5 then return 2 end end; return x+4", 11),
        ("local x=0; if x==1 then return 3 end; x=6; return x+2", 8),
        (
            "if false then return 3 end; if true then return 24 end; return 5",
            24,
        ),
        (
            "if false then return 3 end; if false then return 24 end; return 5",
            5,
        ),
        (
            "local x=5; if x==5 then local y=3; return x*y end; return 0",
            15,
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
fn register_branch_bindings_match_reference_interpreters() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        (
            "local x=4; if false then x=9; return x else return x end",
            4,
        ),
        ("local x=4; if true then x=9; return x else return x end", 9),
        ("local x=4; if false then x=9; return x end; return x", 4),
        ("local x=4; if true then x=9; return x end; return x", 9),
        (
            "local x=4; if false then x=9; return x elseif true then return x end; return 1",
            4,
        ),
        (
            "local x=4; if true then x=9; return x elseif true then return x end; return 1",
            9,
        ),
        (
            "local x=2; if false then x=5; return x elseif true then x=7; return x else return x end",
            7,
        ),
        (
            "local x=2; if false then x=5; return x elseif false then x=7; return x else return x end",
            2,
        ),
        (
            "local x=2; if true then x=5; return x elseif true then x=7; return x else return x end",
            5,
        ),
        (
            "local x=2; if false then x=5; return x else do local x=9; return x end end",
            9,
        ),
        (
            "local x=2; if false then x=5; return x else return x+6 end",
            8,
        ),
        (
            "local x=2; if false then x=5; return x end; do x=x+3 end; return x",
            5,
        ),
        (
            "local x=2; do if false then x=10; return x end end; return x",
            2,
        ),
        (
            "local x=2; do if true then x=10; return x end end; return x",
            10,
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
fn integer_nonreturning_conditional_arms_match_lua_references() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("if true then local tmp=3 end; return 9", 9),
        ("if false then local tmp=1 end; return 8", 8),
        ("local x=4; if x>2 then local y=x*3 end; return x+1", 5),
        (
            "if true then local t=1 elseif true then return 99 else return 1 end; return 4",
            4,
        ),
        (
            "if false then local t=1 elseif true then return 19 else return 1 end; return 4",
            19,
        ),
        ("if false then local t=1 else local q=2 end; return 4", 4),
        ("if true then return 2 else local q=3 end; return 4", 2),
        ("if false then return 2 else local q=3 end; return 4", 4),
        ("if true then local q=8 else return 9 end; return 4", 4),
        ("if false then local q=8 else return 9 end; return 4", 9),
        (
            "local x=2; if false then local y=x+1 elseif true then local z=x*2 end; return x+5",
            7,
        ),
        (
            "local x=1; if true then local y=x+2 end; do x=x+7 end; return x",
            8,
        ),
        ("if false and (1//0>0) then local x=8 end; return 3", 3),
        ("if true or (1//0>0) then local y=3 end; return 12", 12),
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
fn register_merged_conditional_locals_match_fer_and_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (source, expected) in [
        ("local x=1; if true then x=2 end; return x", 2),
        ("local x=1; if false then x=2 end; return x", 1),
        ("local x=1; if true then x=2 else x=3 end; return x", 2),
        ("local x=1; if false then x=2 else x=3 end; return x", 3),
        (
            "local x=1; if false then x=2 elseif true then x=7 end; return x",
            7,
        ),
        (
            "local x=1; if false then x=2 elseif false then x=7 end; return x",
            1,
        ),
        (
            "local x=1; if true then x=2 elseif true then x=7 end; return x",
            2,
        ),
        (
            "local x=1; if false then return 5 else x=2 end; return x",
            2,
        ),
        ("local x=1; if true then return 5 else x=2 end; return x", 5),
        ("local x=1; if true then x=2; do x=x+3 end end; return x", 5),
        ("local a,b=1,2; if true then a,b=b,a end; return a*10+b", 21),
        (
            "local a,b=1,2; if false then a,b=b,a end; return a*10+b",
            12,
        ),
        ("local x=1; if true then local x=5; x=6 end; return x", 1),
        ("local x=1; if false then x=2 else x=x+3 end; return x", 4),
        (
            "local x=1; if true then x=2 end; if false then x=4 end; return x",
            2,
        ),
        (
            "local x=1; if true then x=2 end; if true then x=x+5 end; return x",
            7,
        ),
        (
            "local x=1; if false then x=2 end; if true then x=x+5 end; return x",
            6,
        ),
        ("local x=1; if false then x=1//0 end; return x", 1),
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
