use std::io::Write;
use std::process::{Command, Stdio};

fn fer(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(args)
        .output()
        .expect("launch fer")
}

#[test]
fn eval_expression() {
    let out = fer(&["-e", "print(6 * 7)"]);
    assert!(
        out.status.success(),
        "{:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "42");
}

#[test]
fn evaluate_stdin() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fer"))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"print(12 + 30)\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "42");
}

#[test]
fn script_file_and_arguments() {
    let path = std::env::temp_dir().join(format!(
        "ferrua-args-{}-{}.lua",
        std::process::id(),
        "cli-test"
    ));
    std::fs::write(&path, "print(arg[1] .. ':' .. arg[2])").unwrap();
    let out = fer(&[path.to_str().unwrap(), "first", "second"]);
    let _ = std::fs::remove_file(path);
    assert!(
        out.status.success(),
        "{:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "first:second");
}

#[test]
fn errors_have_nonzero_status() {
    let out = fer(&["-e", "error('failed deliberately')"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("failed deliberately"));
}

#[test]
fn basic_filesystem_scripting() {
    let out = fer(&[
        "-e",
        "local f = assert(io.tmpfile()); f:write('automation'); f:seek('set', 0); assert(f:read('*a') == 'automation'); f:close(); print('ok')",
    ]);
    assert!(
        out.status.success(),
        "{:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
}

#[test]
fn sandbox_is_opt_in() {
    let out = fer(&[
        "--sandbox",
        "-e",
        "assert(os == nil and io == nil); print('safe')",
    ]);
    assert!(
        out.status.success(),
        "{:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "safe");
}

#[test]
fn json_is_available_for_automation_scripts() {
    let out = fer(&[
        "-e",
        r#"local data=json.decode('{"number":42,"ok":true}'); assert(data.number == 42); print(json.encode(data))"#,
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        r#"{"number":42,"ok":true}"#
    );
}

#[test]
fn json_empty_collections_round_trip() {
    let out = fer(&[
        "-e",
        r#"assert(json.encode(json.decode('{}')) == '{}'); assert(json.encode(json.decode('[]')) == '[]'); print('ok')"#,
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
}

#[test]
fn runtime_traceback_includes_source_context() {
    let out = fer(&[
        "-e",
        "local function fail() error('trace marker') end; fail()",
    ]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("trace marker"), "{stderr}");
    assert!(stderr.contains("-e:1"), "{stderr}");
}

#[test]
fn process_module_launches_argv_safely() {
    let out = fer(&[
        "-e",
        r#"local r=process.check("printf", {"%s", "a; echo unsafe"}); assert(r.stdout == "a; echo unsafe"); print(r.code)"#,
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0");
}

#[test]
fn restricted_mode_has_no_subprocess_bridge() {
    let out = fer(&[
        "--sandbox",
        "-e",
        "assert(process == nil and __fer_process_run == nil)",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn restricted_mode_excludes_filesystem_bridge() {
    let out = fer(&[
        "--sandbox",
        "-e",
        "assert(fs == nil and __fer_fs_call == nil)",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn restricted_mode_excludes_http_bridge() {
    let out = fer(&[
        "--sandbox",
        "-e",
        "assert(http == nil and __fer_http_call == nil)",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn structured_data_modules_are_not_in_sandbox() {
    let out = fer(&[
        "--sandbox",
        "-e",
        "assert(toml == nil and yaml == nil and __fer_data_call == nil)",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn github_module_is_only_available_in_trusted_mode() {
    let out = fer(&["--sandbox", "-e", "assert(github == nil and http == nil)"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn syntax_check_compiles_without_running_and_reports_invalid_code() {
    let checked = fer(&["--check", "-e", "error('should never run')"]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(checked.stdout.is_empty());
    let invalid = fer(&["--check", "-e", "local ="]);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("-e"));
}

#[test]
fn owned_engine_opt_in_executes_only_the_first_party_subset() {
    let valid = fer(&[
        "--engine=owned",
        "-e",
        "local x=6*7; assert(x==42); while x<45 do x=x+1 end; assert(x==45)",
    ]);
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    assert!(valid.stdout.is_empty());
    let check = fer(&["--engine=owned", "--check", "-e", "local x=1"]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let invalid = fer(&["--engine=owned", "--check", "-e", "local ="]);
    assert!(!invalid.status.success());
    let missing_global = fer(&["--engine=owned", "-e", "io.write(42)"]);
    assert!(
        !missing_global.status.success(),
        "no fallback to production VM"
    );
    let error = String::from_utf8_lossy(&missing_global.stderr);
    assert!(error.contains("owned flow"), "{error}");
    let budget = fer(&["--engine=owned", "--budget=10", "-e", "while true do end"]);
    assert!(!budget.status.success());
    assert!(
        String::from_utf8_lossy(&budget.stderr).contains("budget"),
        "{}",
        String::from_utf8_lossy(&budget.stderr)
    );
    for flags in [
        &["--engine=owned", "--lua=5.5", "-e", "return 1"][..],
        &["--engine=owned", "--sandbox", "-e", "return 1"][..],
    ] {
        assert!(
            !fer(flags).status.success(),
            "unsupported owned flags: {flags:?}"
        );
    }
}

#[test]
fn owned_print_matches_production_cli_and_preserves_prior_output_on_error() {
    let source = "print(6*7, 'hello', true, nil); print(); local f=print; f('last')";
    let owned = fer(&["--engine=owned", "-e", source]);
    let production = fer(&["--lua=5.4", "-e", source]);
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert!(production.status.success());
    assert_eq!(owned.stdout, production.stdout);
    assert_eq!(owned.stdout, b"42\thello\ttrue\tnil\n\nlast\n");

    let checked = fer(&["--engine=owned", "--check", "-e", source]);
    assert!(checked.status.success());
    assert!(checked.stdout.is_empty());

    let failure = fer(&["--engine=owned", "-e", "print('before'); error('after')"]);
    assert!(!failure.status.success());
    assert_eq!(failure.stdout, b"before\n");
    assert!(!failure.stderr.is_empty());
}

#[test]
fn owned_arg_table_matches_production_cli_and_script_name() {
    let source = "print(arg[0], arg[1], arg[2]); local t=arg; arg[1]='changed'; assert(arg==t); print(arg[1])";
    let owned = fer(&["--engine=owned", "-e", source, "one", "two"]);
    let reference = fer(&["--lua=5.4", "-e", source, "one", "two"]);
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert!(reference.status.success());
    assert_eq!(owned.stdout, reference.stdout);
    assert_eq!(owned.stdout, b"-e\tone\ttwo\nchanged\n");

    let path = std::env::temp_dir().join(format!("ferrua-owned-arg-{}.lua", std::process::id()));
    std::fs::write(&path, "print(arg[0], arg[1])").unwrap();
    let script_path = path.to_str().unwrap();
    let owned_file = fer(&["--engine=owned", script_path, "filearg"]);
    let reference_file = fer(&["--lua=5.4", script_path, "filearg"]);
    let _ = std::fs::remove_file(path);
    assert!(
        owned_file.status.success(),
        "{}",
        String::from_utf8_lossy(&owned_file.stderr)
    );
    assert!(reference_file.status.success());
    assert_eq!(owned_file.stdout, reference_file.stdout);
    let checked = fer(&["--engine=owned", "--check", "-e", source, "one"]);
    assert!(checked.status.success());
    assert!(checked.stdout.is_empty());
}

#[test]
fn owned_global_mutation_and_late_binding_match_production_lua() {
    let source = "counter=1; function_dummy=function() counter=counter+1; return counter end; print(function_dummy(), function_dummy()); print(unbound == nil); local saved=print; print=function(x) return x*2 end; assert(print(21)==42); print=saved; print(_VERSION)";
    let owned = fer(&["--engine=owned", "-e", source]);
    let reference = fer(&["--lua=5.4", "-e", source]);
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(owned.stdout, reference.stdout);
    assert_eq!(owned.stdout, b"2\t3\ntrue\nLua 5.4\n");
}

#[test]
fn owned_named_global_and_method_declarations_match_production() {
    let source = "function factorial(n) if n<=1 then return 1 end return n*factorial(n-1) end; print(factorial(5)); local t={base=40}; function t:add(n) return self.base+n end; print(t:add(2))";
    let owned = fer(&["--engine=owned", "-e", source]);
    let reference = fer(&["--lua=5.4", "-e", source]);
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(owned.stdout, reference.stdout);
    assert_eq!(owned.stdout, b"120\n42\n");
}

#[test]
fn owned_g_table_writes_and_metatable_reads_match_production() {
    let source = "counter=1; assert(_G._G==_G); _G.counter=4; assert(counter==4); counter=9; assert(_G.counter==9); local f=print; _G.print=nil; assert(print==nil); print=f; print(counter)";
    let owned = fer(&["--engine=owned", "-e", source]);
    let reference = fer(&["--lua=5.4", "-e", source]);
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(owned.stdout, reference.stdout);
    assert_eq!(owned.stdout, b"9\n");

    let source = "setmetatable(_G,{__index=function(_,key) if key=='unknown' then return 17 end end}); print(unknown)";
    let owned = fer(&["--engine=owned", "-e", source]);
    let reference = fer(&["--lua=5.4", "-e", source]);
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert!(reference.status.success());
    assert_eq!(owned.stdout, reference.stdout);
    assert_eq!(owned.stdout, b"17\n");
}
