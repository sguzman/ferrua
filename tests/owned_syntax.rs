//! Integration acceptance for experimental Ferrua-owned Lua source tooling.
//! This tests its lexical scanner against public first-party Lua script corpus;
//! actual script execution stays on the pinned external VM until graduation.
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
    if std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA").ok().as_deref() != Some("1") {
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
        let compile_only = format!("local f,err=load('return {source}'); if not f then error(err) end");
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
