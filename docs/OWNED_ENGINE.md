# 🔩 Ferrua-owned language engine

The working fer command currently runs on the pinned third-party pure-Rust luna-core compiler/VM. The experimental owned Rust library modules **do not execute Lua**.

- ferrua::lex::tokenize records Lua keyword, name, number, literal and symbol tokens with original spelling, byte offset and line.
- ferrua::expr::parse_expression uses a Pratt parser for standalone literal/name expressions, unary and binary operators, parentheses, Lua 5.4 precedence and right associativity.
- Neither is a complete Lua grammar parser, a compiler, or a register VM. Expression parsing does not support statements, calls, indexing, tables or anonymous functions.

## Evidence and limits

The lexical integration suite scans 27 first-party Lua scripts (automation, examples, and reference conformance fixtures), checks source-token slicing, source offset monotonicity, and line number ordering. Rust unit tests verify numeric boundaries, comments, escapes, parser precedence, diagnostic offsets, and invalid input.

The production runtime is also compared separately against the upstream Lua 5.4.6 reference interpreter, but those tests **do not demonstrate that the experimental lexer/parser can execute Lua**.

## Graduation

1. Finish all lexical corner cases and first-class parser diagnostics.
2. Implement full statement grammar and scope analysis with golden ASTs.
3. Implement bytecode compilation, VM, closures, coroutines, tables, GC and standard library semantics.
4. Expand reference differential tests for the new engine.
5. Only replace the production backend after language and regression acceptance; do not prematurely label MVP or v1.
