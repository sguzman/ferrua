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
3. Extend runtime value types, Lua tables/metamethods, control-flow bytecode, garbage collection, coroutines and standard libraries.
4. Grow upstream Lua differential testing beyond deterministic smoke fixtures, including error semantics and real automation workloads.
5. Integrate the independent engine with `fer` only after passing the full acceptance gates. Do not claim a stable owned runtime prematurely.
