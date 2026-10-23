# Ferrua 🌙🔩

**Lua scripting, powered by Rust.** Ferrua provides a native command-line Lua interpreter named **fer**, intended for small scripts, Unix automation, and replacing one-off Python glue code.

![Ferrua project color, moonlit teal #15B6A9](assets/ferrua-color.svg)

**Project color:** `#15B6A9` · `oklch(70% 0.12 186)` ([OKLCH decision](docs/COLOR.md)).

> **Early working-source preview, not a validated MVP.** The Rust CLI and smoke tests exist, but build, compatibility and release claims remain gated by CI. Ferrua currently embeds the independent [luna-core](https://crates.io/crates/luna-core) Rust Lua compiler and VM. It does not yet own its own compiler/VM.

## Try it

```sh
cargo run -- -e 'print(6 * 7)'
cargo run -- examples/hello.lua
cargo run -- examples/count.lua
printf 'print(6 * 7)\n' | cargo run -- -
cargo run -- --sandbox -e 'assert(os == nil and io == nil)'
```

After building or installing, run `fer` in place of `cargo run --`. On each successful CI run for `main`, GitHub Actions also uploads a [Linux x86-64 preview binary artifact](https://github.com/sguzman/ferrua/actions/runs/39894224746); this is not yet a stable release. After extracting the artifact ZIP, make the binary executable with `chmod +x fer`. Without input, `fer` opens a simple REPL if standard input is a terminal; use `=expression` to display a value. Script parameters are available as `arg[1]`, `arg[2]`, etc., with the script path at `arg[0]`. `--lua=5.4` selects a Lua dialect and `--budget=N` limits executed VM instructions.

Ferrua also installs a first-party Lua `json` global in every session, with `json.decode(text)` and `json.encode(value)`; no third-party Lua package is required. The JSON implementation preserves empty arrays versus objects after decoding, provides `json.null` for null values, and supports `json.array()` / `json.object()` to construct empty collections. It rejects cycles, invalid input and unsupported types. Its full compatibility matrix is still under development.

Trusted mode exposes the standard Lua `io`, `os`, `math`, `string`, `table`, `utf8`, `coroutine` and `package` libraries. Its shell access carries the caller's OS permissions. `--sandbox` is an explicit restricted-library option, not a promise of audited containment. Native JSON, filesystem helpers, direct HTTP requests and argv-based process execution are included in preview builds; portable releases and a reusable setup action remain upcoming.

### Running other tools

In trusted mode, use `process.run("git", {"status", "--short"})` for structured exit codes and captured output, or `process.check("git", {"status", "--short"})` to raise a Lua error on failure. Arguments are passed directly without invoking a shell. Options include `cwd`, `env`, `stdin`, and `timeout_ms`. The host command inherits the caller's permissions. See [process API](docs/PROCESS.md), [filesystem API](docs/FILESYSTEM.md), [HTTP API](docs/HTTP.md) and [examples](examples/).

## Development and progress

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo run -- tools/roadmap.lua --check
```

**Latest verified CI:** [17 passing Rust tests, Lua roadmap validation, sandbox/REPL smoke and release build](https://github.com/sguzman/ferrua/actions/runs/39894224746). Native EndeavourOS validation and the v1.0.0 inauguration remain pending.

Project management follows the evidence-based style of Posthorn, Typefinch and Rust Age. [🗺️ Roadmap](docs/ROADMAP.md) · [🔢 Machine ledger](docs/progress.json) · [🏗️ Architecture and VM provenance](docs/ARCHITECTURE.md) · [🎨 Project color](docs/COLOR.md) · [📋 Tracking queue](docs/ISSUES.md) · [⚙️ GitHub Actions setup](docs/ACTIONS.md).

The public repository contains only project source and generic fixtures; no user workstation paths, private scripting data, secrets or private conversation text.

## GitHub Actions

Ferrua includes a [reusable composite setup action](docs/ACTIONS.md). It installs `fer` from this repository's source on an Ubuntu runner, and adds it to `PATH`. The independent CI consumer job verifies that it can execute Lua and subprocess automation without Python.

## License

MIT. The independently maintained `luna-core` runtime is MIT/Apache-2.0 licensed; attribution and its implementation boundary are documented in [architecture](docs/ARCHITECTURE.md).
