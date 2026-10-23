# 🚀 Ferrua-owned engine graduation contract

**Status:** Proposed acceptance contract. No engine replacement or stable release is authorized by this document.

The released command name remains `fer`. Its production interpreter currently uses the pinned pure-Rust `luna-core` backend. The first-party lexer, parser, integer register prototype and experimental stack-based flow VM are independent research implementations; their existence and green smoke CI do not establish drop-in Lua compatibility.

This contract defines what must be **demonstrated before** the first-party engine becomes the default interpreter. It separates measurable language parity from release/distribution readiness. See [OWNED_ENGINE.md](OWNED_ENGINE.md), [ROADMAP.md](ROADMAP.md) and the machine-readable [progress.json](progress.json).

## 1. Scope and version decisions

- Specify the exact upstream Lua version/dialect the owned VM implements; keep the current Lua 5.4.6 reference pinned for differential tests until an explicit migration decision is recorded.
- Document the default dialect, every supported `--lua` selection, intentional deviations and any unsupported feature. Do not silently reinterpret a script or auto-fallback to `luna-core` while describing the run as first-party execution.
- Retain the executable name `fer`, the documented CLI flags, exit codes, standard input behavior, REPL semantics, `arg` values and `--check` compilation mode.
- Maintain explicit mapping between the research `src/owned_vm.rs` register IR and `src/owned_flow.rs` stack IR. Either graduate a genuine complete register-based dispatcher or document, review and accept a different engine architecture as a **separate recorded scope change**; the current prototypes do not satisfy T302 merely by existing.

## 2. Language implementation gates

All these areas require positive acceptance evidence rather than a guessed percentage:

- **T301 grammar and lexical scope:** complete lexer, parser and diagnostics for the selected dialect, including valid and invalid token/escape/numeral forms, statements, declarations, labels and `goto`, syntax errors and useful source positions; scope visibility and upvalue behavior must match the reference. The existing limited corpus is necessary but insufficient.
- **T302 compiler and execution:** correct precedence and evaluation order, short-circuit expressions, scalar versus multiple results, tail positions, assignments, local/upvalue closures, varargs, recursion, numeric and generic loops, methods, tables, equality/order/length/concatenation metamethods, and both success and error paths.
- **Runtime lifecycle:** bounded but functioning allocation and garbage collection (including collectible cycles), long-running script memory reuse, weak tables, finalization/closing semantics, coroutines and yield/resume invariants, and explicit host-boundary resource budgets. A strict experimental call-depth limit of 16 is not general Lua recursion parity.
- **Standard libraries and integration:** behavior required by existing trusted-mode `fer` scripts, JSON bridge, subprocess and filesystem automation, `io`, `os`, `package`/module loading, `math`, `string`, `table`, `utf8`, and coroutines. Restricted `--sandbox` behavior must remain intentional and must not be represented as audited isolation.
- **Errors:** verify intentional failures as well as valid output, including `error`, `assert`, `pcall`, `xpcall`, stack unwinding, metamethod failure and protected-call restoration. Successful numeric fixtures alone are not enough.

## 3. Differential and regression evidence

Before switching backends, CI must exercise and retain all of the following:

1. A **version-pinned upstream Lua reference** with documented build provenance and reference output/error capture. Distinguish upstream conformance from matching the current production backend.
2. Golden valid/invalid syntax corpora, automatically generated deterministic expression/value tests, and nested reference workloads that exercise combined semantics rather than isolated operators.
3. Metamethod, multi-return, table-key/array-border, closure/capture, coroutine, GC/weak-table and standard-library matrices, including failure/exception boundaries.
4. Realistic first-party automation scripts executed through three paths: upstream reference where applicable, production `fer` current engine, and candidate owned engine. Compare exit codes, standard output, standard error and observable effects; normalize nondeterministic addresses or paths only with explicit documented rules.
5. Resource and safety tests for budget exhaustion, stack depth, pathological nesting, allocation limits, cycles, malformed source, huge literals, encoding and error recovery. A native stack abort or process termination is a failure even if a smoke suite remains green.
6. Rust formatting, Clippy with warnings denied, full unit/integration suites, Rust minimum supported version, composite-action consumer and native EndeavourOS/Hyprland invocation. CI passing on Ubuntu does not prove native EndeavourOS acceptance.

A failure cannot be masked by deleting a difficult fixture, weakening assertions or automatically routing an unsupported case to the external engine. Reference disagreements require an independently documented resolution, with both fixtures and implementation reviewed. Do not interpret passing the **current** Lua differential smoke job as satisfaction of this broader gate.

## 4. Cutover and release discipline

- Publish an evidence table (test suite/version/commit/run ID/result/open deviations), a compatibility matrix, and a written decision record with an exact source commit before any backend switch.
- Exercise the owned engine through an explicit **opt-in** mode first. Keep the production default intact while the candidate runs the full matrix; an opt-in mode is not permission to ship it as stable.
- Compare regression and performance behavior over representative small scripts and long-running automation. Record real measurements and conditions; do not invent benchmarks.
- Switch the default `fer` interpreter only after **all** T301/T302 implementation and required acceptance tests pass. Record the cutover in `progress.json`, provide a tested rollback path, and avoid stealth fallback to the old backend.
- Stable `v1.0.0` or another major release additionally needs native EndeavourOS QA, checksum-verified tagged Linux artifacts, pinned downstream consumer validation, release notes and explicit completion of the separate distribution gates (T107/T204/T208). Do not publish stable tags just to improve milestone percentages.

## 5. Evidence and progress policy

- The roadmap percentage averages **tracked acceptance checks**, not Lua syntax coverage, standard-library coverage, or runtime compatibility. Adding new tests may temporarily lower it.
- A check reaches 100% only after the relevant implementation and CI/acceptance evidence is recorded. A planning document may satisfy a **documentation check** without satisfying implementation or release checks.
- Keep publicly committed source, documentation, issues and commit messages professional and free of private conversation text, credentials and unlicensed data.
- Work directly on `main` with reviewable, focused commits and machine-readable evidence; do not require manual user QA.

**Current decision:** Not approved for graduation. The owned grammar, complete register VM, garbage collection, coroutine parity, standard libraries, conformance suite and native release gates remain unfinished.
