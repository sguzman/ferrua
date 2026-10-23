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

After building or installing, run `fer` in place of `cargo run --`. Without input, `fer` opens a simple REPL if standard input is a terminal; use `=expression` to display a value. Script parameters are available as `arg[1]`, `arg[2]`, etc., with the script path at `arg[0]`. `--lua=5.4` selects a Lua dialect and `--budget=N` limits executed VM instructions.

Trusted mode exposes the standard Lua `io`, `os`, `math`, `string`, `table`, `utf8`, `coroutine` and `package` libraries. Its shell access carries the caller's OS permissions. `--sandbox` is an explicit restricted-library option, not a promise of audited containment. Native JSON, process APIs and portable release binaries are still upcoming.

## Development and progress

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo run -- tools/roadmap.lua --check
```

Project management follows the evidence-based style of Posthorn, Typefinch and Rust Age. [🗺️ Roadmap](docs/ROADMAP.md) · [🔢 Machine ledger](docs/progress.json) · [🏗️ Architecture and VM provenance](docs/ARCHITECTURE.md) · [🎨 Project color](docs/COLOR.md) · [📋 Tracking queue](docs/ISSUES.md).

The public repository contains only project source and generic fixtures; no user workstation paths, private scripting data, secrets or private conversation text.

## License

MIT. The independently maintained `luna-core` runtime is MIT/Apache-2.0 licensed; attribution and its implementation boundary are documented in [architecture](docs/ARCHITECTURE.md).
