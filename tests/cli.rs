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
