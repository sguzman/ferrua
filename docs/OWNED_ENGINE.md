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

The [engine graduation contract](ENGINE_GRADUATION.md) defines the mandatory syntax, compiler/VM, standard library, resource safety, differential, native QA and release gates. Existing bounded stack-VM smoke tests do not approve production cutover.

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

### Bounded general Lua table key identities

The owned VM now stores integer keys outside the positive-array fast path (zero, negative and large), Boolean keys, table-identity keys, and function-identity keys in an execution-local bounded hash map. String keys share that map and compare by their interned byte value; positive in-range integer keys continue through the sequence array. Assigning nil deletes a hash entry; nil itself remains invalid for assignment keys. Table and function keys compare by identity rather than structural contents. No Lua float-key canonicalization, weak tables, garbage collection or metatables are implemented, and hash entries count toward the shared table storage budget.

### Fresh closure cells for repeated local declarations

The independent VM now distinguishes local variable **declarations** from assignments, allocating a fresh execution cell when a repeated declaration would otherwise reuse a cell retained by an older closure. This covers locals declared inside numeric/generic-for bodies, while and repeat loops, and locally recursive functions re-entered across iterations. A dedicated `RenewLocal` instruction runs before constructing each locally recursive closure, preserving that iteration's self-reference. Uncaptured local cells are still reused under the capture-aware memory policy; the shared cell budget remains enforced.

### Verified named tables, methods, hash keys and repeated-local captures

- [CI #40005930603](https://github.com/sguzman/ferrua/actions/runs/40005930603): named and computed string table keys, dotted access and assignment passed all four jobs.
- [CI #40006536721](https://github.com/sguzman/ferrua/actions/runs/40006536721): receiver-aware method calls, colon/dot method declarations and method multireturns passed all four jobs.
- [CI #40007436327](https://github.com/sguzman/ferrua/actions/runs/40007436327): generalized Lua table hash keys and fresh captured cells for repeated block-local declarations passed all four jobs.

These are accepted bounded-subset steps. Full Lua globals, metatables, arbitrary float semantics, collectable heaps, standard libraries and native engine graduation remain pending.

### Method tail-result expansion inside mixed tables

The mixed named/computed/positional table constructor compiler now treats an unparenthesized final colon method call like any other Lua multireturn call. It expands the returned values into successive positional fields, while `(receiver:method())` remains one value. Regression fixtures verify these cases against the production interpreter and upstream Lua 5.4.6.

### Bounded reuse of uncaptured function-frame cells

Execution now tracks cells allocated by each call frame and recycles only those that have **not** been captured by a closure. Each `run` invocation owns cleanup even across early returns or execution errors, with a free-cell pool shared across recursive frames. Captured upvalue cells remain pinned, protecting escaped closure state. A 70,000-invocation function-loop regression verifies that repeated ordinary calls no longer consume the lifetime cell budget, without weakening call-depth or instruction budgets. A fully collectable/garbage-collected heap and reclaiming unreachable closures remain future work.

### Verified mixed method results and uncaptured call-frame reclamation

- [CI #40008327899](https://github.com/sguzman/ferrua/actions/runs/40008327899): mixed table constructors now expand final method-call results and preserve grouped single-result semantics; four jobs passed.
- [CI #40008713256](https://github.com/sguzman/ferrua/actions/runs/40008713256): uncaptured call-frame cells are recycled on successful returns and execution errors; a 70,000-invocation stress test and all four CI jobs passed. Captured upvalues remain pinned; this is not a fully garbage-collected runtime.

### Bounded integer-string arithmetic coercion

Integer-formatted Lua byte strings now participate in integer arithmetic, integer division/modulo, and unary numeric negation. Bitwise operations intentionally retain their strict integer-operand requirement and do not coerce strings. The VM recognizes signed decimal strings and bounded hexadecimal integer strings, and dynamically concatenated numeric text can be converted as well. Malformed text fails with a type error. This deliberately excludes floating-point numeric strings and full Lua 5.4 numeric coercion corner cases; it does not change normal string comparison or table-key identity rules.

### Verified integer-string arithmetic and strict bitwise semantics

[CI #40009615631](https://github.com/sguzman/ferrua/actions/runs/40009615631): signed decimal and bounded hexadecimal integer strings now coerce for integer arithmetic, floor division and modulo, including concatenated numeric strings. Bitwise operators retain strict integer types; production `fer` and upstream Lua 5.4.6 differential fixtures passed. Full floating-point string conversion remains unsupported in this owned engine.

### Bounded decimal floating-point values and arithmetic

The independently owned control-flow VM can now hold IEEE-754 double values (stored as raw bits in the compact value enum), compile decimal floating literals, and evaluate Lua's `/` and `^` operators. Mixed integer/float arithmetic, floor division, modulo, unary negation, decimal-formatted string conversion, string concatenation, and numeric comparisons are supported in the tested subset. Integer-only `+`, `-`, `*`, `//`, and `%` retain their existing wrapping-integer behavior. Decimal float table keys, hexadecimal float literals, precise comparison of very large integers against non-integral floats, floating numeric `for`, NaN key semantics and the rest of Lua 5.4 number coercions remain separate acceptance gates; production `fer` is unchanged.

### Lua floating-point table-key normalization

The bounded table-key resolver now accepts floating keys and canonicalizes exactly integral, in-range floats to existing integer-key storage, including positive and negative zero. Nonintegral floating keys preserve their byte-level floating identity in the hashed map, while NaN is rejected as an invalid table index. Existing table and function identity keys remain unchanged. Tests verify cross-type integer/float lookups and deletions against upstream Lua 5.4.6 and production `fer`. Float arithmetic support still excludes full Lua numeric-for and hex-float parsing.

### Bounded hexadecimal floating numerals

Lua hexadecimal floating-point forms such as `0x1.8p1`, `0X.8P+2`, `0x1p-1`, and `0x10.0` now compile into IEEE-754 binary values. The Ferrua-owned parser accumulates hexadecimal whole/fractional digits and applies the optional power-of-two exponent without relying on Rust's decimal-only float parser. Exponent magnitudes are capped at 4096 to keep this prototype bounded. Integer-form hex literals continue using the existing integer path. Accepted cases have Lua 5.4.6 differential fixtures.

### Exact mixed integer/float numeric ordering

Cross-type Lua comparisons now avoid converting a 64-bit integer to an imprecise double before ordering. The bounded runtime compares integral portions exactly, then considers floating fractional sign, with explicit handling of NaN and out-of-range double magnitudes. Differential tests cover integers beyond the 53-bit IEEE-754 exact-integer window and signed 64-bit boundaries. Equality still uses the separately guarded exact integral-float comparison.

### Float-mode Lua numeric for iteration

Numeric `for` loops now choose integer mode when all three controls are integral VM values, retaining checked integer overflow behavior, and float mode when any control is floating-point. In float mode, the hidden counter and visible per-iteration variable carry IEEE-754 values and advance with double arithmetic; descending loops and fractional step values are supported. Bounded decimal numeric-string controls are coerced by the existing arithmetic operand reader, and zero steps are rejected in both modes. The existing instruction budget and captured per-iteration cell semantics remain in force. This does not claim exact handling of every Lua 5.4 float-loop edge case, including NaN and infinite steps.

### Integral floating operands for bitwise operations

The owned VM now accepts mathematically integral, representable IEEE-754 double operands in bitwise unary and binary operators, including shifts. It checks finiteness, zero fractional part and the signed 64-bit range before conversion; fractional doubles remain type errors. String operands remain rejected for bitwise operators, while decimal numeric-string coercion continues only for arithmetic. Upstream Lua 5.4.6 and production `fer` differential fixtures cover the accepted cases.

### Verified bounded floating-point acceptance

- [CI #40010815167](https://github.com/sguzman/ferrua/actions/runs/40010815167): decimal floating values, mixed arithmetic, numeric comparisons and Lua / and ^ operators accepted across four jobs.
- [CI #40011221972](https://github.com/sguzman/ferrua/actions/runs/40011221972): integral floating table-key normalization and NaN rejection accepted across four jobs.
- [CI #40011712915](https://github.com/sguzman/ferrua/actions/runs/40011712915): bounded hexadecimal float literals and binary exponents accepted across four jobs.
- [CI #40012287119](https://github.com/sguzman/ferrua/actions/runs/40012287119): precision-safe comparisons between large signed integers and floats accepted across four jobs.
- [CI #40012977915](https://github.com/sguzman/ferrua/actions/runs/40012977915): fractional Lua numeric for loops and exact integral-float bitwise operands accepted across four jobs.

These statuses cover only the recorded bounded behavior. Full Lua metamethods, numeric edge cases, complete float/string coercion, general garbage collection, standard libraries and native production-engine graduation remain unfinished.

### First-class pure Lua type builtin

The experimental owned runtime now represents the standard Lua `type` function as a first-class built-in callable. A reference to `type` compiles as a value only when no lexical binding of the same name exists, so local shadowing preserves Lua lexical behavior. The callable returns interned category strings for nil, Boolean, number (integer and float), string, table and function values. Its execution uses the existing instruction/stack and byte-pool limits. This deliberately narrow core-builtin path does not provide a general global environment, string formatting, Lua metatables, or side-effecting host functions; production `fer` remains unchanged.

The `type` builtin requires at least one argument, matching Lua's missing-value error behavior; an explicit nil argument still returns the string `"nil"`. Extra arguments are ignored as in Lua's base library.

### Bounded first-class Lua tostring builtin

The experimental engine now exposes `tostring` as a first-class, lexically shadowable pure callable alongside `type`. It preserves existing string values, renders nil/Boolean/integer and ordinary double values into the bounded runtime string pool, and emits stable identity labels for tables and functions. A missing argument is rejected; additional arguments are ignored. Lua's `__tostring` metamethod and exact implementation-specific formatting of addresses, very large floats, NaN and infinity remain unsupported, so differential fixtures assert only the defined common subset. Production `fer` is unchanged.

### Bounded first-class Lua tonumber conversion

The owned runtime now exposes `tonumber` as a pure, lexically shadowable callable. Numeric inputs remain numeric; decimal and hex integer strings and bounded decimal/hex floating forms parse to VM numbers, and nonnumeric input yields nil. The optional explicit base supports signed 64-bit strings in bases 2 through 36, with invalid digits producing nil. A missing argument yields nil. Full Lua overflow/wrap semantics for extremely large base-number strings, locale-sensitive parsing and every Lua 5.4 conversion corner case remain unsupported. Both production `fer` and Lua 5.4.6 differential fixtures cover the accepted subset.

### First-class raw table functions

The independent stack VM now exposes first-class, lexically shadowable `rawequal`, `rawget`, and `rawset` functions. Raw equality preserves numeric integer/float equivalence, string byte-content equality, and table/function identity. Raw reads and writes use the same bounded table storage and canonicalized numeric keys as regular table access, and `rawset` returns the input table. Nil values remove entries, with invalid assignment keys rejected. Full metamethod bypass semantics cannot be tested until metatables exist; these are narrowly verified builtins, not a complete Lua base library. The production `fer` interpreter remains unchanged.

### Bounded first-class rawlen

The owned stack VM now exposes a first-class, lexically shadowable `rawlen` builtin. It returns byte lengths for strings and a valid sequence border for the current bounded Lua table representation, sharing the existing table-length logic rather than introducing inconsistent implementations. It rejects unsupported operand types and preserves the existing execution budgets. Metamethods are not yet implemented, so raw-versus-metamethod length differences remain untested. Production `fer` remains unchanged.

### Accepted core Lua conversion and raw-table builtins

- [CI #40014074135](https://github.com/sguzman/ferrua/actions/runs/40014074135): first-class, shadowable `type` passed all four CI jobs.
- [CI #40014718963](https://github.com/sguzman/ferrua/actions/runs/40014718963): bounded `tostring` value formatting passed all four jobs.
- [CI #40015672831](https://github.com/sguzman/ferrua/actions/runs/40015672831): first-class `tonumber` plus `rawequal`, `rawget`, and `rawset` passed all four jobs, after correcting the missing-argument error semantics and maintaining upstream Lua differential parity.
- [CI #40016120064](https://github.com/sguzman/ferrua/actions/runs/40016120064): bounded `rawlen`, using the shared underlying Lua byte-string and table-border length handler, passed all four jobs.

These checks cover only the recorded, bounded first-party runtime subsets. There is still no full Lua standard library, garbage-collected production VM, metatable implementation, or accepted engine migration.

### Bounded first-class Lua select and multiple results

The independent VM now supports first-class, lexically shadowable `select` for counting arguments with `select('#', ...)` and selecting tail argument lists with positive or negative integer offsets. The builtin reuses existing single-result versus multiple-result call contexts, so returned tails expand in final return, assignment, call argument and table constructor positions. It accepts the limited existing numeric-string to integer conversion path; zero and out-of-range negative offsets reject explicitly. All result allocations remain bounded. This is not a full Lua standard-library implementation, and production `fer` remains unchanged.

### First-class bounded assert and Lua result preservation

The owned VM now exposes first-class, lexically shadowable `assert`. When the first value is truthy, it returns every passed argument without collapsing the list; the existing Lua multiple-result adjustment rules apply in return, assignment, call argument and table constructor positions. On false, nil or missing conditions the prototype emits `FlowError::TypeError` rather than constructing Lua's full message-bearing exception; failure-string fidelity and traceback semantics remain outside this bounded subset. Production `fer` is unchanged.

### Verified Lua argument selection and assertions

- [CI #40017051129](https://github.com/sguzman/ferrua/actions/runs/40017051129): first-class `select` argument counting, positive/negative offsets, and bounded multiple-result expansion passed all four CI jobs, including upstream Lua 5.4.6 and production `fer` comparisons.
- [CI #40017471285](https://github.com/sguzman/ferrua/actions/runs/40017471285): first-class `assert`, truthy argument preservation and false/nil rejection passed all four CI jobs. The experimental engine still uses a generic error type rather than reproducing Lua's message and traceback semantics.

Neither milestone implies complete Lua standard-library support or graduation of the first-party interpreter into production.


### Verified bounded raw table iteration

The experimental stack VM now includes first-class, lexically shadowable `next`, `pairs` and `ipairs`. `next` returns raw key/value pairs from the bounded array and hash stores, or nil at exhaustion; invalid previous keys are rejected. `pairs` yields a `next` iterator, the table and initial nil control; `ipairs` yields a sequential raw integer iterator starting at zero and stopping at the first nil. They reuse the existing generic-for control flow, including distinct iteration-local captures. The hash portion has unspecified order, and tests deliberately avoid depending on its order. Repeated `next` scans consume the shared execution budget.

[CI #40024909050](https://github.com/sguzman/ferrua/actions/runs/40024909050) passed all four primary jobs, including upstream Lua 5.4.6 differential fixtures, production `fer`, Rust 1.96, formatting and lint checks. It does not implement `__pairs`, `__index`, metatables, mutation-safe traversal, to-be-closed generic-for iterator values, or complete Lua resource semantics. The production `fer` backend is unchanged.


### Verified bounded table metatable storage

The experimental stack VM now carries an optional metatable handle on each bounded table. Its first-class, lexically shadowable `getmetatable` and `setmetatable` functions can attach a table metatable, remove it with nil, return the original table, and expose the existing metatable by identity. If the metatable has a non-nil raw `__metatable` key, `getmetatable` returns that protected value and `setmetatable` refuses replacement until the protection is removed. Unit and upstream Lua 5.4 differential fixtures cover the common table-only behavior.

[CI #40025867923](https://github.com/sguzman/ferrua/actions/runs/40025867923) passed all four primary jobs, including Lua 5.4 differential checks, after precise Rust formatter corrections. It does **not** implement `__index`, `__newindex`, arithmetic metamethod dispatch, `__len`, `__pairs`, primitive string metatables, garbage collection, or general Lua metatable compatibility. It does not change production `fer`.


### Verified table-backed index and assignment metamethod chains

The owned VM now attempts raw table indexing and assignment first, then follows table-valued `__index` or `__newindex` fallbacks for absent fields. It preserves the distinction between ordinary Lua field operations and `rawget`/`rawset`, including simultaneous prepared indexed assignments and methods resolved through a fallback table. Each chain consumes the VM's shared instruction budget and has a fixed maximum traversal depth; cyclic fallback graphs return an explicit error rather than running indefinitely. Unit and upstream Lua 5.4 differential fixtures exercise table-backed chains.

[CI #40026650737](https://github.com/sguzman/ferrua/actions/runs/40026650737) passed all four primary jobs, including upstream Lua 5.4.6 differential tests, plus aggregate status reporting. Function-valued `__index`/`__newindex` callbacks are tracked separately below. Arithmetic/comparison metamethods, `__len`, `__call`, primitive metatables, and complete Lua error messages remain unsupported. No change is made to production `fer`.

### Verified function-valued indexing and assignment metamethods

The bounded table interpreter can now invoke a first-party Lua closure stored in `__index` with `(table, key)` or in `__newindex` with `(table, key, value)`. It uses the **same** closure executor as ordinary calls, including lexical captures, parameter and vararg adjustment, frame cleanup, recursion limits and the shared execution budget. An `__index` callback contributes only its first returned value; `__newindex` ignores return values. Existing raw table slots still bypass assignment callbacks, while `rawget`/`rawset` bypass both metamethods. Table-valued fallback chains continue to work.

Regression and Lua 5.4 differential fixtures cover method calls through fallback functions, exactly-once assignment effects, shadowing/captures, raw updates, recursive callbacks, missing/extra returned values and nested table fallback chains. Only **owned Lua function closures** are accepted as callable metamethods in this increment; built-in callables, table `__call`, arithmetic/comparison metamethods and full Lua metatable semantics are still outside its verified scope. Production `fer` remains pinned to `luna-core`. [CI #40028891108](https://github.com/sguzman/ferrua/actions/runs/40028891108) passed all four primary acceptance jobs plus status reporting, including differential fixture comparisons with Lua 5.4.6.

### Verified function-valued table length

Lua's ordinary `#table` operator now consults a table's raw `__len` metatable entry. First-party Lua closure values are called through the existing bounded frame and instruction path with the table passed twice, matching the supported Lua 5.4 callback contract. Only the first result contributes to `#`, including a possible nil; no new host callback machinery is introduced. Without a metamethod, the existing valid table-border semantics apply. `rawlen(table)` and UTF-8 **byte** length of ordinary strings remain independent and unchanged.

[CI #40029449968](https://github.com/sguzman/ferrua/actions/runs/40029449968) passed the four primary verification jobs and aggregate status reporting, including upstream Lua 5.4 differential fixtures. `__len` via a callable table or a host builtin is not yet accepted, nor are string metatable overrides, other arithmetic metamethods or garbage-collected production integration. Ordinary table `__call` dispatch is tracked separately below. The production `fer` executable continues using the pinned `luna-core` runtime.

### Verified callable Lua tables and __call forwarding

Normal `Op::Call`, `Op::CallMany` and `Op::CallSpread` can now resolve a table's raw `__call` metatable entry. The original called table is prepended to its existing argument list before invoking the metamethod, and nested table-valued `__call` entries repeat this Lua-compatible forwarding rule. Callbacks ultimately delegate to the existing first-party closure or builtin code path, preserving return-list adjustment, captures, varargs and lexical call depth. Each forwarding step consumes the shared instruction budget, with a maximum chain length and stack quota so cyclic callable tables return a bounded error.

The CI-accepted unit and Lua 5.4 differential fixtures cover aliasing a callable table as a first-class value, extra argument forwarding, multiple returns, builtin call targets, nested callable-table chains, and invalid or cyclic metamethods. The change does not add userdata, full arithmetic/comparison metamethods, garbage collection or host-side function callbacks. Production `fer` is unchanged.

[CI #40030079883](https://github.com/sguzman/ferrua/actions/runs/40030079883) passed all four primary jobs plus aggregate status reporting, including upstream Lua 5.4.6 differential fixtures.

### Verified bounded arithmetic metamethods

The experimental owned stack VM now attempts the existing Lua numeric coercion path first for binary addition, subtraction, multiplication, true division, floor division, remainder and exponentiation. Only an operand type error causes `__add`, `__sub`, `__mul`, `__div`, `__idiv`, `__mod` or `__pow` fallback lookup. The left table is consulted before the right. Supported metamethods are first-party Lua functions, invoked with both original (uncoerced) operands through the already bounded closure executor. Only the first return contributes to a binary expression. Arithmetic errors such as division by zero are not swallowed by fallback dispatch.

[CI #40031328390](https://github.com/sguzman/ferrua/actions/runs/40031328390) passed Linux quality gates, Rust 1.96, action-consumer tests, Lua 5.4 differential fixtures and aggregate status reporting. It does not yet add arithmetic metamethod callbacks backed by tables/builtins, unary metamethods, bitwise metamethods, `__concat`, ordering/equality metamethods, primitive metatables, or full Lua conformance. The production `fer` executable continues to use the pinned external VM.

### Verified bounded Lua unary metamethods

Unary negation (`-value`) and bitwise complement (`~value`) now retain their accepted integer/float behavior and use first-party table metamethods only when the original operand cannot be converted directly. The respective `__unm` and `__bnot` callbacks are invoked via the existing bounded Lua closure executor. Following Lua 5.4's unary metamethod convention, the operand is passed twice. Only the first return contributes to the expression, and recursive callbacks share the VM's call-depth and step limits.

The CI-accepted regression and Lua 5.4 differential fixtures cover duplicate arguments, ordinary numeric operators, extra/no callback results, lexical captures and recursion limits. Builtin/callable-table metamethods, additional binary bitwise dispatch, primitive metatables and full language parity remain outside this increment. Production `fer` remains unchanged.

[CI #40031937707](https://github.com/sguzman/ferrua/actions/runs/40031937707) passed all four primary jobs and the machine-readable status reporter for unary metamethod semantics.

### Verified bounded binary bitwise metamethods

The owned VM now tries its existing integer-only bitwise operation first for `&`, `|`, binary `~`, `<<`, and `>>`. If conversion fails with a type error, table operands are considered left first for `__band`, `__bor`, `__bxor`, `__shl` or `__shr` respectively. First-party Lua function callbacks execute through the shared bounded closure executor with both original operands, and only the first callback result contributes to the surrounding expression.

[CI #40032378817](https://github.com/sguzman/ferrua/actions/runs/40032378817) passed all four primary jobs and aggregate status reporting, including upstream Lua 5.4 differential cases. It does not add bitwise metamethods for callable-table/builtin values, generalized primitive metatables, comprehensive coercion of all numeric-string representations or remaining comparison metamethods. Production `fer` remains unchanged.


### Verified bounded Lua byte concatenation metamethods

The independent stack VM concatenates Lua byte strings and numeric values directly, retaining its bounded interned-string budget. If direct concatenation fails on operand types, it resolves `__concat` from a left-hand table before a right-hand table and invokes the first-party Lua closure with the original operands. The existing callback executor enforces frame depth and execution limits, and only the first returned value contributes to the concatenation expression. Ordinary string concatenation does not consult metatables.

Regression and Lua 5.4 differential fixtures cover both operand orders, callback return adjustment, byte concatenation, nil returns, and recursion. [CI #40033236408](https://github.com/sguzman/ferrua/actions/runs/40033236408) passed all four primary acceptance jobs plus aggregate status reporting, including Lua 5.4 differential fixtures. Table/builtin-valued `__concat`, generalized primitive metatables, and full Lua conformance remain outside the scope. The production `fer` backend stays pinned to `luna-core`.

### Verified table comparison metamethod callbacks

The independent interpreter retains direct numeric and byte-string ordering. Distinct tables can use function-valued `__eq`, `__lt`, and `__le` metamethods through the existing bounded first-party closure executor. Identical table handles compare equal without calling `__eq`; `~=` negates the callback's Lua truthiness. The `>` and `>=` operators reverse the operands and use the appropriate less-than metamethod. When `__le` is absent, `<=` or `>=` can fall back to negating the reversed `__lt` result. A missing callback is distinguished from a present but unsupported metamethod value.

The unit and upstream Lua 5.4 differential fixtures cover primitive comparisons, table identity, distinct-table equality, one-sided callback lookup, reversed operand ordering, nil/false/truthy results and recursion bounds. [CI #40033892089](https://github.com/sguzman/ferrua/actions/runs/40033892089) verified the comparison subset through all four primary jobs and the aggregate reporter, including Lua 5.4 differential fixtures. Callable tables and builtin metatable callbacks, complete primitive metatables, garbage collection, error-message conformance and native first-party production integration remain future gates.

### Verified Lua __pairs custom iteration

The owned Lua runtime's first-class `pairs(table)` function now checks a raw `__pairs` metatable entry before falling back to `next, table, nil`. A first-party Lua function is invoked with the table; its results are adjusted to exactly three iterator-triple positions (iterator, state, initial control), matching Lua 5.4. The normal generic-for compiler and VM then execute that iterator without a special loop path. `ipairs` is unchanged, and direct `next` and raw table access do not consult `__pairs`.

The pending regression and upstream Lua 5.4 differential fixtures cover custom iterators, lexical captures, missing/excess return values, generic-for updates and recursive hook limits. [CI #40034746881](https://github.com/sguzman/ferrua/actions/runs/40034746881) accepted custom iterator triples with Lua 5.4 differential tests and all four primary jobs. Callable-table/builtin metamethod values, resource-closing iterator values and complete standard-library/metatable semantics remain outside its scope. Production `fer` stays on the pinned external interpreter.

### Verified native-stack-safe recursive Lua callbacks

The bounded owned VM caps recursive Lua calls and table-metatable fallback chains at 16 levels. This conservative experimental cap is intentional: the widening opcode dispatcher increased Rust host stack use, and CI exposed a native stack abort before the prior 64-level guard could respond. The smaller cap allows the owned engine to return `CallDepthExceeded` rather than aborting the process. Tests specifically recurse through `__index`, `__add`, `__pairs` and `__len` to guard against the regression.

[CI #40034746881](https://github.com/sguzman/ferrua/actions/runs/40034746881) confirmed recursive callback guards safely return a VM error, with all four primary acceptance jobs and reporter passing. The recursion cap is a bounded-prototype constraint, not a claim that this runtime matches Lua's general recursion capacity. Production `fer` continues to use the pinned external interpreter.


### Verified Lua table stringification hooks

The owned Lua runtime's first-class `tostring(table)` builtin can now invoke a first-party function-valued `__tostring` field on the table's raw metatable. The original table is passed as the callback's sole argument through the existing bounded closure executor. Only the first return value is used; Lua byte strings and numeric callback results are converted to string results. Extra callback results are discarded; missing or non-convertible (for example, Boolean) results raise the experimental VM's type error. Without `__tostring`, the existing bounded table identity string is retained. Primitive `tostring` behavior is unchanged.

The pending unit and upstream Lua 5.4 differential fixtures cover lexical captures, string return validation, multiple returns, fallback stringification and recursive callback limits. [CI #40036891366](https://github.com/sguzman/ferrua/actions/runs/40036891366) passed all four primary jobs and the status reporter, including Lua 5.4 differential fixtures. Primitive metatable overrides, callable-table metamethods, Lua-compatible error strings and full runtime conformance are still outside the accepted subset. Production `fer` remains pinned to the existing external engine.


### Verified Lua ipairs metatable-aware iteration

Lua 5.4's `ipairs` iterator indexes successive positive integer fields through ordinary table indexing, which may invoke a table's `__index` fallback for a missing array element. The owned iterator now uses bounded `lookup_with_metatable` like normal indexed expressions. Iteration stops at the first nil; `ipairs` ignores the separate `__pairs` hook; `rawget` and `next` still bypass indexing metamethods. [CI #40037327110](https://github.com/sguzman/ferrua/actions/runs/40037327110) accepted table/closure fallback, array-hole, termination and raw-access parity fixtures.

### Verified Lua label grammar and bounded same-block goto

The first-party parser accepts `::name::` labels and `goto name` statements as explicit AST nodes, requires valid identifiers, and rejects duplicate labels within one lexical block. The experimental stack compiler also lowers **same-block forward and backward jumps when the active local-binding count at the source equals the target count**. These are real bounded `Op::Jump` instructions, not ignored syntax. Unit tests cover valid same-block control flow, undefined targets, jumps into or across local lifetimes, cross-block rejections, and infinite backward jumps stopped by the instruction budget. [CI #40039264425](https://github.com/sguzman/ferrua/actions/runs/40039264425) passed these owned VM regressions and Lua 5.4 differential fixtures.

The **historical same-block restriction was lifted by T404**. The expanded supported subset now resolves lexical ancestor labels and rejects jumps into new locals, but full upvalue closing and the complete Lua source-position/label-scope compatibility contract remain unverified. Unsupported forms return an explicit compiler error rather than silently skipping the jump. This bounded subset does not complete T301/T302 grammar/compiler graduation; production `fer` retains the pinned external VM.

### Verified deterministic generated numeric differential corpus

The Rust integration test now constructs 128 reproducible arithmetic, floor-division/remainder, bitwise, and comparison Lua chunks from a fixed-seed, non-cryptographic integer generator. Generated chunks use bounded input values with no external data, Python scripts, network or unstable random seed. The **same** fixture runner executes these programs through the Ferrua-owned VM and the working production `fer` CLI; the Lua 5.4 CI job additionally compares output to the upstream reference interpreter. A separate unit assertion checks generator determinism.

[CI #40041470699](https://github.com/sguzman/ferrua/actions/runs/40041470699) passed all four primary acceptance jobs and the aggregate reporter, including upstream Lua 5.4 comparisons for all 128 generated chunks. Generated integer expressions complement the curated semantic cases and must not be treated as comprehensive Lua conformance, especially for parser errors, metatables, coroutines, GC, standard libraries or native production graduation.

### Verified negative-path syntax and runtime parity

A curated deterministic corpus now exercises intentional failures across the first-party parser and bounded owned interpreter. Syntax fixtures must fail at `compile_flow_chunk`; runtime fixtures must compile successfully but yield an execution error. The same sources are invoked against both the production `fer` backend and the upstream Lua 5.4 interpreter when enabled in differential CI. The oracle verifies rejection and process exit status, rather than pretending the first-party VM already reproduces Lua's exact diagnostic text or stack traces. Cases include malformed control flow and table constructors, illegal operands, invalid table keys, builtin misuse and missing callable targets.

[CI #40042287118](https://github.com/sguzman/ferrua/actions/runs/40042287118) passed all four primary jobs and the status reporter, including upstream Lua 5.4 rejection outcomes. Correct rejection of a curated fixture is only a partial language/error conformance result: error objects, protected-call restoration, tracebacks, GC, coroutines, complete grammar and first-party default runtime integration remain independently gated.

### Verified bounded Lua error values and protected closure calls

The experimental owned VM now recognizes first-class `error` and `pcall` values, retaining explicit Lua errors as a typed `FlowError::Thrown(Value)` until a protected call handles them. The bounded `pcall` implementation can invoke owned Lua closures and the `error`/`assert` builtins, preserve success multiple-return values and lexical captures, and produce a `false, error-value` pair on recoverable failures. Generic VM type failures are converted to an interned diagnostic string, but no claim is made that its wording matches Lua 5.4. Explicitly thrown numbers, strings, Booleans and nil remain distinguishable. Exhausted steps, heap limits and protected-call depth are **not caught**: the experiment must retain its hard safety budgets.

The pending Rust and Lua 5.4 differential fixtures cover thrown-value identities, closure argument forwarding, multiple results, nested protected calls and state changes before an error. General callable-table/builtin parity, Lua's default assertion error message, full `xpcall` error-handler re-entry, traceback generation, standard error levels, coroutine/error interactions and production cutover are still open. [CI #40042884149](https://github.com/sguzman/ferrua/actions/runs/40042884149) passed all four primary jobs and the aggregate reporter, including Lua 5.4 protected-call differential fixtures. The production `fer` binary continues using the pinned `luna-core` backend.

### Verified Lua assertion failure error values

The bounded owned `assert` builtin now throws the optional second argument as a Lua error object when its condition is false or nil. With no message, it interns the Lua-compatible default string `assertion failed!`. Both direct calls and protected `pcall(assert, ...)` preserve explicit `nil`, Boolean, numeric and string error values. Successful assertions still forward all original values.

The CI-accepted Rust and Lua 5.4 differential fixtures exercise default and custom error messages, success-result adjustment, protected failure and direct `FlowError::Thrown` propagation. [CI #40043618093](https://github.com/sguzman/ferrua/actions/runs/40043618093) passed all four primary jobs plus aggregate reporting. The zero-argument assertion correctly reports a missing-argument VM error, and protected nested assertions permit source-qualified diagnostics. Exact diagnostic stack traces, source line error levels, full `xpcall` handler recursion, arbitrary protected builtins and comprehensive exception parity remain separate open gates.

### Verified bounded Lua xpcall error-handler callbacks

The experimental owned Lua VM now exposes first-class `xpcall(f, handler, ...)` alongside `pcall` and `error`. The existing first-party protected closure path calls `f` with forwarded arguments and returns `true` with all successful results. On recoverable exceptions, a first-party Lua closure handler receives the original error value: explicitly thrown numbers, Booleans, strings and nil retain their Lua value identity. The handler's first return value becomes the error object after `false`; remaining returns are discarded. Generic bounded runtime faults are represented by a string diagnostic, without claiming byte-for-byte Lua 5.4 message parity.

The protected target subset covers owned Lua closures, `error`/`assert` builtins and callable-table `__call` chains with bounded receiver forwarding. Recoverable errors raised inside a message handler are now returned to that same handler with the new Lua error object. Iterative re-entry is bounded to 16 attempts to protect the native Rust stack; exhaustion produces `error in error handling`. This matches a limited Lua 5.4 re-entry subset, not its exact recursion capacity, stack unwinding or diagnostic text. Arbitrary builtin targets, callable tables as handlers, stack traceback capture, error location levels and coroutines remain unsupported. Execution-budget, allocation and call-depth exhaustion are never swallowed by `xpcall`. This bounded implementation does **not** authorize switching production `fer` from its pinned interpreter.

Rust regressions and twelve differential Lua fixtures exercise argument forwarding, multiple return adjustment, thrown-value transformation, generic runtime errors, bounded handler re-entry, handler failure, lexical handler state and recursion limits. [CI #40044851537](https://github.com/sguzman/ferrua/actions/runs/40044851537) passed the extended three-fixture handler-recovery matrix, Linux tests/lint, Rust 1.96 and composite action acceptance. [CI #40044530094](https://github.com/sguzman/ferrua/actions/runs/40044530094) passed Linux formatting/Clippy/tests and Arch packaging, Rust 1.96, upstream Lua 5.4 differential parity, composite consumer, and reporter.

### Verified callable-table targets for protected calls

The experimental owned VM's `pcall` and `xpcall` can now invoke tables whose raw metatable supplies `__call`. As in ordinary Lua calls, a table is inserted as the first argument when following its `__call` metamethod. Table-valued metamethods can chain through another table, forwarding each receiver in order; the resulting first-party closure receives the original arguments and all inserted receiver values. The same multiple-result adjustment and error-object preservation rules apply to protected calls.

Lookup follows the shared instruction budget, a maximum of 16 table redirects, and the bounded stack size. Cyclic callable tables return `CallDepthExceeded` as a **hard VM limit**, even inside `pcall`. This is necessary for native-stack safety and does not claim Lua's general recursion compatibility. The error-handler argument of `xpcall` still supports first-party Lua closures, not arbitrary callable values. Full builtin dispatch, primitive metatables and default-engine integration remain open.

The accepted Rust unit regression and six reference fixtures cover single and chained callable tables, `self` forwarding, multiple returns, thrown numeric errors, successful `xpcall` handler transformations, missing `__call` and safe cyclic rejection. [CI #40045174135](https://github.com/sguzman/ferrua/actions/runs/40045174135) passed formatting/Clippy/tests, Rust 1.96, upstream Lua 5.4 differential parity, Arch archive smoke, composite action consumer and aggregate status. Production `fer` remains pinned to the existing independent backend.

### Verified strict Lua 5.4 xpcall handler type checking

Upstream Lua 5.4 requires the `xpcall` message handler to be a **function value**, and checks the handler before executing the protected target. A table with a valid `__call` metamethod is still rejected in the handler position, even though callable tables are valid as the protected `f` target. Ferrua now makes this distinction explicitly. Missing, nil, ordinary table and callable-table handlers raise an immediate bounded VM type error without executing `f`; the current experimental VM does not reproduce Lua's exact argument-number diagnostic.

Owned Lua closures and function-valued builtins remain acceptable handler types, with successful target execution and error transformation subject to the documented `protected_target` subset. Five intentional runtime rejection cases exercise both upstream Lua 5.4 and production `fer`, accompanied by positive fixtures for valid closure and selected builtin handlers. [CI #40045839543](https://github.com/sguzman/ferrua/actions/runs/40045839543) passed all five jobs including reference-differential success and failure cases.

This is a verified semantic correction, not support for arbitrary callable handlers. It does not widen the VM's native recursion safety limits, complete Lua's error traceback behavior or authorize production-engine graduation.

### Verified selected Lua core builtins in protected calls

The experimental owned VM's `pcall` can now invoke the existing first-party `type`, `tonumber`, `rawlen` and `rawequal` functions through its bounded protected-target dispatcher. The same selection is usable as `xpcall` message handlers because those builtin values are actual function values. Ordinary Lua numeric raw equality continues to compare integer/float values with numeric semantics; `rawlen` bypasses metatable hooks. Protected invalid-arity or operand failures produce `false, error` for `pcall`; exhausted budgets remain uncatchable.

Ten curated fixtures cover successful type and number conversion, invalid numeral returning nil, raw length, numeric raw equality, builtin error handlers and protected invalid arguments. [CI #40046376588](https://github.com/sguzman/ferrua/actions/runs/40046376588) passed Linux formatting/lint/tests, Rust 1.96, upstream Lua 5.4 differential tests, Arch packaging, composite consumer and aggregate status.

This is a deliberately enumerated subset, **not** general protected invocation of all Lua builtins. arbitrary standard-library calls, error stack traces, coroutines and release cutover remain future conformance gates.

### Lua 5.4 numeric __tostring callback returns

The bounded owned VM now converts string and numeric `__tostring` callback results to Lua byte strings. This corrects an observed discrepancy: the earlier first-party VM rejected numeric returns, whereas both the pinned production `fer` backend and upstream Lua 5.4 accept them. The numeric case is restored to the shared differential suite, including integer and fractional returns. Boolean results remain an invalid callback return type. These behaviors are tested separately from the normal primitive `tostring` conversions and retain the existing bounded callback execution and recursion safeguards.

The initial incorrect discrepancy classification was superseded by actual Lua reference output. [CI #40047343376](https://github.com/sguzman/ferrua/actions/runs/40047343376) accepted corrected numeric coercion, native and protected regressions, Lua 5.4 differential parity and all release-safety jobs. No full standard-library conformance or owned-engine graduation is claimed.

### Verified shared protected Lua tostring execution

Ordinary `tostring` and protected `pcall(tostring, ...)` or `xpcall(error, tostring, ...)` use the same `Execution::tostring_builtin` dispatch. The first-party `__tostring` callback is evaluated once, with only its first result used. The bounded VM follows Lua 5.4 by allowing byte strings and numeric callback results converted to Lua byte strings, while rejecting nil and Boolean results. Metamethod recursion still respects the native-stack call-depth safeguard. [CI #40047343376](https://github.com/sguzman/ferrua/actions/runs/40047343376) passed all five acceptance jobs after correction of an earlier incorrect interpretation of the reference oracle. The production `fer` backend is unchanged.

### Verified protected Lua raw table and metatable builtins

The experimental owned interpreter routes `pcall(rawget, ...)` and `pcall(rawset, ...)` through the same first-party raw table helpers as ordinary bytecode calls, bypassing `__index` and `__newindex` metamethod callbacks. `getmetatable` and `setmetatable` work as protected callable builtin values, preserving the existing `__metatable` protection checks and table-return conventions. `xpcall` may invoke a function-valued builtin such as `getmetatable` as its message handler, but the handler still receives only the error object, per Lua 5.4.

Fourteen curated unit and reference fixtures cover raw access, mutation, missing keys, invalid raw keys and arities, locked and unlocked metatables, protected updates, and a builtin `xpcall` handler. [CI #40051457030](https://github.com/sguzman/ferrua/actions/runs/40051457030) passed all five jobs including Lua 5.4 differential parity, Linux tests/lint and pinned Arch packaging. Capacity failures remain noncatchable VM guard conditions. Complete builtin call semantics, garbage collection and production cutover remain outside the accepted subset.

### Verified protected Lua select, next and ipairs iterator calls

The bounded owned interpreter now accepts first-class `select`, `next` and the internal `ipairs` iterator as protected-call targets, retaining their ordinary Lua variable return lists. This includes `select('#', ...)` result counts, negative selectors, iterator key/value pairs, nil termination, invalid key rejection and `ipairs`' metatable-aware indexing. `pcall` still prepends its Boolean success result and the same VM scan/instruction budgets protect both normal and protected `next` calls. The public `ipairs(table)` constructor and `pairs(table)` function are now callable protected targets through the shared iterator triple logic; they retain the bounded VM's supported metamethod subset.

Fourteen owned-VM unit and upstream Lua 5.4/production `fer` differential fixtures cover these cases, including native error paths and result adjustment. [CI #40051756734](https://github.com/sguzman/ferrua/actions/runs/40051756734) passed all five jobs for the implementation. This does not imply complete generic-for, iterator metatable or standard library parity, nor change the production backend.

### Verified protected Lua pairs/ipairs iterator construction and deferred type errors

Protected `pcall(pairs, value)` and `pcall(ipairs, value)` now use the same owned helper as ordinary calls. For tables, `pairs` executes the callable `__pairs` target (closure, function-valued builtin or table with bounded `__call`) exactly once and adjusts its returns to three iterator components; without a hook it returns `next, value, nil`. `ipairs` returns the first-party sequential iterator, original state and initial index zero, and deliberately ignores `__pairs`. The protected call prepends a successful Boolean without losing the three iterator positions. Both functions accept any **present** Lua value when producing their default iterator triple, including a number or nil; a non-table type error arises only when the default iterator is actually called. Omitted arguments remain an immediate error. Primitive host metatables, unrestricted recursion, and fully general host callable semantics are not implemented.

The CI-accepted curated matrix includes 20 new positive/error fixtures, tests that defer non-table rejection until iterator invocation, callback errors, nil/extra result adjustment, ignored `__pairs` on `ipairs`, and recursion-aware native-safe callbacks. [CI #40052438402](https://github.com/sguzman/ferrua/actions/runs/40052438402) passed all five jobs, including upstream Lua 5.4 and pinned production `fer` differential parity. This does not graduate the owned interpreter into the production CLI.

### Verified bounded nested protected pcall/xpcall targets

The first-party `pcall` and `xpcall` builtin function values can now be targets of other protected calls. These calls preserve Lua's leading success Boolean, nested Boolean/error results, multiple values, side effects, and first-class error handlers. A dedicated `nested_protected_builtin` helper accounts for each nested protected builtin frame using the same conservative `MAX_CALL_DEPTH` guard as Lua closure invocations; its depth counter is restored on both success and recoverable errors. Exhausted call depth remains a hard, noncatchable VM limit.

Fifteen curated success/error fixtures verify `pcall(pcall, ...)`, `pcall(xpcall, ...)`, `xpcall(error, pcall, ...)`, nested `assert` and numeric conversions, and correct result adjustment. A separate dynamically constructed regression nests protected builtin values beyond the configured depth limit, requiring `FlowError::CallDepthExceeded` rather than a native Rust-stack abort. [CI #40052701456](https://github.com/sguzman/ferrua/actions/runs/40052701456) accepted the implementation with full Linux, Rust 1.96, production `fer`, upstream Lua 5.4 and downstream action tests.

This bounded subset is not general exception unwinding, complete error tracing, unrestricted recursion, or permission to migrate the production `fer` binary to the experimental VM.

### Verified callable __pairs metamethods

The shared first-party `pairs_ipairs_builtin` calls the `__pairs` metatable entry as a regular Lua callable value via `Execution::protected_target`. The bounded subset supports owned closures, builtin functions and callable tables with one or more `__call` redirects, forwarding the original table as the callback argument. Its result is adjusted to three iterator-triple values whether invoked normally or inside `pcall`. The `ipairs` path still ignores `__pairs` and iterates sequential integer indices through normal table lookup.

Each `__pairs` callback consumes a bounded host frame even if a builtin references `pairs` recursively; cyclic callable-table chains also terminate with a hard VM call-depth error instead of overflowing the Rust host stack. Twelve curated differential fixtures and two internal recursion tests validate callable receivers, chained `__call`, builtin `next`/`ipairs` targets, ignored hooks, thrown error propagation, malformed callbacks and triple adjustment. [CI #40053300659](https://github.com/sguzman/ferrua/actions/runs/40053300659) passed all five jobs against Lua 5.4, production `fer`, Linux packaging and consumer smoke. This extends the experimental owned VM only; no production switch is implied.

### Verified callable-table and builtin __tostring callbacks

The owned `tostring` builtin now invokes `__tostring` as a Lua callable value through `Execution::protected_target`, rather than requiring an owned closure specifically. It supports callable tables with bounded `__call` redirects and selected first-party builtin function callbacks, forwarding the original object and preserving the first return value. Accepted string and numeric results continue to convert to Lua byte strings. Nil, Boolean and invalid callback values raise recoverable errors; explicit thrown objects are preserved by `pcall`. Each callback call consumes a native-stack-bounded host frame, including recursion through a builtin `tostring` metamethod, while cyclic callable tables terminate with the shared hard `CallDepthExceeded` error.

The 12 new shared Lua 5.4/production `fer` differential fixtures cover direct and chained callable tables, numeric returns, `type` and `error` builtins, invalid return types, thrown values and `xpcall` error-handler invocation. Two local limit regressions cover builtin self-recursion and cyclic `__call`. [CI #40053556576](https://github.com/sguzman/ferrua/actions/runs/40053556576) passed all five jobs. Earlier passages in this design document describing closure-only `__tostring` behavior are historical milestones and are superseded by this verified extension. Production `fer` remains pinned to the existing backend.

### Verified callable Lua __len callbacks and error(message, level) validation

The experimental owned VM's `length_with_metatable` dispatch now accepts first-party owned closures, builtin functions and bounded callable tables (including chained `__call`) as `__len` metamethods. It forwards the length operand twice, per Lua's metamethod calling convention; only the first result is used. The first-party `rawlen` builtin continues to bypass `__len`. Recursive length callbacks, including callable-table cycles and closure self-recursion, return a hard VM call-depth error rather than aborting the Rust host stack.

The regression suite exposed a shared `error(message, level)` issue: Lua passes the `__len` operand as its second callback argument, and the `error` builtin validates an optional second argument as an integer-convertible stack level. The bounded owned VM now validates this argument in both ordinary and protected calls (allowing nil and integer-convertible values), rejects invalid level types, and preserves the original thrown Lua error value when the level is valid. Source-location prefixes for textual errors are still not fully emulated; this is not complete traceback parity.

Twelve length callback fixtures, eight additional level-validation fixtures and two hard-limit regressions are accepted by [CI #40054039668](https://github.com/sguzman/ferrua/actions/runs/40054039668), with upstream Lua 5.4 and pinned production `fer` differential comparisons, Rust 1.96, Linux tests/lint/archive smoke and the consumer action all green. Production `fer` is unchanged.

### Verified callable Lua binary and unary metamethod targets

The experimental owned VM now resolves the first present left-then-right table metatable field for binary arithmetic, bitwise, concatenation, equality and ordering operations as a Lua `Value` rather than requiring an owned closure handle. The `invoke_binary_metamethod` dispatcher then invokes that value through the same bounded callable rules as other first-party functions, supporting Lua closures, selected builtin functions and callable tables with chained `__call`. Numeric precedence, raw table identity, return-value adjustment, equality truthiness, swapped greater-than operands and `__le` fallback to inverted `__lt` behavior remain intact. The shared dispatcher also enables callable values for unary `__unm` and `__bnot`, which pass a duplicated operand.

The callback invocation consumes a host-stack-bounded frame, so recursive binary metamethods and cyclic callable-table redirects produce hard `CallDepthExceeded` errors, even under `pcall`. Noncallable present metamethod values raise recoverable Lua operand errors instead of falling back to the other operand. The 16 new differential fixtures cover callable `__add`, chained `__sub`, builtin `__mul`, numeric arithmetic/bitwise callbacks, `__concat`, `__eq`, `__lt`, `__le` and fallback ordering, thrown errors, invalid callback values and identity bypass. Two internal cycle-limit regressions guard the Rust host stack. [CI #40054377514](https://github.com/sguzman/ferrua/actions/runs/40054377514) passed all five acceptance jobs against upstream Lua 5.4, production `fer`, Rust 1.96 and Linux packaging. This is an owned-VM compatibility extension only; it does not imply full Lua metatable coverage or production graduation.

### Verified function-valued __index/__newindex builtin metamethods

The owned VM's existing table-index and table-assignment fallback now invokes builtin-function entries in `__index` and `__newindex` through the same bounded first-party protected dispatcher as other Lua callable metamethods. The callback receives the original table and key, plus the assigned value for `__newindex`. Its first return determines an indexed read; assignment ignores callback return values. The shared host-frame guard preserves an uncatchable call-depth limit for recursively self-referential closures.

**Lua table precedence remains essential:** if a raw metamethod entry is a table, including a table that also has `__call`, Lua follows table field read/assignment redirects, not a `__call` invocation. Existing raw keys bypass `__index`/`__newindex` as before; rawget/rawset do not invoke either hook. Eighteen upstream Lua 5.4 and production `fer` reference fixtures cover builtin `type`, `rawget`, `rawset`, `error`, redirected tables with callable metatables, existing-key bypass and error-level argument validation. Two hard-depth unit regressions protect against recursive fallback callbacks. [CI #40055237278](https://github.com/sguzman/ferrua/actions/runs/40055237278) passed all five jobs.

Primitive host metatables, unrestricted callable semantics, native memory management, coroutine parity and production owned-engine graduation are still outstanding.

### Verified enclosing-block Lua goto and active local lifetimes

The first-party flow compiler now gives each parsed statement block a stable lexical ID and parent. It records labels and goto jump sites with active local-slot identities, then resolves jumps after compiling the complete function. A goto may target the nearest visible label in its own or an enclosing block, whether the target is forward or backward; a label in a sibling, child or another function is not visible. The destination's active locals must be a subset of the bindings already present at the goto source: jumps into newly introduced bindings are rejected, while legal exits from nested scopes and backward jumps across declarations are preserved. The runtime uses the same bounded `Op::Jump`, with infinite backward loops still stopped by the shared instruction budget.

Thirteen curated success fixtures cover nested `do`, `if`, `while`, numeric and generic `for` bodies, shadowed locals, label shadowing and backwards re-entry. Seven negative cases cover unknown and inaccessible labels, duplicate labels and illegal jumps into a new local lifetime. The differential test harness now reports exact source text when a valid fixture cannot be compiled. [CI #40055943072](https://github.com/sguzman/ferrua/actions/runs/40055943072) passed all five acceptance jobs, including upstream Lua 5.4 and production `fer` comparisons.

This is a larger language-compiler increment than the earlier same-block subset but **does not** establish complete Lua goto compatibility: exact upvalue closing, to-be-closed variable lifetimes, line-aware diagnostics and every lexical corner case remain graduation gates. T301/T302 are not marked accepted, and production `fer` retains its pinned backend.

### Verified same-line statements without mandatory Lua separators

Lua permits consecutive statements without semicolons and without newline boundaries whenever its tokens can be parsed unambiguously as complete statements. The Ferrua-owned `Parser::block` no longer requires a semicolon, a newline or a label between successive statement nodes. It continues to recognize explicit semicolons (including empty separators), lexical block terminators and final `return` restrictions. This resolves previously rejected forms such as `local a=1 local b=2 return a+b`, consecutive same-line assignments, and immediately following a `do/end` or function definition with another statement.

Ten parser acceptance cases, 14 first-party execution regressions and the upstream Lua 5.4 / pinned production `fer` differential suite are verified in [CI #40056402369](https://github.com/sguzman/ferrua/actions/runs/40056402369), with all five jobs green. Lexical tokenization remains authoritative for valid boundaries; malformed statements and code after a final `return` continue to fail. This corrects a specific grammar restriction but does **not** complete T301's full Lua parser, source-position diagnostics, syntax corpus or native owned-engine graduation.

### Verified Lua break statements before additional loop-body statements

The first-party Lua parser no longer treats `break` as a mandatory final statement of its lexical block. Lua 5.4 permits statements, local declarations and labels following a `break` in the same syntactic loop body; those subsequent statements are unreachable when the break executes. The compiler still lowers `break` to its bounded exit jump, so loop termination is unchanged. A `break` outside a loop remains invalid, and `return` remains restricted to the end of its syntactic block.

Ten new owned VM/regression and upstream Lua 5.4/pinned production `fer` differential cases cover unreachable assignments, local declarations, labels, nested `if`, while, repeat and numeric for exits. [CI #40056833051](https://github.com/sguzman/ferrua/actions/runs/40056833051) passed all five checks after formatting was corrected. This extends the experimental parser and bytecode VM only, not production `fer` or complete Lua grammar certification.

### Verified Lua shorthand table and string call arguments

The Ferrua-owned `Parser::call_arguments` recognizes Lua's three legal function-call argument forms: parenthesized lists, a single table constructor, or a single short/long string literal. These forms work for ordinary expressions and colon-method calls, including chained returns, table/field indexing and receiver forwarding. `f{x=7}`, `f"abc"`, `f[[text]]`, `obj:run{x=7}` and valid chained variants compile through the existing first-party call opcodes without switching to production `fer`.

The regression suite verifies 12 accepted and six rejected parser expressions, plus 11 execution fixtures checked against upstream Lua 5.4 and pinned production `fer`. Malformed call tails and unterminated literal arguments remain syntax errors. [CI #40057286292](https://github.com/sguzman/ferrua/actions/runs/40057286292) passed all five jobs, including Rust 1.96, formatter/Clippy, Linux release/archive smoke and the downstream composite consumer. Full Lua parser parity, source-position diagnostics and owned-engine graduation remain open (T301/T302/T304). Production `fer` is unchanged.

### Verified Lua local attributes and const assignment protection

The first-party lexer/parser now accepts Lua 5.4 local declaration attributes in the `local name <const>` and `local name <close>` forms, recording an attribute for each declared name. Unknown/malformed attributes are rejected. The experimental stack-flow compiler supports **`<const>` bindings**: the initial assignment is permitted, subsequent reassignment of that binding is rejected, shadowed writable locals stay independent, and nested closures inherit write protection for captured constant bindings. A constant table reference still allows mutations to the table's fields; `<const>` protects the binding, not recursively all referenced values. Seven positive flow fixtures, five rejected assignments/attributes and 11 parser syntax cases are part of the matrix.

**`<close>` is syntax-only in the owned parser.** The flow compiler rejects it with an explicit unsupported error because Lua's to-be-closed local scope-exit and exception unwinding semantics are not implemented. The separate integer register prototype also rejects local attributes rather than silently ignoring them. The scope analyzer recognizes the expanded AST but does not assert finalizer or attribute-lifetime parity. This prevents accidental interpretation of syntactic acceptance as working resource finalization.

[CI #40057835794](https://github.com/sguzman/ferrua/actions/runs/40057835794) passed all five jobs, including upstream Lua 5.4 and production `fer` parity for the accepted subset, Rust 1.96, formatter/Clippy/tests and isolated Arch archive smoke. This is not a full T301/T302 or native graduation acceptance; production `fer` retains its pinned backend.

### Lexical scope metadata for Lua local attributes

The experimental `src/scope.rs` analyzer now records each parsed `<const>` or `<close>` declaration attribute on the corresponding binding. It marks writes to lexically resolved `<const>` locals even when the write occurs through a nested closure's captured upvalue. A writable shadowed binding remains independent and is not flagged; reads and local initialization are not post-declaration writes. A dedicated regression checks captured writes, shadowed writes, reads and the attribute records. [CI #40112914625](https://github.com/sguzman/ferrua/actions/runs/40112914625) passed the code and formatting fix on `main`.

This is analyzer metadata only, not a source diagnostic or new runtime acceptance gate. The analyzer does not implement `<close>` finalization, label/lifetime validation, or a release-grade compiler; `<close>` remains explicitly unsupported by the first-party flow compiler. A follow-up scope-analysis change now also flags writes to captured or local `<close>` bindings with `Reference.close_reassignment`, independently of `const_reassignment`. This matches Lua 5.4's rule that a to-be-closed local is constant-like after initialization. It remains metadata only; the flow compiler still rejects `<close>` and no `__close` lifetime behavior is claimed. The production `fer` backend remains unchanged.

### Read-only Lua <close> binding metadata

A `<close>` local is read-only after initialization under Lua 5.4 §3.3.8. The experimental lexical analyzer therefore reports `Reference.close_reassignment` on direct writes and writes through nested closure captures, without confusing it with the separate `const_reassignment` flag. Writable same-name shadowed locals, ordinary reads and table field mutations remain permitted in metadata. The scope module's old comments suggesting function scopes are unsupported were corrected; it now explicitly documents that goto lifetime rules and finalizers are beyond this pass. The test cases use spaces between attribute `>` and assignment `=` to avoid Lua's `>=` token. [CI #40113575729](https://github.com/sguzman/ferrua/actions/runs/40113575729) passed all five jobs after formatting and test token corrections.

This does not implement scope-exit `__close` execution. The first-party flow compiler continues to explicitly refuse to compile `<close>` locals and the production `fer` interpreter remains pinned.

### Lua local declarations: at most one <close> attribute

Lua 5.4 (§3.3.7) allows several local bindings with optional `<const>` and `<close>` attributes, but no more than one to-be-closed binding within a **single declaration**. The owned `src/expr.rs` parser now rejects a second `<close>` while keeping valid mixed `<const>`/`<close>` declarations; different local declarations may each contain a closer. Two valid and three invalid mixed-attribute parser regressions exercise this rule. [CI #40114100152](https://github.com/sguzman/ferrua/actions/runs/40114100152) passed all five jobs after rustfmt alignment.

The grammar check does not implement resource finalizers: `<close>` remains syntax-only in the first-party parser and explicitly unsupported by the owned flow compiler. `<const>` assignment protection and scope metadata stay independent; production `fer` is unchanged.

### Verified Lua 5.4 malformed numeral boundary checks

The owned `src/lex.rs` scanner now rejects numeral tokens touching identifier characters, underscores or extra radix points. Cases such as `12abc`, `0x1g`, `1e3foo`, `3.4.5`, and unspaced `1..2` are malformed Lua numerals; the scanner must not silently split them into separate number, name, or concatenation tokens. The parser and flow compiler inherit those lexical rejections, and diagnostics retain the precise byte offset of the malformed suffix.

Valid whitespace-separated `1 .. 2`, hexadecimal binary exponents, ordinary arithmetic after a numeral and decimal exponent signs remain accepted. The implementation has lexical and parser regressions plus six negative-path fixtures compared against production `fer` and the pinned Lua 5.4 reference. [CI #40116606712](https://github.com/sguzman/ferrua/actions/runs/40116606712) passed all five jobs, including Linux formatting/lint/tests, Rust 1.96, Lua differential smoke and the setup action consumer.

This closes a specific T301 lexical gap, **not** the full grammar, source diagnostics or T301/T302 graduation gates. The production backend is unchanged.

### Parser end-of-file diagnostics use source positions

The first-party `src/expr.rs` parser now tracks the actual physical EOF line rather than reporting line 1 whenever a closing token is missing after the last available token. A source-only pass counts CRLF and LFCR as one newline, along with standalone CR/LF; token-level failures still report the offending token's original source byte offset and line. Missing `end`, unterminated expressions, and EOF following trailing comments now produce meaningful source ending positions. The `eof_syntax_errors_report_real_lua_line_and_final_byte` regression covers multiline constructs, comments, CRLF, non-EOF failures and both expression/chunk parser entry points.

[CI #40117211253](https://github.com/sguzman/ferrua/actions/runs/40117211253) passed Linux format/lint/unit tests, Lua 5.4 differential smoke, Rust 1.96 and downstream action acceptance. This is a precise T301 diagnostic improvement, not comprehensive syntax-diagnostic parity or full first-party runtime graduation.

### Lua 5.4 long-bracket line ending normalization

The first-party flow runtime now converts every physical newline spelling inside Lua long-bracket strings to a single LF byte: CRLF, LFCR, lone CR and lone LF. The optional initial line break immediately after the opening delimiter is discarded before the remaining body is normalized, matching Lua's string literal lexical rules. Bytes other than line terminators, including UTF-8 source text, are preserved unchanged.

Eight unit inputs check newline forms and repeated breaks; eight owned/production/reference differential fixtures check byte lengths, equality against escaped short strings, first-newline removal and Unicode source text. [CI #40117631335](https://github.com/sguzman/ferrua/actions/runs/40117631335) passed all five jobs, including Linux tests, formatter, lint, the Lua 5.4 reference comparison, Rust 1.96 and setup action consumer. The production interpreter remains pinned to `luna-core`. This is a bounded string semantic increment, not full compiler or Lua standard-library parity.

### Verified original Lua UTF-8 escape range

Lua 5.4's short-string `\\u{...}` escape encodes a code-point value using its original UTF-8 byte encoding and allows any unsigned value no greater than `0x7FFFFFFF`. Unlike Rust `char::from_u32`, the Lua operation accepts surrogate-range values and values above `0x10FFFF`, producing binary Lua strings that need not be valid Unicode scalar sequences. The first-party `src/owned_flow.rs` decoder now implements one- through six-byte encoding directly, without changing valid modern UTF-8 results or treating these Lua byte strings as Rust `String` instances.

Nine direct byte-array regressions cover NUL, ASCII, two-byte, surrogate, maximum Unicode and extended four-/five-/six-byte values. Seven additional cases compare resulting Lua string byte lengths and byte equality with both the pinned Lua 5.4 reference and production `fer`. Values at or above `0x80000000` are still rejected by the lexer as malformed. [CI #40118094275](https://github.com/sguzman/ferrua/actions/runs/40118094275) passed all five jobs after the old surrogate-rejection test was correctly updated.

This proves the stated `\\u` escape subset only; it does not establish complete text/UTF-8 library support, a production engine switch, or general runtime graduation.

### Generated valid/invalid Lua grammar interaction corpus

The owned compiler differential test harness now generates 48 additional deterministic syntax/program cases instead of relying only on isolated hard-coded feature fixtures: 32 valid combinations and 16 invalid ones. Valid cases combine local shadowing, calls with table/string shorthand, table constructors, functions and captures, varargs, method declarations/calls, conditionals, numeric/repeat/while loops, goto/label resolution, constant declarations and parallel assignments. Invalid cases exercise captured `<const>` writes, prohibited jumps into a local scope, duplicate labels, unterminated blocks, invalid table fields, multiple `<close>` declarations and `break` outside a loop.

Both accepted results and rejected-source status are compared against the pinned production `fer` interpreter and upstream Lua 5.4. The generator is deterministic and includes explicit count and repeatability tests; it adds no runtime randomness, network dependency or new external language tooling. [CI #40118756930](https://github.com/sguzman/ferrua/actions/runs/40118756930) passed all five jobs.

These 48 cases improve the **generated syntax** component of the T301 graduation evidence. They do not prove all lexical/grammar combinations, complete error diagnostics, GC/coroutine behavior or register VM coverage; T301 and T302 remain open.

### Conservative Lua expression parser recursion guard

The standalone first-party expression parser now rejects recursive expression descent beyond 64 frames. Nested grouping, repeated unary operators, right-associative power expressions and nested table constructors fail with `Lua expression nesting limit exceeded` rather than allowing unbounded Rust host-stack recursion. Depth is decremented after successful and failed parsing. This is separate from the existing 64-level statement-block guard. The regression checks both standalone `parse_expression` and `parse_chunk`, and retains successful shallow nested expressions.

[CI #40119363138](https://github.com/sguzman/ferrua/actions/runs/40119363138) accepted Rust test, lint, Rust 1.96, upstream Lua comparison, Arch archive and consumer checks. The 64-frame cap is intentionally more restrictive than all Lua 5.4 inputs and does not satisfy the full T301 parser acceptance gate. Long, iteratively constructed expression chains and other parser resource surfaces require separate testing. The production `fer` interpreter remains unchanged.

### Verified bounded iterative expression AST chains

Recursive Pratt expression descent is not the only route to deep first-party AST nodes. Long left-associative operator sequences and postfix suffix chains of calls, fields, method calls and indexes are constructed iteratively and previously bypassed the recursive parser limit. `Parser::expression_inner` and `Parser::postfix` now independently account for these nested boxed-expression chains against the conservative 64-level parser safety threshold, including the currently active expression recursion depth.

The regression checks deeply repeated arithmetic operators, dot selectors, bracket indexes, ordinary calls and method calls, including parsing as the last expression in a Lua `return` chunk, and verifies shorter chains still parse. [CI #40119727210](https://github.com/sguzman/ferrua/actions/runs/40119727210) passed all five jobs. The limit is an intentional bounded-prototype deviation from unrestricted Lua expression lengths, not language-completeness evidence; the production `fer` runtime remains unchanged. Cross-function block nesting is a separately monitored parser surface.

### Parser-wide block recursion limit across nested function bodies

Named and anonymous Lua function bodies previously entered the parser's block reader with a reset local lexical-depth argument. A chain of nested function declarations could therefore evade the separate per-lexical-block limit and expand the Rust parsing stack. `Parser::block` now maintains a shared `block_depth` counter around `block_inner`, incrementing for every parsed block and restoring it on success or early error. A conservative 64-frame cap applies even where function parsing resets a lexical-depth parameter. Ordinary local scope and function boundaries are otherwise unchanged.

A regression constructs 120 nested named functions and expects `Lua block nesting limit exceeded`; smaller depths still produce valid ASTs. [CI #40120107157](https://github.com/sguzman/ferrua/actions/runs/40120107157) passed all five acceptance jobs. This is a bounded research-parser safety control, not proof that the entire Lua 5.4 block grammar is implemented or that the owned VM is ready for production.

### Register-VM nested do-block integer return propagation

The independent integer-register prototype `src/owned_vm.rs` can now compile a single integer-valued `return` inside nested `do ... end` scopes. A returned register propagates through the enclosing compile frames, and no subsequent unreachable statements in the same chunk are emitted. This preserves integer values from shadowed locals and nested parallel assignments. The parser validates the full source before lowering, so syntactically invalid unreachable code is still rejected. No general value type, branch opcode or coroutine behavior has been added.

The acceptance suite includes eleven Rust unit scenarios plus ten cases exercised through the production `fer --lua=5.4` executable and upstream Lua 5.4 reference when enabled by the differential CI environment. [CI #40120936256](https://github.com/sguzman/ferrua/actions/runs/40120936256) passed all five jobs after a formatting-only correction. Returns involving nil, multiple values, global names, arbitrary calls and unimplemented control-flow remain unsupported. The separate owned stack-flow VM and production `fer` are unaffected; T302's complete register VM and coroutine gates remain open.

### Independent integer-register VM: exhaustive conditional returns

The bounded integer-register prototype now lowers a deliberately narrow `if` / `elseif` / `else` subset into first-party `Compare`, `BranchIfFalse` and `Return` bytecode. Every arm must return a single integer-valued expression; otherwise compilation rejects the chunk. Supported guards are Lua `true`/`false` literals and signed-integer `==`, `~=`, `<`, `<=`, `>`, `>=` comparisons. No integer-to-Boolean coercion is exposed as general Lua value semantics: bare numeric guards and non-exhaustive if-statements remain unsupported, avoiding the error of treating integer zero as Lua false. Branch-local declarations are scoped separately, and a selected branch exits the chunk before evaluating other branches. Invalid backward or out-of-bounds branch bytecode is rejected.

Fifteen deterministic cases exercise true/false branches, all six comparisons, `elseif` selection, lexical shadowing, nested `do` returns, nested `if`, and integer expressions; each is checked against production `fer` and upstream Lua 5.4. Unit tests verify unsupported guards, missing-return paths and invalid jumps. The preceding const-binding regression used lexically malformed `<const>=` tokens, which the Lua lexer correctly treated as `>=`; test fixtures now use `<const> =` without weakening constant-reassignment expectations. [CI #40121950453](https://github.com/sguzman/ferrua/actions/runs/40121950453) passed all five jobs.

The standalone register VM still has no general Lua value tagging, conditional fallthrough/merge, dynamic calls, loops, closure support, coroutines, or garbage collection. The owned stack-flow VM and production `fer` backend are unchanged. T301/T302 graduation gates remain unaccepted.

### Register-VM numeric Lua truthiness and unary not guards

In the bounded independent integer-register prototype, `if` guards can now use any expression already lowerable to an integer, including zero-valued literals, integer locals, and arithmetic results. An internal `NumericTruthy` instruction reads the evaluated operand and produces the Boolean guard `true`, reflecting Lua's rule that **all numbers are truthy, even zero**. It does not expose a fake integer representation of general Lua Boolean values. A separate internal `BooleanNot` guard supports `not` on Boolean literals, nested `not`, integer operands, and supported integer comparisons. Evaluating a numeric guard still propagates runtime faults, including integer division by zero, instead of silently treating the expression as a constant.

Twelve new differential samples compare selected branch results with production `fer` and pinned upstream Lua 5.4. A dedicated Rust regression verifies that condition evaluation errors remain visible. [CI #40122551608](https://github.com/sguzman/ferrua/actions/runs/40122551608) passed all five jobs. Unsupported table/userdata/nil guards, general value-tagged Booleans, `and`/`or` expression semantics and non-exhaustive conditional fallthrough are still open; the production CLI and owned stack-flow VM remain unchanged.

### Short-circuit and/or guards in the integer register prototype

The first-party bounded integer register compiler can now lower `and` and `or` when their results are consumed **only as conditional guards**. It uses forward-only `BranchIfFalse` and `Jump` control transfers, plus a single `BooleanCopy` merge register, to skip evaluating operands that Lua would not evaluate. This preserves precedence (`and` binds more tightly than `or`), nesting with `not`, and Lua numeric truthiness (including zero). The same intentional-failure test confirms that a skipped division by zero is not evaluated, whereas a selected one still raises `DivisionByZero`. All jumps are checked for valid forward instruction positions; register values are validated as initialized on the executed path.

Fourteen deterministic positive fixtures compare the independent register-VM result with both the production `fer` backend and pinned upstream Lua 5.4. Two additional Rust regressions verify error propagation in evaluated operands. [CI #40123272148](https://github.com/sguzman/ferrua/actions/runs/40123272148) passed all five jobs. The result of `and` or `or` is internally lowered to a Boolean guard only; this does **not** implement Lua's general expression-value semantics, which return an original operand. The compiler still requires exhaustive integer-returning branches and does not yet support arbitrary conditionals, calls, coroutine state or native-engine graduation.

### Register-VM guard clauses with a later integer fallback

The bounded independent integer-register prototype now compiles an `if` / `elseif` statement without `else` when each explicit arm returns a single integer. A condition that does not select an arm falls through to subsequent statements in the enclosing block, which must eventually produce a single integer return. This extends the previously accepted exhaustive conditional-return subset without adding general merge registers or arbitrary conditional side effects. True branches still return immediately, false guards patch to the next test or following statement, and nested `do` scopes continue to propagate returns through the enclosing function.

Eighteen curated examples cover true/false, numeric zero truthiness, chained `elseif`, nested `do`, reassignments on the fallthrough path, skipped arithmetic errors, and multiple guard statements. Missing final returns and explicit nonreturning arms remain unsupported rather than silently producing nil or leaking register state. [CI #40124323318](https://github.com/sguzman/ferrua/actions/runs/40124323318) passed all five jobs with upstream Lua 5.4, production `fer`, Rust 1.96, lint/test and Arch consumer checks.

Each explicit conditional arm still has to return; compiling assignments to enclosing bindings across nonreturning branches will require genuine register-state joins before expanding this subset. Neither the owned stack-flow VM nor the production CLI is changed.

### Register-VM returning branch lexical-state isolation

The experimental integer-register compiler now lowers each `if`, `elseif` and `else` returning arm against an independent snapshot of the enclosing static local bindings. Previously, an assignment such as `x=9` inside an arm that was never executed could update the compiler's live binding map for a sibling return, causing that other arm to read an uninitialized register. Every returning arm exits the chunk, so its lexical rebindings do not need to merge back into any later path. The fix preserves assignment semantics *inside* the selected return arm and prevents skipped assignments from contaminating sibling branches or guard-clause fallthrough.

Fourteen regressions cover taken/skipped mutations, sibling `elseif` assignments, `else` returns, nested `do` blocks and later fallback paths, compared with both production `fer` and upstream Lua 5.4. [CI #40124937687](https://github.com/sguzman/ferrua/actions/runs/40124937687) passed all five jobs. Nonreturning branches with outward mutable assignment still require register-state merges and remain outside this accepted prototype subset; no full SSA/value join or production engine cutover is claimed.

### Register-VM nonreturning conditional arms without outward assignments

The bounded integer register prototype now permits `if` / `elseif` / `else` arms that perform local-only work and then rejoin a later integer return. When an arm does not return, the compiler inserts a forward `Jump` past all remaining sibling arms and patches false-guard targets to the next condition or `else`. The selected path executes local initializer arithmetic (including visible division-by-zero failures), while skipped arms do not run. Branches compile using independent snapshots of lexical bindings, so new local declarations do not escape their own scope.

At the T426 checkpoint, the former `enclosing_bindings_changed` validator rejected continuing arms that reassigned enclosing locals, including nested `do` blocks. That restriction was **superseded by T427**, which introduced path-selected register merges; the historical tests and restrictions here describe the earlier accepted boundary, not the current VM. Returning arms may still update locals inside their terminating paths. Fourteen deterministic positive differential fixtures, four compile-negative cases and selected/skipped arithmetic-error tests cover this behavior. [CI #40125718824](https://github.com/sguzman/ferrua/actions/runs/40125718824) passed all five jobs against Lua 5.4, the production `fer` backend and the Linux/Arch quality suite.

The accepted feature is an intentionally limited control-flow increment in `src/owned_vm.rs`, not full conditional assignment, general Lua value semantics, SSA construction, coroutine support or a production runtime replacement.

### Verified mutable register joins across conditional paths (T427)

The integer-register compiler now reserves bounded `MergeCopy` destinations for enclosing locals written across continuing `if`/`elseif`/`else` paths. Every active predecessor writes its selected integer value to a common destination before control flow rejoins, and later statements read that destination. Branches compile against independent lexical scope snapshots: branch-local shadow declarations do not leak, parallel assignments consume their original RHS values, and returning branches bypass subsequent join code. The interpreter validates source register initialization at each copy; malformed bytecode cannot read an unset register.

The runtime and reference matrix covers multiple mutable locals, nested `do` and nested conditional paths, early-return arms, arithmetic errors on selected paths, skipped-branch safety and immutable-local rejection. `tests/owned_vm_merge_edges.rs` supplies 24 further differential cases, four compile-negative const cases and two arithmetic-error cases. [CI #40127120133](https://github.com/sguzman/ferrua/actions/runs/40127120133) passed all five jobs with upstream Lua 5.4, production `fer`, Rust 1.96, Linux/Arch checks and the composite consumer.

### Verified sparse conditional merge allocation (T428)

The compiler now uses an iterative scan of assignments in each branch, including nested `do` and `if` blocks, and reserves merge registers **only** for mutable enclosing names that might be assigned. Read-only and constant locals keep their original registers across joins; conservative over-approximation of a shadowed name can waste one merge register but cannot silently drop an enclosing assignment. This preserves the `MAX_REGISTERS` boundary under long sequences of independent conditionals, rather than reserving every live local for every branch.

Stress regressions compile 300 conditionals with 70 simultaneously live integer locals, both with no enclosing assignments and with a sparse assignment to only one outer variable. [CI #40127305435](https://github.com/sguzman/ferrua/actions/runs/40127305435) passed all five jobs. This remains a bounded, integer-only prototype, without general typed Lua values, loops, closures, arbitrary stack frames or permission to switch production `fer` to the experimental engine.

### Verified integer register bytecode emission cap (T429)

The experimental `compile_integer_chunk` now checks the length of its emitted instruction stream before returning a `Program`. This makes the compiler and `execute_integer_program` agree on the shared `MAX_REGISTERS` instruction-count ceiling: bytecode too large for bounded dispatch returns `Error::TooLarge` during compilation, rather than being accepted as an unusable program that subsequently fails with `InvalidBytecode`.

A deterministic regression constructs 4,104 sequential no-op conditional statements, where each arm emits multiple bytecode instructions while using relatively few registers. This crosses the instruction-count ceiling without first exhausting the register count and confirms the distinction. [CI #40127939465](https://github.com/sguzman/ferrua/actions/runs/40127939465) passed Rust 1.96, Linux lint/format/unit tests, production `fer` and upstream Lua 5.4 differentials, Arch preview smoke and the downstream action consumer.

This is a compile-time guard for the bounded integer prototype, not a general whole-Lua resource quota or evidence that the owned engine can replace the production interpreter.

### Verified integer-register bytecode structural preflight (T430)

Before `execute_integer_program` dispatches any instructions, `validate_integer_program` now checks the entire finite instruction stream and its metadata: nonempty bounded code, valid register count and result register, all register operands and destinations, and strictly forward in-bounds branch targets. It validates **unreachable instructions too**, rather than allowing an early return or untaken branch to hide malformed bytecode. Runtime initialized-register checks remain in place and are not replaced by this structural validation.

One valid execution and six intentionally malformed programs cover early-return-hidden backward jumps, out-of-bounds destinations, branch conditions, merge copies, comparison operands and invalid result metadata. [CI #40128785174](https://github.com/sguzman/ferrua/actions/runs/40128785174) passed all five checks including Linux test/lint, Rust 1.96, upstream Lua 5.4 and production `fer` differential smoke, Arch preview and the composite action consumer. This is an instruction-integrity hardening of the integer prototype, not a complete verifier for general Lua values or independent-VM graduation.

### Verified per-instruction emission limit in the integer register compiler (T431)

All sixteen first-party integer register-compiler append sites now route through `emit_instruction`. The helper returns `Error::TooLarge` before adding a new instruction when the bounded `MAX_REGISTERS` instruction ceiling is reached. It covers expression lowering, Boolean short-circuit guards, branch and merge instructions, and returns; the post-compilation size check remains defense in depth. A deterministic test fills a code vector to the ceiling, verifies the next append fails without growth, removes one instruction, and confirms one legal append succeeds.

[CI #40129324922](https://github.com/sguzman/ferrua/actions/runs/40129324922) passed all five jobs, including formatting, Clippy, Rust 1.96, Lua 5.4/production `fer` differentials, native-preview Arch smoke and the downstream action consumer. This prevents a large intermediate bytecode vector, but does not claim a complete whole-language memory quota, garbage collector, coroutine implementation or runtime graduation.

### Verified deterministic nested branch-merge differential matrix (T432)

The owned integer register prototype now has a 48-case reproducible integration matrix with arithmetic-derived fixed seeds, requiring no randomized dependencies. Each script combines nested `if`/ `elseif` control flow, mutable enclosing variables, locally shadowed names, Boolean `not`/`or`, comparisons, arithmetic updates and a later register merge. The matrix compiles and executes with the first-party register VM and compares the exact resulting integer with the pinned production `fer --lua=5.4`; upstream `lua5.4` is also required when the reference CI flag is enabled.

The generated cases catch changes to path-selected register values that isolated examples could miss, but they are **not** a proof of general Lua control-flow conformance or coverage of loops, closures, garbage collection and coroutines. [CI #40129671233](https://github.com/sguzman/ferrua/actions/runs/40129671233) passed five jobs, including the reference comparisons and the independent Rust/Arch acceptance suite. The production interpreter is unchanged.

### Verified deterministic branch-error differential matrix (T433)

A second 48-case, seed-reproducible test matrix now exercises both successful and intentionally failing integer-register conditional execution. Generated Lua scripts combine enclosing-local mutations, nested `if`/`elseif` branches, short-circuit `and`/`or` Boolean guards, integer remainder checks, and `1//0` expressions that must only be evaluated when their paths are actually selected. The test requires the owned VM to produce either the same integer as the reference implementation or the precise `Error::DivisionByZero` outcome, while requiring actual reference interpreter failures to mention division by zero rather than accidentally accepting a syntax error.

Both production `fer --lua=5.4` and pinned upstream `lua5.4` are compared in reference CI, and the matrix explicitly requires at least one success and one failure. [CI #40130027003](https://github.com/sguzman/ferrua/actions/runs/40130027003) passed all five jobs with these negative-path checks. The result strengthens integer control-flow acceptance, not full Lua exception, traceback or production-backend graduation semantics.

### Owned CLI print capture (experimental)

The explicit `--engine=owned` preview now executes ordinary `print(...)` calls in a pure-Rust bounded capture path. Arguments are converted with the existing first-party `tostring` implementation (including supported `__tostring` callbacks), separated by tabs and terminated with LF. The shared 64 KiB output limit produces an explicit resource error rather than allocating unbounded output. Buffered bytes are emitted by the CLI before reporting a later Lua error. The VM library itself performs no host I/O, and its existing value-only API remains available. Floating-point formatting, general global environments, and full Lua print conformance remain open. The production VM and its default CLI behavior are unchanged.

### Owned argument-table parity (experimental)

The opt-in owned engine creates its writable `arg` table lazily on first reference and reuses the same identity throughout a script and nested calls. `arg[0]` identifies `-e`, `-` or the input script path, and `arg[1..n]` contains the CLI arguments. String-pool and table-cell limits are enforced rather than allocating unbounded argv data. The production interpreter is unchanged. This is not yet a general, mutable Lua global environment or a complete `package` module search-path implementation.

### Shared execution-local globals (experimental)

The opt-in compiler now emits runtime reads for unbound identifiers and prepared writes for nonlocal assignments, rather than embedding immutable function constants in bytecode. Missing globals evaluate to nil; available first-party core functions resolve lazily; user assignments can replace them, including with nil. Nested calls share the same per-execution overrides. The Lua 5.4 `_VERSION` string and the bounded `arg` table resolve through that same environment. The complete `_G` table, module-loading environment model and broader standard library are still unimplemented. Production `fer` remains on luna-core.

### Global function declaration compatibility (experimental)

The owned compiler now lowers ordinary `function name(...) ... end` definitions to the same execution-local global environment as assignments, preserving recursion and forward global references. Dotted definitions and colon methods can target global or lexical table roots, and colon method bodies receive the explicit `self` parameter. This supports only the existing bounded Lua 5.4 subset: full `_ENV`, `_G`, module loading, stack unwinding and coroutine semantics are not implied. Production `fer` is unchanged.

### Materialized global environment table (experimental)

On first access to `_G`, the owned VM constructs a bounded Lua table containing the available core functions, `arg`, `_VERSION`, and a self-reference. Earlier global assignments are applied to that same table. Once materialized, direct global lookups and assignments share table storage with `_G.key` indexing, using the supported `__index` and `__newindex` hooks. Assigning nil removes global fields, including first-party builtins. This is progress toward Lua's environment model but does not implement complete `_ENV` lexical rebinding, library loading or garbage collection.

### Lexical _ENV reads and assignments (experimental)

When a lexical local named `_ENV` is visible, the bounded owned compiler now lowers otherwise-unbound identifier reads to `_ENV[name]`, and prepared assignments to writes into that same table. Closure captures inherit the nearest lexical `_ENV`, allowing functions declared within the scope to observe later changes to that table. Without a lexical `_ENV`, global names continue to use the shared default environment; reading the default `_ENV` returns the default `_G` table. This does not yet guarantee complete Lua 5.4 environment semantics for arbitrary upvalue rebinding, module loading or all bytecode forms.

### Lua 5.4 print conversion (experimental)

Lua 5.4 intentionally bypasses the mutable global `tostring` function inside `print`. Ferrua's first-party print uses its internal conversion and honors supported `__tostring` metamethods, but overriding `tostring` or `_G.tostring` cannot affect it. This behavior follows the Lua 5.4 reference manual's incompatibilities section (https://www.lua.org/manual/5.4/manual.html#8.2) and is covered by upstream CLI differential regressions. The separate 64 KiB output quota still applies.

### Accepted owned-CLI development and differential evidence (T434–T441)

Eight existing requirements from the owned-engine graduation contract have concrete implementations and individually passing five-job CI: strict opt-in CLI ([#40132402892](https://github.com/sguzman/ferrua/actions/runs/40132402892)); captured print ([#40133022798](https://github.com/sguzman/ferrua/actions/runs/40133022798)); argv table ([#40133328233](https://github.com/sguzman/ferrua/actions/runs/40133328233)); mutable globals ([#40133684877](https://github.com/sguzman/ferrua/actions/runs/40133684877)); named functions and methods ([#40133912837](https://github.com/sguzman/ferrua/actions/runs/40133912837)); writable `_G` ([#40134287690](https://github.com/sguzman/ferrua/actions/runs/40134287690)); 48 generated global/branch CLI cases ([#40134491004](https://github.com/sguzman/ferrua/actions/runs/40134491004)); and lexical `_ENV` including 24 generated upstream reference cases ([#40134992509](https://github.com/sguzman/ferrua/actions/runs/40134992509)).

These are narrow **bounded-subset acceptance results**. The full T301/T302 grammar and compiler/runtime gates remain unaccepted; no production VM switch, coroutine support, garbage collection, native EndeavourOS test, or stable release is claimed. The README and generated roadmap are synchronized from the machine ledger, not manually inflated.

### Explicit Lua error-object diagnostics (T442, experimental)

The owned execution wrapper retains a copy of an explicitly thrown error object (strings and representative primitive values) before the bounded VM's execution-local string storage is dropped. The opt-in CLI reports the captured value on stderr while preserving stdout produced before the fault. This corrects earlier generic `explicit Lua error` diagnostics but does not claim complete Lua 5.4 source-line attribution or traceback parity. The regression cases and full CI passed on [#40135554634](https://github.com/sguzman/ferrua/actions/runs/40135554634). The default production `fer` interpreter remains unchanged.

### Accepted bounded numeric math functions (experimental)

The first-party Lua 5.4 opt-in engine exposes a shared writable `math` table with `abs`, `floor`, `ceil`, `max`, `min`, `type`, and `tointeger`, plus `pi`, `huge`, `maxinteger`, and `mininteger`. The functions handle the existing bounded integer/float numeric types, preserve number-kind results when supported, and obey the same execution, table, and string limits as the rest of the VM. The initial implementation passed Rust 1.96, Linux tests, Lua 5.4 differential, Arch container, and action consumer CI [#40135923220](https://github.com/sguzman/ferrua/actions/runs/40135923220). A generated numeric differential matrix additionally passed 129 signed inputs, fractional rounding, numeric types and invalid-argument cases against production and upstream Lua in [CI #40136109998](https://github.com/sguzman/ferrua/actions/runs/40136109998). This is **not** complete Lua math library support: trigonometry, random numbers, logarithms, and other functions are not yet available. Default `fer` remains on luna-core.

### Indexed string interning (experimental)

The owned compiler and runtime now keep bounded byte-string-to-handle indexes and running pool byte totals. This removes repeated full-pool linear searches and repeated byte-length summation when compiling duplicate literals, resolving globals, or creating dynamic strings. The indices share stable string handles with existing table keys, closures, and captured Lua values, and preserve the 16,384-string and 64 KiB data-pool limits. Tests include 640 static uses of 64 literals, 1,500 dynamically generated numeric strings, and rejection of a pool beyond its byte quota. Full five-job CI passed at [#40136373226](https://github.com/sguzman/ferrua/actions/runs/40136373226). The expected complexity improvement has not yet been validated with a controlled wall-clock benchmark, and the additional hash-table storage is a documented memory tradeoff.

These changes do not satisfy T301/T302 graduation: the owned engine still lacks garbage collection, coroutine lifecycle, full standard libraries, source-complete diagnostics, and general Lua 5.4 conformance. The default production `fer` runtime remains unchanged.

### Root-frame cyclic table reclamation (T443; bounded preview)

The owned execution heap can now mark reachable Lua tables, metatables, table keys/values and closure-captured cell values from its root frame, then reclaim unreachable table cycles when new table constructors encounter table-count or table-cell pressure. Table-field writes can still fail at their quota without initiating a collection. Reclaimed table slots are reused; root references from globals, the Lua stack, lexical slots, prepared assignments and built-in tables remain protected. Marking and sweeping are iterative, with a shared instruction budget and no heap mutation until tracing finishes. The collector is **disabled while nested Lua frames are suspended** because their hidden evaluation stacks are not yet provided as roots. It does not reclaim closures or strings, implement weak tables/finalizers, or provide the complete Lua 5.4 garbage collector.

The initial 17,000-table cyclic case, retention/resource-limit checks, and format/lint/Arch/MSRV/consumer smoke passed [CI #40137971039](https://github.com/sguzman/ferrua/actions/runs/40137971039). A stronger [40,000-table differential](https://github.com/sguzman/ferrua/actions/runs/40138156924) passed all five jobs against production `fer` and upstream Lua 5.4 while preserving a nested captured closure and metatable lookup. No native EndeavourOS acceptance or production backend switch is implied.

### Captured closure sweep and nested call-frame roots (T444–T445, experimental)

The first-party collector now traces reachable function objects alongside tables, including transitive lexical cell captures, and recycles function handles and abandoned captures after a fully budgeted mark. Active root-frame cells, retained closures and table-held functions stay reachable. Regression suites exercise 42,000 short-lived captured closures while preserving a separately retained function; a fully held heap still fails within the bounded resource quotas. [CI #40197239081](https://github.com/sguzman/ferrua/actions/runs/40197239081) passed all five quality, reference Lua, compatibility, installation and consumer jobs.

For nested Lua calls, each active frame now owns pre-instruction operand-stack and prepared indexed-target snapshots. A nested collector roots the suspended caller stacks and conservatively retains ancestor allocated cells. This prevents reclamation of operand tables and captured functions that the caller needs after the inner function returns. Nested 21,000-allocation tests passed [CI #40197714330](https://github.com/sguzman/ferrua/actions/runs/40197714330), followed by the dedicated Lua 5.4 CLI comparison in [CI #40198348836](https://github.com/sguzman/ferrua/actions/runs/40198348836).

The collector uses bounded mark-and-sweep, not a complete Lua GC: weak-key/value table semantics, finalizers, incremental/generational controls, accurate heap counting, string reclamation, and resource behavior at arbitrary nested host boundaries are **not** accepted. Root snapshots add memory/copy overhead; no performance speedup is claimed.

### Indexed table-write GC under quota pressure (T446, experimental)

Array/hash table writes now measure planned storage growth before mutating the destination. If the bounded table-cell quota would be exceeded, the collector traces the destination, field key, value, current Lua frame and suspended frame roots, then retries. A 72,000-field write test survives repeated temporary table allocations; a companion case with all tables still reachable correctly returns a resource-limit error. The upstream [Lua 5.4 differential suite](https://github.com/sguzman/ferrua/actions/runs/40198348836) passed. Full weak/ephemeron semantics, finalizers, complete string reclamation and whole-runtime memory accounting remain distinct graduation work.

### Multi-return table-constructor tail GC at array quota (T447, experimental)

Lua table constructors such as `{tag=i, values()}` append the final call's variable-length result list through `Op::SetTableTail`. That bytecode path previously grew arrays directly and returned a resource-limit error without attempting garbage collection, even if earlier temporary tables were unreachable. The compiler/VM now checks the complete tail growth against its bounded cell quota, pins the target table and all returned objects through a budgeted mark-and-sweep pass, and retries before returning an error. A temporary 1,800-iteration × 40-value tail case completes, whereas keeping those tables reachable still rejects the over-quota allocation. Additional Lua CLI differential tests preserve a returned nested table value across collection and match both production `fer` and upstream Lua 5.4. All five CI jobs passed for the core change in [#40199755341](https://github.com/sguzman/ferrua/actions/runs/40199755341) and for the upstream differential suite in [#40199927574](https://github.com/sguzman/ferrua/actions/runs/40199927574). This is **not** acceptance of weak tables, finalizers, coroutine lifetime or complete GC semantics.

### Lua GC stop/restart state controls (T448, bounded preview)

The opt-in owned Lua engine now provides `collectgarbage('stop')`, `collectgarbage('restart')`, and `collectgarbage('isrunning')` in addition to the existing default/`'collect'` full pass. It begins in the running state; stopping suppresses its **automatic quota-pressure** sweeps on new tables, new closures, indexed field writes and variable-result constructor tails. It does not remove resource bounds: a stopped engine still fails if the heap exceeds its limits. A manual `collectgarbage('collect')` performs a complete bounded pass even while automatic collection is stopped. Restarting restores automatic sweeps. These narrow semantics align with the corresponding Lua 5.4 controls; they do not imply incremental/generational steps or precise accounting for Lua heap memory.

The owned unit tests cover the state transitions, a stopped 17,000-allocation resource-limit rejection, two 15,000-table epochs separated by an explicit collection while stopped, and restart before a subsequent 20,000-table workload. The runtime change passed [CI #40200501075](https://github.com/sguzman/ferrua/actions/runs/40200501075), and observable output comparisons against production `fer` and upstream Lua 5.4 passed [CI #40200671209](https://github.com/sguzman/ferrua/actions/runs/40200671209), each with five successful jobs. The default interpreter is unaffected.

### Tracked owned-heap `collectgarbage('count')` accounting (T449, bounded preview)

The experimental owned runtime now returns a Lua floating-point number of estimated KiB retained by its primary table, closure, lexical-cell and string pools. The accounting includes allocation capacities for table array/hash storage and live closure capture vectors; it is intentionally **not** the exact memory consumption of Rust's allocator or a promise of numerically identical values to Lua 5.4. It does not cover every host/VM allocation, and does not itself reclaim strings or provide an incremental collector. Lua scripts can now inspect whether the tracked heap rises after allocating temporary tables and falls after a manual full sweep, including when automatic collection was stopped.

The owned unit regressions verify the numeric result, allocation growth and reclamation decrease. The CLI differential checks the observable predicates, not platform-specific heap byte counts, against both production `fer` and upstream `lua5.4`. The implementation and tests passed all five jobs in [CI #40202464541](https://github.com/sguzman/ferrua/actions/runs/40202464541). Full Lua GC lifecycle, weak-table semantics, finalizers and accurate allocator-wide memory usage remain unaccepted.

### Recycled slots for lazy standard-library tables (T450, bounded preview)

The experimental engine's lazy `_G`, `math` and `arg` table constructors previously checked only `tables.len() >= MAX_TABLES`, incorrectly rejecting allocation even when a prior explicit GC had filled the reusable free-slot list. Under automatic GC, these constructors could also fail instead of collecting other unreachable tables before allocating. The new shared `reserve_lazy_table_slot` preflight first uses available recycled handles, otherwise performs the bounded root-preserving sweep when automatic collection is running. If all table handles remain reachable, it preserves the hard resource limit.

The regression suite fills all 16,384 table slots before materializing each lazy table, checks manual recycling with automatic GC stopped, and requires an out-of-quota failure when a reachable table retains all available handles. CLI comparison against production `fer` and upstream `lua5.4` verifies observable table initialization under pressure. The implementation passed all five jobs in [CI #40202985644](https://github.com/sguzman/ferrua/actions/runs/40202985644). This is a bounded lifecycle fix, not complete standard-library or Lua GC acceptance.
