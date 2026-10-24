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
    (std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA").ok().as_deref() == Some("1"))
        .then(|| Command::new("lua5.4").args(["-e", source]).output().expect("lua5.4"))
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
