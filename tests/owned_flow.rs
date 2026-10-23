//! Differential acceptance for experimental bounded Lua control-flow and return values.
use ferrua::owned_flow::{DEFAULT_STEP_BUDGET, Value, compile_flow_chunk, execute_flow_values};
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
    "local s=0; for i=1,5 do s=s+i end; return s",
    "local s=0; for i=6,2,-2 do s=s+i end; return s",
    "local s=0; for i=1,10,3 do s=s+i end; return s",
    "local s=0; local i=3; for i=i,5 do s=s+i end; return s+i",
    "return 0 or 99",
    "return false or 9",
    "return 3 and 7",
    "return nil or 12",
    "if false and (1+true) then return 99 else return 5 end",
    "if true or (1+true) then return 12 else return 9 end",
    "return -7//3",
    "return -7%3",
    "return 7%-3",
    "return (5&3)|8",
    "return (7~3)<<2",
    "return 0x8000000000000000>>63",
    "return ~0",
    "return 8<<-1",
    "local x=-7; if x//3==-3 and x%3==2 then return 1 else return 0 end",
    "return false and (1//0) or 4",
    "if true then return 5 else return 1//0 end",
    "local x=0; while true do x=x+1; if x==3 then break end end; return x",
    "local x=0; repeat x=x+1; if x==2 then break end until x==100; return x",
    "local s=0; for i=1,10 do if i==4 then break end; s=s+i end; return s",
    "local x=0; while x<3 do local j=0; while true do j=j+1; break end; x=x+j end; return x",
    "local x; if x==nil then return 6 else return 0 end",
    "local x=1,2; return x",
    "local x,y=3; if y==nil then return x else return 0 end",
    "local x,y=3,4; x,y=7; if y==nil then return x else return 0 end",
    "local x,y=3,4; x=7,8; return x",
    "local x,y=1,2; x,y=3,4,5; return x+y",
    "return true",
    "return false",
    "return nil",
    "return false and (1//0)",
    "return true or (1//0)",
    "return nil or false",
    "return 0 and true",
    "return not 0",
    "return 5>3",
    "local x; return x",
    "return",
    "return 1,2,3",
    "return nil,7,false",
    "return true,0",
    "if true then return 1,2 else return 3,4 end",
    "local x,y=2,3; return y,x",
];

fn render(values: &[Value]) -> String {
    values
        .iter()
        .map(|value| match value {
            Value::Integer(number) => number.to_string(),
            Value::Boolean(boolean) => boolean.to_string(),
            Value::Nil => "nil".to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\t")
}

fn check(reference: &str, prefix: &[&str]) {
    for source in FIXTURES {
        let compiled = compile_flow_chunk(source).unwrap();
        let owned = render(&execute_flow_values(&compiled, DEFAULT_STEP_BUDGET).unwrap());
        let code = format!("print((function() {source} end)())");
        let output = Command::new(reference)
            .args(prefix)
            .args(["-e", &code])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        assert_eq!(owned, expected, "{reference} differs on {source}");
    }
}

#[test]
fn production_fer_agrees_with_owned_control_flow_subset() {
    check(env!("CARGO_BIN_EXE_fer"), &["--lua=5.4"]);
}

#[test]
fn upstream_lua_agrees_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1")
    {
        check("lua5.4", &[]);
    }
}
