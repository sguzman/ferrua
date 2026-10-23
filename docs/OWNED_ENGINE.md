# 🔩 Ferrua-owned language engine

The production `fer` executable still uses the pinned, independently maintained pure-Rust `luna-core` compiler and VM. Ferrua's separate experimental Rust-owned components are library APIs, and **do not replace the production backend**.

## Current independent components

- **Lexer:** [`src/lex.rs`](../src/lex.rs) tokenizes a documented Lua 5.4 subset, preserving spelling, line number and source byte offsets; [the first-party source corpus](../tests/owned_syntax.rs) covers 27 real Lua scripts.
- **Parser:** [`src/expr.rs`](../src/expr.rs) builds Pratt expression trees with operator precedence, chained calls, fields and indexing. Its limited chunk grammar includes local declarations, assignments, calls, returns, nested `do/end`, `if/elseif/else`, `while`, `repeat/until`, numeric `for`, and generic `for/in`.
- **Scope analysis:** [`src/scope.rs`](../src/scope.rs) records local bindings and name references, including block/branch/loop scopes, shadowing, parallel declaration visibility, and `repeat` conditions evaluated in the body scope.
- **Register prototype:** [`src/owned_vm.rs`](../src/owned_vm.rs) compiles and runs integer expression bytecode with bounded registers. Supported operators include unary minus, integer addition/subtraction/multiplication, Lua floor division and modulo, and integer bitwise/shift operations. Division by zero yields an explicit error. Integer overflow wraps as on the supported 64-bit Lua reference.
- **Static integer chunks:** `compile_integer_chunk` compiles straight-line integer locals, parallel assignments to existing locals, nested `do/end` scopes, and a final scalar return into the same owned register IR. An initializer sees the outer/previous binding, while parallel assignment evaluates all RHS expressions before rebinding names.

## Explicit boundaries

The parser is **not** complete Lua syntax. The original register-based integer bytecode engine supports neither Lua functions nor dynamic loops, branches, global variables, calls, Lua tables, strings, floats, metatables, coroutines, closures/upvalues, garbage collection, or a standard library. The separate stack-based flow engine has since added some of these capabilities. The static chunk compiler refuses unsupported forms instead of silently falling back to the production engine. Native script entrypoints remain powered by `luna-core`.

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

The separate first-party [`owned_flow.rs`](../src/owned_flow.rs) module now compiles a restricted integer/Boolean/nil Lua subset into **stack-based control-flow bytecode** with runtime branches, `while`, `repeat/until`, numeric `for` loops, scoped locals, parallel assignment semantics, and fixed-arity return lists. The prototype has an explicit instruction-step budget and no host-side I/O. Numeric `for` control expressions are evaluated once before the read-only loop variable becomes visible; both step directions are supported, with zero-step rejection and loop-boundary overflow handling.

This is a **second experimental execution IR**, separate from the [register-based integer expression prototype](../src/owned_vm.rs). Unifying them into a complete Ferrua-owned Lua compiler/VM is still pending. Later milestones added a bounded function/closure and positional-table subset; generic iterator execution, general table/string/value semantics, metatables, coroutines, standard libraries and production integration remain outside the accepted scope.

- [CI #39965337560](https://github.com/sguzman/ferrua/actions/runs/39965337560): bounded conditionals and loops passed Linux tests, Rust 1.96, composite consumer and Lua 5.4.6 differential checks.
- [CI #39966547309](https://github.com/sguzman/ferrua/actions/runs/39966547309): numeric `for` scopes and overflow tests passed all four jobs, including upstream Lua fixture comparisons.

### Lua lazy logical operands

The bounded control-flow bytecode also implements Lua's lazy `and` and `or` operators. Each returns the selected original operand (which may be an integer, Boolean, or nil), and the unneeded RHS is not evaluated. The VM respects Lua truthiness, including the fact that **0 is true**. Calls and the full Lua runtime remain unsupported. [CI #39967285721](https://github.com/sguzman/ferrua/actions/runs/39967285721) passed all four jobs and upstream Lua 5.4.6 fixture comparison.

### Shared integer semantics in bounded control flow

The stack-based control-flow prototype reuses Ferrua's existing Rust-owned integer division, modulo and logical shift semantics from the register VM. The supported subset now covers `//`, `%`, `&`, `|`, bitwise `~`, `<<`, `>>`, and unary bitwise `~`, including negative operands, negative shift counts and full-width hexadecimal bit patterns. Division and modulo by zero return explicit errors; incompatible operand types still reject execution. Lazy logical branches do not evaluate skipped arithmetic. The additional parity fixtures compare accepted integer results with production `fer` and upstream Lua 5.4.6, and do not imply support for floats or arbitrary Lua values.

### Lexically nested Lua break statements

The experimental chunk parser now recognizes `break` only inside a loop and only as the final statement of its immediate block, including nested conditional branches. The bounded control-flow VM emits deferred jumps to exit the innermost `while`, `repeat/until`, or numeric `for` loop. Breaking out of a `repeat` skips its `until` condition; exiting a numeric loop skips its normal iteration step. Generic `for` execution remains unsupported, even though its syntax parses. The new examples are checked against production `fer` and upstream Lua 5.4.6.

### Assignment arity, excess values and missing nils

The owned control-flow bytecode now handles ordinary fixed-count Lua local declarations and local assignments with fewer or more RHS expressions than targets. Missing values become `nil`; surplus values are evaluated and discarded, and all RHS values are evaluated before rebinding local slots. This includes zero-initializer `local x` declarations and repeated target slots. Function calls, varargs, and Lua multi-result expansion are still unsupported, so this is not complete Lua assignment semantics. The parity suite checks the supported fixed-result cases directly against production `fer` and upstream Lua 5.4.6.

### Typed values returned by bounded owned flow

The experimental `execute_flow_value` API returns the VM's currently supported `Integer`, `Boolean` or `Nil` value without losing the original short-circuit operand type. The original `execute_flow_chunk` API remains integer-only for backwards compatibility, explicitly rejecting Boolean and nil return values. The differential suite checks the text representation of all three supported return types against production `fer` and upstream Lua 5.4.6. Strings, tables, floats, functions and all other general Lua runtime values remain unsupported.

### Verified continuation acceptance

- [CI #39985589783](https://github.com/sguzman/ferrua/actions/runs/39985589783): shared integer floor/modulo/bitwise flow semantics — all four jobs passed.
- [CI #39986059053](https://github.com/sguzman/ferrua/actions/runs/39986059053): lexically scoped nested loop breaks — all four jobs passed.
- [CI #39986717040](https://github.com/sguzman/ferrua/actions/runs/39986717040): Lua fixed-result assignment arity and nil padding — all four jobs passed.
- [CI #39987091738](https://github.com/sguzman/ferrua/actions/runs/39987091738): typed Boolean/nil/integer flow return values — all four jobs passed.

These are bounded first-party prototypes, not a certified replacement for the production `luna-core` interpreter.

### Fixed-arity Lua return lists

The experimental `execute_flow_values` API now returns a list of supported values from a `return` statement, including empty and multi-value lists. It keeps the result order and the original value types. The existing `execute_flow_value` and `execute_flow_chunk` APIs explicitly reject results of any arity other than one. This adds support for syntactically fixed return lists only; evaluation of function calls, vararg expansion and full Lua multiple-result adjustment remain future work. The production `fer` and upstream Lua 5.4.6 tests compare printed multi-result forms, including empty return lists.

- [CI #39987809090](https://github.com/sguzman/ferrua/actions/runs/39987809090): fixed-arity Lua return lists and backward-compatible single-result interfaces — all four jobs passed.

### Experimental function syntax and lexical captures

Ferrua's independent expression/chunk parser now recognizes simple named and local function declarations, anonymous function expressions, parameters and terminal varargs. The syntax tree preserves function-body statements, while parser state isolates loop `break` from an enclosing function. The lexical analyzer records parameter slots and distinguishes references captured from an enclosing function (`Upvalue`) from direct locals; local function names are bound before their bodies for recursion. The Rust-owned execution backends still reject function construction and calls; this is **syntax and binding analysis**, not executable closure support or complete Lua function grammar. Full vararg execution and complete Lua compatibility are not yet accepted. Dotted function declarations and method declarations now preserve their member names; method bodies bind an implicit `self` parameter, but still cannot execute on the owned VM.

### Closure capture descriptors

The independent scope analyzer now emits ordered `FunctionInfo` descriptors for nested functions: lexical parent ID, explicit parameter names, implicit method `self`, and unique captured binding slots. Captures propagate through intermediate function scopes where needed to construct deeper closures. The descriptor is an analysis artifact; the production VM is unchanged, and Ferrua's experimental backends do not yet allocate closures or execute calls. 

### Verified function syntax and capture analysis

- [CI #39989539203](https://github.com/sguzman/ferrua/actions/runs/39989539203): function declaration/expression syntax, local recursion and basic lexical capture detection accepted in all four jobs.
- [CI #39990190628](https://github.com/sguzman/ferrua/actions/runs/39990190628): dotted/colon method AST and upstream Lua 5.4 syntax fixture parity accepted in all four jobs.
- [CI #39990585046](https://github.com/sguzman/ferrua/actions/runs/39990585046): transitive nested closure capture descriptors accepted in all four jobs.

The full parser, executable call frames and closure runtime are **not** yet complete; these checks validate bounded syntax/scoping infrastructure only.

### Table constructor syntax foundation

The Rust-owned parser now builds explicit field ASTs for empty and nested table constructors, list elements, `name=value` entries, and `[expr]=value` computed keys, including mixed comma/semicolon separators and trailing separators. Lexical analysis reads only computed keys and value expressions, not literal field names. Upstream Lua 5.4 syntax fixtures cover supported and malformed cases. Neither owned execution prototype yet allocates table values or implements metatables.

### Bounded indexed-table execution

The experimental stack-based `owned_flow` VM can now create array-style positional tables using a per-execution bounded heap, read positive integer keys and assign one computed integer-key target at a time. Aliases share the same internal table handle, and missing positive indices yield `nil`. Table identity uses a handle comparison. The allocated heap is limited in total cells and count to avoid unbounded allocation. The supported numeric results are compared against production `fer` and upstream Lua 5.4.6. General Lua keys, named fields, nil deletion, hash semantics, metatables, iteration, simultaneous indexed LHS assignment, and complete table value compatibility remain unsupported. Opaque table handles returned from the experimental API cannot be dereferenced outside a particular VM execution.

### Verified table parsing and bounded execution

- [CI #39991282612](https://github.com/sguzman/ferrua/actions/runs/39991282612): Lua table syntax AST and computed-key/name reference analysis passed all four jobs, including upstream Lua 5.4 syntax comparisons.
- [CI #39991929210](https://github.com/sguzman/ferrua/actions/runs/39991929210): bounded execution-local indexed tables, aliased mutations and missing-key semantics passed all four jobs, including upstream Lua 5.4 output comparisons.

No general hash-table behavior, arbitrary keys, metatables, or stable user-facing table API is implied by these narrow checks.

### Bounded executable functions and call frames

The first-party stack bytecode VM now supports local named functions, anonymous functions and first-class function values with runtime identity, positional arguments, isolated call-frame locals, explicit and implicit returns, and direct local self-recursion. All calls share one instruction budget and one table heap; recursion is additionally bounded by a 64-frame ceiling. Surplus arguments are ignored, absent arguments become nil, and an expression-call context currently collapses every return to its first value (or nil). This is **not Lua-complete**: lexical free-variable captures, varargs, methods, globals, expanded multireturns, and arbitrary function/table semantics remain unsupported. The production `fer` VM remains unchanged. Tests compare the accepted numeric subset with production `fer` and upstream Lua 5.4.6.

### Mutable closure upvalue cells

Closures from the bounded owned VM now capture lexically visible locals through a bounded shared-cell heap, preserving mutations after creation, sibling closures, transitive nested captures and separate factory invocations. A recursive local function observes reassignment of its own outer binding. For predictable analysis and containment, captured bindings are collected eagerly in stable name order, not optimized by liveness; the execution allocates at most 65,536 slots across its lifetime. This does not implement garbage collection, Lua globals, varargs, methods or complete multireturn semantics. Production `fer` remains untouched.

### Parenthesized expression result boundaries

Ferrua's independent syntax tree now retains parentheses as explicit `Expr::Grouped` nodes instead of erasing them during parsing. Both current execution prototypes preserve the enclosed expression's ordinary single-value arithmetic semantics, and the scope resolver traverses grouped expressions. This is necessary for future Lua multi-result adjustment because `return f()` and `return (f())` must differ. Grouped assignment targets are rejected by the restricted parser. This AST change is not itself a claim of complete multiple-return support.

### Bounded variadic functions and final-call result adjustment

The owned stack VM now expands the results of an unparenthesized final call or `...` in return and assignment lists, preserving the single-result boundary of parentheses. Variadic functions receive surplus arguments, and frame-local dynamic result counts feed `CallMany`, `AdjustTail`, and `ReturnTail` instructions. Ordinary expression calls retain the first result or nil, preserving previous behavior. Nested last-argument expansion, table-constructor tail expansion, Lua globals, general method calls and complete Lua 5.4 language semantics remain unsupported. This is an experimental runtime and does not change production `fer`.

### Tail expansion in function arguments and array constructors

The independent compiler now propagates a final unparenthesized call or `...` result list into an enclosing call's final argument position and into the last positional element of a bounded table constructor. `CallSpread` and `NewTableTail` consume the frame-local dynamic result count before constructing the final arity, while parenthesized calls remain single-valued. Upstream Lua 5.4 and production `fer` differential fixtures cover accepted numeric outcomes. Named/hash table fields, method calls, general Lua keys and full VM/runtime compatibility remain unsupported.

### Implicit function fallthrough

Every first-party compiled function ends with an implicit zero-result return, even if other branches contain explicit returns. A conditional branch therefore no longer causes `NoReturn` when its return is not taken; a call in a single-result expression context adjusts its empty result to nil. The differential fixtures cover successful and unsuccessful conditional return paths against Lua 5.4.6 and production `fer`.

### Verified closure, variadic function and result-adjustment acceptance

- [CI #39993807376](https://github.com/sguzman/ferrua/actions/runs/39993807376): bounded owned function values, calls, recursion and shared instruction budget — four jobs passed.
- [CI #39994624379](https://github.com/sguzman/ferrua/actions/runs/39994624379): mutable lexical captures, nested and independent closures — four jobs passed.
- [CI #39995260412](https://github.com/sguzman/ferrua/actions/runs/39995260412): parenthesized AST single-result semantics — four jobs passed.
- [CI #39995832249](https://github.com/sguzman/ferrua/actions/runs/39995832249): variadic arguments and return/assignment tail-result adjustment — four jobs passed.
- [CI #39996328937](https://github.com/sguzman/ferrua/actions/runs/39996328937): final call-argument and array-constructor tail expansion — four jobs passed.
- [CI #39996750649](https://github.com/sguzman/ferrua/actions/runs/39996750649): implicit zero-result function fallthrough — four jobs passed.

Earlier sections document the incremental prototypes as they existed at the time of each milestone. The current supported subset is summarized here and in the README. Neither the standalone register VM nor the flow VM has graduated into production `fer`.

### Captured-slot allocation efficiency

Execution frames now map captured lexical slots directly to their already-allocated shared cells; they allocate new heap cells only for their own parameters and local variables. The allocation is guarded against duplicated/out-of-range capture descriptors. A bounded 30,000-call regression verifies that repeated invocations with shared captured variables do not waste a duplicate heap cell for each capture. This optimization does not implement garbage collection, reclamation of returned-frame local cells, or unbounded allocation.

- [CI #39997490573](https://github.com/sguzman/ferrua/actions/runs/39997490573): captured slots now reuse their parent cells without allocating unused duplicates; 30,000-call regression and all four CI jobs passed.

### Mixed parallel lvalue assignment

The bounded owned-flow VM now resolves local and integer-indexed table destinations before evaluating any right-hand expressions. Pending destination descriptors preserve table identity and evaluated integer keys even when the RHS mutates the local that originally referenced that table. The VM evaluates the full RHS list, applies Lua's bounded final-expression result adjustment, and performs the writes afterward. This adds multi-indexed and mixed local/table assignments, including key-expression side effects, while retaining explicit memory and key-type limits. Assignment order for overlapping destinations, general table keys and full Lua assignment semantics are not claimed; supported non-overlapping examples are checked against production `fer` and upstream Lua 5.4.6.

### Bounded generic for iteration

The Rust-owned stack VM can now execute function-based Lua generic `for` loops over the restricted iterator/state/control triple. Each iteration calls the iterator with state and current control, stops on a nil first result, updates the control and read-only loop variables, and executes the loop body with `break` support. The initial iterator expression list is evaluated once with final-call result adjustment, so an iterator-factory function may supply the triple. The shared step and call-depth budgets still apply. This is not full Lua 5.4 generic-for support: the optional fourth to-be-closed value, automatic closing semantics, arbitrary iterator objects, and fresh captured loop-variable cells on each iteration remain unsupported.

### Per-iteration lexical cells for Lua loops

The bounded numeric and generic `for` VM paths now give visible loop variables distinct mutable cells per successful iteration, while keeping the hidden iteration control separate. Closures created on different iterations therefore retain the correct iteration's value, including in descending numeric loops. Allocation remains bounded by the execution-local cell cap. This is tested against production `fer` and upstream Lua 5.4.6 and does not imply general garbage collection or closing-variable semantics.

### Verified parallel assignments and generic-loop acceptance

- [CI #39997991621](https://github.com/sguzman/ferrua/actions/runs/39997991621): mixed local/indexed destination evaluation and parallel assignment semantics passed all four jobs.
- [CI #39998627491](https://github.com/sguzman/ferrua/actions/runs/39998627491): function-based generic `for` iteration, multiple variables, nil termination and `break` passed all four jobs.
- [CI #39999404758](https://github.com/sguzman/ferrua/actions/runs/39999404758): fresh lexical cells per iteration, including closures from ascending, descending, and generic loops, passed all four jobs.

These milestones cover tested bounded Lua subsets. A full independent Lua 5.4 runtime, closing iterators, general table/hash semantics, release artifacts, and native EndeavourOS acceptance remain unfinished.

### Capture-aware loop cell allocation

Fresh per-iteration cells are allocated only when a live loop-variable cell has been captured by at least one closure. The bounded VM records captured cell identifiers when constructing closures, retaining distinct values for captured iterations while reusing the same cell across ordinary noncapturing loop iterations. A 70,000-iteration numeric loop regression verifies that uncaptured variables do not waste the execution-local cell budget. The optimization does not implement garbage collection or collection of unreachable closures.

### Verified capture-aware loop allocation

- [CI #40000123176](https://github.com/sguzman/ferrua/actions/runs/40000123176): capture-aware per-iteration cell renewal and a 70,000-iteration uncaptured-loop heap regression passed all four jobs. As before, this is a bounded independent prototype, not the production interpreter or a garbage-collected Lua runtime.

### Mixed positional and computed integer table constructors

The experimental flow VM now executes table constructors combining ordered positional fields and explicit computed positive integer keys. Indexed field keys and values evaluate once in the owned VM's chosen source order. Overlapping explicit and positional assignments are deliberately excluded from cross-interpreter parity fixtures because Lua does not guarantee a single cross-implementation assignment order in that case. An unparenthesized final positional call or `...` still expands into successive positional indices with bounded heap accounting. This subset does **not** implement named fields, string keys, sparse associative maps beyond the bounded positive-integer region, full Lua length behavior, or metatables. Differential fixtures compare supported outcomes against `fer` and Lua 5.4.6.

### Bounded table length operator

The owned-flow VM now supports Lua's unary `#` operator on its bounded positive-integer table representation. For dense sequences, length is exact. For tables with holes, the runtime selects the first missing positive index as a valid border; the Lua specification does not require a unique border for sparse tables, so differential assertions use unambiguous shapes. Non-table operands remain rejected until the owned value model supports Lua strings and any relevant metamethods. Production `fer` remains unchanged.

### Immutable Lua byte-string literals

The experimental VM now interns string literals by byte content in a bounded, compile-time-owned pool. String identity in this subset is content-based, so equality between distinct quote styles compares the same bytes. Unary `#` returns the **byte count**, not Unicode character count. Supported string decoding covers long-bracket text, short-string escapes for simple controls, decimal and hexadecimal byte escapes, and the whitespace-skipping `\\z` escape. Unicode code-point escapes and general runtime concatenation remain separate work; unsupported spellings reject explicitly rather than silently accepting incorrect values. This has no effect on production `fer`.

### Bounded runtime string concatenation and ordering

The experimental byte-string pool now supports dynamically created strings through Lua's right-associative `..` operator, with integer-to-decimal-string concatenation as a supported Lua coercion. Result strings are interned by byte content with a hard 65,536-byte pool cap, preserving equality across literal and computed values. Ordered string comparisons use lexicographic byte ordering; `#` measures the actual byte length of computed strings. General numeric parsing, float strings, full Unicode escapes, garbage collection and metatables are still outside this VM subset. The working production `fer` interpreter is unchanged.

### Verified table and string value milestones

- [CI #40002668288](https://github.com/sguzman/ferrua/actions/runs/40002668288): mixed positional and computed integer-key table construction passed all four jobs; overlapping explicit/list writes are not assumed deterministic across independent Lua engines.
- [CI #40002966676](https://github.com/sguzman/ferrua/actions/runs/40002966676): dense Lua table length and bounded positive-index border handling passed all four jobs.
- [CI #40003835476](https://github.com/sguzman/ferrua/actions/runs/40003835476): immutable interned Lua byte strings, escapes and byte length passed all four jobs.
- [CI #40004295352](https://github.com/sguzman/ferrua/actions/runs/40004295352): bounded Lua concatenation, integer coercion and lexicographic byte comparisons passed all four jobs.

The independent VM still lacks general associative string-keyed tables, metatables, full function and float behavior, GC and the standard Lua runtime; no production engine switch has been accepted.

### Unicode scalar string escapes

The owned string decoder now accepts valid Unicode scalar values in Lua \u{hex} escapes and emits their UTF-8 byte sequences, including multibyte code points. Invalid scalars and unfinished or overflowing escapes reject explicitly rather than silently changing bytes. Lua also permits certain extended non-scalar escape encodings; those remain outside this narrowed compiler subset and are not claimed compatible. Upstream differential tests exercise accepted scalar escapes.

### Unicode scalar escape acceptance

[CI #40004973760](https://github.com/sguzman/ferrua/actions/runs/40004973760): Unicode scalar byte encoding, invalid-scalar rejection, UTF-8 lengths and string equality passed all four jobs. The independent runtime remains a bounded Lua subset and does not implement Lua's extended non-scalar escape encodings or a full standard library.

### Bounded Lua named and string-keyed table fields

The owned stack VM now provides a bounded string-keyed map alongside each table's positive-integer sequence. `t.name`, `t["name"]`, named constructor fields and computed string keys share byte-interned key identities; nil assignments remove string-keyed entries. Mixed named/integer fields are supported and the existing array length operator ignores string keys. Key entries count toward the same bounded table heap budget. Negative/zero/boolean/table keys, general hash semantics, metatables and method calls are not yet supported; neither garbage collection nor the production interpreter has changed.

### Receiver-aware method calls

The experimental bytecode compiler now supports Lua `object:method(arguments)` calls against function values stored in string-keyed table fields. The receiver expression is evaluated only once and passed as the first argument, including through nested and tail-expanding call contexts. An explicit stack swap preserves the callee/receiver order. Tests compare inherited call-frame, vararg, and multireturn behavior with production `fer` and upstream Lua 5.4.6. Method declaration syntax remains a separate compiler milestone.

### Dotted and colon function declarations

The bounded owned compiler now handles `function table.field(...)` and `function table:method(...)` declarations rooted in lexically visible local tables, including nested field paths. Colon declarations insert implicit `self` as the first parameter while ordinary dotted declarations leave arguments unchanged. Runtime table lookup and assignments use the existing string-key map and closure capture mechanics, so declarations retain captured lexical state and normal multi-result returns. Unbound global declarations and dynamically computed destination paths remain unsupported.
