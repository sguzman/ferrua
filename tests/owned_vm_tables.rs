//! Bounded Lua table construction and indexed read acceptance for the independent
//! Ferrua register VM. Not a claim of metatable or full Lua-table compatibility.
use ferrua::owned_vm::{
    Error, RegisterValue, compile_integer_chunk, execute_register_program_with_strings,
};
use std::process::Command;

fn render(code: &str) -> String {
    let program = compile_integer_chunk(code).unwrap_or_else(|error| panic!("{code}: {error}"));
    let result = execute_register_program_with_strings(&program)
        .unwrap_or_else(|error| panic!("{code}: {error}"));
    let mut rendered = result.values().len().to_string();
    for &value in result.values() {
        rendered.push('|');
        match value {
            RegisterValue::Integer(number) => rendered.push_str(&number.to_string()),
            RegisterValue::Float(bits) => {
                rendered.push_str(&format!("{:?}", f64::from_bits(bits)));
            }
            RegisterValue::Boolean(boolean) => rendered.push_str(&boolean.to_string()),
            RegisterValue::Nil => rendered.push_str("nil"),
            RegisterValue::String(_) => rendered.push_str(&String::from_utf8_lossy(
                result
                    .value_string_bytes(value)
                    .expect("valid string handle"),
            )),
            RegisterValue::Table(_) => rendered.push_str("table"),
            RegisterValue::Builtin(_) => rendered.push_str("function"),
        }
    }
    rendered
}

#[test]
fn register_table_literals_lookup_and_identity_match_lua54() {
    let compare_lua = std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1");
    for (chunk, expected) in [
        ("return type({})", "1|table"),
        ("local t={a=7}; return t.a", "1|7"),
        ("local t={a=7}; return t['a']", "1|7"),
        ("local t={a=7}; return t.missing", "1|nil"),
        ("local t={a=1,b=2}; return t.a,t.b", "2|1|2"),
        ("local t={9,8,7}; return t[1],t[2],t[3]", "3|9|8|7"),
        ("local t={9,8,7}; return #t", "1|3"),
        ("local t={}; return #t", "1|0"),
        ("local t={[1]=7,[2]=8}; return t[1]+t[2]", "1|15"),
        ("local t={[2.0]=5}; return t[2]", "1|5"),
        (
            "local t={[true]=7,[false]=8}; return t[true]+t[false]",
            "1|15",
        ),
        ("local t={['x']=4,x=5}; return t.x", "1|5"),
        ("local t={x='ab'}; return t.x..'c'", "1|abc"),
        ("local t={a=nil}; return t.a", "1|nil"),
        ("local t={a=1,a=2}; return t.a", "1|2"),
        ("local t={}; local u=t; return t==u", "1|true"),
        ("local t={}; local u={}; return t==u", "1|false"),
        ("local t={}; return t==t,t~=t", "2|true|false"),
        ("local t={}; t.x=7; return t.x", "1|7"),
        ("local t={}; t['x']=7; return t.x", "1|7"),
        ("local t={x=1}; t.x=t.x+4; return t.x", "1|5"),
        ("local t={}; t[2]=8; return t[2]", "1|8"),
        ("local t={}; t[2.0]=8; return t[2]", "1|8"),
        ("local t={}; t.a,t.b=1,2; return t.a,t.b", "2|1|2"),
        (
            "local t={a=1,b=2}; t.a,t.b=t.b,t.a; return t.a,t.b",
            "2|2|1",
        ),
        ("local t={a=1}; t.a,t.a=2,3; return t.a", "1|3"),
        ("local t={}; local u=t; u.k=11; return t.k", "1|11"),
        ("local t={a=1}; t.a=nil; return t.a", "1|nil"),
        ("local t={}; local k='a'; t[k]=9; return t.a", "1|9"),
        ("local t={}; t[true]=8; return t[true]", "1|8"),
        ("t={}; t.value=20; return t.value", "1|20"),
        ("local t={}; return type(t),type(t.x)", "2|table|nil"),
    ] {
        assert_eq!(render(chunk), expected, "{chunk}");
        for binary in [env!("CARGO_BIN_EXE_fer"), "lua5.4"] {
            if binary == "lua5.4" && !compare_lua {
                continue;
            }
            let mut command = Command::new(binary);
            if binary != "lua5.4" {
                command.arg("--lua=5.4");
            }
            let script = format!(
                "local function f() {chunk} end; local t=table.pack(f()); io.write(t.n); for i=1,t.n do io.write('|',tostring(t[i])) end"
            );
            let output = command.args(["-e", &script]).output().unwrap();
            assert!(
                output.status.success(),
                "{binary} rejected {chunk}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, expected.as_bytes(), "{binary}: {chunk}");
        }
    }
}

#[test]
fn returned_table_handle_is_readable_only_in_owning_execution() {
    let program = compile_integer_chunk("local t={a=9}; return t,'a'").unwrap();
    let result = execute_register_program_with_strings(&program).unwrap();
    assert!(matches!(result.values()[0], RegisterValue::Table(_)));
    assert_eq!(
        result.table_value(result.values()[0], result.values()[1]),
        Some(RegisterValue::Integer(9))
    );
    assert_eq!(
        result.table_value(result.values()[0], RegisterValue::Integer(2)),
        Some(RegisterValue::Nil)
    );
}

#[test]
fn table_heap_rejects_invalid_access_and_excessive_allocation() {
    for script in [
        "local t=7; return t.a",
        "local t={}; return t.a.x",
        "return {[nil]=1}",
        "return {[0/0]=1}",
        "local t={}; t[nil]=1",
        "local t={}; t[0/0]=1",
        "local t=7; t.x=1",
    ] {
        let program = compile_integer_chunk(script).unwrap();
        assert_eq!(
            execute_register_program_with_strings(&program),
            Err(Error::TypeError),
            "{script}"
        );
    }
    let program =
        compile_integer_chunk("local t={}; for i=1,600 do t={} end return type(t)").unwrap();
    assert_eq!(
        execute_register_program_with_strings(&program),
        Err(Error::TooLarge)
    );
}
