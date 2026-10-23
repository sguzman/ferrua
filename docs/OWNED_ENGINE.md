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

### First-party name binding subset

`ferrua::scope::analyze(source)` assigns binding slots to locals in the supported semicolon-delimited chunk grammar. It reports identifier references as local or global and as reads/writes, including shadowing semantics where the initializer of a newly declared local sees the preceding binding, not the new binding. It now resolves nested do/end blocks and independent conditional/while/for/repeat scopes. Full function scopes, upvalues, closures, goto/labels, and Lua's complete grammar remain unimplemented. [CI #39955169495](https://github.com/sguzman/ferrua/actions/runs/39955169495) validated the analyzer with all four jobs green.

### Scoped control flow and loops (experimental)

The owned AST now covers bounded `do/end`, `if/elseif/else`, `while/do`, `repeat/until`, numeric `for`, and generic `for/in` for its documented expression/statement subset. Newlines may separate those supported statements. Scope analysis recognizes separate branch and loop frames, including numeric/generic iterator bounds evaluated before their locals exist, and `repeat` conditions evaluated **inside** the body frame. The new syntax tests do not imply that the owned VM can execute any of these constructs; production still uses `luna-core`.

**Acceptance:** [nested/conditional #39956479889](https://github.com/sguzman/ferrua/actions/runs/39956479889), [repeat/newline #39956981511](https://github.com/sguzman/ferrua/actions/runs/39956981511), [numeric for #39957386939](https://github.com/sguzman/ferrua/actions/runs/39957386939), and [generic for #39957918722](https://github.com/sguzman/ferrua/actions/runs/39957918722) each passed all four CI jobs. All are limited source-analysis features, not full grammar/VM acceptance.
