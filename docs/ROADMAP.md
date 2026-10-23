# 🗺️ Ferrua roadmap

**Source of truth:** [progress.json](progress.json). Progress is evidence-backed atomic acceptance checks averaged by phase, milestone and feature. No unverified CI claim reaches 100%.

**Overall:** `██████░░░░ 64%` across 22 atomic tasks. This describes recorded work, not full Lua compatibility.

## 🌙 M0 — Working fer interpreter MVP — `█████████░ 93%`

**Acceptance gate:** A usable fer CLI executes basic Lua scripts and passes Linux CI; stable v1.0.0 requires release acceptance.

### 🧭 P0 — Foundation and identity — `██████████ 100%`

- 🎨 **T001** Select OKLCH identity and README swatch — `██████████ 100%`
  - [x] Document perceptual accent — docs/COLOR.md
  - [x] Ship accessible SVG swatch — assets/ferrua-color.svg
- 📋 **T002** Roadmap, machine ledger and evidence policy — `██████████ 100%`
  - [x] Structured ledger committed — docs/progress.json
  - [x] Generator and README roadmap links committed — tools/roadmap.lua; docs/ROADMAP.md; README.md
  - [x] Generator passes CI comparison — GitHub Actions 39894224746: Rust 8+9 tests, fmt, clippy, roadmap check, CLI smoke, release build, artifact upload passed
- 🦀 **T003** Rust binary scaffold and quality gates — `██████████ 100%`
  - [x] Rust fer crate and GitHub workflow checked in — Cargo.toml; .github/workflows/ci.yml
  - [x] Rust compile, lint and formatting verified in CI — GitHub Actions 39894224746: Rust 8+9 tests, fmt, clippy, roadmap check, CLI smoke, release build, artifact upload passed

### ⚙️ P1 — Lua interpreter and command interface — `█████████░ 90%`

- 🧠 **T101** Compile and execute Lua bytecode in pure Rust — `██████████ 100%`
  - [x] Pinned pure-Rust Lua VM integrated — Cargo.toml; src/main.rs; docs/ARCHITECTURE.md
  - [x] Lua execution verified by CI — GitHub Actions 39894224746: Rust 8+9 tests, fmt, clippy, roadmap check, CLI smoke, release build, artifact upload passed
- ⌨️ **T102** Execute script paths, -e and stdin — `██████████ 100%`
  - [x] CLI entry points implemented — src/main.rs
  - [x] Integration tests authored — tests/cli.rs
  - [x] CLI integration tests pass CI — GitHub Actions 39894224746: Rust 8+9 tests, fmt, clippy, roadmap check, CLI smoke, release build, artifact upload passed
- 💬 **T103** Interactive Lua REPL — `██████████ 100%`
  - [x] Persistent-session REPL implemented — src/main.rs
  - [x] Interactive pseudo-terminal acceptance verified — GitHub Actions 39894224746: Rust 8+9 tests, fmt, clippy, roadmap check, CLI smoke, release build, artifact upload passed
- 🧳 **T104** Arguments and actionable errors — `██████████ 100%`
  - [x] arg table and Lua error rendering implemented — src/main.rs
  - [x] CLI argument and traceback assertions pass CI — GitHub Actions 39894224746: Rust 8+9 tests, fmt, clippy, roadmap check, CLI smoke, release build, artifact upload passed
- 🛡️ **T105** Optional restricted execution — `██████████ 100%`
  - [x] Safe-library opt-in and instruction budget implemented — src/main.rs
  - [x] Sandbox tests authored — tests/cli.rs; src/main.rs
  - [x] Sandbox and budget verified under CI — GitHub Actions 39894224746: Rust 8+9 tests, fmt, clippy, roadmap check, CLI smoke, release build, artifact upload passed
- 🧪 **T106** Linux build, tests and CI — `██████████ 100%`
  - [x] Tests and CI workflow authored — tests/cli.rs; .github/workflows/ci.yml
  - [x] CI all checks green — GitHub Actions 39894224746: Rust 8+9 tests, fmt, clippy, roadmap check, CLI smoke, release build, artifact upload passed
  - [x] Clean release build verified — GitHub Actions 39894224746: Rust 8+9 tests, fmt, clippy, roadmap check, CLI smoke, release build, artifact upload passed
- 📦 **T107** Distribution and MVP inauguration — `███░░░░░░░ 33%`
  - [x] Release-mode Linux preview binary built and uploaded — GitHub Actions 39894224746: fer-linux-x86_64-preview artifact
  - [ ] Runtime independently verified on EndeavourOS
  - [ ] Versioned v1.0.0 release after acceptance

**Feature inventory** (computed from associated atomic task progress):

- 🌙 Lua interpreter — `██████████ 100%`
- ⌨️ CLI and REPL — `██████████ 100%`
- 🛡️ Opt-in sandbox — `██████████ 100%`
- 📋 Documented, auditable progress — `██████████ 100%`
- 📦 Distribution readiness — `███░░░░░░░ 33%`

## 🧰 M1 — Daily automation and Python replacement — `██████░░░░ 58%`

**Acceptance gate:** Representative filesystem, JSON, process, HTTP, structured-data and versioned downstream CI scripts run without Python. Stable distribution and adoption acceptance required.

### 🗂️ P2 — Everyday host and data capabilities — `█████████░ 93%`

- 📂 **T201** Read, write and iterate local files — `██████████ 100%`
  - [x] Lua io and os library available in trusted mode — src/main.rs
  - [x] Filesystem smoke passes CI — GitHub Actions 39894224746: Rust 8+9 tests, fmt, clippy, roadmap check, CLI smoke, release build, artifact upload passed
- 🧾 **T202** First-party JSON and structured data modules — `██████████ 100%`
  - [x] Document supported JSON interface and cases — README.md, docs/ARCHITECTURE.md, tools/json.lua
  - [x] JSON decoding and encoding implemented — tools/json.lua, embedded src/main.rs
  - [x] Round-trip integration cases accepted — GitHub Actions 39894224746: JSON encoding/decoding and empty-collection Rust/CLI tests passed
- 🔧 **T203** Reliable subprocess and Git automation — `██████████ 100%`
  - [x] Document process execution and exit status contract — docs/PROCESS.md documents exit, timeout and no-shell behavior
  - [x] Safe argv-based child process module — src/process.rs, tools/process.lua; GitHub Actions 39896443549
  - [x] GitHub Actions CI exercise — GitHub Actions 39896443549: subprocess smoke passes in Linux and composite consumer jobs
- 📦 **T204** Modules, release binaries and GitHub setup action — `███████░░░ 67%`
  - [ ] Linux release artifact published
  - [x] Reusable setup-fer GitHub action published — action.yml and docs/ACTIONS.md; GitHub Actions 39896443549 action consumer passes
  - [x] End-to-end consumer example passes — GitHub Actions 39896443549: separate consumer job installs fer and executes Lua and process automation
- 📂 **T205** Rust-backed filesystem operations and directory traversal — `██████████ 100%`
  - [x] Define bounded text IO, copy and deterministic traversal contract — docs/FILESYSTEM.md, examples/fs.lua
  - [x] Expose trusted filesystem API with sandbox separation — src/fs.rs, tools/fs.lua, src/main.rs
  - [x] Native and composite action consumer filesystem smoke pass — GitHub Actions 39896443549: temporary-dir read/write/copy/walk and sandbox tests

### 🌐 P4 — Portable data and CI graduation — `░░░░░░░░░░ 0%`

- 🌐 **T206** First-party HTTP API and GitHub REST client — `░░░░░░░░░░ 0%`
  - [ ] Provide documented HTTP GET, POST and response contracts with timeouts
  - [ ] Expose trusted Rust HTTP client with no Python dependency
  - [ ] Offline/local-server fixture tests pass CI
- 🗃️ **T207** TOML and YAML parsing and serialization — `░░░░░░░░░░ 0%`
  - [ ] Document round-trip formats, supported types and limits
  - [ ] Implement native TOML and YAML modules
  - [ ] Schema and round-trip fixtures pass CI
- 📦 **T208** Stable binary distribution and cross-repository adoption — `░░░░░░░░░░ 0%`
  - [ ] Publish semver-tagged Linux artifacts with reproducible SHA-256 checksums
  - [ ] Offer installer that consumes versioned release binaries
  - [ ] Validate pinned downstream GitHub workflow with representative scripts

**Feature inventory** (computed from associated atomic task progress):

- 📂 Filesystem and data workflows — `██████████ 100%`
- 🔧 Processes and CI reuse — `████████░░ 84%`
- 🗂️ Directory traversal and file automation — `██████████ 100%`
- 🌐 HTTP and structured external data — `░░░░░░░░░░ 0%`
- 📦 Versioned CI reuse — `███░░░░░░░ 34%`

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

