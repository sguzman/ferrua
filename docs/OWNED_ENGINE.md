# 🔩 Ferrua-owned language engine

Ferrua's normal executable `fer` currently uses the independently maintained pure-Rust `luna-core` compiler and bytecode VM. The new `ferrua::lex` and `ferrua::expr` modules are **independent Ferrua-owned experimental Rust components**, but they are **not** used to run scripts yet.

## Current independent components

- `ferrua::lex::tokenize(source)`: a lexical scanner producing token kinds, exact spelling slices, byte offsets, and line locations. Covers keywords, operators, short and long strings, comments, decimal/hexadecimal number forms, and Unix script shebangs.
- `ferrua::expr::parse_expression(source)`: a Pratt-style AST builder for standalone expressions. It covers literals, variables, parentheses, unary operators, binary operators, operator precedence, and right-associative exponentiation and concatenation.

## Explicit limitations

The lexer still needs complete Lua escape validation, locale-sensitive identifiers, numeric corner cases, and more grammar fixtures. The expression parser deliberately does not yet support function calls, indexing, tables, function literals, statement grammar, or complete Lua parsing. **Neither component is a compiler or a register-based VM**. Do not use either as the default Lua runtime or claim compatibility without differential evidence.

## Graduation requirements

1. Expand lexer/parser syntax coverage and error-location tests.
2. Construct a complete AST for Lua statements, scopes, function definitions and calls.
3. Build the independent compiler, bytecode format and execution engine.
4. Verify tables, metamethods, closures, coroutines, garbage collection and standard libraries.
5. Compare representative programs against the pinned Lua 5.4.6 reference, then add older dialect tests.
6. Switch the default `fer` backend only after the ledger's independent-VM acceptance gates pass.

The current differential corpus lives in `tests/conformance/` and is executed via `tools/diff_lua.lua`. It verifies the production third-party engine, not the new AST modules. All new Rust-owned components are unit tested separately in Linux CI.
