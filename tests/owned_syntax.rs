//! Integration acceptance for experimental Ferrua-owned Lua source tooling.
//! This tests its lexical scanner against public first-party Lua script corpus;
//! actual script execution stays on the pinned external VM until graduation.
use ferrua::expr::{Statement, parse_chunk};
use ferrua::lex::tokenize;
use std::fs;
use std::path::Path;

#[test]
fn scans_real_checked_in_lua_scripts_without_losing_source_slices() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let samples = [
        "tools/roadmap.lua",
        "tools/json.lua",
        "tools/process.lua",
        "tools/http.lua",
        "tools/github.lua",
        "tools/data.lua",
        "tools/fs.lua",
        "tools/check_compat.lua",
        "tools/diff_lua.lua",
        "tools/package_release.lua",
        "tests/json_spec.lua",
        "tests/data.lua",
        "examples/count.lua",
        "examples/process.lua",
        "examples/fs.lua",
        "tests/conformance/01-control-flow.lua",
        "tests/conformance/02-closures.lua",
        "tests/conformance/03-tables.lua",
        "tests/conformance/04-string-patterns.lua",
        "tests/conformance/05-upvalues.lua",
        "tests/conformance/06-scopes.lua",
        "tests/conformance/07-coroutines.lua",
        "tests/conformance/08-metatables.lua",
        "tests/conformance/09-generic-iterators.lua",
        "tests/conformance/10-multiple-returns.lua",
        "tests/conformance/11-bitwise.lua",
        "tests/conformance/12-utf8.lua",
    ];
    for relative in samples {
        let source = fs::read_to_string(root.join(relative)).expect("corpus fixture exists");
        let tokens = tokenize(&source)
            .unwrap_or_else(|error| panic!("owned lexer could not scan {relative}: {error}"));
        assert!(!tokens.is_empty(), "empty tokens for {relative}");
        let mut last_end = 0;
        let mut last_line = 1;
        for token in &tokens {
            assert!(
                token.byte_offset >= last_end,
                "overlapping lexer token in {relative}"
            );
            assert_eq!(
                source.get(token.byte_offset..token.byte_offset + token.spelling.len()),
                Some(token.spelling),
                "token spelling does not reproduce bytes in {relative}"
            );
            assert!(
                token.line >= last_line,
                "lexer line regressed in {relative}"
            );
            last_end = token.byte_offset + token.spelling.len();
            last_line = token.line;
        }
    }
}

#[test]
fn owned_goto_label_grammar_accepts_valid_lua_syntax() {
    for source in [
        "::first:: goto done; ::done:: return 4",
        "goto done ::done:: return 3",
        "do ::inner:: local x=2 end; ::inner:: return 5",
        "local n=1; ::mark:: n=n+1; if n<3 then goto mark end; return n",
    ] {
        let syntax = parse_chunk(source).unwrap_or_else(|error| {
            panic!("owned parser rejected valid label/goto syntax {source:?}: {error}")
        });
        assert!(
            syntax
                .iter()
                .any(|stmt| matches!(stmt, Statement::Label(_) | Statement::Goto(_)))
                || source.contains("::inner::")
        );
    }
    for source in [
        "::",
        "::3::",
        "goto 3",
        "::named:",
        "::dup:: ::dup:: return 1",
    ] {
        assert!(
            parse_chunk(source).is_err(),
            "malformed label/goto syntax {source:?}"
        );
    }
}

#[test]
fn goto_syntax_and_scope_boundaries_match_lua_54_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    for (source, valid) in [
        ("::first:: goto done; ::done:: return 4", true),
        ("goto done ::done:: return 3", true),
        ("do ::inner:: local x=2 end; ::outer:: return 5", true),
        (
            "local n=1; ::mark:: n=n+1; if n<3 then goto mark end; return n",
            true,
        ),
        ("goto missing", false),
        ("goto ready; local x=1; ::ready:: return 1", false),
        ("::dup:: ::dup:: return 1", false),
    ] {
        let output = std::process::Command::new("lua5.4")
            .args(["-e", source])
            .output()
            .expect("upstream Lua 5.4 installed by differential job");
        assert_eq!(
            output.status.success(),
            valid,
            "upstream Lua label/goto source {source:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        // The owned parser recognizes grammar, but cross-scope goto resolution
        // is an explicit future compilation gate; do not equate parsing with
        // complete upstream validity.
        if valid {
            assert!(parse_chunk(source).is_ok(), "{source}");
        }
    }
}

#[test]
fn lua_c_locale_unicode_identifier_rules_match_upstream_when_requested() {
    let cases = [
        ("local cafe_42=7; return cafe_42", true),
        ("local café=42", false),
        ("local α=2", false),
        ("local helloé=3", false),
        ("return naïve", false),
        ("local s='café'; return s", true),
        ("return [=[α😀]=]", true),
        ("local n=1 -- café, α and 😀 in comments\nreturn n", true),
    ];
    for (source, valid) in cases {
        let accepted = tokenize(source).is_ok();
        assert_eq!(accepted, valid, "owned C-locale scanning: {source:?}");
        if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
            .ok()
            .as_deref()
            == Some("1")
        {
            let reference = std::process::Command::new("lua5.4")
                .args(["-e", source])
                .output()
                .expect("reference interpreter installed by differential job");
            assert_eq!(
                reference.status.success(),
                valid,
                "Lua 5.4 differs on {source:?}: {}",
                String::from_utf8_lossy(&reference.stderr)
            );
        }
    }
}

#[test]
fn malformed_unicode_and_numeric_corners_do_not_panic() {
    for source in [
        "local x='café'",
        "local x=[==[☃️ Moon 🌙]==]",
        "local café = 42",
        "5..7",
        "0x1.2p-3",
        "1e-",
        "0x",
        "[=[unfinished",
        "\"unfinished",
        "1.0e+10",
        "a .5",
    ] {
        let _ = tokenize(source);
    }
}

#[test]
fn lua_local_attribute_syntax_matches_lua_54_when_requested() {
    let cases = [
        ("local x<const> = 7; return x", true),
        ("local x<const>, y = 4,5; return x+y", true),
        ("local x, y<const> = 4,5; return x+y", true),
        ("local x<close> = nil; return 1", true),
        ("local x<const>; return x", true),
        ("local x<const>,y<const> = 1,2; return x+y", true),
        ("local x<unknown> = 1", false),
        ("local x<const = 1", false),
        ("local x<1> = 1", false),
        ("local x<> = 1", false),
        ("local x<const><close> = 1", false),
    ];
    for (source, valid) in cases {
        let parsed = ferrua::expr::parse_chunk(source).is_ok();
        assert_eq!(parsed, valid, "owned local attribute grammar: {source}");
        if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
            .ok()
            .as_deref()
            == Some("1")
        {
            let reference = format!("assert(load([====[{source}]====]))");
            let result = std::process::Command::new("lua5.4")
                .args(["-e", &reference])
                .output()
                .unwrap();
            assert_eq!(
                parsed,
                result.status.success(),
                "Lua 5.4 local attribute grammar differs: {source}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}

#[test]
fn lua_short_string_escape_syntax_matches_upstream_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    let cases = [
        (r#"local s="\x41""#, true),
        (r#"local s="\u{1F600}""#, true),
        (r#"local s="\u{7FFFFFFF}""#, true),
        (r#"local s="\255""#, true),
        (r#"local s="\x4F""#, true),
        (r#"local s="\0012""#, true),
        (r#"local s="\a\b\f\n\r\t\v\\\"\'""#, true),
        ("local s=\"continued\\\nnext\"", true),
        ("local s=\"continued\\\r\nnext\"", true),
        (r#"local s="\q""#, false),
        (r#"local s="\x""#, false),
        (r#"local s="\xG0""#, false),
        (r#"local s="\x1""#, false),
        (r#"local s="\u""#, false),
        (r#"local s="\u{}""#, false),
        (r#"local s="\u{GG}""#, false),
        (r#"local s="\u{80000000}""#, false),
        (r#"local s="\999""#, false),
        (r#"local s="\256""#, false),
    ];
    for (source, expected_ok) in cases {
        let accepted_by_owned_lexer = tokenize(source).is_ok();
        assert_eq!(
            accepted_by_owned_lexer, expected_ok,
            "owned lexer case: {source:?}"
        );
        let upstream = std::process::Command::new("lua5.4")
            .args(["-e", source])
            .output()
            .unwrap();
        assert_eq!(
            accepted_by_owned_lexer,
            upstream.status.success(),
            "Lua 5.4.6 differs from lexer on {source:?}: {}",
            String::from_utf8_lossy(&upstream.stderr)
        );
    }
}

#[test]
fn owned_method_call_syntax_matches_lua_54_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    for (source, valid) in [
        ("obj:run(1)", true),
        ("obj:run(1).value", true),
        ("obj.child:run(1,2)", true),
        ("obj:run", false),
        ("obj:run(", false),
        ("obj:run(1,)", false),
        ("obj:3(1)", false),
        ("obj:(1)", false),
    ] {
        let owned = ferrua::expr::parse_expression(source).is_ok();
        assert_eq!(owned, valid, "{source}");
        let compile_only =
            format!("local f,err=load('return {source}'); if not f then error(err) end");
        let reference = std::process::Command::new("lua5.4")
            .args(["-e", &compile_only])
            .output()
            .unwrap();
        assert_eq!(
            owned,
            reference.status.success(),
            "method call syntax differs for {source}: {}",
            String::from_utf8_lossy(&reference.stderr)
        );
    }
}

#[test]
fn lua_shorthand_call_argument_grammar_matches_lua_54() {
    let cases = [
        ("f'abc'", true),
        ("f\"abc\"", true),
        ("f[[abc]]", true),
        ("f[=[abc]=]", true),
        ("f{n=3}", true),
        ("f{}", true),
        ("obj:run'abc'", true),
        ("obj:run[[abc]]", true),
        ("obj:run{n=3}", true),
        ("obj.child:run{n=3}.value", true),
        ("f{}'more'", true),
        ("f'first'{n=3}", true),
        ("obj:run", false),
        ("obj:run{n=}", false),
        ("obj:run(1,)", false),
        ("obj:run + 2", false),
        ("f'broken", false),
        ("f{n=}", false),
    ];
    for (source, expected_ok) in cases {
        let owned = ferrua::expr::parse_expression(source).is_ok();
        assert_eq!(owned, expected_ok, "owned shorthand call: {source}");
        if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
            .ok()
            .as_deref()
            == Some("1")
        {
            let reference = format!("assert(load([====[return {source}]====]))");
            let output = std::process::Command::new("lua5.4")
                .args(["-e", &reference])
                .output()
                .unwrap();
            assert_eq!(
                owned,
                output.status.success(),
                "Lua 5.4 shorthand differs: {source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn owned_function_syntax_matches_lua_54_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    for (source, expected_ok) in [
        ("function f() return 1 end", true),
        ("local function f(x,...) return x,... end", true),
        ("function t.f(x) return x end", true),
        ("function t.ns.f(x) return x end", true),
        ("function t:m(x) return self,x end", true),
        ("return function(x) return x end", true),
        (
            "local f=function(x) return function(y) return x+y end end",
            true,
        ),
        ("function t:() end", false),
        ("function t.() end", false),
        ("local function t.f() end", false),
        ("function f(a,) end", false),
        ("function f(a,...,b) end", false),
        ("while true do local function f() break end end", false),
    ] {
        let ours = ferrua::expr::parse_chunk(source).is_ok();
        assert_eq!(ours, expected_ok, "owned syntax: {source}");
        let lua = format!("assert(load([==[{source}]==]))");
        let upstream = std::process::Command::new("lua5.4")
            .args(["-e", &lua])
            .output()
            .unwrap();
        assert_eq!(
            ours,
            upstream.status.success(),
            "Lua 5.4 differs on {source}: {}",
            String::from_utf8_lossy(&upstream.stderr)
        );
    }
}

#[test]
fn owned_table_constructor_syntax_matches_lua_54_when_requested() {
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    for (source, expected_ok) in [
        ("local t={}", true),
        ("local t={a=1,[2]=3,4;}", true),
        ("local t={inner={x=1},[true]=nil}", true),
        ("return {function(x) return x end, x=3}", true),
        ("local t={a=}", false),
        ("local t={[a] 2}", false),
        ("local t={a=1,,b=2}", false),
        ("local t={1 2}", false),
        ("local t={1", false),
        ("local t={]}", false),
    ] {
        let owned = ferrua::expr::parse_chunk(source).is_ok();
        assert_eq!(owned, expected_ok, "owned table grammar: {source}");
        let reference = format!("assert(load([==[{source}]==]))");
        let lua = std::process::Command::new("lua5.4")
            .args(["-e", &reference])
            .output()
            .unwrap();
        assert_eq!(
            owned,
            lua.status.success(),
            "upstream Lua differs on {source}: {}",
            String::from_utf8_lossy(&lua.stderr)
        );
    }
}
