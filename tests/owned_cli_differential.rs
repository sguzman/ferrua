//! Deterministic end-to-end owned CLI compatibility matrix.
//! This intentionally uses real binaries, including upstream Lua in the
//! reference CI job, rather than using production fer as a hidden fallback.

use std::process::{Command, Output};

fn run_fer(owned: bool, source: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fer"));
    if owned {
        command.arg("--engine=owned");
    } else {
        command.arg("--lua=5.4");
    }
    command.args(["-e", source]).output().expect("launch fer")
}

fn reference_output(source: &str) -> Option<Output> {
    (std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1"))
    .then(|| {
        Command::new("lua5.4")
            .args(["-e", source])
            .output()
            .expect("lua5.4")
    })
}

fn assert_parity(source: &str, should_succeed: bool) {
    let owned = run_fer(true, source);
    let production = run_fer(false, source);
    assert_eq!(
        owned.status.success(),
        should_succeed,
        "owned result disagrees with expected status for {source}: {}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert_eq!(
        owned.status.success(),
        production.status.success(),
        "production status differs: {source}"
    );
    assert_eq!(
        owned.stdout, production.stdout,
        "production stdout differs: {source}"
    );
    if let Some(reference) = reference_output(source) {
        assert_eq!(
            owned.status.success(),
            reference.status.success(),
            "upstream Lua 5.4 status differs: {source}"
        );
        assert_eq!(
            owned.stdout, reference.stdout,
            "upstream Lua 5.4 stdout differs: {source}"
        );
    }
}

#[test]
fn curated_global_function_print_and_error_semantics_match_lua54() {
    for source in [
        "function first() return second() end; function second() return 7 end; print(first())",
        "local t={base=40}; function t:add(x) return self.base+x end; print(t:add(2))",
        "counter=1; assert(_G.counter==1); _G.counter=4; print(counter)",
        "local f=print; _G.print=nil; assert(print==nil); _G.print=f; print('restored')",
        "setmetatable(_G,{__index=function(_,key) if key=='missing' then return 17 end end}); print(missing)",
        "local f=print; local ok=pcall(f,'protected',9); assert(ok)",
        "print(); print('hello',true,nil); print(6*7)",
        "tostring=function() return 'override' end; print(7,true); _G.tostring=nil; print('hello')",
        "local t=setmetatable({}, {__tostring=function() return 'tag' end}); print(t)",
        "local m=math; assert(_G.math==m); print(math.abs(-7),math.floor(3.75),math.ceil(-3.75),math.max(2,9,5),math.min(2,9,5),math.type(3),math.type(3.0),math.tointeger('42'))",
        "print(math.pi>3 and math.pi<4,math.huge>1e100,math.maxinteger>0,math.mininteger<0)",
        "local old=math.abs; math.abs=function(v) return v end; print(math.abs(-5)); math.abs=old; print(math.abs(-5))",
        "local outer=_ENV; local box={print=print,base=40}; do local _ENV=box; answer=base+2; function double(x) return x*2 end; print(double(answer)) end; assert(box.answer==42); print(box.double(3))",
        "local env={print=print,n=3}; do local _ENV=env; function read() return n end; n=9; print(read()) end; print(env.n)",
        "local env={print=print}; do local _ENV=env; print('first'); do local _ENV={print=print}; print('nested') end; print('last') end",
    ] {
        assert_parity(source, true);
    }
    for source in [
        "print('before'); error('after')",
        "print('before'); local t=nil; t.value=1",
    ] {
        assert_parity(source, false);
    }
}

#[test]
fn generated_branch_and_environment_interactions_match_lua54() {
    for seed in 0_i64..48 {
        let base = (seed * 11) % 23 - 8;
        let add = seed % 7 + 1;
        let source = format!(
            "counter={seed}; function bump(x) counter=counter+x; return counter end; \
             local box={{base={base}}}; function box:plus(x) return self.base+x end; \
             if {seed}%2==0 then _G.counter=bump({add}) else counter=bump({add}) end; \
             print(counter, box:plus(3), type(_G), _VERSION)"
        );
        assert_parity(&source, true);
    }
}

#[test]
fn generated_lexical_environment_closure_cases_match_lua54() {
    for seed in 0_i64..24 {
        let original = (seed * 7) % 17 - 4;
        let step = seed % 5 + 1;
        let source = format!(
            "local outer=_ENV; local e={{print=print,n={original}}}; \
             do local _ENV=e; function inc(x) n=n+x; return n end; \
             if {seed}%2==0 then n=inc({step}) else n=inc({step})+1 end; \
             print(n) end; print(e.inc(1),e.n)"
        );
        assert_parity(&source, true);
    }
}

#[test]
fn generated_numeric_math_cases_match_lua54() {
    for seed in -64_i64..=64 {
        let source = format!(
            "local n={seed}/7; print(math.floor(n),math.ceil(n),math.abs({seed}),math.max({seed},5,-3),math.min({seed},5,-3),math.type(math.floor(n)),math.tointeger(n)==nil)"
        );
        assert_parity(&source, true);
    }
    for source in [
        "print(math.min('alpha','beta'),math.max('alpha','beta'))",
        "print(math.tointeger('0x10'),math.type(4.0),math.type('4')==nil)",
    ] {
        assert_parity(source, true);
    }
    for source in [
        "math.max()",
        "math.min(1,true)",
        "math.floor({})",
        "math.abs('invalid')",
    ] {
        assert_parity(source, false);
    }
}

#[test]
fn owned_root_table_collection_matches_lua_after_repeated_cyclic_pressure() {
    let source = "local getter; do local hidden=setmetatable({answer=42}, {__index=function() return 7 end}); getter=function() return hidden.answer,hidden.missing end end; for i=1,40000 do local t={}; t.self=t end; print(getter())";
    let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["--engine=owned", "--budget=2000000", "-e", source])
        .output()
        .expect("launch owned fer");
    let production = run_fer(false, source);
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert!(production.status.success());
    assert_eq!(owned.stdout, production.stdout);
    assert_eq!(owned.stdout, b"42\t7\n");
    if let Some(upstream) = reference_output(source) {
        assert!(upstream.status.success());
        assert_eq!(owned.stdout, upstream.stdout);
    }
}

#[test]
fn owned_nested_and_field_pressure_gc_matches_lua54() {
    for (source, expected) in [
        (
            "local function p() for i=1,21000 do local t={}; t.self=t end return 7 end; local function plus(t,n) return t.v+n end; print(plus({v=35},p()))",
            "42\n",
        ),
        (
            "local function p() for i=1,21000 do local f=function() return i end end return 4 end; local function plus(f,n) return f()+n end; local function make() local v=38; return function() return v end end; print(plus(make(),p()))",
            "42\n",
        ),
        (
            "for i=1,1800 do local t={}; for j=1,40 do t[j]=j end end; print('ok')",
            "ok\n",
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        let production = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(
            production.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&production.stderr)
        );
        assert_eq!(owned.stdout, expected.as_bytes());
        assert_eq!(owned.stdout, production.stdout, "{source}");
        if let Some(upstream) = reference_output(source) {
            assert!(
                upstream.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&upstream.stderr)
            );
            assert_eq!(owned.stdout, upstream.stdout, "{source}");
        }
    }
}

#[test]
fn owned_explicit_full_gc_mode_matches_lua54() {
    for (source, output) in [
        (
            "local t={value=42}; print(type(collectgarbage()),type(collectgarbage('collect')),t.value)",
            "number\tnumber\t42\n",
        ),
        (
            "local okay=pcall(collectgarbage,'invalid'); print(okay)",
            "false\n",
        ),
        (
            "local function worker() for i=1,21000 do local t={}; t.self=t end; collectgarbage('collect'); return 42 end; print(worker())",
            "42\n",
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=2000000", "-e", source])
            .output()
            .expect("launch owned fer");
        let reference = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(reference.status.success());
        assert_eq!(owned.stdout, output.as_bytes(), "{source}");
        assert_eq!(owned.stdout, reference.stdout, "{source}");
        if let Some(lua) = reference_output(source) {
            assert!(lua.status.success());
            assert_eq!(owned.stdout, lua.stdout, "{source}");
        }
    }
}
