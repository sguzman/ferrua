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

#[test]
fn owned_gc_tail_constructor_matches_lua54_across_heap_pressure() {
    let numbers = (1..=40)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let after_table = (2..=40)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sources = [
        format!(
            "local function values() return {numbers} end; for i=1,1800 do local t={{tag=i,values()}} end; print('ok')"
        ),
        format!(
            "local function values() return {{value=42}},{after_table} end; local saved; for i=1,1800 do local t={{tag=i,values()}}; if i==1800 then saved=t[1] end end; print(saved.value)"
        ),
    ];
    for (source, expected) in sources.iter().zip([b"ok\n".as_slice(), b"42\n".as_slice()]) {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        let reference = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(reference.status.success(), "{source}");
        assert_eq!(owned.stdout, expected, "{source}");
        assert_eq!(owned.stdout, reference.stdout, "{source}");
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success(), "{source}");
            assert_eq!(owned.stdout, upstream.stdout, "{source}");
        }
    }
}

#[test]
fn owned_weak_mode_flag_characters_match_lua54() {
    for mode in ["kvv", "xxvkx", "kkx", "vvx", "xyz"] {
        let source = format!(
            "local w=setmetatable({{}}, {{__mode='{mode}'}}); \
             do local key={{}}; local value={{}}; w[key]=17; w[1]=value end; \
             collectgarbage('collect'); \
             local key_gone=true; for k,v in pairs(w) do if type(k)=='table' then key_gone=false end end; \
             print(key_gone,w[1]==nil)"
        );
        assert_parity(&source, true);
    }
}

#[test]
fn owned_weak_value_gc_matches_upstream_lua54() {
    for source in [
        "local w=setmetatable({}, {__mode='v'}); local keep={id=42}; local gone={}; w[1]=keep; w[2]=gone; w.named=function() return 7 end; w.plain=23; w.words='hello'; gone=nil; collectgarbage('collect'); print(w[1].id,w[2]==nil,w.named==nil,w.plain,w.words); keep=nil; collectgarbage('collect'); print(w[1]==nil)",
        "local w=setmetatable({}, {__mode='v'}); do local key={}; w[key]=99 end; collectgarbage('collect'); local key,value=next(w); print(type(key),value)",
        "local meta={__mode='v'}; local w=setmetatable({},meta); meta.__mode=nil; local t={n=7}; w[1]=t; t=nil; collectgarbage('collect'); print(w[1].n)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_noncallable_finalizer_warnings_match_lua54() {
    for finalizer in ["12", "true", "'not callable'", "{}"] {
        let source = format!(
            "warn('@on'); do local t=setmetatable({{}}, {{__gc={finalizer}}}) end; collectgarbage('collect'); print('alive')"
        );
        let owned = run_fer(true, &source);
        assert!(owned.status.success(), "{source}");
        if let Some(reference) = reference_output(&source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
            assert_eq!(owned.stderr, reference.stderr, "{source}");
        }
    }
}

#[test]
fn owned_finalizer_warning_stream_matches_upstream_lua54() {
    for source in [
        "warn('muted'); warn('@on'); warn('hello',' world'); warn('@off'); warn('muted'); warn('@on'); print(select('#',warn('restored')))",
        "warn('@on'); do local t=setmetatable({}, {__gc=function() error('gc-failed',0) end}) end; collectgarbage('collect'); print('alive')",
        "warn('@on'); local t=setmetatable({}, {__gc=function() error('shutdown-failed',0) end}); print('body')",
        "do local t=setmetatable({}, {__gc=function() error('muted',0) end}) end; collectgarbage('collect'); print('alive')",
    ] {
        let owned = run_fer(true, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        if let Some(reference) = reference_output(source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
            assert_eq!(owned.stderr, reference.stderr, "{source}");
        }
    }
}

#[test]
fn owned_finalizer_collectgarbage_nil_arity_matches_lua54() {
    // Both the pinned production backend and Lua 5.4 suppress a nested GC
    // request from __gc and return one nil value in this calling context.
    let cases: [(&str, &[u8]); 2] = [
        (
            "local count=-1; do local t=setmetatable({}, {__gc=function() count=select('#',collectgarbage('collect')) end}) end; collectgarbage('collect'); print(count,select('#',collectgarbage('collect')))",
            b"1\t1\n",
        ),
        (
            "local t=setmetatable({}, {__gc=function() print('close',select('#',collectgarbage('collect'))) end}); print('body')",
            b"body\nclose\t1\n",
        ),
    ];
    for (source, expected) in cases {
        let owned = run_fer(true, source);
        let production = run_fer(false, source);
        assert!(owned.status.success(), "{source}");
        assert!(production.status.success(), "{source}");
        assert_eq!(owned.stdout, expected, "{source}");
        assert_eq!(production.stdout, expected, "{source}");
        if let Some(reference) = reference_output(source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
        }
    }
}

#[test]
fn owned_finalizer_suppresses_nested_collection_like_lua54() {
    let cases: [(&str, &[u8], &[u8]); 2] = [
        (
            "local called=false; do local t=setmetatable({}, {__gc=function() local ok,v=pcall(collectgarbage,'collect'); called=ok and v==nil end}) end; collectgarbage('collect'); print(called)",
            b"true\n",
            // Both interpreters suppress nested collection; pcall
            // succeeds with a nil second result in scalar context.
            b"true\n",
        ),
        (
            "local t=setmetatable({}, {__gc=function() local ok,v=pcall(collectgarbage,'collect'); print('closing',ok,v) end}); print('body')",
            b"body\nclosing\ttrue\tnil\n",
            b"body\nclosing\ttrue\tnil\n",
        ),
    ];
    for (source, expected, production_expected) in cases {
        let owned = run_fer(true, source);
        let production = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(production.status.success(), "{source}");
        assert_eq!(owned.stdout, expected, "{source}");
        assert_eq!(production.stdout, production_expected, "{source}");
        if let Some(reference) = reference_output(source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
        }
    }
}

#[test]
fn owned_lua_state_shutdown_finalizers_match_lua54() {
    // The pinned production backend currently closes live marked tables in
    // registration order, while Lua 5.4 explicitly requires reverse order.
    // Keep that divergence visible, without teaching the owned VM the bug.
    let cases: [(&str, &[u8], &[u8]); 4] = [
        (
            "local a=setmetatable({id='A'}, {__gc=function(v) print('close',v.id) end}); local b=setmetatable({id='B'}, {__gc=function(v) print('close',v.id) end}); print('body',a.id,b.id)",
            b"body\tA\tB\nclose\tB\nclose\tA\n",
            b"body\tA\tB\nclose\tA\nclose\tB\n",
        ),
        (
            "local a=setmetatable({}, {__gc=function() print('once') end}); a=nil; collectgarbage('collect'); print('body')",
            b"once\nbody\n",
            b"once\nbody\n",
        ),
        (
            "local original=setmetatable({}, {__gc=function() print('original'); setmetatable({}, {__gc=function() print('new') end}) end}); print('body')",
            b"body\noriginal\n",
            b"body\noriginal\n",
        ),
        (
            "local a=setmetatable({}, {__gc=function() error('finalizer failure') end}); print('body')",
            b"body\n",
            b"body\n",
        ),
    ];
    for (source, expected, production_expected) in cases {
        let owned = run_fer(true, source);
        let production = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(production.status.success(), "{source}");
        assert_eq!(owned.stdout, expected, "{source}");
        assert_eq!(production.stdout, production_expected, "{source}");
        if let Some(reference) = reference_output(source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
        }
    }
}

#[test]
fn owned_reregistered_finalizers_match_lua54_on_later_gc_cycles() {
    for source in [
        "local calls=0; local mt={}; mt.__gc=function(v) calls=calls+1; if calls==1 then setmetatable(v,mt) end end; do local t=setmetatable({},mt) end; collectgarbage('collect'); print(calls); collectgarbage('collect'); print(calls); collectgarbage('collect'); print(calls)",
        "local calls=0; local saved; local mt={}; mt.__gc=function(v) calls=calls+1; if calls==1 then saved=v end end; do local t=setmetatable({},mt) end; collectgarbage('collect'); print(calls,saved~=nil); setmetatable(saved,mt); saved=nil; collectgarbage('collect'); print(calls); collectgarbage('collect'); print(calls)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_explicit_table_finalizers_match_lua54_on_lifecycle_cases() {
    for source in [
        "local log=''; do local a=setmetatable({id='A'},{__gc=function(v) log=log..v.id end}); local b=setmetatable({id='B'},{__gc=function(v) log=log..v.id end}) end; collectgarbage('collect'); print(log); collectgarbage('collect'); print(log)",
        "local called=false; local mt={}; do local t=setmetatable({},mt); mt.__gc=function() called=true end end; collectgarbage('collect'); print(called)",
        "local rescued; do local t=setmetatable({answer=42},{__gc=function(v) rescued=v end}) end; collectgarbage('collect'); print(rescued.answer); rescued=nil; collectgarbage('collect'); print('once')",
        "local weak=setmetatable({}, {__mode='v'}); local cleared=false; do local x=setmetatable({}, {__gc=function(v) cleared=weak[1]==nil end}); weak[1]=x end; collectgarbage('collect'); print(cleared)",
        "local weak=setmetatable({}, {__mode='k'}); local seen=false; do local x=setmetatable({}, {__gc=function(v) seen=weak[v]==42 end}); weak[x]=42 end; collectgarbage('collect'); print(seen); collectgarbage('collect'); print(next(weak)==nil)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_lazy_standard_tables_survive_string_pressure_like_lua54() {
    let source = "local keep=tostring(123456789); for i=1,19000 do local s=tostring(i) end; local g=_G; print(g._VERSION,type(g.arg),type(g.math),keep)";
    let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["--engine=owned", "--budget=3000000", "-e", source])
        .output()
        .expect("launch owned fer");
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert_eq!(owned.stdout, b"Lua 5.4\ttable\ttable\t123456789\n");
    let production = run_fer(false, source);
    assert!(production.status.success());
    assert_eq!(owned.stdout, production.stdout);
    if let Some(upstream) = reference_output(source) {
        assert!(upstream.status.success());
        assert_eq!(owned.stdout, upstream.stdout);
    }
}

#[test]
fn owned_type_and_math_type_under_string_pressure_match_lua54() {
    for (source, expected) in [
        (
            "local hold=tostring(123456789); for i=1,19000 do local x=tostring(i) end; print(type(hold),math.type(7),hold)",
            b"string\tinteger\t123456789\n".as_slice(),
        ),
        (
            "local hold=tostring(123456789); local function stress() for i=1,19000 do local x=tostring(i) end return 17 end; print(type(hold,stress()),math.type(1.5),hold)",
            b"string\tfloat\t123456789\n".as_slice(),
        ),
        (
            "local hold=tostring(123456789); for i=1,19000 do local x=tostring(i) end; local ok1,t=pcall(type,hold); local ok2,n=pcall(math.type,7); print(ok1,t,ok2,n,hold)",
            b"true\tstring\ttrue\tinteger\t123456789\n".as_slice(),
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert_eq!(owned.stdout, expected, "{source}");
        let production = run_fer(false, source);
        assert!(production.status.success());
        assert_eq!(owned.stdout, production.stdout);
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success());
            assert_eq!(owned.stdout, upstream.stdout);
        }
    }
}

#[test]
fn owned_tostring_and_print_string_pressure_match_lua54() {
    for (source, expected) in [
        (
            "for i=1,19000 do local s=tostring(i) end; print('ok')",
            b"ok\n".as_slice(),
        ),
        (
            "local saved=tostring(99999); for i=1,19000 do local s=tostring(i) end; print(saved)",
            b"99999\n".as_slice(),
        ),
        (
            "local function pressure() for i=1,19000 do local s=tostring(i) end return 7 end; print(tostring(123456),pressure())",
            b"123456\t7\n".as_slice(),
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert_eq!(owned.stdout, expected, "{source}");
        let production = run_fer(false, source);
        assert!(production.status.success());
        assert_eq!(owned.stdout, production.stdout);
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success());
            assert_eq!(owned.stdout, upstream.stdout);
        }
    }
}

#[test]
fn owned_concat_string_pressure_gc_matches_lua54() {
    for (source, expected) in [
        (
            "for i=1,19000 do local s='v'..i end; print('ok')",
            b"ok\n".as_slice(),
        ),
        (
            "local saved={'v'..777}; for i=1,19000 do local s='v'..i end; collectgarbage('collect'); print(saved[1])",
            b"v777\n".as_slice(),
        ),
        (
            "local function pressure() for i=1,19000 do local s='v'..i end; return 7 end; local function use(s,n) print(s,n) end; use('sentinel'..42,pressure())",
            b"sentinel42\t7\n".as_slice(),
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert_eq!(owned.stdout, expected, "{source}");
        let production = run_fer(false, source);
        assert!(production.status.success());
        assert_eq!(owned.stdout, production.stdout);
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success());
            assert_eq!(owned.stdout, upstream.stdout);
        }
    }
}

#[test]
fn owned_explicit_string_collection_matches_lua54_after_two_epochs() {
    assert_parity(
        "collectgarbage('stop'); local literal='stable'; for i=1,3200 do local s=tostring(i) end; local high=collectgarbage('count'); collectgarbage('collect'); local low=collectgarbage('count'); for i=3201,6400 do local s=tostring(i) end; collectgarbage('collect'); print(high>low,literal,'stable',tostring(42))",
        true,
    );
    assert_parity(
        "local keep={}; local weak=setmetatable({}, {__mode='kv'}); local key=tostring(11111); keep[1]=tostring(22222); weak[key]=tostring(33333); key=nil; collectgarbage('collect'); print(keep[1],weak['11111'])",
        true,
    );
}

#[test]
fn owned_weak_key_ephemeron_and_double_weak_gc_match_upstream_lua54() {
    for source in [
        "local w=setmetatable({}, {__mode='k'}); local key={}; w[key]={back=key}; key=nil; collectgarbage('collect'); print(next(w)==nil)",
        "local w=setmetatable({}, {__mode='k'}); local key={}; w[key]={n=42}; collectgarbage('collect'); print(w[key].n); key=nil; collectgarbage('collect'); print(next(w)==nil)",
        "local w=setmetatable({}, {__mode='k'}); w['name']={n=42}; collectgarbage('collect'); print(w['name'].n)",
        "local w=setmetatable({}, {__mode='k'}); local seed={}; local key2={}; w[seed]={child=key2}; w[key2]={n=42}; key2=nil; collectgarbage('collect'); print(w[w[seed].child].n); seed=nil; collectgarbage('collect'); print(next(w)==nil)",
        "local w=setmetatable({}, {__mode='kv'}); local key={}; w[key]=42; key=nil; w[1]={}; w[2]='still'; collectgarbage('collect'); print(w[1]==nil,w[2],next(w)~=nil)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn generated_ephemeron_chains_match_lua54_across_insertion_orders() {
    // The weak-key hash iteration order is unspecified; every insertion
    // order must reach the same ephemeron marking fixpoint.
    for length in 2..=10 {
        for reverse in [false, true] {
            let insert = if reverse {
                format!("for i={length}-1,1,-1 do weak[keys[i]]={{next=keys[i+1]}} end")
            } else {
                format!("for i=1,{length}-1 do weak[keys[i]]={{next=keys[i+1]}} end")
            };
            let source = format!(
                "local weak=setmetatable({{}},{{__mode='k'}}); local keys={{}}; \
                 for i=1,{length} do keys[i]={{}} end; \
                 {insert}; weak[keys[{length}]]={{answer=42}}; \
                 local root=keys[1]; keys=nil; collectgarbage('collect'); \
                 local cursor=root; for i=1,{length}-1 do cursor=weak[cursor].next end; \
                 print(weak[cursor].answer); root=nil; cursor=nil; \
                 collectgarbage('collect'); print(next(weak)==nil)"
            );
            assert_parity(&source, true);
        }
    }

    assert_parity(
        "local weak=setmetatable({}, {__mode='k'}); local key=function() end; weak[key]={back=key}; key=nil; collectgarbage('collect'); print(next(weak)==nil)",
        true,
    );
}

#[test]
fn owned_gc_count_reports_live_heap_growth_and_reclamation_like_lua54() {
    assert_parity(
        "collectgarbage('stop'); local before=collectgarbage('count'); for i=1,1400 do local t={1,2,3,4,5,6,7,8,9,10,11,12} end; local high=collectgarbage('count'); collectgarbage('collect'); local low=collectgarbage('count'); print(type(low),high>before,low<high,collectgarbage('isrunning'))",
        true,
    );
}

#[test]
fn owned_lazily_materialized_lua_tables_survive_recycled_heap_pressure() {
    let source = "collectgarbage('stop'); for i=1,16384 do local t={} end; collectgarbage('restart'); print(type(_G),type(math),type(arg))";
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
    assert_eq!(owned.stdout, b"table\ttable\ttable\n");
    assert_eq!(owned.stdout, production.stdout);
    if let Some(upstream) = reference_output(source) {
        assert!(upstream.status.success());
        assert_eq!(owned.stdout, upstream.stdout);
    }
}

#[test]
fn owned_gc_stop_restart_and_manual_collection_match_lua54() {
    for (source, expected) in [
        (
            "local before=collectgarbage('isrunning'); collectgarbage('stop'); local during=collectgarbage('isrunning'); collectgarbage('collect'); collectgarbage('restart'); print(before,during,collectgarbage('isrunning'))",
            "true\tfalse\ttrue\n",
        ),
        (
            "collectgarbage('stop'); for i=1,15000 do local t={} end; collectgarbage('collect'); for i=1,15000 do local t={} end; print('manual')",
            "manual\n",
        ),
        (
            "collectgarbage('stop'); for i=1,1000 do local t={} end; collectgarbage('restart'); for i=1,20000 do local t={} end; print('restart')",
            "restart\n",
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        let reference = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(
            reference.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&reference.stderr)
        );
        assert_eq!(owned.stdout, expected.as_bytes(), "{source}");
        assert_eq!(owned.stdout, reference.stdout, "{source}");
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success(), "{source}");
            assert_eq!(owned.stdout, upstream.stdout, "{source}");
        }
    }
}
