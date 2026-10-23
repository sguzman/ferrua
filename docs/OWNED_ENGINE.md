# 🔩 Ferrua-owned language engine

The production `fer` executable still uses the pinned, independently maintained pure-Rust `luna-core` compiler and VM. Ferrua's separate experimental Rust-owned components are library APIs, and **do not replace the production backend**.

## Current independent components

- **Lexer:** [`src/lex.rs`](../src/lex.rs) tokenizes a documented Lua 5.4 subset, preserving spelling, line number and source byte offsets; [the first-party source corpus](../tests/owned_syntax.rs) covers 27 real Lua scripts.
- **Parser:** [`src/expr.rs`](../src/expr.rs) builds Pratt expression trees with operator precedence, chained calls, fields and indexing. Its limited chunk grammar includes local declarations, assignments, calls, returns, nested `do/end`, `if/elseif/else`, `while`, `repeat/until`, numeric `for`, and generic `for/in`.
- **Scope analysis:** [`src/scope.rs`](../src/scope.rs) records local bindings and name references, including block/branch/loop scopes, shadowing, parallel declaration visibility, and `repeat` conditions evaluated in the body scope.
- **Register prototype:** [`src/owned_vm.rs`](../src/owned_vm.rs) compiles and runs integer expression bytecode with bounded registers. Supported operators include unary minus, integer addition/subtraction/multiplication, Lua floor division and modulo, and integer bitwise/shift operations. Division by zero yields an explicit error. Integer overflow wraps as on the supported 64-bit Lua reference.
- **Static integer chunks:** `compile_integer_chunk` compiles straight-line integer locals, parallel assignments to existing locals, nested `do/end` scopes, and a final scalar return into the same owned register IR. An initializer sees the outer/previous binding, while parallel assignment evaluates all RHS expressions before rebinding names.

## Explicit boundaries

The parser is **not** complete Lua syntax. The independent bytecode engine supports neither Lua functions nor dynamic loops, branches, global variables, calls, Lua tables, strings, floats, metatables, coroutines, closures/upvalues, garbage collection, or a standard library. The static chunk compiler refuses unsupported forms instead of silently falling back to the production engine. Native script entrypoints remain powered by `luna-core`.

The tests compare the narrow owned integer subset to the production `fer` CLI and, in a separate Linux CI job, directly to upstream **Lua 5.4.6**. These comparisons do not certify full Lua conformance.

## Acceptance evidence

- [Lexer/parser corpus — CI #39953838372](https://github.com/sguzman/ferrua/actions/runs/39953838372)
- [Scoped conditional/loop parser — CI #39957918722](https://github.com/sguzman/ferrua/actions/runs/39957918722)
- [Owned integer IR against upstream Lua — CI #39959317291](https://github.com/sguzman/ferrua/actions/runs/39959317291)
- [Extended floor/modulo/bitwise parity — CI #39960437635](https://github.com/sguzman/ferrua/actions/runs/39960437635)
- [Static integer local chunks against upstream Lua — CI #39961500042](https://github.com/sguzman/ferrua/actions/runs/39961500042)

Each cited CI run passed all four existing jobs. The ledger in [`progress.json`](progress.json) credits only these narrower implemented tasks; **full parser/scoping (T301), full VM/compiler (T302), and native runtime graduation (T304) remain open**.

## Graduation sequence

1. Expand syntax and diagnostics to complete Lua grammar, including function definitions and expression forms.
2. Implement complete lexical bindings, upvalues, closures, and function call mechanics.
3. Extend runtime value types, Lua tables/metamethods, general control-flow bytecode, garbage collection, coroutines and standard libraries.
4. Grow upstream Lua differential testing beyond deterministic smoke fixtures, including error semantics and real automation workloads.
5. Integrate the independent engine with `fer` only after passing the full acceptance gates. Do not claim a stable owned runtime prematurely.

## Expanded parity and lexical validation

The integer bytecode prototype now parses full-width hexadecimal integer literals using modulo-64-bit bit patterns, including 0xffffffffffffffff as -1. A deterministic generated reference test compares approximately 1,280 signed arithmetic/bitwise cases, including negative shifts and floor-div/modulo sign behavior, to upstream Lua 5.4.6. Zero-divisor error behavior is also checked. [Verified CI #39962459351](https://github.com/sguzman/ferrua/actions/runs/39962459351).

The source scanner validates short Lua string escapes: simple escapes, decimal byte escapes, two-digit hexadecimal escapes, brace-delimited Unicode escapes, escaped newline pairs, and backslash-z whitespace skipping. It reports malformed or out-of-range forms as lexical errors. Nineteen representative valid/invalid cases are compared to upstream Lua 5.4.6 in CI, in addition to the first-party Lua corpus. [Verified CI #39963411673](https://github.com/sguzman/ferrua/actions/runs/39963411673). Lexical acceptance remains narrower than full Lua compiler conformance.

## Bounded control-flow bytecode experiment

The separate first-party [`owned_flow.rs`](../src/owned_flow.rs) module now compiles limited integer/Boolean Lua chunks into **stack-based control-flow bytecode** with runtime branches, `while`, `repeat/until`, numeric `for` loops, scoped locals, parallel assignment semantics, and a required integer return value. The prototype has an explicit instruction-step budget and no host-side I/O. Numeric `for` control expressions are evaluated once before the read-only loop variable becomes visible; both step directions are supported, with zero-step rejection and loop-boundary overflow handling.

This is a **second experimental execution IR**, separate from the [register-based integer expression prototype](../src/owned_vm.rs). Unifying them into a complete Ferrua-owned Lua compiler/VM is still pending. Functions, generic iterator execution, closures, tables, strings, metatables, full Lua value semantics, coroutines, standard libraries, and production integration remain outside the accepted scope.

- [CI #39965337560](https://github.com/sguzman/ferrua/actions/runs/39965337560): bounded conditionals and loops passed Linux tests, Rust 1.96, composite consumer and Lua 5.4.6 differential checks.
- [CI #39966547309](https://github.com/sguzman/ferrua/actions/runs/39966547309): numeric `for` scopes and overflow tests passed all four jobs, including upstream Lua fixture comparisons.

### Lua lazy logical operands

The bounded control-flow bytecode also implements Lua's lazy `and` and `or` operators. Each returns the selected original operand (which may be an integer, Boolean, or nil), and the unneeded RHS is not evaluated. The VM respects Lua truthiness, including the fact that **0 is true**. Calls and the full Lua runtime remain unsupported. [CI #39967285721](https://github.com/sguzman/ferrua/actions/runs/39967285721) passed all four jobs and upstream Lua 5.4.6 fixture comparison.

### Shared integer semantics in bounded control flow

The stack-based control-flow prototype reuses Ferrua's existing Rust-owned integer division, modulo and logical shift semantics from the register VM. The supported subset now covers `//`, `%`, `&`, `|`, bitwise `~`, `<<`, `>>`, and unary bitwise `~`, including negative operands, negative shift counts and full-width hexadecimal bit patterns. Division and modulo by zero return explicit errors; incompatible operand types still reject execution. Lazy logical branches do not evaluate skipped arithmetic. The additional parity fixtures compare accepted integer results with production `fer` and upstream Lua 5.4.6, and do not imply support for floats or arbitrary Lua values.

### Lexically nested Lua break statements

The experimental chunk parser now recognizes `break` only inside a loop and only as the final statement of its immediate block, including nested conditional branches. The bounded control-flow VM emits deferred jumps to exit the innermost `while`, `repeat/until`, or numeric `for` loop. Breaking out of a `repeat` skips its `until` condition; exiting a numeric loop skips its normal iteration step. Generic `for` execution remains unsupported, even though its syntax parses. The new examples are checked against production `fer` and upstream Lua 5.4.6.
