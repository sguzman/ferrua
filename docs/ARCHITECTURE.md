# 🏗️ Architecture and provenance

Ferrua is a Rust native CLI that compiles and executes Lua source through the independent pure-Rust `luna-core` 4.0.2 engine. `luna-core` is a dependency, **not** Ferrua-owned source code. Its upstream project is [luna-luarust](https://github.com/tempbottle/luna-luarust). This is not `mlua` and does not embed a C Lua interpreter.

Pipeline: `fer` parses arguments → selects dialect and allowed libraries → supplies Lua `arg` → engine parses Lua → emits register-based bytecode → bytecode VM executes → optional Lua standard libraries handle IO and subprocess calls.

The default trusted execution mode supplies conventional Lua host functionality, including filesystem and shell access. It is intended for local automation chosen by the person running it. The optional sandbox creates a minimal VM and opens only the base, math, string, table, coroutine and version-applicable UTF-8 libraries; bytecode loading is disabled. Instruction budgets are optional and not a replacement for OS isolation.

Because Lua sources are evaluated by the engine, Ferrua's current conformance, bytecode semantics, closures, garbage collection, coroutine behavior, and standard libraries come from that engine. Do not claim that Ferrua has independently implemented them. An **owned compiler and VM** are queued under M2 and will require real differential and acceptance evidence before substituting this engine.

The first priority is working shell scripting, reliable scripts, diagnostics and automated acceptance. CI's Linux build, tests and smoke scripts are the acceptance source; checked-in test code alone is not acceptance.

Lua versions supported by the selected engine are 5.1–5.5. Default is 5.5. The project intentionally does not claim that every detail of every dialect has been independently tested by Ferrua.
