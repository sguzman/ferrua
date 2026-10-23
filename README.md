# Ferrua 🌙🔩

**Lua scripting, powered by Rust.** Ferrua provides a native command-line Lua interpreter named **fer**, intended for small scripts, Unix automation, and replacing one-off Python glue code.

![Ferrua project color, moonlit teal #15B6A9](assets/ferrua-color.svg)

**Project color:** `#15B6A9` · `oklch(70% 0.12 186)` ([OKLCH decision](docs/COLOR.md)).

> **CI-validated development preview, not a stable release.** The Rust `fer` CLI, Lua automation libraries, and release packaging are exercised by Linux CI. Ferrua still embeds the independent [luna-core](https://crates.io/crates/luna-core) pure-Rust Lua compiler and VM; the fully owned VM is a future milestone.

## Try it

```sh
cargo run --locked -- -e 'print(6 * 7)'
cargo run --locked -- examples/hello.lua
cargo run --locked -- examples/count.lua
printf 'print(6 * 7)\n' | cargo run --locked -- -
cargo run --locked -- --sandbox -e 'assert(os == nil and io == nil)'
```

After building or installing, run `fer` in place of `cargo run --`. On each successful CI run for `main`, GitHub Actions uploads a Linux x86-64 preview binary and its SHA-256 checksum; this is not yet a stable release. After extracting the artifact ZIP, verify `sha256sum -c fer.sha256` and make the binary executable with `chmod +x fer`. Without input, `fer` opens a simple REPL if standard input is a terminal; use `=expression` to display a value. Script parameters are available as `arg[1]`, `arg[2]`, etc., with the script path at `arg[0]`. `--lua=5.4` selects a Lua dialect, `--budget=N` limits executed VM instructions, and `--check` compiles Lua source without executing it (useful for CI syntax validation).

Ferrua also installs a first-party Lua `json` global in every session, with `json.decode(text)` and `json.encode(value)`; no third-party Lua package is required. The JSON implementation preserves empty arrays versus objects after decoding, provides `json.null` for null values, and supports `json.array()` / `json.object()` to construct empty collections. It rejects cycles, invalid input and unsupported types. Its full compatibility matrix is still under development.

Trusted mode exposes the standard Lua `io`, `os`, `math`, `string`, `table`, `utf8`, `coroutine` and `package` libraries. Its shell access carries the caller's OS permissions. `--sandbox` is an explicit restricted-library option, not a promise of audited containment. Native JSON, TOML/YAML conversion, filesystem helpers, direct HTTP requests and argv-based process execution are included in preview builds; versioned distribution remains upcoming; the reusable source-build setup action already passes CI.

### Running other tools

In trusted mode, use `process.run("git", {"status", "--short"})` for structured exit codes and captured output, or `process.check("git", {"status", "--short"})` to raise a Lua error on failure. Arguments are passed directly without invoking a shell. Options include `cwd`, `env`, `stdin`, and `timeout_ms`. The host command inherits the caller's permissions. See [process API](docs/PROCESS.md), [filesystem API](docs/FILESYSTEM.md), [HTTP API](docs/HTTP.md), [structured data](docs/DATA.md), [GitHub REST API](docs/GITHUB.md) and [examples](examples/).

## Development and progress

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo run --locked -- tools/roadmap.lua --check
```

**Latest verified CI:** [all four jobs green, including owned Lua table constructor parsing and bounded indexed-table execution compared with Lua 5.4.6](https://github.com/sguzman/ferrua/actions/runs/39991929210). Native EndeavourOS acceptance, a published stable release, and a complete Ferrua-owned Lua VM remain pending.

Project management follows the evidence-based style of Posthorn, Typefinch and Rust Age. [🗺️ Roadmap](docs/ROADMAP.md) · [🔢 Machine ledger](docs/progress.json) · [🏗️ Architecture and VM provenance](docs/ARCHITECTURE.md) · [🎨 Project color](docs/COLOR.md) · [📋 Tracking queue](docs/ISSUES.md) · [⚙️ GitHub Actions setup](docs/ACTIONS.md).

The public repository contains only project source and generic fixtures; no user workstation paths, private scripting data, secrets or private conversation text.

## GitHub Actions

Ferrua includes a [reusable composite setup action](docs/ACTIONS.md). It installs `fer` from this repository's source on an Ubuntu runner, and adds it to `PATH`. The independent CI consumer job verifies that it can execute Lua and subprocess automation without Python. Supply an explicit `version` input to install a checksum-verified tagged Linux release instead of compiling the runtime from source.

## License

MIT. The independently maintained `luna-core` runtime is MIT/Apache-2.0 licensed; attribution and its implementation boundary are documented in [architecture](docs/ARCHITECTURE.md).

For a future tagged release, the Python-free packaging entrypoint is `target/release/fer tools/package_release.lua v0.1.0` after a locked release build. It creates a versioned Linux tarball with `fer`, README and LICENSE, plus a verified SHA-256 checksum. The separate release workflow publishes assets only when a matching version tag is explicitly pushed; merely committing to `main` does not inaugurate a stable release.

The [most recent tested preview archive](https://github.com/sguzman/ferrua/actions/runs/39946114053) is named `fer-v0.1.0-linux-x86_64.tar.gz` and includes its `.sha256` checksum. CI verifies the archive after construction. This is an Actions artifact, **not** a published GitHub Release.

The CI acceptance suite also runs `fer tools/check_compat.lua`, a Lua-authored test harness that invokes the local `fer` binary across Lua 5.1–5.5. It checks JSON, TOML/YAML, basic scripting, syntax-only validation, and sandbox API separation. Passing these smoke cases does not imply full dialect conformance.

### Install a tagged Linux binary

On Linux x86-64, `sh scripts/install.sh v0.1.0` installs a SHA-256-verified `fer` into `~/.local/bin`, provided that the tagged release has been published. Optionally provide a custom installation prefix as the second argument. The installer uses standard Linux utilities and does not use Python or Cargo. A versioned GitHub Release is **not yet published**; use the source-build installation until one exists.

For engine parity work, CI also runs a Lua-authored differential test harness against the Ubuntu Lua 5.4 reference interpreter. The suite presently compares twelve deterministic semantic fixtures and is explicitly not a complete Lua certification.

### Rust-owned language components

Ferrua's independent language engine is developed separately from the working `fer` CLI, which continues to use the pinned `luna-core` VM. Current Rust-owned library components include a lexical scanner, expression and limited statement parser, nested-scope resolver, and a **bounded integer register-bytecode interpreter**.

The first owned register-bytecode engine executes Lua-compatible integer arithmetic, modulo/floor division, bitwise shifts, and straight-line chunks. A separate **bounded control-flow bytecode prototype** executes an integer/Boolean/nil Lua subset with `if`, `while`, `repeat/until`, numeric `for`, loop `break`, scoped locals, assignment padding and fixed-arity return lists. Both are compared with Lua 5.4.6. The bounded control-flow engine now supports small positional tables with integer-key reads/writes, but neither engine provides complete Lua values, functions, general table semantics, generic iterator calls, closures, coroutines or the full runtime.

Read [🔩 independent engine design and acceptance evidence](docs/OWNED_ENGINE.md). The complete independent Lua compiler and VM remain future acceptance gates.
