# 🗺️ Ferrua roadmap

**Source of truth:** [progress.json](progress.json). Progress is evidence-backed atomic acceptance checks averaged by phase, milestone and feature. No unverified CI claim reaches 100%.

**Overall:** `███░░░░░░░ 34%` across 17 atomic tasks. This describes recorded work, not full Lua compatibility.

## 🌙 M0 — Working fer interpreter MVP — `██████░░░░ 59%`

**Acceptance gate:** A usable fer CLI executes basic Lua scripts on Linux; CI passes compile, tests, CLI smoke and roadmap checks.

### 🧭 P0 — Foundation and identity — `███████░░░ 72%`

- 🎨 **T001** Select OKLCH identity and README swatch — `██████████ 100%`
  - [x] Document perceptual accent — docs/COLOR.md
  - [x] Ship accessible SVG swatch — assets/ferrua-color.svg
- 📋 **T002** Roadmap, machine ledger and evidence policy — `███████░░░ 67%`
  - [x] Structured ledger committed — docs/progress.json
  - [x] Generator and README roadmap links committed — tools/roadmap.lua; docs/ROADMAP.md; README.md
  - [ ] Generator passes CI comparison
- 🦀 **T003** Rust binary scaffold and quality gates — `█████░░░░░ 50%`
  - [x] Rust fer crate and GitHub workflow checked in — Cargo.toml; .github/workflows/ci.yml
  - [ ] Rust compile, lint and formatting verified in CI

### ⚙️ P1 — Lua interpreter and command interface — `█████░░░░░ 53%`

- 🧠 **T101** Compile and execute Lua bytecode in pure Rust — `█████░░░░░ 50%`
  - [x] Pinned pure-Rust Lua VM integrated — Cargo.toml; src/main.rs; docs/ARCHITECTURE.md
  - [ ] Lua execution verified by CI
- ⌨️ **T102** Execute script paths, -e and stdin — `███████░░░ 67%`
  - [x] CLI entry points implemented — src/main.rs
  - [x] Integration tests authored — tests/cli.rs
  - [ ] CLI integration tests pass CI
- 💬 **T103** Interactive Lua REPL — `█████░░░░░ 50%`
  - [x] Persistent-session REPL implemented — src/main.rs
  - [ ] Interactive pseudo-terminal acceptance verified
- 🧳 **T104** Arguments and actionable errors — `█████░░░░░ 50%`
  - [x] arg table and Lua error rendering implemented — src/main.rs
  - [ ] CLI argument and traceback assertions pass CI
- 🛡️ **T105** Optional restricted execution — `███████░░░ 67%`
  - [x] Safe-library opt-in and instruction budget implemented — src/main.rs
  - [x] Sandbox tests authored — tests/cli.rs; src/main.rs
  - [ ] Sandbox and budget verified under CI
- 🧪 **T106** Linux build, tests and CI — `███░░░░░░░ 33%`
  - [x] Tests and CI workflow authored — tests/cli.rs; .github/workflows/ci.yml
  - [ ] CI all checks green
  - [ ] Clean release build verified

**Feature inventory** (computed from associated atomic task progress):

- 🌙 Lua interpreter — `██████░░░░ 59%`
- ⌨️ CLI and REPL — `██████░░░░ 56%`
- 🛡️ Opt-in sandbox — `███████░░░ 67%`
- 📋 Documented, auditable progress — `██████░░░░ 63%`

## 🧰 M1 — Daily automation and Python replacement — `█░░░░░░░░░ 13%`

**Acceptance gate:** Representative filesystem, JSON, process and release automation scripts run under fer without Python.

### 🗂️ P2 — Everyday host and data capabilities — `█░░░░░░░░░ 13%`

- 📂 **T201** Read, write and iterate local files — `█████░░░░░ 50%`
  - [x] Lua io and os library available in trusted mode — src/main.rs
  - [ ] Filesystem smoke passes CI
- 🧾 **T202** First-party JSON and structured data modules — `░░░░░░░░░░ 0%`
  - [ ] Document supported JSON interface and cases
  - [ ] JSON decoding and encoding implemented
  - [ ] Round-trip integration cases accepted
- 🔧 **T203** Reliable subprocess and Git automation — `░░░░░░░░░░ 0%`
  - [ ] Document process execution and exit status contract
  - [ ] Safe argv-based child process module
  - [ ] GitHub Actions CI exercise
- 📦 **T204** Modules, release binaries and GitHub setup action — `░░░░░░░░░░ 0%`
  - [ ] Linux release artifact published
  - [ ] Reusable setup-fer GitHub action published
  - [ ] End-to-end consumer example passes

**Feature inventory** (computed from associated atomic task progress):

- 📂 Filesystem and data workflows — `███░░░░░░░ 25%`
- 🔧 Processes and CI reuse — `░░░░░░░░░░ 0%`

## 🔩 M2 — Independent Ferrua VM and compatibility — `░░░░░░░░░░ 0%`

**Acceptance gate:** Ferrua-owned parser/compiler/bytecode VM runs a documented Lua compatibility suite without external VM implementation.

### 🔣 P3 — Ferrua-owned language engine — `░░░░░░░░░░ 0%`

- 🧩 **T301** Lexing, parsing and lexical scope — `░░░░░░░░░░ 0%`
  - [ ] Implement owned lexer/parser
  - [ ] Parse golden Lua fixtures
- 📜 **T302** Bytecode compiler and register VM — `░░░░░░░░░░ 0%`
  - [ ] Implement compiler and register dispatcher
  - [ ] Closures, frames and coroutines tested
- 🔬 **T303** Lua conformance and differential tests — `░░░░░░░░░░ 0%`
  - [ ] Pin official Lua reference and fixtures
  - [ ] Differential test runner integrated
- 🚀 **T304** Native runtime graduation — `░░░░░░░░░░ 0%`
  - [ ] Document engine switch criteria
  - [ ] Switch main fer runtime after parity tests
  - [ ] Tag major stable release after acceptance

**Feature inventory** (computed from associated atomic task progress):

- 🔩 Independent compiler and VM — `░░░░░░░░░░ 0%`
- 🧪 Verified Lua compatibility — `░░░░░░░░░░ 0%`

