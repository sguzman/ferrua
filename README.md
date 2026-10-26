# Ferrua 🌙🔩

**Lua scripting, powered by Rust.** Ferrua provides a native command-line Lua interpreter named **fer**, intended for small scripts, Unix automation, and replacing one-off Python glue code.

![Ferrua project color, moonlit teal #15B6A9](assets/ferrua-color.svg)

**Project color:** `#15B6A9` · `oklch(70% 0.12 186)` ([OKLCH decision](docs/COLOR.md)).

> **CI-validated development preview, not a stable release.** The default `fer` interpreter still uses pinned pure-Rust `luna-core`. `--engine=owned` explicitly opts into Ferrua's bounded first-party Lua 5.4 subset without fallback. It supports captured `print`, CLI `arg`, mutable globals, `_G`, lexical `_ENV`, named functions, a GC-rooted mutable `string` table with byte-correct `len`, `sub`, `byte`, `char`, `lower`, `upper`, `reverse`, `rep`, and bounded pattern-based `find`, `match`, `gsub` and first-class `gmatch` iterators, plus bounded partial `format`, and a bounded `table` library with `concat`, `unpack`, `pack`, `insert`, `remove`, `move` and `sort`, plus `math.sqrt`, unsigned `math.ult`, `math.fmod`, multi-result `math.modf`, and `math.sin`/`cos`/`tan`/`rad`/`deg` together with `math.asin`/`acos`/`atan`, `math.exp` and `math.log`, and Lua 5.4-compatible seeded `math.random`/`randomseed` (not cryptographic), and a bounded Lua 5.4 `utf8.char`/`len`/`codepoint`/`offset`/`codes`/`charpattern` API surface with strict/lax byte handling; full standard libraries, REPL, coroutines, complete garbage collection, and complete conformance remain unfinished. The bounded owned collector now traces suspended Lua call frames, reclaims unreachable cyclic tables and function objects, recycles orphaned captured cells, retries array/hash field growth and multi-return table-constructor tail expansion under memory pressure, and reuses reclaimed slots when lazily creating `_G`, `math` and `arg`, and charges full-sweep slot traversal against the shared instruction budget. Limited Lua garbage-collector controls (`collect`, `count`, `stop`, `restart`, `isrunning`) are supported; `count` estimates tracked owned-heap storage rather than reporting allocator-exact memory. The owned collector now supports weak keys (`__mode` containing `k`), weak values (containing `v`), double-weak tables (both flags), and an iterative ephemeron marking fixpoint; in pinned Lua 5.4.6, mode flags after an embedded NUL are ignored and long mode strings still count. Explicit full GC now reclaims unreachable runtime-interned strings while retaining compiler literals and reachable string keys/values. Automatic collection under string quota pressure is supported at direct concatenation, rooted `tostring`/`print`, `type`, `math.type`, lazy `_G`/`math`/`arg` construction, and default assertion and protected-call diagnostic messages (`pcall`/`xpcall`). Rooted allocation does not establish complete GC or upstream error-message parity. Bounded table `__gc` finalizers now run on explicit full collections and when the owned state closes, with registration-order, resurrection, weak-table interactions and later-cycle re-registration regressions. Lua-style warnings for string-valued finalizer errors now use bounded stderr capture with `warn('@on')`/`warn('@off')` controls. Noncallable numeric, boolean, string and table finalizers now emit method-specific Lua warnings; diagnostics for arbitrary internal callback failures, reentrant GC, incremental/generational collection, exact accounting and complete Lua 5.4 GC parity remain unaccepted. This is not an audited sandbox.

**Owned engine cutover:** the [graduation contract](docs/ENGINE_GRADUATION.md) defines the full implementation, conformance and native release gates. Roadmap percentages count accepted atomic tasks; they are **not** Lua compatibility percentages.

## Try it

```sh
cargo run --locked -- -e 'print(6 * 7)'
cargo run --locked -- --engine=owned -e 'print(6 * 7)'
cargo run --locked -- examples/hello.lua
cargo run --locked -- examples/count.lua
printf 'print(6 * 7)\n' | cargo run --locked -- -
cargo run --locked -- --sandbox -e 'assert(os == nil and io == nil)'
```

After building or installing, run `fer` in place of `cargo run --`. On each successful CI run for `main`, GitHub Actions uploads a Linux x86-64 preview binary and its SHA-256 checksum; this is not yet a stable release. After extracting the artifact ZIP, verify `sha256sum -c fer.sha256` and make the binary executable with `chmod +x fer`. Without input, `fer` opens a simple REPL if standard input is a terminal; use `=expression` to display a value. Script parameters are available as `arg[1]`, `arg[2]`, etc., with the script path at `arg[0]`. `--engine=owned` explicitly selects the experimental first-party Lua 5.4 subset (other dialects and `--sandbox` are rejected); `--lua=5.4` selects a Lua dialect, `--budget=N` limits executed VM instructions, and `--check` compiles Lua source without executing it (useful for CI syntax validation).

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

**Latest verified implementation CI:** [all five jobs green at `b41b57d` for 11 first-party Lua conformance programs and GC-safe shared string-method lookups](https://github.com/sguzman/ferrua/actions/runs/40353453413). Only the coroutine program remains unsupported in the 12-program owned-engine execution suite; this is not a full Lua conformance percentage or T302 graduation. Prior parser acceptance: [27 first-party Lua source files parsed by the owned AST at `de4e525` — all five CI jobs green](https://github.com/sguzman/ferrua/actions/runs/40351211626). This accepts the T301 golden-fixture check (50% of T301), not complete Lua grammar or production cutover. Earlier work: [all five jobs green at `b21a2f2` for out-of-range pattern positions and deferred invalid-pattern errors](https://github.com/sguzman/ferrua/actions/runs/40349035985). Additional accepted work: [C-locale source whitespace `ddb21b7`](https://github.com/sguzman/ferrua/actions/runs/40348591095), [Lua short-string `\\z` vertical-tab handling `9118dc9`](https://github.com/sguzman/ferrua/actions/runs/40348031694), and [Lua 5.4 label declaration ordering `8745557`](https://github.com/sguzman/ferrua/actions/runs/40347202058). The owned parser/runtime also passed [`58bc379` for reachable capture limits](https://github.com/sguzman/ferrua/actions/runs/40344371756). Previously accepted pattern semantics: [all five jobs green at `ae2ae8d` for Lua 5.4 literal repetition markers after structural pattern atoms](https://github.com/sguzman/ferrua/actions/runs/40343192671). The owned matcher also now defers errors for unreachable [malformed bracket classes #40342207612](https://github.com/sguzman/ferrua/actions/runs/40342207612) and [incomplete percent-escapes or frontier patterns #40342594142](https://github.com/sguzman/ferrua/actions/runs/40342594142); both passed all five jobs. Earlier verified conformance includes [all five jobs green at `8165725` for exact Lua 5.4 integer arguments across the owned string library](https://github.com/sguzman/ferrua/actions/runs/40339825235), including decimal/exponent/hexadecimal positional strings for `find`, `match`, `byte`, `sub`, `gmatch`, and `gsub` limits. Prior accepted incremental evidence covers [`gmatch` initial positions and end-of-string captures #40339094353](https://github.com/sguzman/ferrua/actions/runs/40339094353), [C-locale vertical-tab `%s` classes #40337049236](https://github.com/sguzman/ferrua/actions/runs/40337049236), and [pattern capture handling #40333708253](https://github.com/sguzman/ferrua/actions/runs/40333708253). The prior prefix-expression grammar was verified at [CI #40328962852](https://github.com/sguzman/ferrua/actions/runs/40328962852). C-locale ASCII identifiers and Unicode-after-numeral diagnostics are also verified at [CI #40327439776](https://github.com/sguzman/ferrua/actions/runs/40327439776) and [CI #40328202421](https://github.com/sguzman/ferrua/actions/runs/40328202421); the math required-argument correction passed [CI #40327793295](https://github.com/sguzman/ferrua/actions/runs/40327793295). The earlier 1–16-byte binary-field, `!`/`X` alignment, NUL-terminated format, and string-format tests remain green. Garbage-collected first-party `string.gmatch` iterators remain verified at [CI #40259936950](https://github.com/sguzman/ferrua/actions/runs/40259936950), with bounded hexadecimal-float formatting at [CI #40257452885](https://github.com/sguzman/ferrua/actions/runs/40257452885). The production `luna-core` backend is unchanged. Cross-ABI conformance, native EndeavourOS/Hyprland QA, first-party engine graduation and stable tagged distribution remain unaccepted.

Project management follows the evidence-based style of Posthorn, Typefinch and Rust Age. [🗺️ Roadmap](docs/ROADMAP.md) · [🔢 Machine ledger](docs/progress.json) · [🏗️ Architecture and VM provenance](docs/ARCHITECTURE.md) · [🎨 Project color](docs/COLOR.md) · [📋 Tracking queue](docs/ISSUES.md) · [⚙️ GitHub Actions setup](docs/ACTIONS.md).

The public repository contains only project source and generic fixtures; no user workstation paths, private scripting data, secrets or private conversation text.

## GitHub Actions

Ferrua includes a [reusable composite setup action](docs/ACTIONS.md). It installs `fer` from this repository's source on an Ubuntu runner, and adds it to `PATH`. The independent CI consumer job verifies that it can execute Lua and subprocess automation without Python. Supply an explicit `version` input to install a checksum-verified tagged Linux release instead of compiling the runtime from source.

## License

MIT. The independently maintained `luna-core` runtime is MIT/Apache-2.0 licensed; attribution and its implementation boundary are documented in [architecture](docs/ARCHITECTURE.md).

For a future tagged release, the Python-free packaging entrypoint is `target/release/fer tools/package_release.lua v0.1.0` after a locked release build. It creates a versioned Linux tarball with `fer`, README and LICENSE, plus a verified SHA-256 checksum. The separate release workflow publishes assets only when a matching version tag is explicitly pushed; merely committing to `main` does not inaugurate a stable release. Before any tagged publication, `fer tools/roadmap.lua --release-gate TAG` checks that the tag matches `Cargo.toml`; 0.x versions remain prereleases, and 1.x is blocked until owned-engine graduation, native EndeavourOS QA, and versioned downstream distribution acceptance.

The [most recent tested preview archive](https://github.com/sguzman/ferrua/actions/runs/39946114053) is named `fer-v0.1.0-linux-x86_64.tar.gz` and includes its `.sha256` checksum. CI verifies the archive after construction. This is an Actions artifact, **not** a published GitHub Release.

The CI acceptance suite also runs `fer tools/check_compat.lua`, a Lua-authored test harness that invokes the local `fer` binary across Lua 5.1–5.5. It checks JSON, TOML/YAML, basic scripting, syntax-only validation, and sandbox API separation. Passing these smoke cases does not imply full dialect conformance.

### Install a tagged Linux binary

On Linux x86-64, `sh scripts/install.sh v0.1.0` installs a SHA-256-verified `fer` into `~/.local/bin`, provided that the tagged release has been published. Optionally provide a custom installation prefix as the second argument. The installer uses standard Linux utilities and does not use Python or Cargo. A versioned GitHub Release is **not yet published**; use the source-build installation until one exists.

For engine parity work, CI also runs a Lua-authored differential test harness against the Ubuntu Lua 5.4 reference interpreter. The suite presently compares twelve deterministic semantic fixtures and is explicitly not a complete Lua certification.

### Rust-owned language components

Ferrua's independent language engine is developed separately from the working `fer` CLI, which continues to use the pinned `luna-core` VM. Current Rust-owned library components include a lexical scanner, expression and limited statement parser, nested-scope resolver, and a **bounded integer register-bytecode interpreter**.

The first owned register-bytecode engine executes Lua-compatible integer arithmetic, modulo/floor division, bitwise shifts and straight-line chunks. The separate **bounded stack-based control-flow VM** has progressed to conditionals and numeric/generic loops, closures with mutable upvalues, method calls, varargs and Lua multiple-result adjustment, numeric floats, byte strings, mixed-key tables and selected first-class standard functions. The accepted subset is differentially compared with upstream Lua 5.4.6 and production `fer`. The production CLI **continues to use the pinned `luna-core` VM**. The owned engine has an accepted **bounded subset** of table metatables and callable metamethods, but remains far from Lua-complete. Full metatable parity, garbage collection, coroutines, comprehensive standard libraries, protected-call/error semantics, full language conformance and native production integration are still open.

Read [🔩 independent engine design and acceptance evidence](docs/OWNED_ENGINE.md). The complete independent Lua compiler and VM remain future acceptance gates.
