# 🏗️ Architecture and provenance

Ferrua is a Rust native CLI that compiles and executes Lua source through the independent pure-Rust `luna-core` 4.0.2 engine. `luna-core` is a dependency, **not** Ferrua-owned source code. Its upstream project is [luna-luarust](https://github.com/tempbottle/luna-luarust). This is not `mlua` and does not embed a C Lua interpreter.

Pipeline: `fer` parses arguments → selects dialect and allowed libraries → supplies Lua `arg` → engine parses Lua → emits register-based bytecode → bytecode VM executes → optional Lua standard libraries handle IO and subprocess calls.

The default trusted execution mode supplies conventional Lua host functionality, including filesystem and shell access. It is intended for local automation chosen by the person running it. The optional sandbox creates a minimal VM and opens only the base, math, string, table, coroutine and version-applicable UTF-8 libraries; bytecode loading is disabled. Instruction budgets are optional and not a replacement for OS isolation.

Because Lua sources are evaluated by the engine, Ferrua's current conformance, bytecode semantics, closures, garbage collection, coroutine behavior, and standard libraries come from that engine. Do not claim that Ferrua has independently implemented them. An **owned compiler and VM** are queued under M2 and will require real differential and acceptance evidence before substituting this engine.

Ferrua provides its own small JSON parser/serializer from `tools/json.lua` as a built-in global available in both trusted and restricted execution modes. No Python and no dynamic Rust library are required for the module. Parsed arrays and objects carry distinct Lua metatables so even `[]` and `{}` preserve their shape on re-encoding. The parser checks strict JSON number/whitespace rules and encodes Unicode escapes without depending on the Lua 5.3+ `utf8` library; version-spanning regression cases are in `tests/json_spec.lua`.

The trusted `fs` module provides bounded UTF-8 file IO, direct file copy, safe directory creation and deterministic directory enumeration and traversal through a Rust-native bridge; see [filesystem API](FILESYSTEM.md).

The trusted `toml` and `yaml` modules parse and serialize JSON-compatible configuration data through Rust's Serde ecosystem. See [structured-data semantics](DATA.md), including comment/order loss and TOML datetime limitations.

The trusted `http` module uses the pure-Rust blocking `ureq` client with bounded UTF-8 response bodies, configurable request deadlines and TLS. Network access is explicit and confined to trusted mode; see [HTTP API](HTTP.md).

The trusted `process` module calls the Rust `std::process::Command` API directly using JSON-encoded arguments through a native VM callback. It is never installed in the restricted sandbox. Child processes use the caller's permissions, have a default 60-second timeout and capped captured output. Shells run only when deliberately requested as the executable. See [docs/PROCESS.md](PROCESS.md).

For GitHub Actions use, the root [composite action](../action.yml) installs the Rust CLI with Cargo and exposes it on PATH. It currently builds from source, rather than downloading an unversioned arbitrary binary; see [Actions integration](ACTIONS.md).

The first priority is working shell scripting, reliable scripts, diagnostics and automated acceptance. CI's Linux build, tests and smoke scripts are the acceptance source; checked-in test code alone is not acceptance.

Lua versions supported by the selected engine are 5.1–5.5. Default is 5.5. The project intentionally does not claim that every detail of every dialect has been independently tested by Ferrua.
