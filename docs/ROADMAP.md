# 🗺️ Ferrua roadmap

**Source of truth:** [progress.json](progress.json). Progress is evidence-backed atomic acceptance checks averaged by phase, milestone and feature. No unverified CI claim reaches 100%.

**Overall:** `█████████░ 93%` across 65 atomic tasks. This describes recorded work, not full Lua compatibility.

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

## 🔩 M2 — Independent Ferrua VM and compatibility — `█████████░ 93%`

**Acceptance gate:** Ferrua-owned parser/compiler/bytecode VM runs a documented Lua compatibility suite without external VM implementation.

### 🔣 P3 — Ferrua-owned language engine — `█████████░ 93%`

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
- 🔢 **T313** Lua integer floor, modulo, bitwise and shift instructions — `██████████ 100%`
  - [x] Implement Lua 5.4 floor-div/modulo sign and division-by-zero rules in owned integer IR — src/owned_vm.rs: floor_div, lua_mod, Error::DivisionByZero, wrapping overflow regression tests
  - [x] Implement unsigned logical bitwise operations and signed-direction shift counts — src/owned_vm.rs: BitAnd/BitOr/BitXor/BitNot/Shl/Shr instructions and shift boundary fixtures
  - [x] Verify against production fer and independent upstream Lua 5.4.6 — tests/owned_vm.rs; GitHub Actions 39960437635: four CI jobs green
- 🧱 **T314** Static integer chunk compilation with local assignments — `██████████ 100%`
  - [x] Compile a bounded straight-line Lua chunk subset to the existing owned integer IR — src/owned_vm.rs: compile_integer_chunk and scoped compile_statements
  - [x] Preserve parallel RHS evaluation, block shadowing, and rejection of unsupported statements — src/owned_vm.rs unit tests for nested do/end, local shadowing, parallel reassignment, invalid call/control-flow forms
  - [x] Verify static chunk results against fer and upstream Lua 5.4.6 — tests/owned_vm.rs; GitHub Actions 39961500042 all four jobs passed
- 🧪 **T315** 64-bit hexadecimal and generated integer parity coverage — `██████████ 100%`
  - [x] Accept full-width unsigned hexadecimal literals as wrapped Lua 64-bit integer values — src/owned_vm.rs: hexadecimal u64-to-i64 conversion and edge-case tests
  - [x] Compare deterministic signed integer arithmetic and bitwise operator matrix directly against upstream Lua — tests/owned_vm.rs: seeded integer operator matrix, division-by-zero reference cases
  - [x] Verify Linux, MSRV, standalone distribution and upstream Lua reference jobs — GitHub Actions 39962459351: all four jobs passed
- 🔠 **T316** Lua short-string escape validation and reference parity — `██████████ 100%`
  - [x] Validate decimal, hex, Unicode, and whitespace/newline short-string escapes with diagnostics — src/lex.rs: quoted scanner, newline normalization and invalid escape tests
  - [x] Compare valid and invalid string escape syntax against upstream Lua 5.4.6 — tests/owned_syntax.rs: 19 cases; CI Lua reference job 39963411673
  - [x] Preserve first-party Lua corpus acceptance across all existing jobs — GitHub Actions 39963411673: four jobs green, including 27-file lexer corpus
- 🔀 **T317** Bounded Lua integer and Boolean control-flow bytecode experiment — `██████████ 100%`
  - [x] Compile runtime if/elseif/else, while, repeat/until and scoped local assignments into independent control-flow bytecode — src/owned_flow.rs: compile_flow_chunk, stack bytecode, lexical scope frames
  - [x] Enforce execution budget and reject unsupported calls, globals, types and language features — src/owned_flow.rs: execute_flow_chunk budget, unsupported compilation tests, unbound globals and NoReturn handling
  - [x] Match production fer and upstream Lua 5.4.6 on supported control-flow fixtures — tests/owned_flow.rs; CI 39965337560: all four jobs successful
- 🔁 **T318** Bounded numeric for-loop bytecode with read-only iteration variables — `██████████ 100%`
  - [x] Evaluate control expressions once and execute ascending and descending numeric for loops — src/owned_flow.rs: ForCheck and ForStep, numeric for direction/step tests
  - [x] Preserve outer binding scope and reject mutation or invalid loop steps; bound overflow and runtime work — src/owned_flow.rs: numeric-for read-only slots, step-zero and boundary tests, step budget
  - [x] Verify supported numeric for-loop fixtures against production fer and upstream Lua — tests/owned_flow.rs; CI 39966547309: all four jobs successful
- 🔀 **T319** Lua lazy logical operand semantics on the bounded flow VM — `██████████ 100%`
  - [x] Short-circuit and/or without evaluating skipped operands; preserve selected operand values — src/owned_flow.rs: JumpIfFalseKeep, JumpIfTrueKeep, Discard and logical expression codegen
  - [x] Honor Lua truthiness (including truthy zero), Boolean/not and nil in conditions — src/owned_flow.rs: logical_and_or_preserve_values_and_short_circuit unit test
  - [x] Compare supported lazy logical cases against fer and official Lua 5.4.6 in CI — tests/owned_flow.rs; GitHub Actions 39967285721 all four jobs succeeded
- 🔣 **T320** Shared Lua integer and bitwise semantics in owned control-flow VM — `██████████ 100%`
  - [x] Reuse the Ferrua-owned integer floor/modulo/shift semantics across both experimental VM prototypes — src/owned_flow.rs; src/owned_vm.rs
  - [x] Verify signed, bitwise, lazy error skipping and divide-by-zero behavior — src/owned_flow.rs: control_flow_reuses_register_vm_integer_semantics
  - [x] Compare supported arithmetic output against fer and upstream Lua 5.4.6 in four CI jobs — tests/owned_flow.rs; GitHub Actions 39985589783: four jobs successful
- ⛓️ **T321** Loop break parsing and innermost control-flow jump patching — `██████████ 100%`
  - [x] Parse break only inside a loop and as last statement of its immediate block — src/expr.rs: break_is_scoped_to_loops_and_must_terminate_its_block
  - [x] Compile nested while, repeat and numeric-for break to the innermost loop exit — src/owned_flow.rs: break_targets_innermost_loop_and_skips_repeat_condition
  - [x] Verify bounded loop-exit parity with fer and upstream Lua 5.4.6 — tests/owned_flow.rs; GitHub Actions 39986059053: four jobs successful
- 🧩 **T322** Fixed-result assignment arity with nil padding and excess evaluation — `██████████ 100%`
  - [x] Supply nil for omitted local initializers and assignment values — src/owned_flow.rs: Compiler::values, lua_assignment_arity_and_nil_padding
  - [x] Evaluate all explicit RHS expressions before discarding excess values or updating locals — src/owned_flow.rs: Compiler::values; division-by-zero surplus RHS regression
  - [x] Compare fixed-result assignment cases with fer and upstream Lua 5.4.6 — tests/owned_flow.rs; GitHub Actions 39986717040: four jobs successful
- 🎭 **T323** Typed Boolean and nil return values from bounded owned flow — `██████████ 100%`
  - [x] Return the restricted Value enum through a new typed experimental API — src/owned_flow.rs: execute_flow_value, returns_lua_boolean_nil_and_lazy_operand_values
  - [x] Keep existing integer-return execution API and explicit noninteger rejection — src/owned_flow.rs: execute_flow_chunk wrapper and regression test
  - [x] Verify Boolean, nil and integer representations against fer and upstream Lua 5.4.6 — tests/owned_flow.rs; GitHub Actions 39987091738: four jobs successful
- 🎭 **T324** Fixed-arity Lua return lists in bounded owned flow — `██████████ 100%`
  - [x] Compile zero, single, and multiple return expressions into bounded stack bytecode — src/owned_flow.rs: ReturnMany and compile_flow_chunk
  - [x] Expose ordered typed return lists without breaking older single-value and integer return APIs — src/owned_flow.rs: execute_flow_values, execute_flow_value, execute_flow_chunk; returns_fixed_count_lua_values_and_preserves_single_value_apis
  - [x] Verify fixed-arity result lists against production fer and upstream Lua 5.4.6 — tests/owned_flow.rs; GitHub Actions 39987809090: all four jobs successful
- 🌿 **T325** Function AST, parameters, varargs and recursive lexical scopes — `██████████ 100%`
  - [x] Parse local, named and anonymous Lua function bodies while enforcing valid parameter syntax and break isolation — src/expr.rs: function_body, owned_parser_recognizes_function_scopes_and_varargs, function_syntax_rejects_invalid_parameters_and_escaped_break
  - [x] Distinguish direct locals from upvalues and support recursive local function binding — src/scope.rs: function_scope, function_parameters_and_closure_captures_have_distinct_resolution, anonymous_function_captures_and_shadowed_parameters
  - [x] Pass Linux, Rust 1.96, downstream action and Lua reference CI after acceptance fixes — GitHub Actions 39989539203: all four jobs successful
- 🧭 **T326** Dotted function/method syntax and upstream Lua parser parity — `██████████ 100%`
  - [x] Parse dotted member functions and colon-method declarations with implicit self metadata — src/expr.rs: named_fields_and_methods_parse_as_function_statements; src/scope.rs: named_methods_have_implicit_self_and_read_the_base_table
  - [x] Compare valid and invalid function syntax directly to upstream Lua 5.4 — tests/owned_syntax.rs: owned_function_syntax_matches_lua_54_when_requested
  - [x] Pass all four CI jobs including upstream Lua 5.4.6 — GitHub Actions 39990190628: four jobs successful
- 🪢 **T327** Stable transitive capture descriptors for nested closures — `██████████ 100%`
  - [x] Record lexical parent, parameter list, implicit self and distinct captured binding slots — src/scope.rs: FunctionInfo, function_scope
  - [x] Propagate captures through intermediate nested closures in deterministic slot order — src/scope.rs: record, nested_closure_captures_are_transitive_and_stable
  - [x] Pass four CI jobs with upvalue descriptor regression tests — GitHub Actions 39990585046: all four jobs successful
- 🧺 **T328** Lua table constructor AST and field name resolution — `██████████ 100%`
  - [x] Parse empty, nested, positional, named and computed table fields with correct separators — src/expr.rs: TableField, Parser::table, table_constructor_fields_and_nested_expressions, invalid_table_constructor_fields_are_rejected
  - [x] Resolve expression references in computed keys/values while ignoring literal field names — src/scope.rs: read_expression Table branch, table_field_names_are_not_variable_references
  - [x] Compare valid and invalid Lua table constructor syntax to reference Lua 5.4 — tests/owned_syntax.rs: owned_table_constructor_syntax_matches_lua_54_when_requested; GitHub Actions 39991282612 all four jobs successful
- 🗃️ **T329** Bounded indexed table values on owned flow VM — `██████████ 100%`
  - [x] Allocate positional tables in a bounded execution-local heap; read/write positive integer indices — src/owned_flow.rs: Op::NewTable, Op::GetTable, Op::SetTable, MAX_TABLE_CELLS_TOTAL
  - [x] Preserve alias identity and nil on missing positive indices; reject unsupported key shapes — src/owned_flow.rs: bounded_array_tables_preserve_reference_identity_and_missing_nil, bounded_tables_reject_unsupported_keys_and_fields
  - [x] Match supported numeric table fixtures against production fer and upstream Lua 5.4.6 — tests/owned_flow.rs; GitHub Actions 39991929210 all four jobs successful
- 📞 **T330** Bounded local function calls and isolated execution frames — `██████████ 100%`
  - [x] Compile local and anonymous function declarations to owned function bytecode with first-class handles — src/owned_flow.rs: Compiler::compile_function; Op::NewFunction; Op::Call
  - [x] Enforce a shared instruction budget and finite recursion depth across nested calls — src/owned_flow.rs: Execution::run; local_function_call_frames_and_self_recursion; bounded_calls_reject_excessive_recursion
  - [x] Verify production fer and upstream Lua 5.4.6 call-frame behavior in full CI — tests/owned_flow.rs; GitHub Actions 39993807376: 4 jobs passed
- 🪢 **T331** Shared mutable lexical upvalues and transitive returned closures — `██████████ 100%`
  - [x] Capture visible lexically scoped bindings in stable order with live shared cells — src/owned_flow.rs: BTreeMap capture mapping; Closure; Execution::cells
  - [x] Verify returned factory closures, sibling mutation, nested captures and recursive rebinding — src/owned_flow.rs: captured_locals_share_mutable_cells_across_calls
  - [x] Pass differential Lua 5.4.6 and production fer CI in four jobs — tests/owned_flow.rs; GitHub Actions 39994624379: 4 jobs passed
- 🧷 **T332** Parenthesized single-result expression boundaries — `██████████ 100%`
  - [x] Preserve an explicit AST node for parenthesized expressions — src/expr.rs: Expr::Grouped and parenthesized_call_retains_single_result_boundary
  - [x] Preserve grouping across register VM, flow VM and lexical analyzer — src/owned_vm.rs; src/owned_flow.rs; src/scope.rs
  - [x] Verify syntax and regression suite on Rust 1.96 and Lua 5.4.6 CI — GitHub Actions 39995260412: 4 jobs passed
- 🔄 **T333** Variadic function parameters and final-result expansion — `██████████ 100%`
  - [x] Implement bounded vararg reads and dynamic result arity for return and assignment tails — src/owned_flow.rs: VarargSingle, VarargMany, CallMany, AdjustTail, ReturnTail
  - [x] Distinguish unparenthesized multi-result tails from parenthesized calls — src/owned_flow.rs: last_expression_multireturn_and_variadic_adjustment
  - [x] Verify result-list behavior against production fer and Lua 5.4.6 — tests/owned_flow.rs; GitHub Actions 39995832249: 4 jobs passed
- 🧩 **T334** Lua last-argument and positional table-tail result expansion — `██████████ 100%`
  - [x] Expand final call arguments with dynamic count while preserving earlier arguments as single-valued — src/owned_flow.rs: Compiler::call; Op::CallSpread
  - [x] Expand final array-constructor results with bounded allocation — src/owned_flow.rs: Op::NewTableTail; last_call_argument_and_table_field_expand_results
  - [x] Verify differential parity against production fer and upstream Lua 5.4.6 — tests/owned_flow.rs; GitHub Actions 39996328937: 4 jobs passed
- ⤵️ **T335** Implicit zero-result return on conditional function fallthrough — `██████████ 100%`
  - [x] Emit an implicit empty return at the end of every compiled function — src/owned_flow.rs: Compiler::compile_function
  - [x] Verify both taken and untaken conditional return branches — src/owned_flow.rs: conditional_function_fallthrough_returns_zero_values
  - [x] Pass Lua 5.4.6, Rust and downstream action compatibility tests — tests/owned_flow.rs; GitHub Actions 39996750649: 4 jobs passed
- 🧠 **T336** Reuse captured closure cells without duplicate frame allocation — `██████████ 100%`
  - [x] Map captured lexical slots directly to existing shared-cell references — src/owned_flow.rs: Execution::run slot mapping
  - [x] Reject duplicate and invalid capture indices while respecting the total cell budget — src/owned_flow.rs: Execution::run inherited slot validation and MAX_CELLS_TOTAL
  - [x] Verify 30,000 repeated captured calls and full Rust/upstream Lua CI — src/owned_flow.rs: repeated_captured_calls_do_not_allocate_duplicate_upvalue_cells; GitHub Actions 39997490573: four jobs successful
- 🔀 **T337** Parallel mixed local and indexed table assignments — `██████████ 100%`
  - [x] Resolve indexed table destinations and local slots before evaluating right-hand expressions — src/owned_flow.rs: Op::PrepareLocal, Op::PrepareIndexed, Op::AssignPrepared
  - [x] Support mixed indexed/local LHS, indexed swaps, keyed LHS side effects, and final-call RHS adjustment — src/owned_flow.rs: mixed_parallel_lvalue_assignment_evaluates_addresses_before_rhs
  - [x] Pass upstream Lua 5.4.6 and production fer differential CI, Rust 1.96 and action consumer — tests/owned_flow.rs; GitHub Actions 39997991621: all four jobs passed
- 🔁 **T338** Bounded function-based generic for loops — `██████████ 100%`
  - [x] Compile evaluated-once iterator/state/control triples and multiple read-only loop variables — src/owned_flow.rs: Statement::GenericFor; Op::GenericForCheck
  - [x] Support iterator function factories, nil termination, break and step-budget exhaustion — src/owned_flow.rs: generic_iterator_calls_and_early_exit, generic_iterator_rejects_readonly_variable_and_noncallable
  - [x] Pass four CI jobs including differential Lua 5.4.6 and production fer — tests/owned_flow.rs; GitHub Actions 39998627491: all four jobs passed
- 🧵 **T339** Per-iteration closure capture identities in numeric and generic loops — `██████████ 100%`
  - [x] Separate numeric loop-control slots from visible lexical variables and renew captured iteration cells — src/owned_flow.rs: Op::ForCheck; Execution::iteration_cell
  - [x] Ensure generic-for loop closures capture distinct values across iterations — src/owned_flow.rs: Op::GenericForCheck; loop_closures_capture_distinct_iteration_cells
  - [x] Verify ascending/descending and function-iterator captures against Lua 5.4.6 and production fer — tests/owned_flow.rs; GitHub Actions 39999404758: all four jobs passed
- ♻️ **T340** Capture-aware reuse of numeric and generic loop cells — `██████████ 100%`
  - [x] Track captured cell identifiers at closure construction to avoid renewing uncaptured loop cells — src/owned_flow.rs: Execution::captured_cells, Op::NewFunction, iteration_cell
  - [x] Retain per-iteration captured values without exhausting the heap for noncapturing loops — src/owned_flow.rs: loop_closures_capture_distinct_iteration_cells; noncapturing_numeric_loops_do_not_exhaust_cell_heap
  - [x] Pass Linux, Rust 1.96, action-consumer and Lua 5.4.6 differential CI — GitHub Actions 40000123176: all four jobs successful

**Feature inventory** (computed from associated atomic task progress):

- 🔩 Independent compiler and VM — `░░░░░░░░░░ 0%`
- 🧪 Verified Lua compatibility — `█████░░░░░ 50%`
- 🔡 Ferrua-owned lexer scaffold — `██████████ 100%`
- 🧮 Experimental expression AST — `██████████ 100%`
- 🧾 Early statement syntax trees — `██████████ 100%`
- 🌿 Experimental local binding analysis — `██████████ 100%`
- 🌳 Nested lexical control-flow and loop parsing — `██████████ 100%`
- 🔢 Experimental integer register VM proof of concept — `██████████ 100%`
- 🔣 Lua integer arithmetic and bitwise coverage — `██████████ 100%`
- 🧱 Static integer chunk bytecode — `██████████ 100%`
- 🧪 Generated integer differential coverage — `██████████ 100%`
- 🔠 Short-string lexical escape parity — `██████████ 100%`
- 🔀 Experimental bounded control-flow bytecode — `██████████ 100%`
- 🔁 Numeric iteration on owned control-flow VM — `██████████ 100%`
- 🔀 Lua short-circuit logical expressions — `██████████ 100%`
- 🔣 Shared integer arithmetic across owned VM prototypes — `██████████ 100%`
- ⛓️ Lexically scoped loop break execution — `██████████ 100%`
- 🧩 Lua fixed-result assignment padding — `██████████ 100%`
- 🎭 Experimental typed flow return values — `██████████ 100%`
- 🎭 Fixed-arity experimental return lists — `██████████ 100%`
- 🌿 Owned function syntax and lexical upvalues — `██████████ 100%`
- 🧭 Lua dotted/method function syntax — `██████████ 100%`
- 🪢 Nested closure capture descriptors — `██████████ 100%`
- 🧺 Table constructor syntax and field resolution — `██████████ 100%`
- 🗃️ Bounded indexed table execution — `██████████ 100%`
- 📞 Bounded local function calls and isolated execution frames — `██████████ 100%`
- 🪢 Shared mutable lexical upvalues and transitive returned closures — `██████████ 100%`
- 🧷 Parenthesized single-result expression boundaries — `██████████ 100%`
- 🔄 Variadic function parameters and final-result expansion — `██████████ 100%`
- 🧩 Lua last-argument and positional table-tail result expansion — `██████████ 100%`
- ⤵️ Implicit zero-result return on conditional function fallthrough — `██████████ 100%`
- 🧠 Reuse captured closure cells without duplicate frame allocation — `██████████ 100%`
- 🔀 Parallel indexed and local LHS execution — `██████████ 100%`
- 🔁 Function-based generic for loop execution — `██████████ 100%`
- 🧵 Fresh captured loop-variable cells — `██████████ 100%`
- ♻️ Capture-aware loop-cell allocation — `██████████ 100%`

