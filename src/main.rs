//! Ferrua command-line interpreter.
//! Lua compilation, bytecode execution and garbage collection are provided
//! by the pure-Rust luna-core engine; this crate owns the fer CLI and host setup.
use luna_core::{version::LuaVersion, vm::Vm};
use std::io::{self, BufRead, IsTerminal, Read, Write};

const HELP: &str = "\
Ferrua - Lua on a Rust virtual machine\n\
\n\
Usage:\n\
  fer script.lua [arguments...]\n\
  fer -e 'print(1 + 2)'\n\
  fer -                  Read a Lua program from standard input\n\
  fer                    Interactive REPL (or read piped standard input)\n\
\n\
Options (before script or -e):\n\
  --lua=5.1..5.5         Select Lua dialect (default: 5.5)\n\
  --sandbox              Safe libraries only; no OS, file or package access\n\
  --budget=N             Instruction budget (positive integer)\n\
  -h, --help             Show help\n\
  -v, --version          Show version\n\
\n\
Inside REPL use =expression to display an expression value.\n";

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Help,
    Version,
    Eval(String, Vec<String>),
    Script(String, Vec<String>),
    Stdin,
    Repl,
}

#[derive(Debug)]
struct Options {
    lua: LuaVersion,
    sandbox: bool,
    budget: Option<i64>,
    action: Action,
}

fn parse(args: &[String], interactive_stdin: bool) -> Result<Options, String> {
    let mut lua = LuaVersion::Lua55;
    let mut sandbox = false;
    let mut budget = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let action = match arg.as_str() {
            "-h" | "--help" => Some(Action::Help),
            "-v" | "--version" => Some(Action::Version),
            "--sandbox" => {
                sandbox = true;
                None
            }
            "-e" => {
                i += 1;
                let code = args.get(i).ok_or("missing Lua source after -e")?.clone();
                Some(Action::Eval(code, args[i + 1..].to_vec()))
            }
            "-" => Some(Action::Stdin),
            "--" => {
                i += 1;
                let path = args.get(i).ok_or("expected script path after --")?;
                Some(Action::Script(path.clone(), args[i + 1..].to_vec()))
            }
            s if s.starts_with("--lua=") => {
                lua = match &s[6..] {
                    "5.1" => LuaVersion::Lua51,
                    "5.2" => LuaVersion::Lua52,
                    "5.3" => LuaVersion::Lua53,
                    "5.4" => LuaVersion::Lua54,
                    "5.5" => LuaVersion::Lua55,
                    _ => return Err(format!("unsupported Lua dialect: {s}")),
                };
                None
            }
            s if s.starts_with("--budget=") => {
                let value = s[9..]
                    .parse::<i64>()
                    .map_err(|_| format!("invalid instruction budget: {s}"))?;
                if value <= 0 {
                    return Err("instruction budget must be positive".into());
                }
                budget = Some(value);
                None
            }
            s if s.starts_with('-') => return Err(format!("unknown option: {s}")),
            _ => Some(Action::Script(arg.clone(), args[i + 1..].to_vec())),
        };
        if let Some(action) = action {
            return Ok(Options {
                lua,
                sandbox,
                budget,
                action,
            });
        }
        i += 1;
    }
    Ok(Options {
        lua,
        sandbox,
        budget,
        action: if interactive_stdin {
            Action::Repl
        } else {
            Action::Stdin
        },
    })
}

fn vm_for(options: &Options) -> Vm {
    let mut vm = if options.sandbox {
        let mut vm = Vm::new_minimal(options.lua);
        vm.open_base();
        vm.open_math();
        vm.open_string();
        vm.open_table();
        vm.open_coroutine();
        if !matches!(options.lua, LuaVersion::Lua51 | LuaVersion::Lua52) {
            vm.open_utf8();
        }
        vm.set_bytecode_loading(false);
        vm
    } else {
        Vm::new(options.lua)
    };
    // Built-in first-party data support, including sandboxed pure Lua parsing.
    let json = vm
        .eval_chunk(include_str!("../tools/json.lua"), "@ferrua/json.lua")
        .expect("embedded JSON module must compile")
        .into_iter()
        .next()
        .expect("embedded JSON module must return a table");
    vm.set_global("json", json)
        .expect("embedded JSON module must register");
    // User-supplied budget applies to the user script, not runtime initialization.
    if let Some(budget) = options.budget {
        vm.set_instr_budget(Some(budget));
    }
    vm
}

fn set_args(vm: &mut Vm, name: &str, args: &[String]) -> Result<(), String> {
    let mut builder = vm.new_table().with(0_i64, name);
    for (index, arg) in args.iter().enumerate() {
        builder = builder.with((index + 1) as i64, arg.as_str());
    }
    let table = builder.build();
    vm.set_global("arg", table).map_err(|e| vm.error_text(&e))
}

fn error_message(vm: &mut Vm, err: &luna_core::vm::LuaError) -> String {
    let mut message = vm.error_text(err);
    if let Some(trace) = vm.take_error_traceback()
        && !trace.is_empty()
        && !message.contains(&trace)
    {
        message.push('\n');
        message.push_str(&trace);
    }
    message
}

fn run_source(vm: &mut Vm, source: &str, name: &str) -> Result<(), String> {
    // Lua CLI convention: ignore a Unix shebang, preserving original line numbers.
    let normalized = source
        .starts_with("#!")
        .then(|| format!("--{}", &source[2..]));
    vm.eval_chunk(normalized.as_deref().unwrap_or(source), name)
        .map(|_| ())
        .map_err(|err| error_message(vm, &err))
}

fn repl(vm: &mut Vm) -> Result<(), String> {
    eprintln!(
        "Ferrua {} (Lua VM) - Ctrl-D to exit",
        env!("CARGO_PKG_VERSION")
    );
    let input = io::stdin();
    let mut lines = input.lock();
    let mut line = String::new();
    loop {
        print!("fer> ");
        io::stdout().flush().map_err(|e| e.to_string())?;
        line.clear();
        if lines.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            println!();
            return Ok(());
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let code = if let Some(expression) = line.strip_prefix('=') {
            format!("return {expression}")
        } else {
            line.to_owned()
        };
        match vm.eval_chunk(&code, "=repl") {
            Ok(values) => {
                for value in values {
                    match vm.tostring_value(value) {
                        Ok(bytes) => println!("{}", String::from_utf8_lossy(&bytes)),
                        Err(err) => eprintln!("fer: {}", error_message(vm, &err)),
                    }
                }
            }
            Err(err) => eprintln!("fer: {}", error_message(vm, &err)),
        }
    }
}

fn execute(options: Options) -> Result<(), String> {
    match options.action {
        Action::Help => {
            print!("{HELP}");
            Ok(())
        }
        Action::Version => {
            println!(
                "fer {} (Ferrua; pure Rust Lua VM via luna-core 4.0.2)",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        Action::Eval(ref source, ref arguments) => {
            let mut vm = vm_for(&options);
            set_args(&mut vm, "-e", arguments)?;
            run_source(&mut vm, source, "=-e")
        }
        Action::Script(ref path, ref arguments) => {
            let source = std::fs::read_to_string(path).map_err(|err| format!("{path}: {err}"))?;
            let mut vm = vm_for(&options);
            set_args(&mut vm, path, arguments)?;
            run_source(&mut vm, &source, path)
        }
        Action::Stdin => {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .map_err(|err| err.to_string())?;
            let mut vm = vm_for(&options);
            set_args(&mut vm, "-", &[])?;
            run_source(&mut vm, &source, "=stdin")
        }
        Action::Repl => {
            let mut vm = vm_for(&options);
            set_args(&mut vm, "=repl", &[])?;
            repl(&mut vm)
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = parse(&args, io::stdin().is_terminal());
    if let Err(err) = parsed.and_then(execute) {
        eprintln!("fer: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn argv(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parse_eval_and_args() {
        let opt = parse(&argv(&["--lua=5.4", "-e", "print(arg[1])", "hello"]), false).unwrap();
        assert_eq!(
            opt.action,
            Action::Eval("print(arg[1])".into(), vec!["hello".into()])
        );
        assert_eq!(opt.lua, LuaVersion::Lua54);
    }
    #[test]
    fn reject_broken_flags() {
        assert!(parse(&argv(&["--budget=0"]), false).is_err());
        assert!(parse(&argv(&["-e"]), false).is_err());
        assert!(parse(&argv(&["--lua=8.0"]), false).is_err());
        assert!(parse(&argv(&["--bad"]), false).is_err());
    }
    #[test]
    fn eval_lua_math_tables_closures_loops() {
        let mut vm = vm_for(&parse(&[], false).unwrap());
        run_source(
            &mut vm,
            r#"
            local function square(x) return x * x end
            local numbers = {2, 3, 4}
            local total = 0
            for _, number in ipairs(numbers) do
                total = total + square(number)
            end
            assert(total == 29)
        "#,
            "=test",
        )
        .unwrap();
    }
    #[test]
    fn script_arguments_are_one_indexed() {
        let mut vm = vm_for(&parse(&[], false).unwrap());
        set_args(&mut vm, "script.lua", &argv(&["alpha", "beta"])).unwrap();
        run_source(
            &mut vm,
            "assert(arg[0] == 'script.lua'); assert(arg[1] == 'alpha'); assert(arg[2] == 'beta')",
            "=test",
        )
        .unwrap();
    }
    #[test]
    fn sandbox_excludes_os_and_io() {
        let opts = parse(&argv(&["--sandbox"]), false).unwrap();
        let mut vm = vm_for(&opts);
        run_source(
            &mut vm,
            "assert(io == nil and os == nil and package == nil); assert(math.abs(-2) == 2)",
            "=test",
        )
        .unwrap();
    }
    #[test]
    fn unix_shebang_is_accepted() {
        let mut vm = vm_for(&parse(&[], false).unwrap());
        run_source(
            &mut vm,
            "#!/usr/bin/env fer\nassert(6 * 7 == 42)",
            "=shebang",
        )
        .unwrap();
    }
    #[test]
    fn built_in_json_round_trips() {
        let mut vm = vm_for(&parse(&[], false).unwrap());
        run_source(
            &mut vm,
            r#"local x = json.decode('{"alpha": [1, 2], "ok": true}')
               assert(x.ok and x.alpha[2] == 2)
               assert(json.encode(x) == '{"alpha":[1,2],"ok":true}')
               assert(json.decode('"\\uD83C\\uDF19"') == "🌙")
               assert(json.encode(json.decode('{}')) == '{}')
               assert(json.encode(json.decode('[]')) == '[]')
               assert(json.encode(json.object()) == '{}')
               assert(json.encode(json.array()) == '[]')
               assert(json.decode('null') == json.null)"#
            "=json-test",
        )
        .unwrap();
    }

    #[test]
    fn runtime_errors_are_propagated() {
        let mut vm = vm_for(&parse(&[], false).unwrap());
        assert!(
            run_source(&mut vm, "error('expected failure')", "=test")
                .unwrap_err()
                .contains("expected failure")
        );
    }
}
