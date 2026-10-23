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

The owned Lua runtime's first-class `tostring(table)` builtin can now invoke a first-party function-valued `__tostring` field on the table's raw metatable. The original table is passed as the callback's sole argument through the existing bounded closure executor. Only the first return value is used, and it must be a Lua byte string. Extra callback results are discarded; missing or non-string results raise the experimental VM's type error. Without `__tostring`, the existing bounded table identity string is retained. Primitive `tostring` behavior is unchanged.

The pending unit and upstream Lua 5.4 differential fixtures cover lexical captures, string return validation, multiple returns, fallback stringification and recursive callback limits. [CI #40036891366](https://github.com/sguzman/ferrua/actions/runs/40036891366) passed all four primary jobs and the status reporter, including Lua 5.4 differential fixtures. Primitive metatable overrides, callable-table metamethods, Lua-compatible error strings and full runtime conformance are still outside the accepted subset. Production `fer` remains pinned to the existing external engine.


### Verified Lua ipairs metatable-aware iteration

Lua 5.4's `ipairs` iterator indexes successive positive integer fields through ordinary table indexing, which may invoke a table's `__index` fallback for a missing array element. The owned iterator now uses bounded `lookup_with_metatable` like normal indexed expressions. Iteration stops at the first nil; `ipairs` ignores the separate `__pairs` hook; `rawget` and `next` still bypass indexing metamethods. [CI #40037327110](https://github.com/sguzman/ferrua/actions/runs/40037327110) accepted table/closure fallback, array-hole, termination and raw-access parity fixtures.

### Verified Lua label grammar and bounded same-block goto

The first-party parser accepts `::name::` labels and `goto name` statements as explicit AST nodes, requires valid identifiers, and rejects duplicate labels within one lexical block. The experimental stack compiler also lowers **same-block forward and backward jumps when the active local-binding count at the source equals the target count**. These are real bounded `Op::Jump` instructions, not ignored syntax. Unit tests cover valid same-block control flow, undefined targets, jumps into or across local lifetimes, cross-block rejections, and infinite backward jumps stopped by the instruction budget. [CI #40039264425](https://github.com/sguzman/ferrua/actions/runs/40039264425) passed these owned VM regressions and Lua 5.4 differential fixtures.

General Lua `goto` remains incomplete: cross-block visibility, upvalue closing on jumps across scopes, and all nuanced local-lifetime cases are **not implemented**. Unsupported forms return an explicit compiler error rather than silently skipping the jump. This bounded subset does not complete T301/T302 grammar/compiler graduation; production `fer` retains the pinned external VM.

### Verified deterministic generated numeric differential corpus

The Rust integration test now constructs 128 reproducible arithmetic, floor-division/remainder, bitwise, and comparison Lua chunks from a fixed-seed, non-cryptographic integer generator. Generated chunks use bounded input values with no external data, Python scripts, network or unstable random seed. The **same** fixture runner executes these programs through the Ferrua-owned VM and the working production `fer` CLI; the Lua 5.4 CI job additionally compares output to the upstream reference interpreter. A separate unit assertion checks generator determinism.

[CI #40041470699](https://github.com/sguzman/ferrua/actions/runs/40041470699) passed all four primary acceptance jobs and the aggregate reporter, including upstream Lua 5.4 comparisons for all 128 generated chunks. Generated integer expressions complement the curated semantic cases and must not be treated as comprehensive Lua conformance, especially for parser errors, metatables, coroutines, GC, standard libraries or native production graduation.

### Verified negative-path syntax and runtime parity

A curated deterministic corpus now exercises intentional failures across the first-party parser and bounded owned interpreter. Syntax fixtures must fail at `compile_flow_chunk`; runtime fixtures must compile successfully but yield an execution error. The same sources are invoked against both the production `fer` backend and the upstream Lua 5.4 interpreter when enabled in differential CI. The oracle verifies rejection and process exit status, rather than pretending the first-party VM already reproduces Lua's exact diagnostic text or stack traces. Cases include malformed control flow and table constructors, illegal operands, invalid table keys, builtin misuse and missing callable targets.

[CI #40042287118](https://github.com/sguzman/ferrua/actions/runs/40042287118) passed all four primary jobs and the status reporter, including upstream Lua 5.4 rejection outcomes. Correct rejection of a curated fixture is only a partial language/error conformance result: error objects, protected-call restoration, tracebacks, GC, coroutines, complete grammar and first-party default runtime integration remain independently gated.

### Bounded Lua error values and protected closure calls (pending CI)

The experimental owned VM now recognizes first-class `error` and `pcall` values, retaining explicit Lua errors as a typed `FlowError::Thrown(Value)` until a protected call handles them. The bounded `pcall` implementation can invoke owned Lua closures and the `error`/`assert` builtins, preserve success multiple-return values and lexical captures, and produce a `false, error-value` pair on recoverable failures. Generic VM type failures are converted to an interned diagnostic string, but no claim is made that its wording matches Lua 5.4. Explicitly thrown numbers, strings, Booleans and nil remain distinguishable. Exhausted steps, heap limits and protected-call depth are **not caught**: the experiment must retain its hard safety budgets.

The pending Rust and Lua 5.4 differential fixtures cover thrown-value identities, closure argument forwarding, multiple results, nested protected calls and state changes before an error. General callable-table/builtin parity, Lua's default assertion error message, `xpcall`, traceback generation, standard error levels, coroutine/error interactions and production cutover are still open. This is **not yet CI-accepted**. The production `fer` binary continues using the pinned `luna-core` backend.
