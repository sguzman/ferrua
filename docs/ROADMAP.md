# 🗺️ Ferrua roadmap

**Source of truth:** [progress.json](progress.json). Progress is evidence-backed atomic acceptance checks averaged by phase, milestone and feature. No unverified CI claim reaches 100%.

**Overall:** `█████████░ 87%` across 37 atomic tasks. This describes recorded work, not full Lua compatibility.

## 🌙 M0 — Working fer interpreter MVP — `█████████░ 94%`

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

### ⚙️ P1 — Lua interpreter and command interface — `█████████░ 92%`

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
- 📝 **T108** Non-executing syntax-only compilation for CI — `██████████ 100%`
  - [x] Accept --check for script paths, expressions and piped Lua source — src/main.rs: check_source, parse and execute
  - [x] Never run validated Lua during syntax-only check — tests/cli.rs and src/main.rs unit test; GitHub Actions 39947311765
  - [x] Report syntax errors with nonzero exit status — GitHub Actions 39947311765: --check invalid Lua rejects, 42 tests green

**Feature inventory** (computed from associated atomic task progress):

- 🌙 Lua interpreter — `██████████ 100%`
- ⌨️ CLI and REPL — `██████████ 100%`
- 🛡️ Opt-in sandbox — `██████████ 100%`
- 📋 Documented, auditable progress — `██████████ 100%`
- 📦 Distribution readiness — `███░░░░░░░ 33%`
- 📝 Side-effect-free source validation — `██████████ 100%`

## 🧰 M1 — Daily automation and Python replacement — `█████████░ 93%`

**Acceptance gate:** Representative filesystem, JSON, process, HTTP, structured-data and versioned downstream CI scripts run without Python. Stable distribution and adoption acceptance required.

### 🗂️ P2 — Everyday host and data capabilities — `██████████ 96%`

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
  - [ ] Tagged GitHub Release published with verified versioned artifact
  - [x] Reusable setup-fer GitHub action published — action.yml and docs/ACTIONS.md; GitHub Actions 39896443549 action consumer passes
  - [x] End-to-end consumer example passes — GitHub Actions 39896443549: separate consumer job installs fer and executes Lua and process automation
- 📂 **T205** Rust-backed filesystem operations and directory traversal — `██████████ 100%`
  - [x] Define bounded text IO, copy and deterministic traversal contract — docs/FILESYSTEM.md, examples/fs.lua
  - [x] Expose trusted filesystem API with sandbox separation — src/fs.rs, tools/fs.lua, src/main.rs
  - [x] Native and composite action consumer filesystem smoke pass — GitHub Actions 39896443549: temporary-dir read/write/copy/walk and sandbox tests
- 🧵 **T211** Drain bounded subprocess output without stalls — `██████████ 100%`
  - [x] Read streams after capture ceiling instead of blocking child — src/process.rs: limited_read drains all output and caps capture at 2 MiB
  - [x] Exercise verbose child without artificial timeout — GitHub Actions 39946114053: verbose_child_is_drained_without_spurious_timeout accepted
  - [x] Preserve explicit truncation and timeout result flags — src/process.rs; GitHub Actions 39946114053: all Rust tests passed
- 🛡️ **T213** Bound JSON recursion for robust restricted execution — `██████████ 100%`
  - [x] Limit encoder and decoder nesting to 128 levels — tools/json.lua: MAX_DEPTH
  - [x] Reject malformed deep arrays and Lua table encoding without crashing — tests/json_spec.lua: 130-level decode and encode regressions
  - [x] Pass cross-dialect and restricted CI fixtures — GitHub Actions 39947311765: tests/json_spec.lua across 5 Lua dialects
- 🔬 **T214** Run basic automation smoke across Lua 5.1–5.5 — `██████████ 100%`
  - [x] Build first-party Lua QA harness without Python — tools/check_compat.lua
  - [x] Exercise JSON, TOML/YAML, ordinary Lua, syntax and sandbox checks across five dialects — tools/check_compat.lua; .github/workflows/ci.yml
  - [x] Verify all five dialect runs and three CI jobs — GitHub Actions 39947311765: five dialect matrix passed

### 🌐 P4 — Portable data and CI graduation — `█████████░ 89%`

- 🌐 **T206** First-party HTTP API and GitHub REST client — `██████████ 100%`
  - [x] Provide documented HTTP GET, POST and response contracts with timeouts — docs/HTTP.md and tools/http.lua specify methods, output, limits and errors
  - [x] Expose trusted Rust HTTP client with no Python dependency — src/http.rs, tools/http.lua, src/main.rs
  - [x] Offline/local-server fixture tests pass CI — GitHub Actions 39897603798: loopback GET, POST, 404 and sandbox tests pass
- 🗃️ **T207** TOML and YAML parsing and serialization — `██████████ 100%`
  - [x] Document round-trip formats, supported types and limits — docs/DATA.md documents JSON-compatible TOML/YAML conversions and format limitations
  - [x] Implement native TOML and YAML modules — src/data.rs, tools/data.lua, deps toml/yaml_serde
  - [x] Schema and round-trip fixtures pass CI — GitHub Actions 39897603798: tests/data.lua round trips and sandbox tests pass in Linux and composite consumer
- 📦 **T208** Stable binary distribution and cross-repository adoption — `███░░░░░░░ 33%`
  - [ ] Publish semver-tagged Linux artifacts with reproducible SHA-256 checksums
  - [x] Offer installer that consumes versioned release binaries — scripts/install.sh and action.yml version-pinned optional SHA binary installation; CI 39950728455 installs real archive outside checkout
  - [ ] Validate pinned downstream GitHub workflow with representative scripts
- 🌙 **T209** Dogfood Lua for release archive packaging — `██████████ 100%`
  - [x] Write version-and-checksum-aware packaging script in Lua — tools/package_release.lua, docs/ACTIONS.md
  - [x] Verify executable, README, LICENSE and SHA-256 archive locally in CI — GitHub Actions 39946114053: fer-v0.1.0-linux-x86_64.tar.gz generated and checksum verified
  - [x] Provide explicit tag-triggered publication path without premature v1 inauguration — .github/workflows/release.yml: matching tag gate, v0 prerelease logic
- 🐙 **T210** Authenticated GitHub REST mutation and redirect protection — `██████████ 100%`
  - [x] Expose REST read/write, JSON request/response, and token handling — tools/github.lua and docs/GITHUB.md
  - [x] Disable automatic HTTP redirects and exercise token-bearing 302 without forwarding — src/http.rs and tests/github.rs; GitHub Actions 39946114053
  - [x] Verify GitHub POST fixture, sandbox exclusion, and MSRV CI — GitHub Actions 39946114053: 40 Rust tests, three jobs green
- 🔒 **T212** Pinned dependencies and cached Rust quality gates — `██████████ 100%`
  - [x] Commit CI-generated Cargo.lock and enforce --locked everywhere — Cargo.lock, .github/workflows/ci.yml, action.yml
  - [x] Verify pinned minimum Rust 1.96 and composite consumer — GitHub Actions 39946114053: msrv and action-consumer green
  - [x] Populate restoreable dependency/build cache with successful CI — GitHub Actions 39946114053: cache saved linux; MSRV cache restored

**Feature inventory** (computed from associated atomic task progress):

- 📂 Filesystem and data workflows — `██████████ 100%`
- 🔧 Processes and CI reuse — `████████░░ 84%`
- 🗂️ Directory traversal and file automation — `██████████ 100%`
- 🌐 HTTP and structured external data — `██████████ 100%`
- 📦 Versioned CI reuse — `█████░░░░░ 50%`
- 🧵 Reliable bounded subprocess capture — `██████████ 100%`
- 🐙 Native GitHub REST automation — `██████████ 100%`
- 🌙 Release packaging and dependency reproducibility — `████████░░ 78%`
- 🔬 Dialect matrix and JSON resilience — `██████████ 100%`

## 🔩 M2 — Independent Ferrua VM and compatibility — `████████░░ 75%`

**Acceptance gate:** Ferrua-owned parser/compiler/bytecode VM runs a documented Lua compatibility suite without external VM implementation.

### 🔣 P3 — Ferrua-owned language engine — `████████░░ 75%`

- 🧩 **T301** Lexing, parsing and lexical scope — `░░░░░░░░░░ 0%`
  - [ ] Implement owned lexer/parser
  - [ ] Parse golden Lua fixtures
- 📜 **T302** Bytecode compiler and register VM — `░░░░░░░░░░ 0%`
  - [ ] Implement compiler and register dispatcher
  - [ ] Closures, frames and coroutines tested
- 🔬 **T303** Lua conformance and differential tests — `██████████ 100%`
  - [x] Pin official Lua reference and fixtures — tools/diff_lua.lua pins upstream Lua 5.4.6, 12 fixtures in tests/conformance/; CI 39950728455
  - [x] Differential test runner integrated — CI 39950728455: all 12 deterministic parity fixtures passed against Lua 5.4.6 reference
- 🚀 **T304** Native runtime graduation — `░░░░░░░░░░ 0%`
  - [ ] Document engine switch criteria
  - [ ] Switch main fer runtime after parity tests
  - [ ] Tag major stable release after acceptance
- 🔡 **T305** Standalone Ferrua-owned lexical scanner foundation — `██████████ 100%`
  - [x] Tokenize Lua keyword, names, literals, operators and comments with source offsets — src/lex.rs and src/lib.rs
  - [x] Run first-party Lua source corpus and malformed-lexeme regressions — tests/owned_syntax.rs: 27 Lua scripts; src/lex.rs numeric, Unicode and backslash-z fixtures
  - [x] Accept library scanner on supported Rust and CI jobs — GitHub Actions 39953838372: all four jobs pass, including Rust 1.96 compatibility
- 🧮 **T306** Pratt expression AST for documented Lua syntax subset — `██████████ 100%`
  - [x] Parse arithmetic, bitwise, logical and right-associative operators — src/expr.rs precedence/associativity unit tests
  - [x] Parse chained function calls, field selections and bracket indexes — src/expr.rs: Call, Field and Index AST variants, invalid-syntax regressions
  - [x] Accept standalone expression parser without swapping VM backend — GitHub Actions 39953838372: four jobs green; src/main.rs still uses luna-core
- 🧾 **T307** Limited Lua statement AST foundation — `██████████ 100%`
  - [x] Parse local declarations, calls, assignments, and final returns — src/expr.rs: public Statement and parse_chunk APIs
  - [x] Reject unsupported grammar, invalid assignment targets and missing separators — src/expr.rs: chunk parser invalid syntax and assignment fixtures
  - [x] Validate parser subset without changing the production VM — GitHub Actions 39954592909: all four jobs green with unit/integration tests
- 🌿 **T308** Local binding and shadowing analyzer foundation — `██████████ 100%`
  - [x] Record local slots and reads/writes in experimental Lua chunks — src/scope.rs: public analyze, Binding and Reference APIs
  - [x] Resolve previous locals during same-name initializer before new declaration — src/scope.rs: local_shadowing_preserves_prior_initializer_binding and new_local_does_not_see_itself_in_initializer tests
  - [x] Verify single-scope analyzer on supported Rust, CLI and reference CI — GitHub Actions 39955169495: all four jobs green
- 🌳 **T309** Nested block, conditional and while-loop syntax/scoping — `██████████ 100%`
  - [x] Parse bounded nested do/end blocks and restore scope visibility — src/expr.rs and src/scope.rs; CI 39956080746
  - [x] Parse if/elseif/else and while with independent branch/loop scopes — src/expr.rs and src/scope.rs; CI 39956479889
  - [x] Verify old CLI, Rust MSRV and Lua 5.4.6 references unchanged — CI 39956479889: four jobs passed
- 🔁 **T310** Repeat-until body scope and newline statement parsing — `██████████ 100%`
  - [x] Parse repeat/until and newline-separated supported Lua statements — src/expr.rs: repeat and statement separator tests
  - [x] Resolve repeat condition against body locals, then discard frame — src/scope.rs: repeat_condition_sees_body_local_but_later_code_does_not
  - [x] Accept syntax and resolver regression suite in full CI — CI 39956981511: four jobs passed
- 🧮 **T311** Numeric and generic for-loop syntax and variable scopes — `██████████ 100%`
  - [x] Implement numeric for (optional step), iteration locals and outside bounds — src/expr.rs, src/scope.rs; CI 39957386939
  - [x] Implement generic for-in name lists, iterator expressions and scoped locals — src/expr.rs, src/scope.rs; CI 39957918722
  - [x] Verify malformed headers and all four CI acceptance jobs — CI 39957918722: four jobs passed
- 🔢 **T312** Experimental integer register IR and execution subset — `██████████ 100%`
  - [x] Implement bounded first-party register bytecode compiler and dispatcher for integer arithmetic only — src/owned_vm.rs: compile_integer_expression, execute_integer_program, explicit unsupported forms
  - [x] Reject malformed registers and validate wrapping integer arithmetic — src/owned_vm.rs: invalid-register and overflow unit tests; CI 39958915661
  - [x] Compare owned integer bytecode with official upstream Lua 5.4.6 — tests/owned_vm.rs and CI 39959317291: Compare experimental integer register IR against upstream Lua — success

**Feature inventory** (computed from associated atomic task progress):

- 🔩 Independent compiler and VM — `░░░░░░░░░░ 0%`
- 🧪 Verified Lua compatibility — `█████░░░░░ 50%`
- 🔡 Ferrua-owned lexer scaffold — `██████████ 100%`
- 🧮 Experimental expression AST — `██████████ 100%`
- 🧾 Early statement syntax trees — `██████████ 100%`
- 🌿 Experimental local binding analysis — `██████████ 100%`
- 🌳 Nested lexical control-flow and loop parsing — `██████████ 100%`
- 🔢 Experimental integer register VM proof of concept — `██████████ 100%`

