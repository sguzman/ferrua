# 🔧 Ferrua process API

The trusted-mode `process` module is intended for ordinary shell automation without Python. It is not available in `fer --sandbox`.

```lua
local status = process.check("git", {"status", "--short"}, {cwd = "."})
print(status.stdout)
local result, err = process.run("git", {"rev-parse", "--is-inside-work-tree"})
if not result then error(err) end
if result.ok then print(result.stdout) end
```

`process.run(program, args?, options?)` launches the executable as a direct argv array: it does **not** invoke a shell, interpolate arguments, expand glob patterns or perform redirections. The program must be a nonempty executable name or path; each argument must be a string. Missing or invalid options raise Lua validation errors. Spawn errors return `nil, err`; nonzero exit codes return a normal result.

`process.check(...)` wraps `run` and throws a Lua error if launch fails, the process times out, or the exit status is not successful.

### Options

- `cwd = "path"`: child working directory; defaults to the caller's current directory
- `env = {NAME = "value"}`: adds or overrides child environment variables, inheriting the rest
- `stdin = "text"`: send this byte string to the child's standard input
- `timeout_ms = 60000`: timeout in milliseconds; accepted range 1 through 300000

A process inherits its parent's OS privileges and environment unless explicitly overridden. Commands such as `sh -c` do involve a shell **only if requested**. The default input stream is closed, rather than waiting for interactive input.

### Result

`{ok, code, signal, timed_out, stdout, stderr, stdout_truncated, stderr_truncated, spawn_error}`

The `code`, `signal`, and `spawn_error` fields may contain `json.null`. `ok` means zero exit status without a timeout. `stdout` and `stderr` are lossy UTF-8 text, individually bounded to 2 MiB; the corresponding `*_truncated` flags indicate truncation. The `signal` field is only meaningful on Unix. Avoid this API for streaming huge binary data or long-running interactive applications.

Timeouts terminate the directly launched child, **not its entire process tree**. Descendants may inherit pipes; choose well-behaved commands and avoid backgrounding subprocesses in this API. The interface is designed for local automation, not hostile-input sandbox containment.

### Verification

`cargo run -- examples/process.lua` exercises argument passing, exit status, errors, stdin, child environment, and timeouts. CI also verifies that `process` and its native bridge are unavailable in sandbox mode.

The Rust subprocess reader continues consuming stdout and stderr after either 2 MiB capture ceiling, rather than shutting the pipe and risking child deadlock. Text beyond the ceiling is discarded while `stdout_truncated`/`stderr_truncated` is set; a regression launches a real process emitting more than the bound. Timeouts still apply independently.
