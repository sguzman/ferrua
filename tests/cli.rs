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
