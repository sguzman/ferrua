//! Typed, non-shell subprocess execution for trusted Lua scripts.
//! Every argument is passed to Command directly; no string is reinterpreted by a shell.

use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_OUTPUT: u64 = 2 * 1024 * 1024;
const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const MAX_TIMEOUT_MS: u64 = 300_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    env: Option<BTreeMap<String, String>>,
    stdin: Option<String>,
    timeout_ms: Option<u64>,
}

fn error(reason: impl Into<String>) -> String {
    json!({
        "ok": false,
        "code": null,
        "signal": null,
        "timed_out": false,
        "stdout": "",
        "stderr": "",
        "stdout_truncated": false,
        "stderr_truncated": false,
        "spawn_error": reason.into(),
    })
    .to_string()
}

fn limited_read<R: Read>(reader: R) -> io::Result<(String, bool)> {
    let mut bytes = Vec::new();
    reader.take(MAX_OUTPUT + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() as u64 > MAX_OUTPUT;
    bytes.truncate(MAX_OUTPUT as usize);
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

/// Receives one JSON request from the trusted Lua adapter and returns one JSON result.
/// OS failures are returned as spawn_error; nonzero child exit codes are regular results.
pub fn run_serialized(request: String) -> String {
    let args: Request = match serde_json::from_str(&request) {
        Ok(request) => request,
        Err(err) => return error(format!("invalid process request: {err}")),
    };
    if args.program.is_empty() {
        return error("process program cannot be empty");
    }
    let timeout_ms = args.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
        return error(format!("timeout_ms must be between 1 and {MAX_TIMEOUT_MS}"));
    }
    let mut command = Command::new(args.program);
    command
        .args(args.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = args.cwd {
        command.current_dir(path);
    }
    if let Some(env) = args.env {
        command.envs(env);
    }
    command.stdin(if args.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => return error(format!("could not start process: {err}")),
    };
    // Drain stdout/stderr concurrently so neither full pipe blocks the child.
    let out = child.stdout.take().expect("piped stdout");
    let err = child.stderr.take().expect("piped stderr");
    let stdout = thread::spawn(move || limited_read(out));
    let stderr = thread::spawn(move || limited_read(err));
    let stdin = args.stdin.map(|data| {
        let mut input = child.stdin.take().expect("piped stdin");
        thread::spawn(move || input.write_all(data.as_bytes()))
    });
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(err) => break Err(err),
            Ok(None) if start.elapsed() >= Duration::from_millis(timeout_ms) => {
                timed_out = true;
                let _ = child.kill();
                break child.wait();
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    };
    if let Some(writer) = stdin {
        let _ = writer.join();
    }
    let stdout = stdout
        .join()
        .map_err(|_| "stdout reader panicked")
        .and_then(|result| result.map_err(|_| "stdout read failed"));
    let stderr = stderr
        .join()
        .map_err(|_| "stderr reader panicked")
        .and_then(|result| result.map_err(|_| "stderr read failed"));
    let (stdout, stdout_truncated) = match stdout {
        Ok(output) => output,
        Err(err) => return error(err),
    };
    let (stderr, stderr_truncated) = match stderr {
        Ok(output) => output,
        Err(err) => return error(err),
    };
    let status = match status {
        Ok(status) => status,
        Err(err) => return error(format!("process wait failed: {err}")),
    };
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    json!({
        "ok": status.success() && !timed_out,
        "code": status.code(),
        "signal": signal,
        "timed_out": timed_out,
        "stdout": stdout,
        "stderr": stderr,
        "stdout_truncated": stdout_truncated,
        "stderr_truncated": stderr_truncated,
        "spawn_error": null,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn rejects_invalid_request_and_empty_program() {
        assert!(run_serialized("oops".into()).contains("invalid process request"));
        assert!(run_serialized(r#"{"program":""}"#.into()).contains("cannot be empty"));
    }

    #[test]
    fn rejects_out_of_range_timeout() {
        let result = run_serialized(r#"{"program":"echo","timeout_ms":0}"#.into());
        assert!(result.contains("timeout_ms"));
    }

    #[test]
    fn executes_direct_argv_without_shell() {
        let result: Value = serde_json::from_str(&run_serialized(
            r#"{"program":"printf","args":["%s","a;b $(false)"]}"#.into(),
        ))
        .unwrap();
        assert_eq!(result["stdout"], "a;b $(false)");
        assert_eq!(result["ok"], true);
    }
}
