# 🔩 Ferrua-owned language engine

The working fer command currently runs on the pinned third-party pure-Rust luna-core compiler/VM. The experimental owned Rust library modules **do not execute Lua**.

- ferrua::lex::tokenize records Lua keyword, name, number, literal and symbol tokens with original spelling, byte offset and line.
- ferrua::expr::parse_expression uses a Pratt parser for standalone literal/name expressions, unary and binary operators, parentheses, Lua 5.4 precedence and right associativity.
- Neither is a complete Lua grammar parser, a compiler, or a register VM. Expression parsing supports only semicolon-delimited local declarations, assignments, call statements, and an optional final return through `parse_chunk`. It does not yet support complete Lua statement grammar, block scopes, method-call syntax, table constructors or anonymous functions. The supported expression subset now includes calls, chained field selection and bracket indexing.

## Evidence and limits

The lexical integration suite scans 27 first-party Lua scripts (automation, examples, and reference conformance fixtures), checks source-token slicing, source offset monotonicity, and line number ordering. Rust unit tests verify numeric boundaries, comments, escapes, parser precedence, diagnostic offsets, and invalid input.

The production runtime is also compared separately against the upstream Lua 5.4.6 reference interpreter, but those tests **do not demonstrate that the experimental lexer/parser can execute Lua**.

## Graduation

1. Finish all lexical corner cases and first-class parser diagnostics.
2. Implement full statement grammar and scope analysis with golden ASTs.
3. Implement bytecode compilation, VM, closures, coroutines, tables, GC and standard library semantics.
4. Expand reference differential tests for the new engine.
5. Only replace the production backend after language and regression acceptance; do not prematurely label MVP or v1.

**CI acceptance:** [run #39953838372](https://github.com/sguzman/ferrua/actions/runs/39953838372) passed four jobs including 27-script lexer acceptance and the expanded expression parser tests. The source scanner understands Lua's `\\z` whitespace-skipping string escape, but semantic string-escape validation is still future work. Parent T301 remains incomplete until the full language statement parser and lexical scopes exist.

**Additional parser acceptance:** [run #39954592909](https://github.com/sguzman/ferrua/actions/runs/39954592909) verifies the experimental `parse_chunk` subset and malformed statement tests. The parent full-parser/scoping task remains incomplete.
