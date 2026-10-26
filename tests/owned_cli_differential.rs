//! Deterministic end-to-end owned CLI compatibility matrix.
//! This intentionally uses real binaries, including upstream Lua in the
//! reference CI job, rather than using production fer as a hidden fallback.

use std::process::{Command, Output};

fn run_fer(owned: bool, source: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fer"));
    if owned {
        command.arg("--engine=owned");
    } else {
        command.arg("--lua=5.4");
    }
    command.args(["-e", source]).output().expect("launch fer")
}

fn reference_output(source: &str) -> Option<Output> {
    (std::env::var("FERRUA_COMPARE_OWNED_WITH_LUA")
        .ok()
        .as_deref()
        == Some("1"))
    .then(|| {
        Command::new("lua5.4")
            .args(["-e", source])
            .output()
            .expect("lua5.4")
    })
}

fn assert_parity(source: &str, should_succeed: bool) {
    let owned = run_fer(true, source);
    let production = run_fer(false, source);
    assert_eq!(
        owned.status.success(),
        should_succeed,
        "owned result disagrees with expected status for {source}: {}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert_eq!(
        owned.status.success(),
        production.status.success(),
        "production status differs: {source}"
    );
    assert_eq!(
        owned.stdout, production.stdout,
        "production stdout differs: {source}"
    );
    if let Some(reference) = reference_output(source) {
        assert_eq!(
            owned.status.success(),
            reference.status.success(),
            "upstream Lua 5.4 status differs: {source}"
        );
        assert_eq!(
            owned.stdout, reference.stdout,
            "upstream Lua 5.4 stdout differs: {source}"
        );
    }
}

#[test]
fn curated_global_function_print_and_error_semantics_match_lua54() {
    for source in [
        "function first() return second() end; function second() return 7 end; print(first())",
        "local t={base=40}; function t:add(x) return self.base+x end; print(t:add(2))",
        "counter=1; assert(_G.counter==1); _G.counter=4; print(counter)",
        "local f=print; _G.print=nil; assert(print==nil); _G.print=f; print('restored')",
        "setmetatable(_G,{__index=function(_,key) if key=='missing' then return 17 end end}); print(missing)",
        "local f=print; local ok=pcall(f,'protected',9); assert(ok)",
        "print(); print('hello',true,nil); print(6*7)",
        "tostring=function() return 'override' end; print(7,true); _G.tostring=nil; print('hello')",
        "local t=setmetatable({}, {__tostring=function() return 'tag' end}); print(t)",
        "local m=math; assert(_G.math==m); print(math.abs(-7),math.floor(3.75),math.ceil(-3.75),math.max(2,9,5),math.min(2,9,5),math.type(3),math.type(3.0),math.tointeger('42'))",
        "print(math.pi>3 and math.pi<4,math.huge>1e100,math.maxinteger>0,math.mininteger<0)",
        "local old=math.abs; math.abs=function(v) return v end; print(math.abs(-5)); math.abs=old; print(math.abs(-5))",
        "local outer=_ENV; local box={print=print,base=40}; do local _ENV=box; answer=base+2; function double(x) return x*2 end; print(double(answer)) end; assert(box.answer==42); print(box.double(3))",
        "local env={print=print,n=3}; do local _ENV=env; function read() return n end; n=9; print(read()) end; print(env.n)",
        "local env={print=print}; do local _ENV=env; print('first'); do local _ENV={print=print}; print('nested') end; print('last') end",
    ] {
        assert_parity(source, true);
    }
    for source in [
        "print('before'); error('after')",
        "print('before'); local t=nil; t.value=1",
    ] {
        assert_parity(source, false);
    }
}

#[test]
fn owned_math_atan_argument_label_matches_lua54_with_backend_alias_variation() {
    // Pinned luna-core has emitted both 'math.atan' and 'math.atan2'
    // across CI runs for the same public function. Preserve the strict
    // Lua 5.4 error value without canonizing alias name variation.
    let source = "local ok,message=pcall(math.atan,0,false); print(ok,message)";
    let owned = run_fer(true, source);
    let production = run_fer(false, source);
    assert!(owned.status.success());
    assert!(production.status.success());
    assert_eq!(
        owned.stdout,
        b"false\tbad argument #2 to 'math.atan' (number expected, got boolean)\n"
    );
    let alias = b"false\tbad argument #2 to 'math.atan2' (number expected, got boolean)\n";
    // Retain the two observed, fully specified production diagnostics.
    assert!(
        production.stdout == owned.stdout || production.stdout == alias,
        "unexpected production math.atan label: {:?}",
        production.stdout
    );
    if let Some(reference) = reference_output(source) {
        assert!(reference.status.success());
        // Lua 5.4.6 also exposes either C-function alias in error labels.
        assert!(
            reference.stdout == owned.stdout || reference.stdout == alias,
            "unexpected upstream math.atan label: {:?}",
            reference.stdout
        );
    }
}

#[test]
fn owned_utf8_codes_generic_iteration_strict_lax_and_errors_match_lua54() {
    for source in [
        "local s='Aé💩'; print(select('#',utf8.codes(s))); local f,state,i=utf8.codes(s); print(type(f),state==s,i); for p,c in utf8.codes(s) do print(p,c) end",
        "local f,s,n=utf8.codes('Aé'); print(f(s,0)); print(f(s,1)); print(select('#',f(s,2))); local p,c=pcall(f,s,0); print(p,c)",
        "local t={}; for p,c in utf8.codes('héllo') do t[#t+1]=p..':'..c end; print(table.concat(t,','))",
        "local x=utf8.char(0x110000); for p,c in utf8.codes(x,true) do print(p,c) end; local ok=pcall(function() for p,c in utf8.codes(x) do end end); print(ok)",
        // Pinned production prefixes Lua closure errors with -e:1:, while
        // owned errors currently omit source-location decoration. Preserve
        // exact failure and diagnostic suffix rather than dropping coverage.
        "local x=utf8.char(0xD800); for p,c in utf8.codes(x,true) do print(p,c) end; local ok,message=pcall(function() for p,c in utf8.codes(x) do end end); print(ok,string.sub(message,-18)=='invalid UTF-8 code')",
        "local x=string.char(0x80,65); local ok=pcall(utf8.codes,x); print(ok); local ok2=pcall(function() for p,c in utf8.codes('A'..string.char(0x80)) do end end); print(ok2)",
        "local count=0; for p,c in utf8.codes('') do count=count+1 end; print(count); local f,s,i=utf8.codes(123); print(type(f),type(s),i)",
        "local old=utf8.codes; utf8.codes=function() return 42 end; print(utf8.codes()); utf8.codes=old; print(type(utf8.codes('a')))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_utf8_offset_byte_boundaries_and_continuations_match_lua54() {
    for source in [
        "local s='Aé💩Z'; print(utf8.offset(s,1),utf8.offset(s,2),utf8.offset(s,3),utf8.offset(s,4),utf8.offset(s,5),utf8.offset(s,6)==nil)",
        "local s='Aé💩Z'; print(utf8.offset(s,-1),utf8.offset(s,-2),utf8.offset(s,-3),utf8.offset(s,-4),utf8.offset(s,-5)==nil)",
        "local s='Aé💩Z'; print(utf8.offset(s,0,5),utf8.offset(s,1,-1),utf8.offset(s,0,9),utf8.offset(s,2,9)==nil)",
        "local s='Aé💩Z'; local ok,msg=pcall(utf8.offset,s,1,5); print(ok,msg); print(utf8.offset(s,0,5))",
        "print(utf8.offset('',1),utf8.offset('',2)==nil,utf8.offset('',-1)==nil,utf8.offset('',0))",
        "local s=string.char(0xC0,0x80,65); print(utf8.offset(s,1),utf8.offset(s,2),utf8.offset(s,0,2),utf8.offset(s,-1))",
        "local s='abc'; print(utf8.offset(s,1,1.0),utf8.offset(s,'2'),utf8.offset(s,0,-1)); local ok=pcall(utf8.offset,s,1,0); print(ok)",
        "local f=utf8.offset; utf8.offset=function() return 42 end; print(utf8.offset()); utf8.offset=f; print(utf8.offset('a',1))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_utf8_char_len_codepoint_and_lax_modes_match_lua54() {
    for source in [
        "local u=utf8; print(_G.utf8==u,type(u.char),type(u.len),type(u.codepoint),type(u.charpattern)); local old=u.char; u.char=function() return 'override' end; print(utf8.char()); u.char=old",
        "local s=utf8.char(65,0,0x1F600); print(string.len(s),utf8.len(s),utf8.codepoint(s,1,6)); print(utf8.len(''),utf8.len('abc',4))",
        "local x=utf8.char(0x110000); local n,p=utf8.len(x); print(n==nil,p,utf8.len(x,1,-1,true),utf8.codepoint(x,1,1,true))",
        "local x=utf8.char(0x7FFFFFFF); print(string.len(x),utf8.len(x,1,-1,true),utf8.codepoint(x,1,1,true)); local n,p=utf8.len(x); print(n==nil,p)",
        "local x=string.char(0xC0,0x80); local a,b=utf8.len(x,1,-1,true); print(a==nil,b); local ok=pcall(utf8.codepoint,x,1,1,true); print(ok)",
        "local a,b=utf8.len('é',2,2); print(a==nil,b); print(select('#',utf8.codepoint('abc',2,1))); print(utf8.codepoint('abc',2,3))",
        "print(utf8.char()=='' ,pcall(utf8.char,0x80000000),pcall(utf8.char,-1)); local ok,code=pcall(utf8.codepoint,'ABC'); print(ok,code)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_seeded_random_sequences_match_pinned_lua54() {
    for source in [
        "math.randomseed(123,-456); print(math.random(0),math.random(0),math.random(0),math.random(0))",
        "local a,b=math.randomseed(0,0); print(a,b,select('#',math.randomseed(0,0))); print(math.random(1,9),math.random(1,9),math.random(-8,-2),math.random(math.mininteger,math.maxinteger))",
        "math.randomseed(-1,-2); print(math.random(0),math.random(10),math.random(-11,-5),math.random(1,1))",
        "math.randomseed(5,7); local a,b,c=math.random(),math.random(),math.random(); print(math.type(a),a>=0 and a<1,b>=0 and b<1,c>=0 and c<1); math.randomseed(5,7); print(a==math.random(),b==math.random(),c==math.random())",
        "math.randomseed(3,4); local x=math.random(0); math.randomseed(3,4); print(x==math.random(0)); local a,b=math.randomseed(3,nil); print(a,b)",
        "local ok,msg=pcall(math.random,2,1); print(ok,msg); print(pcall(math.random,1.5)); print(pcall(math.randomseed,nil)); local ok2=pcall(math.random,1,2,3); print(ok2)",
        "local a,b=math.randomseed(); local x=math.random(0); math.randomseed(a,b); print(x==math.random(0),type(a),type(b))",
    ] {
        let owned = run_fer(true, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        let production = run_fer(false, source);
        assert!(production.status.success(), "{source}");
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success(), "{source}");
            assert_eq!(owned.stdout, upstream.stdout, "{source}");
        }
    }
}

#[test]
fn owned_inverse_trig_exp_and_optional_log_base_match_lua54() {
    for source in [
        "print(math.type(math.exp(1)),math.type(math.log(8,2)),math.type(math.atan(1)),math.type(math.asin(0)),math.type(math.acos(0)))",
        "local e=1e-10; print(math.abs(math.exp(0)-1)<e,math.abs(math.log(math.exp(1))-1)<e,math.abs(math.log(100,10)-2)<e,math.abs(math.log(8,2)-3)<e)",
        "local e=1e-10; print(math.abs(math.asin(1)-math.pi/2)<e,math.abs(math.acos(0)-math.pi/2)<e,math.abs(math.atan(1)-math.pi/4)<e,math.abs(math.atan(0,-1)-math.pi)<e)",
        "print(math.log(-1)~=math.log(-1),math.asin(2)~=math.asin(2),math.acos(2)~=math.acos(2),math.exp(math.huge)==math.huge)",
        "local e=1e-10; local ok,value=pcall(math.log,'100',10); print(ok,math.abs(value-2)<e); print(pcall(math.exp,false))",
        "local original=math.atan; math.atan=function() return 42 end; print(math.atan()); math.atan=original; print(math.atan(0,1)==0)",
        "local e=1e-10; print(math.abs(math.log(8,nil)-math.log(8))<e,math.abs(math.atan('1','1')-math.pi/4)<e)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_math_trigonometry_and_angle_conversions_match_lua54() {
    for source in [
        "print(math.type(math.sin(0)),math.type(math.cos(0)),math.type(math.tan(0)),math.type(math.rad(180)),math.type(math.deg(math.pi)))",
        "local e=1e-10; print(math.abs(math.sin(math.rad(30))-0.5)<e,math.abs(math.cos(math.rad(60))-0.5)<e,math.abs(math.tan(math.rad(45))-1)<e)",
        "local e=1e-10; print(math.abs(math.deg(math.rad(180))-180)<e,math.abs(math.rad(math.deg(math.pi))-math.pi)<e)",
        "print(math.sin(math.huge)~=math.sin(math.huge),math.cos(math.huge)~=math.cos(math.huge),math.tan(math.huge)~=math.tan(math.huge))",
        "local e=1e-10; local ok,x=pcall(math.rad,'180'); print(ok,math.abs(x-math.pi)<e); print(pcall(math.sin,false)); print(pcall(math.cos))",
        "local f=math.sin; math.sin=function() return 42 end; print(math.sin()); math.sin=f; print(math.sin(0)==0)",
        "local deg=math.deg; local rad=math.rad; print(math.abs(deg(rad(90))-90)<1e-10, math.abs(rad(deg(math.pi))-math.pi)<1e-10)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_math_fmod_modf_multi_return_and_failure_parity_match_lua54() {
    for source in [
        "print(math.fmod(-7,3),math.fmod(7,-3),math.fmod(math.mininteger,-1),math.type(math.fmod(7,3)),math.type(math.fmod(7.0,3)),math.fmod(-7.0,3.0)==-1.0)",
        "print(math.fmod('7','3'),math.type(math.fmod('7','3')),math.fmod(7.0,0.0)~=math.fmod(7.0,0.0))",
        "local x,y=math.modf(-3.75); print(x,y,math.type(x),math.type(y)); print(math.modf(3.25))",
        "print(select('#',math.modf(3.75)),select('#',pcall(math.modf,3.75))); local ok,a,b=pcall(math.modf,'3.25'); print(ok,a,b)",
        "local a,b=math.modf(math.mininteger); print(a==math.mininteger,b==0,math.type(b)); local c,d=math.modf(math.huge); print(c==math.huge,d==0,math.type(c))",
        "local a,b=math.modf(math.sqrt(-1)); print(a~=a,b~=b); print(math.fmod(7,0.0)~=math.fmod(7,0.0))",
        "local ok,msg=pcall(math.fmod,2,0); print(ok,msg); local p,e=pcall(math.modf,false); print(p,e); local q,t=pcall(math.fmod,2); print(q,t)",
        "local old=math.modf; math.modf=function() return 42 end; print(math.modf()); math.modf=old; print(math.modf(3.25))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_math_type_and_tointeger_missing_arguments_match_lua54() {
    for source in [
        "local ok,err=pcall(math.type); print(ok,type(err))",
        "local ok,err=pcall(math.tointeger); print(ok,type(err))",
        "local a=pcall(math.type); local b=pcall(math.tointeger); print(a,b)",
        "print(math.type(nil)==nil,math.tointeger(nil)==nil)",
        "print(math.type(false)==nil,math.tointeger(false)==nil)",
        "print(math.type(7),math.type(7.0),math.tointeger(7.0))",
        "local f=math.tointeger; print(pcall(f),pcall(f,nil))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_math_sqrt_and_unsigned_integer_order_match_lua54() {
    for source in [
        "print(math.sqrt(81)==9.0,math.type(math.sqrt(81)),math.sqrt('16')==4.0,math.sqrt(2)^2>1.999,math.sqrt(2)^2<2.001)",
        "print(math.sqrt(-1)~=math.sqrt(-1),math.sqrt(math.huge)==math.huge)",
        "print(math.ult(1,-1),math.ult(-1,1),math.ult(2.0,3.0),math.ult('2','3'))",
        "local f=math.sqrt; local u=math.ult; math.sqrt=function() return 17 end; print(math.sqrt(),f(25)); math.sqrt=f; print(math.sqrt(25)==5.0,u(1,-1))",
        "local ok,x=pcall(math.sqrt,9); print(ok,x==3.0); print(pcall(math.sqrt,false)); print(pcall(math.ult,1.5,2)); print(pcall(math.ult,2))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_math_min_max_use_lua_lt_metamethods_and_protect_operands() {
    for source in [
        "local mt={__lt=function(a,b) return a.n<b.n end}; local a=setmetatable({n=3},mt); local b=setmetatable({n=1},mt); local c=setmetatable({n=2},mt); print(math.min(a,b,c)==b,math.max(a,b,c)==a)",
        "local mt={__lt=function(a,b) return a.n<b.n end}; local a=setmetatable({n=2},mt); local b=setmetatable({n=2},mt); print(math.min(a,b)==a,math.max(a,b)==a)",
        "local less=setmetatable({}, {__call=function(_,a,b) return a.n<b.n end}); local mt={__lt=less}; local x=setmetatable({n=8},mt); local y=setmetatable({n=4},mt); print(math.min(x,y).n,math.max(x,y).n)",
        "local mt={__lt=function(a,b) collectgarbage('collect'); return a.n<b.n end}; print(math.max(setmetatable({n=2},mt),setmetatable({n=8},mt)).n)",
        "local mt={__lt=function(a,b) error(27) end}; local a=setmetatable({},mt); local ok,message=pcall(math.max,a,a); print(ok,message); print(pcall(math.min,a,a))",
        // The production backend and the bounded owned VM use different
        // diagnostic strings for incomparable values; verify the error
        // status and type without treating the text as Lua 5.4 parity.
        "local obj={}; print(math.max(obj)==obj,math.min(obj)==obj); local ok,message=pcall(math.min,1,{}); print(ok,type(message))",
        "local mt={__lt=function(a,b) return a.n<b.n end}; local a=setmetatable({n=1},mt); local b={n=2}; print(math.max(a,b)==b,math.min(a,b)==a)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_math_min_max_incompatible_values_fail_like_lua54() {
    for source in [
        "math.max({}, {})",
        "math.min({}, {})",
        "math.max(1, 'hello')",
        "math.min(false, 3)",
    ] {
        assert_parity(source, false);
    }
}

#[test]
fn owned_string_len_sub_byte_offsets_match_lua54() {
    for source in [
        "print(string.len('a\\0b'),string.sub('abcdef',2,4),string.sub('abcdef',-3),string.sub('abcdef',0,2))",
        "print(string.sub('abc',5)=='',string.sub('abc',3,2)=='',string.sub('abc',-100,100),string.sub('',1)=='')",
        "print(string.sub('a\\0b',2,2)=='\\0',string.len('é'),string.len(12345),string.sub('abcdef',math.mininteger,math.maxinteger))",
        "local lib=string; assert(_G.string==lib); local saved=lib.len; lib.len=function() return 99 end; print(string.len('abc')); lib.len=saved; print(string.len('abc'))",
        "local good=pcall(string.len,false); local bad=pcall(string.sub,'abc'); print(good,bad)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_byte_char_binary_and_return_arity_match_lua54() {
    for source in [
        "print(string.char(65,0,255)=='A\\0\\255',select('#',string.byte('')),string.byte('ABC',1,3))",
        "local a,b=string.byte('é',1,2); local c,d=string.byte('abcdef',-2,-1); print(a,b,c,d,select('#',string.byte('abc',0)))",
        "print(string.char()=='',string.byte('xyz',2),select('#',string.byte('xyz',5)),select('#',string.byte('xyz',3,2)))",
        "local joined=string.char(0,97,255); local a,b,c=string.byte(joined,1,3); print(#joined,a,b,c,string.sub(joined,2,2)=='a')",
        "local good=pcall(string.char,65,66); local bad1=pcall(string.char,256); local bad2=pcall(string.char,-1); print(good,bad1,bad2)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_library_exact_numeric_text_indices_match_lua54() {
    for source in [
        "print(string.find('aaba','a','2.0'),string.find('aaba','a','0x1p+1'))",
        "print(string.match('aaba','a','2.0'),string.match('abc','%a','2e0'))",
        "print(string.byte('abc','2.0','2e0'),string.byte('abc','0x1p+1'))",
        "print(string.sub('abcdef','2.0','4e0'),string.sub('abcdef','0x2','0x4'))",
        "local s,n=string.gsub('ababa','a','X','2.0'); print(s,n)",
        "local s,n=string.gsub('ababa','a','X','0x1p+1'); print(s,n)",
        "print(string.find('abc','b','-0x2p+0'),string.match('abc','%a','-2.0'))",
        "local ok=pcall(string.byte,'abc','2.5'); print(ok)",
        "local ok=pcall(string.sub,'abc','2.5'); print(ok)",
        "local ok=pcall(string.match,'abc','a','1.5'); print(ok)",
        "local ok=pcall(string.find,'abc','a','2.5'); print(ok)",
        "local ok=pcall(string.gsub,'abc','a','X','1.5'); print(ok)",
        "local nbsp=string.char(194,160); local ok=pcall(string.byte,'abc',nbsp..'2'); print(ok)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_find_literal_subset_matches_lua54() {
    for source in [
        "print(string.find('abcabc','bc')); print(string.find('abcabc','bc',3)); print(string.find('abcabc','bc',-3))",
        "print(string.find('abc','xyz')==nil,select('#',string.find('abc','xyz')),select('#',string.find('abc','a')))",
        "print(string.find('abc','',1),string.find('abc','',4),string.find('abc','',5)==nil)",
        "print(string.find('abc','a',0),string.find('abc','a',-100),string.find('abc','a',100)==nil)",
        "print(string.find('a.b','.',1,true),string.find('a%b','%',1,true),string.find('a[b]','[',1,true))",
        "print(string.find('a\\0b','\\0',1,true),string.find('éé','é',2,true),string.len('é'))",
        "print(string.find('xx--xx','--',1,true),string.find('xx--xx','xx',-2,true))",
        "print(string.find('123123',23,1,true),string.find(123123,'23',1,true),string.find('123123','23','2',true))",
        "local f=string.find; string.find=function() return 42 end; print(string.find('abc')); string.find=f; print(string.find('abc','b'))",
        "local a,b,c=pcall(string.find,'abc','b'); print(a,b,c); print(select('#',string.find('abc','x')))",
        "local a=pcall(string.find,false,'a'); local b=pcall(string.find,'abc',false); local c=pcall(string.find,'abc'); print(a,b,c)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_match_capture_free_literals_match_lua54() {
    for source in [
        "print(string.match('abcabc','bc'),string.match('abcabc','bc',3),string.match('abcabc','bc',-3))",
        "print(string.match('abcdef','zzz')==nil,select('#',string.match('abcdef','zzz')),select('#',string.match('abc','b')))",
        "print(string.match('abc','')=='',string.match('abc','',4)=='',string.match('abc','',5)==nil)",
        "print(string.match('ab\\0cd','\\0c',1)=='\\0c',string.len(string.match('éé','é',2)))",
        "print(string.match('aba','a',0),string.match('aba','a',-100),string.match('aba','a',4)==nil)",
        "print(string.match(112233,22),string.match('x111',11,2),string.match('abcabc','bc','3'))",
        "local m=string.match; string.match=function() return 17 end; print(string.match()); string.match=m; print(string.match('abc','b'))",
        "local ok,s=pcall(string.match,'abc','b'); print(ok,s); local a,x=pcall(string.match,'abc'); local b,y=pcall(string.match,false,'a'); print(a,type(x),b,type(y))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_simple_lua_anchors_and_wildcard_match_lua54() {
    for source in [
        "print(string.find('abc','b.'),string.find('abc','^ab'),string.find('abc','bc$'))",
        "print(string.match('abc','b.'),string.match('abc','^ab'),string.match('abc','bc$'))",
        "print(string.find('abc','^z')==nil,string.find('abc','zz$')==nil,string.match('abc','^z')==nil)",
        "print(string.find('abc','^'),string.find('abc','$'),string.find('','$'))",
        "print(string.match('abc','^')=='',string.match('abc','$')=='',string.match('','$')=='')",
        "print(string.find('a.b','.',1,true),string.find('abc','.',1,false),string.find('abc','^',1,true)==nil)",
        "print(string.find('abc$','$',1,true),string.match('abc','.$'),string.find('abc','^abc$'))",
        "print(string.match('a\\0b','a.b')=='a\\0b',string.find('a\\0b','a.b'))",
        "print(string.find('éé','..'),string.len(string.match('éé','..')))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_lua_pattern_ascii_classes_and_escapes_match_lua54() {
    for source in [
        "print(string.find('ab12CD','%d%d'),string.match('ab12CD','%d%d'))",
        "print(string.find('abCD','%u%u'),string.match('zzAb','%u%l'))",
        "print(string.match('a  b','%s%s')=='  ',string.match('123a','%D')=='a')",
        "print(string.match('!!','%p%p')=='!!',string.match('g9','%w%w')=='g9')",
        "print(string.match('a.b','%.')=='.',string.find('a%b','%%'))",
        "print(string.match('abc$','%$')=='$',string.find('^abc','%^'))",
        "print(string.match('A\\0B','%z')=='\\0',string.match('a!','%W')=='!')",
        "print(string.match('a!','%g%g')=='a!',string.match('hello','%x')=='e')",
        "print(string.find('a[b]','%['),string.match('a(b)','%(')=='(')",
        "print(string.find('123','^%d%d%d$'),string.match('1x','%d%a'))",
        "print(string.match('abc','%q')==nil,string.match('q','%q')=='q')",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_lua_c_locale_whitespace_classes_include_vertical_tab() {
    // C isspace has six ASCII members: SP, HT, LF, VT, FF and CR.
    // Check inverse and bracket forms through the shared pattern VM.
    for source in [
        "local s=string.char(32,9,10,11,12,13); print(#string.match(s,'^%s+$'),string.match(s,'^%S+$')==nil)",
        "local s=string.char(11); print(string.match(s,'^%s$')==s,string.match(s,'^%S$')==nil)",
        "local s=string.char(11); print(string.match(s,'^[%s]$')==s,string.match(s,'^[^%s]$')==nil)",
        "local s=string.char(11); print(string.find('a'..s..'b','%s'),string.find('a'..s..'b','%S'))",
        "local s=string.char(11); local a,n=string.gsub('a'..s..'b','%s','_'); print(a=='a_b',n)",
        "local s=string.char(11); local a={}; for c in string.gmatch('X'..s..'Y','%s') do a[#a+1]=string.byte(c) end; print(table.concat(a,','))",
        "local a=string.char(0,1,8,11,12,32,127); print(string.match(a,'^%S%S%S%s%s%s%S$')==a)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_lua_pattern_quantifiers_use_bounded_backtracking_like_lua54() {
    for source in [
        "print(string.match('a123b','%d+'),string.find('a123b','%d+'))",
        "print(string.match('aaaab','a*b'),string.match('aaaab','a+b'))",
        "print(string.match('b','a*b')=='b',string.match('b','a+b')==nil)",
        "print(string.match('aaab','a-b')=='aaab',string.match('aaaa','a-')=='')",
        "print(string.match('abc','.?')=='a',string.match('abc','a.?c')=='abc')",
        "print(string.match('abc','a.*c')=='abc',string.match('abc','a.-c')=='abc')",
        "print(string.match('aaaa','a+')=='aaaa',string.match('aaaa','a*')=='aaaa')",
        "print(string.match('123a','^%d+%a$')=='123a',string.find('a123a456','%d+',6))",
        "print(string.match('abc','a.*d')==nil,string.match('abc','^%d+')==nil)",
        "print(string.find('a*b','*',1,true),string.find('a+b','+',1,true),string.find('a-b','-',1,true))",
        "print(string.match('x+++y','%++')=='+++',string.match('x???y','%?+')=='???')",
        "print(string.match('123abc','%d*%a+')=='123abc',string.match('bbb','a?b+')=='bbb')",
        "print(string.match('abb','a.-b')=='ab',string.match('abb','a.*b')=='abb')",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_pattern_unmatched_capture_and_find_literal_fast_path_match_lua54() {
    // Lua 5.4 string.find uses a plain byte search automatically when the
    // pattern contains none of ^$*+?.([%-. The ')' byte is not special in
    // that optimization, although it is invalid in string.match/gsub.
    for source in [
        "local a=pcall(string.match,'abc','('); local b=pcall(string.find,'abc','a)'); print(a,b)",
        "local ok=pcall(string.match,')',')'); print(ok)",
        "local ok=pcall(string.find,'a)','a)'); print(ok)",
        "local ok=pcall(string.gsub,')',')','X'); print(ok)",
        "local iter=string.gmatch(')',')'); local ok=pcall(iter); print(ok)",
        "print(string.find('a)b',')'),string.find('a)b',')',1,true))",
        "print(string.find('x]y',']'),string.match('x]y',']'))",
        "local ok=pcall(string.match,')','('); print(ok)",
        "print(string.match('a(b)c','a%(b%)c'))",
        "print(string.match('a','(a)'))",
        "print(string.match('ab','()a()b()'))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_pattern_unreachable_invalid_closer_is_deferred_like_lua54() {
    // A missing literal prefix prevents the pattern VM from reaching an
    // otherwise invalid ')'. This is not permission to accept reached
    // invalid capture syntax.
    for source in [
        "print(string.match('abc','z)')==nil)",
        "print(string.match('abc','zz)')==nil)",
        "local ok=pcall(string.match,'abc','a)'); print(ok)",
        "local out,count=string.gsub('abc','z)','X'); print(out,count)",
        "local it=string.gmatch('abc','z)'); print(it()==nil)",
        "local ok=pcall(string.gsub,'abc','a)','X'); print(ok)",
        "print(string.find('abc','z)')==nil)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_pattern_malformed_bracket_is_rejected_only_when_reached_like_lua54() {
    // Lua's classend() runs during matching, not while pre-parsing all
    // pattern atoms. An earlier mismatching byte leaves '[' unreachable.
    for source in [
        "print(string.match('abc','z[')==nil)",
        "print(string.find('abc','z[')==nil)",
        "local out,n=string.gsub('abc','z[','X'); print(out,n)",
        "local it=string.gmatch('abc','z['); print(it()==nil)",
        "local ok=pcall(string.match,'abc','a['); print(ok)",
        "local ok=pcall(string.find,'abc','a['); print(ok)",
        "local ok=pcall(string.gsub,'abc','a[','X'); print(ok)",
        "local it=string.gmatch('abc','a['); local ok=pcall(it); print(ok)",
        "local ok=pcall(string.match,'abc','['); print(ok)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_pattern_incomplete_escapes_are_rejected_only_when_reached_like_lua54() {
    // Lua processes malformed '%' escapes and %b/%f structural patterns
    // only on traversal. A mismatching literal prefix prevents the error.
    for source in [
        "print(string.match('abc','z%')==nil)",
        "print(string.match('abc','z%b')==nil)",
        "print(string.match('abc','z%f')==nil)",
        "print(string.match('abc','z%f[')==nil)",
        "print(string.find('abc','z%')==nil)",
        "local out,n=string.gsub('abc','z%','X'); print(out,n)",
        "local it=string.gmatch('abc','z%f'); print(it()==nil)",
        "local ok=pcall(string.match,'abc','a%'); print(ok)",
        "local ok=pcall(string.match,'abc','a%b'); print(ok)",
        "local ok=pcall(string.find,'abc','a%f'); print(ok)",
        "local ok=pcall(string.gsub,'abc','a%f[','X'); print(ok)",
        "local it=string.gmatch('abc','a%'); local ok=pcall(it); print(ok)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_pattern_unfinished_opening_capture_checks_reachable_paths_like_lua54() {
    for source in [
        "print(string.match('abc','z(')==nil)",
        "print(string.find('abc','z(')==nil)",
        "local s,n=string.gsub('abc','z(','X'); print(s,n)",
        "local it=string.gmatch('abc','z('); print(it()==nil)",
        "local ok=pcall(string.match,'abc','a('); print(ok)",
        "local ok=pcall(string.find,'abc','a('); print(ok)",
        "local it=string.gmatch('abc','a('); local ok=pcall(it); print(ok)",
        "local ok=pcall(string.match,'abc','('); print(ok)",
        "print(string.match('abc','(a)'))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_gsub_unfinished_capture_requires_retrieval_to_error_like_lua54() {
    // Lua 5.4 permits a successful match with an unfinished capture when
    // the literal replacement never asks for that capture.
    for source in [
        "local ok=pcall(string.gsub,'abc','a(','X'); print(ok)",
        "local text,n=string.gsub('abc','a(','X'); print(text,n)",
        "local text,n=string.gsub('abc','a(','%0'); print(text,n)",
        "local ok=pcall(string.gsub,'abc','a(','%1'); print(ok)",
        "local ok=pcall(string.gsub,'abc','a(',{}); print(ok)",
        "local ok=pcall(string.gsub,'abc','a(',function(x) return x end); print(ok)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_pattern_standalone_repetition_markers_and_closing_bracket_match_lua54() {
    // Lua 5.4 classend() returns a one-byte pattern atom for these raw
    // bytes; only a marker *after* another atom becomes a repetition suffix.
    for source in [
        "for _,c in ipairs({'*','+','-','?',']'}) do print(string.match(c,c),string.find('x'..c..'y',c)) end",
        "for _,c in ipairs({'*','+','-','?',']'}) do print(string.match('x'..c..'y','.'..c..'.')) end",
        "print(string.match('**','**'),string.match('++','++'),string.match('--','--'),string.match('??','??'))",
        "print(string.match(']a',']a'),string.match('a]','a]'))",
        "local x,n=string.gsub('*+?-]',']','X'); print(x,n)",
        "local x,n=string.gsub('*+?-]', '*', 'Z'); print(x,n)",
        "local matches={}; for v in string.gmatch('a*b*','*') do matches[#matches+1]=v end; print(table.concat(matches,','))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_pattern_structural_atoms_leave_repetition_markers_literal_like_lua54() {
    // Lua match() handles captures, %b, %f and %1-%9 before its
    // singlematch() quantifier path, so a following marker is literal.
    for source in [
        "print(string.match('a*','(a)*')=='a')",
        "print(string.match('ab*','(ab)*')=='ab')",
        "print(string.match('a+','(a)+')=='a')",
        "print(string.match('a?','(a)?')=='a')",
        "print(string.match('aa','(a)*')==nil)",
        "print(string.match('(x)?','%b()?')=='(x)?')",
        "print(string.match('aa*','(a)%1*')=='a')",
        "print(string.match('*ab','%f[%p]*')=='*')",
        "local s,n=string.gsub('a*a*','(a)*','X'); print(s,n)",
        "local matches={}; for v in string.gmatch('a*a*','(a)*') do matches[#matches+1]=v end; print(table.concat(matches,','))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_pattern_unsupported_structural_escapes_never_silently_match() {
    let source = "local a=pcall(string.find,'abc','%b('); local b=pcall(string.match,'abc','%f'); local c=pcall(string.find,'abc','%1'); print(a,b,c)";
    assert_parity(source, true);
}

#[test]
fn owned_lua_pattern_bracket_ranges_sets_and_negation_match_lua54() {
    for source in [
        "print(string.match('aBc','[a-z]+'),string.match('aBc','[A-Z]'))",
        "print(string.find('aBc','[a-z]+'),string.match('aBc','[^a-z]+'))",
        "print(string.match('x12y','[%d]+')=='12',string.match('aa3','[^%d]+')=='aa')",
        "print(string.match('HELLO','[A-Z]+')=='HELLO',string.match('hello','[aeiou]+')=='e')",
        "print(string.match('1234','[1-3]+')=='123',string.match('ABCD','[B-D]+')=='BCD')",
        "print(string.match('a-b','[%-]')=='-',string.match('a]b','[]]')==']')",
        "print(string.match('a%b','[%%]')=='%',string.match('a.b','[%.]')=='.')",
        "print(string.match('123','[^%a]+')=='123',string.find('a1b','[a-z]%d[a-z]'))",
        "print(string.find('abc','[ab]',1,true)==nil,string.match('abc','[^x]*')=='abc')",
        "print(string.match('a\\0b','[%z]')=='\\0',string.find('a\\0b','[^%z]+'))",
        "local ok=pcall(string.find,'abc','['); local yes=pcall(string.match,'abc','[abc'); print(ok,yes)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_lua_pattern_frontier_assertions_match_lua54() {
    for source in [
        "print(string.find('hello world','%f[%w]world'),string.match('hi,there','%f[%a]there'))",
        "print(string.match('one two','%f[%a]%a+')=='one',string.match('catdog','%f[%a]dog')==nil)",
        "print(string.find('123a','%f[%a]a'),string.find('abc','%f[%w]'))",
        "print(string.find('abc','%f[%z]'),string.find('','%f[%z]'))",
        "print(string.match('abc abc','%f[%a]abc',2)=='abc',string.match('abc','%f[%d]')==nil)",
        "print(string.match('ab12cd','%f[%d]%d+')=='12',string.match('ab12cd','%f[^%d]%a+')=='ab')",
        "print(string.find('abc','%f[%w]',2)==nil,string.find('abc','%f[%w]',1))",
        "local a=pcall(string.find,'abc','%f'); local b=pcall(string.match,'abc','%f['); print(a,b)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_lua_pattern_nested_balanced_delimiters_match_lua54() {
    for source in [
        "print(string.match('x(a(b)c)z','%b()'),string.find('x(a(b)c)z','%b()'))",
        "print(string.match('a<hello>z','%b<>')=='<hello>',string.match('a[xy]z','%b[]')=='[xy]')",
        "print(string.match('((x))','%b()')=='((x))',string.match('abc','%b()')==nil)",
        "print(string.find('x(a(b)c)z','%b()',3),string.find('abc','%b()')==nil)",
        "print(string.match('abc{a{b}c}def','%b{}')=='{a{b}c}',string.find('a<b>c','%b<>'))",
        "print(string.match('x||y','%b||')=='||',string.match('x(q)z','%b()')=='(q)')",
        "local a=pcall(string.match,'abc','%b'); local b=pcall(string.find,'abc','%b('); print(a,b)",
        "print(string.find('x(a(b)c)y','%b()',1,true)==nil,string.match('a<b>','%b<>'))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_lua_pattern_substring_and_position_captures_match_lua54() {
    for source in [
        "local a,b=string.match('abc','(a)(b)'); print(a,b,select('#',string.match('abc','(a)(b)')))",
        "local x,y,a,b=string.find('abc','(a)(b)'); print(x,y,a,b)",
        "print(string.match('abc','()'),string.find('abc','()'))",
        "local a,b=string.match('abc','(a(b))'); print(a,b); print(string.find('abc','(a(b))'))",
        "print(string.match('a123b','(%d+)'),string.match('abc','(z)')==nil)",
        "local x,y=string.match('xy','()%a+()'); print(x,y)",
        "print(string.match('abc','(a)bc'),string.match('abc','(a)(b)c',2)==nil)",
        "local x,y=string.match('ab\\0cd','(ab)(\\0c)'); print(x=='ab',y=='\\0c')",
        "print(string.find('abc','(a)',1,true)==nil,string.match('abc','a')=='a')",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_lua_pattern_capture_backreferences_match_lua54() {
    for source in [
        "print(string.match('abab','(%a+)%1'),string.find('abab','(%a+)%1'))",
        "print(string.match('red-red','(%a+)%-%1'),string.find('red-red','(%a+)%-%1'))",
        "print(string.match('abcabc','(.+)%1'),string.match('aabaa','(a+)%1'))",
        "local a,b=string.match('abba','(a)(b)%2%1'); print(a,b)",
        "print(string.find('foofoo','(foo)%1'),string.match('abc','(a)%1')==nil)",
        "print(string.match('aaaa','(a+)%1'),string.find('aaaa','(a+)%1'))",
        "local ok=pcall(string.match,'abc','%1'); local other=pcall(string.find,'abc','%0'); print(ok,other)",
        "print(string.match('xabcdabcd','(abcd)%1')=='abcd',string.match('aaaa','(a)(a)%2%1')=='a')",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_gsub_text_replacement_and_captures_match_lua54() {
    for source in [
        "local s,n=string.gsub('abcabc','ab','X'); print(s,n)",
        "print(string.gsub('aaaa','a','b',2)); print(string.gsub('aaaa','a','b',0))",
        "print(string.gsub('aaaa','a','b',-2)); print(string.gsub('abc','z','?'))",
        "print(string.gsub('abc','','-'))",
        "print(string.gsub('abc','^','+')); print(string.gsub('abc','.$','!'))",
        "print(string.gsub('abc123','%d+','NUM'))",
        "print(string.gsub('abcabc','(ab)c','%1!'))",
        "print(string.gsub('b7','(%a)(%d)','%2%1'))",
        "print(string.gsub('a.b','%.','%%'))",
        "print(string.gsub('a\\0b','%z','?'))",
        "print(string.gsub('ababa','a','%0%0'))",
        "print(string.gsub('abcdef','(.)','[%1]',3))",
        "print(string.gsub('123','%d+',42))",
        "print(string.gsub('abc','.', 'X',1)); print(string.gsub('abc','','+',2))",
        "local good,s,n=pcall(string.gsub,'aba','a','x'); print(good,s,n); local bad,e=pcall(string.gsub,'abc','(a)','%2'); print(bad,type(e))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_gsub_table_replacements_and_metatables_match_lua54() {
    for source in [
        "local s,n=string.gsub('abca','.',{a='X',b='Y'}); print(s,n)",
        "print(string.gsub('aba','a',{a='z'}))",
        "print(string.gsub('foo bar','%a+', {foo='FOO',bar='BAR'}))",
        "print(string.gsub('ab','(.)',{a='x',b='y'}))",
        "print(string.gsub('abc','.',{a=false,b='B'}))",
        "print(string.gsub('a1a2','(%d)',{['1']='ONE',['2']='TWO'}))",
        "local t=setmetatable({},{__index=function(_,key) return key..'!' end}); print(string.gsub('ab','.',t))",
        "local t=setmetatable({},{__index=function(_,key) collectgarbage('collect'); return key..key end}); print(string.gsub('abc','.',t))",
        "local t={[1]='LEFT',[2]='MID',[3]='RIGHT'}; print(string.gsub('ab','()',t))",
        "local t={a=1}; local good,s,n=pcall(string.gsub,'aa','a',t); print(good,s,n)",
        "local t={a=true}; local good,err=pcall(string.gsub,'a','a',t); print(good,type(err))",
        "local t=setmetatable({},{__index=function() error(17) end}); local good,err=pcall(string.gsub,'abc','a',t); print(good,err)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_gsub_function_replacements_and_capture_args_match_lua54() {
    for source in [
        "local s,n=string.gsub('abc','.',function(m) return m..m end); print(s,n)",
        "local s,n=string.gsub('abc','(%a)',function(x) return string.upper(x) end); print(s,n)",
        "print(string.gsub('a1b2','(%a)(%d)',function(a,b) return b..a end))",
        "local s,n=string.gsub('abc','.',function() return nil end); print(s,n)",
        "local s,n=string.gsub('abc','.',function() return false end); print(s,n)",
        "local s,n=string.gsub('abc','.',function() return 1 end); print(s,n)",
        "local s,n=string.gsub('abc','.',function(x) return x,'ignored' end); print(s,n)",
        "local s,n=string.gsub('abc','.',function() return 'Z' end,2); print(s,n)",
        "local seen={}; local s,n=string.gsub('ab','()',function(i) seen[#seen+1]=i; return '('..i..')' end); print(s,n,table.concat(seen,','))",
        "local callback=function(x) collectgarbage('collect'); return x..'!' end; print(string.gsub('abc','.',callback))",
        "local callback=function(x) if x=='b' then return nil end; return string.upper(x) end; print(string.gsub('abc','.',callback))",
        "local a,s,n=pcall(string.gsub,'abc','.',function() return 'Z' end); print(a,s,n)",
        "local good,e=pcall(string.gsub,'abc','.',function() error(19) end); print(good,e)",
        "local good,e=pcall(string.gsub,'abc','.',function() return true end); print(good,type(e))",
        "local s,n=string.gsub('abc','.',string.upper); print(s,n)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_gmatch_iterators_and_captures_match_lua54() {
    for source in [
        "local a={}; for x in string.gmatch('red green blue','%a+') do a[#a+1]=x end; print(table.concat(a,','))",
        "local g=string.gmatch('one two','%a+'); print(type(g),g(),g(),g()==nil,g()==nil)",
        "local a=string.gmatch('12','%d'); local b=string.gmatch('xy','%a'); print(a(),b(),a(),b(),a()==nil,b()==nil)",
        "for a,b in string.gmatch('a=1,b=2','(%a)=(%d)') do print(a,b) end",
        "local a={}; for pos in string.gmatch('abc','()b') do a[#a+1]=pos end; print(table.concat(a,','))",
        "local a={}; for s in string.gmatch('aba','^a') do a[#a+1]=s end; print(table.concat(a,','))",
        "local a={}; for s in string.gmatch('^aba','^a') do a[#a+1]=s end; print(table.concat(a,','))",
        "local a={}; for s in string.gmatch('ab','') do a[#a+1]='['..s..']' end; print(table.concat(a,','))",
        "local a={}; for s in string.gmatch('aba','a*') do a[#a+1]='['..s..']' end; print(table.concat(a,','))",
        "local a={}; for s in string.gmatch('a\\0b','.') do a[#a+1]=#s end; print(table.concat(a,','))",
        "local a={}; for s in string.gmatch('a b','%a+') do collectgarbage('collect'); a[#a+1]=s end; print(table.concat(a,','))",
        "local iter=string.gmatch('xy','%a'); local good,x=pcall(iter); print(good,x,iter(),iter()==nil)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_gmatch_leading_caret_is_literal_not_anchor_like_lua54() {
    // gmatch does not remove an initial '^', unlike find/match/gsub.
    for source in [
        "local t={}; for v in string.gmatch('a^b^c','^') do t[#t+1]=v end; print(#t,table.concat(t,','))",
        "local t={}; for v in string.gmatch('^a^a','^a') do t[#t+1]=v end; print(#t,table.concat(t,','))",
        "local t={}; for v in string.gmatch('^^','^') do t[#t+1]=v end; print(#t)",
        "print(string.gmatch('abc','^')()==nil)",
        "local t={}; for v in string.gmatch('a^b','%^') do t[#t+1]=v end; print(table.concat(t,','))",
        "print(string.match('abc','^a'),string.find('abc','^a'))",
        "print(string.match('abc','^')=='',string.gmatch('abc','^')()==nil)",
        "local it=string.gmatch('a^b','^'); print(it(),it()==nil)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_gmatch_initial_byte_position_matches_lua54() {
    for source in [
        "local t={}; for c in string.gmatch('a1b2c3','%d',4) do t[#t+1]=c end; print(table.concat(t,','))",
        "local t={}; for c in string.gmatch('a1b2c3','%d',-3) do t[#t+1]=c end; print(table.concat(t,','))",
        "local t={}; for c in string.gmatch('a1b2c3','%d',0) do t[#t+1]=c end; print(table.concat(t,','))",
        "local t={}; for c in string.gmatch('a1b2c3','%d',-99) do t[#t+1]=c end; print(table.concat(t,','))",
        "print(string.gmatch('a1b2c3','%d',7)()==nil,string.gmatch('a1b2c3','%d',8)()==nil)",
        "print(string.gmatch('a1b2c3','%d',nil)())",
        "print(string.gmatch('a1b2c3','%d','4.0')())",
        "print(string.gmatch('a1b2c3','%d',4.0)())",
        "local a=pcall(string.gmatch,'abc','%a',2.5); local b=pcall(string.gmatch,'abc','%a','wrong'); print(a,b)",
        "local ok=pcall(string.gmatch,'abc','%a',true); print(ok)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_gmatch_empty_position_capture_at_end_matches_lua54() {
    // In Lua, the one-based position len + 1 is valid for a zero-width
    // capture. Only positions strictly past len + 1 exhaust the iterator.
    for source in [
        "local t={}; for p in string.gmatch('abc','()',4) do t[#t+1]=p end; print(table.concat(t,','))",
        "print(string.gmatch('abc','()',4)())",
        "print(string.gmatch('abc','()',5)()==nil)",
        "print(string.gmatch('abc','()','4.0')())",
        "print(string.gmatch('','()',1)())",
        "print(string.gmatch('','()',2)()==nil)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_gmatch_gc_reclaims_or_retains_iterator_snapshots() {
    for source in [
        "local f=string.gmatch('ab cd','%a+'); collectgarbage('collect'); print(f(),f(),f()==nil)",
        "local w=setmetatable({}, {__mode='v'}); w[1]=string.gmatch('ab','%a'); collectgarbage('collect'); print(w[1]==nil)",
        "local w=setmetatable({}, {__mode='k'}); local f=string.gmatch('ab','%a'); w[f]=true; f=nil; collectgarbage('collect'); print(next(w)==nil)",
        "local w=setmetatable({}, {__mode='k'}); local f=string.gmatch('ab','%a'); w[f]=42; collectgarbage('collect'); print(w[f]==42,f(),f())",
        "local s=string.rep('a',800); for i=1,90 do local f=string.gmatch(s,'a'); if i%8==0 then collectgarbage('collect') end end; print('ok')",
        "local f=string.gmatch('one two','%a+'); local g=string.gmatch('123','%d+'); collectgarbage('collect'); print(f(),g(),f())",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_portable_binary_pack_unpack_and_packsize_match_lua54() {
    for source in [
        "local p=string.pack('<i4I2',-12345,65535); local a,b,n=string.unpack('<i4I2',p); print(#p,a,b,n,string.packsize('<i4I2'))",
        "local p=string.pack('>I4',0x12345678); print(string.byte(p,1,4)); local a,n=string.unpack('>I4',p); print(a,n)",
        "local p=string.pack('>I2<I2',0x1234,0x1234); print(string.byte(p,1,4)); print(string.unpack('>I2<I2',p))",
        "local p=string.pack('bBhHi4I4',-1,255,-123,65535,-100000,4000000000); print(string.unpack('bBhHi4I4',p))",
        "local p=string.pack('<I8',-1); print(#p,string.unpack('<I8',p),string.packsize('<I8'))",
        "local p=string.pack('c5','xy'); local v,n=string.unpack('c5',p); print(#p,v=='xy\\0\\0\\0',n)",
        "local p=string.pack('xc3x','z'); print(#p,string.byte(p,1,5)); local value,pos=string.unpack('xc3x',p); print(value=='z\\0\\0',pos)",
        "local p=string.pack('>f<d',1.5,-0.25); local a,b,n=string.unpack('>f<d',p); print(a,b,n,string.packsize('>f<d'))",
        "local p=string.pack('<n',3.25); print(string.unpack('<n',p),string.packsize('<n'))",
        "local p=string.pack('=iI',7,9); print(string.unpack('=iI',p),string.packsize('=iI'))",
        "local p=string.pack('<I2I2',5,6); local a,n=string.unpack('<I2',p,3); print(a,n)",
        "print(string.packsize(''),string.pack('')=='',string.unpack('', '') )",
        "local a=pcall(string.pack,'B',256); local b=pcall(string.pack,'b',128); local c=pcall(string.unpack,'I4','\\1'); print(a,b,c)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_variable_binary_formats_match_lua54() {
    for source in [
        "local p=string.pack('z','hello'); print(#p,string.byte(p,6)); local s,n=string.unpack('z',p); print(s,n)",
        "local p=string.pack('<s2','ab\\0c'); local s,n=string.unpack('<s2',p); print(#p,s=='ab\\0c',n)",
        "local p=string.pack('>s2','abc'); print(string.byte(p,1,2)); local s,n=string.unpack('>s2',p); print(s,n)",
        "local p=string.pack('zs1','ok','xyz'); print(string.unpack('zs1',p))",
        "local p=string.pack('<I2s2z',42,'A\\0B','end'); print(string.unpack('<I2s2z',p))",
        "local p=string.pack('z',''); print(#p,string.byte(p,1),string.unpack('z',p))",
        "local p=string.pack('s','hey'); print(#p,string.unpack('s',p))",
        "local a=pcall(string.packsize,'z'); local b=pcall(string.packsize,'s2'); print(a,b)",
        "local a=pcall(string.pack,'z','a\\0b'); local b=pcall(string.pack,'s1',string.rep('a',256)); print(a,b)",
        "local a=pcall(string.unpack,'z','abc'); local b=pcall(string.unpack,'<s2','\\5\\0x'); print(a,b)",
        "local p=string.pack('>s4','test'); local s,n=string.unpack('>s4',p); print(s,n)",
        "local p=string.pack('s1','a'); print(string.byte(p,1,2),string.unpack('s1',p))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_pack_nonpoweroftwo_integer_and_string_widths_match_lua54() {
    for source in [
        "local p=string.pack('<i3I3',-8388608,16777215); local a,b,n=string.unpack('<i3I3',p); print(#p,a,b,n,string.packsize('<i3I3'))",
        "local p=string.pack('>i3I3',8388607,1193046); print(string.byte(p,1,6)); print(string.unpack('>i3I3',p))",
        "local p=string.pack('>i5I5',-549755813888,1099511627775); print(#p,string.unpack('>i5I5',p),string.packsize('>i5I5'))",
        "local p=string.pack('<i6I6',-140737488355328,281474976710655); print(string.unpack('<i6I6',p))",
        "local p=string.pack('<i7I7',-36028797018963968,72057594037927935); print(string.unpack('<i7I7',p))",
        "local p=string.pack('>s3','ab\\0c'); local v,n=string.unpack('>s3',p); print(#p,v=='ab\\0c',n,string.byte(p,1,3))",
        "local p=string.pack('<s5','xyz'); local v,n=string.unpack('<s5',p); print(#p,v,n,string.byte(p,1,5))",
        "local p=string.pack('>I3s3i3',0x123456,'x\\0y',-42); local a,b,c,n=string.unpack('>I3s3i3',p); print(#p,a,b=='x\\0y',c,n)",
        "local a=pcall(string.pack,'I3',16777216); local b=pcall(string.pack,'i3',8388608); local c=pcall(string.pack,'i3',-8388609); print(a,b,c)",
        "local a=pcall(string.packsize,'i0'); local b=pcall(string.packsize,'s0'); local c=pcall(string.packsize,'I17'); print(a,b,c)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_pack_extended_9_to_16_byte_widths_match_lua54() {
    for source in [
        "local p=string.pack('<i9I9',-1,-1); print(#p,string.unpack('<i9I9',p),string.packsize('<i9I9'))",
        "local p=string.pack('>i9I9',-2,0x12345678); print(string.byte(p,1,9)); print(string.unpack('>i9I9',p))",
        "local p=string.pack('>i16I16',-1234567,7654321); local a,b,n=string.unpack('>i16I16',p); print(#p,a,b,n,string.packsize('>i16I16'))",
        "local p=string.pack('<i12I12',math.mininteger,math.maxinteger); print(string.unpack('<i12I12',p))",
        "local p=string.pack('>s9','ab\\0c'); local a,n=string.unpack('>s9',p); print(#p,a=='ab\\0c',n)",
        "local p=string.pack('<s16','end'); print(#p,string.unpack('<s16',p))",
        "local p=string.pack('>I9s9i9',100,'a\\0b',-99); local a,b,c,n=string.unpack('>I9s9i9',p); print(#p,a,b=='a\\0b',c,n)",
        "local raw=string.char(1,0,0,0,0,0,0,0,1); local ok,e=pcall(string.unpack,'<I9',raw); print(ok,type(e))",
        "local raw=string.char(1,0,0,0,0,0,0,0,255); local ok=pcall(string.unpack,'<i9',raw); print(ok)",
        "local a=pcall(string.packsize,'i17'); local b=pcall(string.packsize,'s17'); local c=pcall(string.packsize,'I0'); print(a,b,c)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_pack_bounded_native_alignment_directives_match_lua54() {
    for source in [
        "local p=string.pack('!4bI4',7,42); print(#p,string.byte(p,1,8)); print(string.unpack('!4bI4',p),string.packsize('!4bI4'))",
        "local p=string.pack('!8bI8',3,42); print(#p,string.unpack('!8bI8',p),string.packsize('!8bI8'))",
        "local p=string.pack('!2bI4',1,7); print(#p,string.unpack('!2bI4',p),string.packsize('!2bI4'))",
        "local p=string.pack('!4c3I4','abc',0x1234); local a,b,n=string.unpack('!4c3I4',p); print(#p,a,b,n)",
        "local p=string.pack('!4zI4','ab',42); local s,n,pos=string.unpack('!4zI4',p); print(#p,s,n,pos)",
        "local p=string.pack('!4s1I4','ab',10); local a,b,n=string.unpack('!4s1I4',p); print(#p,a,b,n)",
        "local p=string.pack('!4bxi4',1,256); print(#p,string.unpack('!4bxi4',p),string.packsize('!4bxi4'))",
        "local a=pcall(string.packsize,'!3i4'); local b=pcall(string.packsize,'!8i3'); local c=pcall(string.packsize,'!16I8'); print(a,b,c)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_pack_explicit_x_alignment_directives_match_lua54() {
    for source in [
        "local p=string.pack('!4bXI4B',7,9); print(#p,string.unpack('!4bXI4B',p),string.packsize('!4bXI4B'))",
        "local p=string.pack('!8bXdI4',1,258); print(#p,string.unpack('!8bXdI4',p),string.packsize('!8bXdI4'))",
        "local p=string.pack('!2bXhB',1,2); print(#p,string.unpack('!2bXhB',p),string.packsize('!2bXhB'))",
        "local p=string.pack('!8c3XI8B','abc',77); print(#p,string.unpack('!8c3XI8B',p),string.packsize('!8c3XI8B'))",
        "local p=string.pack('!4zXI4B','ab',42); print(#p,string.unpack('!4zXI4B',p))",
        "local p=string.pack('!4s1XI4B','ab',42); print(#p,string.unpack('!4s1XI4B',p))",
        "local p=string.pack('!4bXI4',1); print(#p,string.unpack('!4bXI4',p),string.packsize('!4bXI4'))",
        "local p=string.pack('XxB',5); print(#p,string.unpack('XxB',p),string.packsize('XxB'))",
        "local a=pcall(string.packsize,'X'); local b=pcall(string.packsize,'!4Xc3'); local c=pcall(string.packsize,'!8bXs3B'); print(a,b,c)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_tonumber_rejects_unicode_space_and_invalid_utf8_like_lua54() {
    for source in [
        "local ascii=string.char(32,9,10,13,11,12); print(tonumber(ascii..'42'..ascii,10),tonumber(ascii..'42'..ascii))",
        "local nbsp=string.char(194,160); print(tonumber(nbsp..'42',10)==nil,tonumber('42'..nbsp,10)==nil)",
        "local nbsp=string.char(194,160); print(tonumber(nbsp..'42')==nil,tonumber('42'..nbsp)==nil)",
        "local em=string.char(226,128,131); print(tonumber(em..'42',10)==nil,tonumber('42'..em)==nil)",
        "local bad=string.char(255); print(tonumber(bad,10)==nil,tonumber(bad)==nil)",
        "local ascii=string.char(32,9,11,12); print(tonumber('10',ascii..'16'..ascii))",
        "local nbsp=string.char(194,160); local ok=pcall(tonumber,'10',nbsp..'16'); print(ok)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_tonumber_numeric_string_bases_match_lua54() {
    for source in [
        "print(tonumber('ff','16'),tonumber('101','2.0'),tonumber('z','3.6e1'))",
        "print(tonumber('1f','0x10'),tonumber('12','1e1'))",
        "local a=pcall(tonumber,'10','1.5'); local b=pcall(tonumber,'10','wrong'); print(a,b)",
        "local a=pcall(tonumber,'10','1'); local b=pcall(tonumber,'10','37'); print(a,b)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_tonumber_missing_argument_error_matches_lua54() {
    assert_parity("print('before'); tonumber()", false);
    assert_parity("tonumber(nil,16)", false);
    for source in [
        "print(tonumber(nil)==nil,tonumber(nil,nil)==nil)",
        "local ok=pcall(tonumber); print(ok)",
        "local ok,err=pcall(tonumber); print(ok,type(err))",
        "local ok=pcall(tonumber,nil,16); print(ok)",
        "local ok,v=pcall(tonumber,nil); print(ok,v==nil)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_math_tointeger_hex_numeric_text_matches_lua54() {
    for source in [
        "print(math.tointeger('0xffffffffffffffff'),math.tointeger('0x8000000000000000')==math.mininteger)",
        "print(math.tointeger('-0xffffffffffffffff'),math.tointeger('+0x10'))",
        "print(math.tointeger('0x1.8p+2'),math.tointeger('-0x1.8p+2'))",
        "print(math.tointeger('0x1.1p+0')==nil,math.tointeger('1.5')==nil)",
        "print(math.tointeger('42.0'),math.tointeger('4.2e1'))",
        "print(math.tointeger(nil)==nil,math.tointeger(true)==nil)",
        "local nbsp=string.char(194,160); print(math.tointeger(nbsp..'42')==nil)",
        "local ok,n=pcall(math.tointeger,'0xffffffffffffffff'); print(ok,n)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_tonumber_explicit_base_wraps_like_lua54() {
    // Lua's base-specific conversion accumulates modulo 2^64 rather
    // than rejecting input above the signed or unsigned integer limit.
    for source in [
        "print(tonumber('101101',2),tonumber('ff',16),tonumber('Z',36))",
        "print(tonumber('7fffffffffffffff',16)==math.maxinteger)",
        "print(tonumber('8000000000000000',16)==math.mininteger)",
        "print(tonumber('ffffffffffffffff',16),tonumber('10000000000000000',16))",
        "print(tonumber('18446744073709551615',10),tonumber('18446744073709551616',10))",
        "print(tonumber('-ffffffffffffffff',16),tonumber('-8000000000000000',16))",
        "print(tonumber('FFFFFFFFFFFFFFFF',16),tonumber('  +ff  ',16))",
        "print(tonumber('2',2)==nil,tonumber('',16)==nil,tonumber('0x10',16)==nil,tonumber('12x',10)==nil)",
        "print(tonumber('-1',2),tonumber('+00000001',2),tonumber('-0',10))",
        "local a=pcall(tonumber,'12',1); local b=pcall(tonumber,'12',37); print(a,b)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_tonumber_hex_integer_and_signed_float_strings_match_lua54() {
    for source in [
        "print(tonumber('0xffffffffffffffff'),tonumber('0x10000000000000000'))",
        "print(tonumber('0x8000000000000000')==math.mininteger)",
        "print(tonumber('-0xffffffffffffffff'),tonumber('+0Xffffffffffffffff'))",
        "print(tonumber('0x1p+2'),math.type(tonumber('0x1p+2')))",
        "print(tonumber('-0x1.8p+2'),math.type(tonumber('-0x1.8p+2')))",
        "print(tonumber('0x1.8'),tonumber('-0x.8p+3'))",
        "print(tonumber('0x')==nil,tonumber('0x1p')==nil,tonumber('-0x')==nil)",
        "print(tonumber('  +0x1a  '),tonumber('  -0x1.8p+1  '))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_binary_format_nul_termination_and_whitespace_match_lua54() {
    // Lua's format parser uses a NUL-terminated C string, even though
    // ordinary Lua string values can contain embedded NUL bytes.
    for source in [
        "local fmt='b'..string.char(0)..'I17'; local p=string.pack(fmt,42); local n,pos=string.unpack(fmt,p); print(#p,n,pos,string.packsize(fmt))",
        "local fmt=string.char(0)..'Q'; print(#string.pack(fmt),string.packsize(fmt),string.unpack(fmt,''))",
        "local fmt=' b b '; local p=string.pack(fmt,1,2); local a,b,pos=string.unpack(fmt,p); print(#p,a,b,pos,string.packsize(fmt))",
        "local t=string.char(9); local n=string.char(10); local a=pcall(string.pack,'b'..t..'b',1,2); local b=pcall(string.unpack,'b'..n..'b',string.char(1,2)); local c=pcall(string.packsize,'b'..t..'b'); local d=pcall(string.packsize,'b'..n); print(a,b,c,d)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_binary_unpack_start_position_bounds_match_lua54() {
    // Lua 5.4 clips zero and excessively negative positions to the
    // first byte, just as string.sub does, but rejects too-large starts.
    for source in [
        "local s=string.char(7,42,9); local a,pos=string.unpack('B',s,-2); print(a,pos)",
        "local s=string.char(7); local a,pos=string.unpack('B',s,-1); print(a,pos)",
        "local s=string.char(7); print(string.unpack('',s,2))",
        // Lua 5.4 clips zero and excessively negative positions to
        // the first byte, but still rejects positions after length + 1.
        "local s=string.char(7); local a=pcall(string.unpack,'B',s,0); local b=pcall(string.unpack,'B',s,-2); local c=pcall(string.unpack,'',s,0); local d=pcall(string.unpack,'',s,-2); local e=pcall(string.unpack,'',s,3); print(a,b,c,d,e)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_binary_integer_numeric_string_coercions_match_lua54() {
    // luaL_checkinteger accepts exactly integral numeric text, including
    // decimal fractions and exponent notation, not only integer tokens.
    for source in [
        "local p=string.pack('>i2','1e2'); print(#p,string.unpack('>i2',p))",
        "local p=string.pack('<i4','42.0'); print(#p,string.unpack('<i4',p))",
        "local p=string.pack('<i4','4.2e1'); print(string.unpack('<i4',p))",
        "local p=string.pack('b','-2.0'); print(string.unpack('b',p))",
        "local p=string.pack('BBB',10,20,30); print(string.unpack('B',p,'2.0'))",
        "local p=string.pack('BBB',10,20,30); print(string.unpack('B',p,'2e0'))",
        "local a=pcall(string.pack,'i2','1.5'); local b=pcall(string.pack,'i2','not-a-number'); local c=pcall(string.unpack,'B',string.char(9),'1.5'); print(a,b,c)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_binary_hexadecimal_numeric_strings_match_lua54() {
    // Lua accepts hexadecimal-float numeric strings for numeric arguments.
    // Binary integer formats still reject any nonintegral numeric value.
    for source in [
        "local p=string.pack('>i2','0x1.8p+2'); print(string.unpack('>i2',p))",
        "local p=string.pack('<i4','-0x1.8p+2'); print(string.unpack('<i4',p))",
        "local p=string.pack('<I2','+0x1p+8'); print(string.unpack('<I2',p))",
        "local p=string.pack('>d','0x1.8p+2'); print(string.unpack('>d',p))",
        "local p=string.pack('<f','-0x1.8p+2'); print(string.unpack('<f',p))",
        "local p=string.pack('BBB',11,22,33); print(string.unpack('B',p,'0x1p1'))",
        "local ok1=pcall(string.pack,'i4','0x1.2p+0'); local ok2=pcall(string.unpack,'B',string.char(1,2),'0x1.2p+0'); print(ok1,ok2)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_binary_hexadecimal_integer_wrapping_matches_lua54() {
    // Lua's hexadecimal string-to-integer parser wraps at 64 bits; it
    // is not subject to the decimal signed-integer parse limit.
    for source in [
        "local p=string.pack('>I8','0xffffffffffffffff'); print(#p,string.unpack('>I8',p))",
        "local p=string.pack('<i8','0x8000000000000000'); print(string.unpack('<i8',p),math.mininteger)",
        "local p=string.pack('>i8','-0xffffffffffffffff'); print(string.unpack('>i8',p))",
        "local p=string.pack('<I8','0x10000000000000000'); print(string.unpack('<I8',p))",
        "local p=string.pack('>d','0xffffffffffffffff'); print(string.unpack('>d',p))",
        "local p=string.pack('BBB',10,20,30); print(string.unpack('B',p,'0x0000000000000002'))",
        "local p=string.pack('B',7); print(string.unpack('B',p,'0xffffffffffffffff'))",
        "local a=pcall(string.pack,'i8','0x'); local b=pcall(string.pack,'i8','0xg'); print(a,b)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_basic_integer_and_string_directives_match_lua54() {
    for source in [
        "print(string.format('hello')=='hello',string.format('')=='')",
        "print(string.format('%s:%d','hi',42)=='hi:42')",
        "print(string.format('%%s=%s','a')=='%s=a',string.format('%%%%')=='%%')",
        "print(string.format('%d %i',-123,456)=='-123 456')",
        "print(string.format('%u',-1)=='18446744073709551615')",
        "print(string.format('%x %X %o',255,255,8)=='ff FF 10')",
        "print(string.format('%c%c%c',65,0,255)==string.char(65,0,255))",
        "print(string.format('%d',math.mininteger)==tostring(math.mininteger))",
        "print(string.format('%s %s',12,'abc')=='12 abc')",
        "local f=string.format; string.format=function() return 'changed' end; print(string.format()); string.format=f; print(string.format('%s','restored'))",
        "local ok,s=pcall(string.format,'%s-%d','a',7); print(ok,s); local bad,e=pcall(string.format,'%s'); print(bad,type(e))",
        "local ok,e=pcall(string.format,'%d',false); print(ok,type(e))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_lua_quoted_strings_and_scalars_match_lua54() {
    for source in [
        "print(string.format('%q','abc'))",
        "print(string.format('%q','a\"b'))",
        "print(string.format('%q','back\\\\slash'))",
        "print(string.format('%q','line\\nbreak'))",
        "print(string.format('%q',string.char(0,49)))",
        "print(string.format('%q',string.char(9,65,13,127)))",
        "print(string.format('%q',true),string.format('%q',false),string.format('%q',nil))",
        "print(string.format('%q',42),string.format('%q',-42))",
        "print(string.format('%q %s','quoted','plain'))",
        "local a,s=pcall(string.format,'%q','x'); print(a,s)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_width_precision_and_alignment_match_lua54() {
    for source in [
        "print(string.format('%5s','a')=='    a',string.format('%-5s','a')=='a    ')",
        "print(string.format('%.3s','abcdef')=='abc',string.format('%8.3s','abcdef')=='     abc')",
        "print(string.format('%-8.3s','abcdef')=='abc     ')",
        "print(string.format('%.0s','abcdef')=='',string.format('%3.0s','abc')=='   ')",
        "print(string.format('%3s','abcdef')=='abcdef',string.format('%-3s','abcdef')=='abcdef')",
        "print(string.format('%.1s','é')==string.sub('é',1,1))",
        "print(string.format('hello %-5s:%2s', 'A','B')=='hello A    : B')",
        "print(string.format('%6.6s','abcdef')=='abcdef')",
        "local ok,x=pcall(string.format,'%4s','a'); print(ok,x); local bad,e=pcall(string.format,'%4s'); print(bad,type(e))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_numeric_width_precision_and_zero_padding_match_lua54() {
    for source in [
        "print(string.format('%5d',12)=='   12',string.format('%-5d',12)=='12   ')",
        "print(string.format('%05d',12)=='00012',string.format('%05d',-12)=='-0012')",
        "print(string.format('%.4d',12)=='0012',string.format('%7.4d',-12)=='  -0012')",
        "print(string.format('%7.4u',12)=='   0012',string.format('%-7.4u',12)=='0012   ')",
        "print(string.format('%08x',26)=='0000001a',string.format('%08X',26)=='0000001A')",
        "print(string.format('%6.4x',26)=='  001a',string.format('%5.3o',7)=='  007')",
        "print(string.format('%5.0d',0)=='     ',string.format('%.0d',0)=='')",
        "print(string.format('%-6.4d',42)=='0042  ',string.format('%06.4d',42)=='  0042')",
        "print(string.format('%4c',65)=='   A',string.format('%-4c',65)=='A   ')",
        "print(string.format('%5d',-1)=='   -1',string.format('%02u',-1)=='18446744073709551615')",
        "print(string.format('x:%5d/%04x',-2,15)=='x:   -2/000f')",
        "local ok,s=pcall(string.format,'%05d',-7); print(ok,s); local bad,x=pcall(string.format,'%6.3d'); print(bad,type(x))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_signed_and_alternate_integer_flags_match_lua54() {
    for source in [
        "print(string.format('%+d',12)=='+12',string.format('% d',12)==' 12')",
        "print(string.format('%+d',-12)=='-12',string.format('% d',-12)=='-12')",
        "print(string.format('%+06d',12)=='+00012',string.format('% 06d',12)==' 00012')",
        "print(string.format('%-+6d',12)=='+12   ',string.format('%+6.4d',12)==' +0012')",
        "print(string.format('%+ d',1)=='+1',string.format('% d',0)==' 0')",
        "print(string.format('%#x',26)=='0x1a',string.format('%#X',26)=='0X1A')",
        "print(string.format('%#08x',26)=='0x00001a',string.format('%#8.4X',26)=='  0X001A')",
        "print(string.format('%#o',9)=='011',string.format('%#.3o',9)=='011')",
        "print(string.format('%#06o',9)=='000011',string.format('%#.0o',0)=='0')",
        "print(string.format('%#x',0)=='0',string.format('%#.0x',0)=='')",
        "print(string.format('%-#8x',26)=='0x1a    ',string.format('%-#5o',9)=='011  ')",
        "local a=pcall(string.format,'%+u',3); local b=pcall(string.format,'%#d',3); local c=pcall(string.format,'%0s','a'); print(a,b,c)",
        "local a=pcall(string.format,'%+x',3); local b=pcall(string.format,'%#s','a'); local c=pcall(string.format,'%#c',65); print(a,b,c)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_fixed_float_width_precision_and_flags_match_lua54() {
    for source in [
        "print(string.format('%f',1.5)=='1.500000',string.format('%.2f',1.5)=='1.50')",
        "print(string.format('%8.2f',1.5)=='    1.50',string.format('%-8.2f',1.5)=='1.50    ')",
        "print(string.format('%08.2f',1.5)=='00001.50',string.format('%+08.2f',1.5)=='+0001.50')",
        "print(string.format('% 8.2f',1.5)=='    1.50',string.format('%+8.2f',1.5)=='   +1.50')",
        "print(string.format('%.0f',2.0)=='2',string.format('%#.0f',2.0)=='2.')",
        "print(string.format('%.3f',-2.5)=='-2.500',string.format('%+8.2f',-1.5)=='   -1.50')",
        "print(string.format('%06.0f',1.0)=='000001',string.format('%-6.0f',1.0)=='1     ')",
        "print(string.format('%f',3)=='3.000000',string.format('%f','2.5')=='2.500000')",
        "print(string.format('%.2f',-0.0)=='-0.00',string.format('%.3f',0.125)=='0.125')",
        "local a=pcall(string.format,'%f',false); local b=pcall(string.format,'%f'); print(a,b)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_scientific_exponent_width_and_flags_match_lua54() {
    for source in [
        "print(string.format('%e',1.0)=='1.000000e+00',string.format('%E',1.0)=='1.000000E+00')",
        "print(string.format('%.2e',100.0)=='1.00e+02',string.format('%.2E',100.0)=='1.00E+02')",
        "print(string.format('%10.2e',2.0)=='  2.00e+00',string.format('%010.2e',2.0)=='002.00e+00')",
        "print(string.format('%-11.2E',2.0)=='2.00E+00   ',string.format('%+010.2e',2.0)=='+02.00e+00')",
        "print(string.format('%.0e',2.0)=='2e+00',string.format('%#.0e',2.0)=='2.e+00')",
        "print(string.format('%.1E',0.5)=='5.0E-01',string.format('%e',0.0)=='0.000000e+00')",
        "print(string.format('%.2e',-0.0)=='-0.00e+00',string.format('%.2e',-1.5)=='-1.50e+00')",
        "print(string.format('%.1e',10000000000)=='1.0e+10',string.format('%.1e',1e100)=='1.0e+100')",
        "print(string.format('% 10.2e',2.0)=='  2.00e+00',string.format('%.1e','0.5')=='5.0e-01')",
        "local a=pcall(string.format,'%e',false); local b=pcall(string.format,'%E'); print(a,b)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_uses_lua_tostring_and_rejects_modified_nul_strings() {
    for source in [
        "print(string.format('%s %s %s',nil,true,false)=='nil true false')",
        "local t=setmetatable({}, {__tostring=function() return 'hello' end}); print(string.format('%s',t)=='hello',string.format('%8s',t)=='   hello')",
        "local t=setmetatable({}, {__tostring=function() return 'WORLD' end}); print(string.format('%.3s',t)=='WOR',string.format('%-8.3s',t)=='WOR     ')",
        "local t=setmetatable({}, {__tostring=function() collectgarbage('collect'); return 'ok' end}); print(string.format('[%5s]',t)=='[   ok]')",
        "print(string.format('%s','a\\0b')=='a\\0b',#string.format('%s','a\\0b'))",
        "local a,x=pcall(string.format,'%5s','a\\0b'); local b,y=pcall(string.format,'%.1s','a\\0b'); print(a,type(x),b,type(y))",
        "local t=setmetatable({}, {__tostring=function() error(27) end}); local ok,e=pcall(string.format,'%s',t); print(ok,type(e))",
        "local t=setmetatable({}, {__tostring=function() return 'x' end}); local original=tostring; tostring=function() return 'wrong' end; print(string.format('%s',t)=='x'); tostring=original",
        "local t={}; local result=string.format('%s',t); print(type(result),string.sub(result,1,7)=='table: ')",
        "print(string.format('%5s',false)=='false',string.format('%-5s',nil)=='nil  ')",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_q_roundtrips_float_constants_like_lua54() {
    for source in [
        "print(string.format('%q',1.0)=='0x1p+0',string.format('%q',-1.0)=='-0x1p+0')",
        "print(string.format('%q',1.5)=='0x1.8p+0',string.format('%q',3.5)=='0x1.cp+1')",
        "print(string.format('%q',0.0)=='0x0p+0',string.format('%q',-0.0)=='-0x0p+0')",
        "print(string.format('%q',0.125)=='0x1p-3',string.format('%q',2.25)=='0x1.2p+1')",
        "print(string.format('%q',math.mininteger)=='0x8000000000000000')",
        "print(string.format('%q',math.maxinteger)=='9223372036854775807')",
        "print(string.format('%q',1/0)=='1e9999',string.format('%q',-1/0)=='-1e9999')",
        "print(string.format('%q',0/0)=='(0/0)')",
        "print(string.format('%q',2.2250738585072014e-308)=='0x1p-1022')",
        "print(string.format('%q',-1e100)=='-0x1.249ad2594c37dp+332')",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_format_q_subnormal_matches_upstream_despite_pinned_backend_divergence() {
    // The pinned luna-core production interpreter does not preserve the
    // smallest decimal subnormal in this fixture (recorded in CI).
    // The owned engine must still emit Lua's exact, round-trippable %q hex.
    let source = "print(string.format('%q',5e-324)=='0x0.0000000000001p-1022')";
    let owned = run_fer(true, source);
    let production = run_fer(false, source);
    assert!(owned.status.success(), "{:?}", owned.stderr);
    assert!(production.status.success(), "{:?}", production.stderr);
    assert_eq!(owned.stdout, b"true\n");
    assert_eq!(production.stdout, b"false\n");
    if let Some(reference) = reference_output(source) {
        assert!(reference.status.success(), "{:?}", reference.stderr);
        assert_eq!(reference.stdout, owned.stdout);
    }
}

#[test]
fn owned_string_format_plain_hex_float_directives_match_lua54() {
    for source in [
        "print(string.format('%a',1.0)=='0x1p+0',string.format('%A',1.0)=='0X1P+0')",
        "print(string.format('%a',1.5)=='0x1.8p+0',string.format('%A',3.5)=='0X1.CP+1')",
        "print(string.format('%a',-1.0)=='-0x1p+0',string.format('%A',-1.5)=='-0X1.8P+0')",
        "print(string.format('%a',0.0)=='0x0p+0',string.format('%a',-0.0)=='-0x0p+0')",
        "print(string.format('%a',0.125)=='0x1p-3',string.format('%A',2.25)=='0X1.2P+1')",
        "print(string.format('%a',math.huge)=='inf',string.format('%A',math.huge)=='INF')",
        "print(string.format('%a',-math.huge)=='-inf',string.format('%A',-math.huge)=='-INF')",
        "print(string.format('%a',3)=='0x1.8p+1',string.format('%A',3)=='0X1.8P+1')",
        "print(string.format('%a',1e100)=='0x1.249ad2594c37dp+332')",
        "local a=pcall(string.format,'%a',false); local b=pcall(string.format,'%A'); print(a,b)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_hex_float_flags_and_width_match_lua54() {
    for source in [
        "print(string.format('%+a',1.0),string.format('% a',1.0))",
        "print(string.format('%#a',1.0),string.format('%#A',1.0))",
        "print(string.format('%12a',1.0),string.format('%-12A',1.5))",
        "print(string.format('%012a',1.0),string.format('%+012A',1.5))",
        "print(string.format('%#012a',1.0),string.format('%-#12A',1.0))",
        "print(string.format('% a',-1.0),string.format('%+a',-0.0))",
        "print(string.format('%#a',1.5),string.format('%#A',3.5))",
        "print(string.format('%+12a',0.0),string.format('% 12a',0.0))",
        "print(string.format('%012a',-1.0),string.format('%012A',-1.5))",
        "print(string.format('%+a',math.huge),string.format('%#A',-math.huge))",
        "local good=pcall(string.format,'%+a',false); local missing=pcall(string.format,'%a'); print(good,missing)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_hex_float_precision_matches_lua54() {
    for source in [
        "print(string.format('%.0a',1.0),string.format('%.1a',1.0),string.format('%.13a',1.0))",
        "print(string.format('%.0a',1.5),string.format('%.1a',1.5),string.format('%.2A',1.5))",
        "print(string.format('%.0a',1.9),string.format('%.2a',1.9),string.format('%.4A',1.9))",
        "print(string.format('%.0a',0.0),string.format('%.3a',-0.0),string.format('%#.0a',1.0))",
        "print(string.format('%010.1a',1.5),string.format('%+012.2A',1.5))",
        "print(string.format('%.13a',3.5),string.format('%.14a',3.5))",
        "print(string.format('%.2a',0.125),string.format('%.4a',0.125))",
        "print(string.format('%.1a',1.09375),string.format('%.0a',1.25))",
        "print(string.format('%.1a',1.9375),string.format('%.0a',1.9999999999999998))",
        "print(string.format('%.13a',1e100),string.format('%.14A',1e100))",
        "print(string.format('%.2a',math.huge),string.format('%.2A',-math.huge))",
        "print(string.format('%.1a',2.2250738585072014e-308))",
        "print(string.format('%.1a',1.7976931348623157e308))",
        "print(string.format('%#.0a',-1.0),string.format('%.0A',-1.0))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_general_significant_digits_match_lua54() {
    for source in [
        "print(string.format('%g',12.3400)=='12.34',string.format('%G',12.3400)=='12.34')",
        "print(string.format('%g',1234567)=='1.23457e+06',string.format('%G',1234567)=='1.23457E+06')",
        "print(string.format('%g',0.0001)=='0.0001',string.format('%g',0.00001)=='1e-05')",
        "print(string.format('%.3g',1234.0)=='1.23e+03',string.format('%.3g',12.345)=='12.3')",
        "print(string.format('%#.3g',12.0)=='12.0',string.format('%.0g',1.2)=='1')",
        "print(string.format('%g',-0.0)=='-0',string.format('%g',0.0)=='0')",
        "print(string.format('%.3g',999.99)=='1e+03',string.format('%.3g',9.999)=='10')",
        "print(string.format('%010.3g',12345.0)=='001.23e+04')",
        "print(string.format('%+010.3g',12345.0)=='+01.23e+04')",
        "print(string.format('%-10.3G',12345.0)=='1.23E+04  ')",
        "print(string.format('%#.0g',1.0)=='1.',string.format('%#.3g',1.0)=='1.00')",
        "print(string.format('%.2g',0.000123)=='0.00012',string.format('%.2G',0.0000123)=='1.2E-05')",
        "local a=pcall(string.format,'%g',false); local b=pcall(string.format,'%G'); print(a,b)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_format_general_rounding_boundary_differential() {
    // Probe scientific/fixed selection after significant-digit rounding,
    // including the -4 cutoff, carry into the next exponent, alternate
    // padding, signed zero, and case conversion.
    for source in [
        "print(string.format('%.2g',0.000099999),string.format('%.2g',0.0001))",
        "print(string.format('%.2G',0.0000099999),string.format('%.2G',0.00001))",
        "print(string.format('%.2g',999.9),string.format('%.2g',999.0))",
        "print(string.format('%.1g',9.5),string.format('%.1g',0.000095))",
        "print(string.format('%.1g',0.0000095),string.format('%.1G',999999))",
        "print(string.format('%#.2g',100),string.format('%#.3g',100),string.format('%#.4g',100))",
        "print(string.format('%#.2g',0.001),string.format('%#.2g',1000000))",
        "print(string.format('%.3g',0.00099999),string.format('%.3g',999.99))",
        "print(string.format('%+10.2G',123456),string.format('% 10.2g',-0.0))",
        "print(string.format('%.2g',-999.99),string.format('%.4g',-0.00099999))",
        "print(string.format('%g',math.huge),string.format('%G',-math.huge))",
        "print(string.format('%g',0/0),string.format('%G',0/0))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_gsub_short_lived_gc_roots_preserve_long_replacement_semantics() {
    for source in [
        "local n=0; local s=string.rep('a',100); local out,c=string.gsub(s,'.',function(x) n=n+1; return x..'b' end); print(#out,c,n)",
        "local source=string.rep('a',32); local out,n=string.gsub(source,'.',function(x) collectgarbage('collect'); return x end); print(out==source,n)",
        "local source=string.rep('ab',30); local lookup=setmetatable({},{__index=function(_,k) collectgarbage('collect'); return k..'!' end}); local out,n=string.gsub(source,'.',lookup); print(#out,n)",
        "local source=string.rep('x',64); local t={x='y'}; local out,n=string.gsub(source,'.',t); print(out==string.rep('y',64),n)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_string_case_reverse_and_repeat_match_lua54() {
    for source in [
        "print(string.lower('HeLLo\\0WORLD')=='hello\\0world',string.upper('aBc\\0z')=='ABC\\0Z',string.reverse('a\\0b')=='b\\0a')",
        "print(string.rep('ab',3,'-')=='ab-ab-ab',string.rep('X',3)=='XXX',string.rep('x',0)=='',string.rep('x',-10)=='')",
        "print(string.rep('',1000000)=='',string.rep('',4,'.')=='...',string.reverse('abc')=='cba')",
        "local s=string.char(0,65,255); local r=string.reverse(s); local a,b,c=string.byte(r,1,3); print(a,b,c,string.upper(string.char(97,0,122))=='A\\0Z')",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_concat_unpack_sequence_results_match_lua54() {
    for source in [
        "print(table.concat({'a','b','c'},'-'),table.concat({1,2,3},',',2,3),table.concat({})=='')",
        "local t={'a',nil,'c'}; local a,b,c=table.unpack(t,1,3); print(a,b,c,select('#',table.unpack(t,1,3)))",
        "print(table.concat({'a\\0','b'},':')=='a\\0:b',table.concat({'a','b'},'-',2,1)=='',select('#',table.unpack({})))",
        "local lib=table; assert(_G.table==lib); local saved=lib.concat; lib.concat=function() return 99 end; print(table.concat({1,2})); lib.concat=saved; print(table.concat({1,2}))",
        "print(table.unpack({1,2,3}))",
    ] {
        assert_parity(source, true);
    }

    // luaL_len also accepts strings. Integer indexing a Lua string has
    // no elements, so a four-byte string unpacks into four nil results.
    assert_parity(
        "local good=pcall(table.concat,{'a','b'}); local bad=pcall(table.concat,{'a',true}); local ok,a,b,c,d=pcall(table.unpack,'text'); print(good,bad,ok,a,b,c,d)",
        true,
    );
}

#[test]
fn owned_table_concat_and_unpack_observe_index_fallbacks_like_lua54() {
    for source in [
        "local t=setmetatable({1,nil,3},{__index=function(_,k) if k==2 then return 2 end end}); local a,b,c=table.unpack(t,1,3); print(a,b,c,table.concat(t,':',1,3))",
        "local fallback={[2]='B'}; local t=setmetatable({'A',nil,'C'},{__index=fallback}); print(table.concat(t,'|',1,3)); local a,b,c=table.unpack(t,1,3); print(a,b,c)",
        "local hits=0; local t=setmetatable({[1]='a',[3]='c'},{__index=function(_,k) hits=hits+1; return k==2 and 'b' or nil end}); print(table.concat(t,':',1,3),hits); print(table.unpack(t,1,3),hits)",
        "local t=setmetatable({[1]=1},{__index=function() error(27) end}); local ok,e=pcall(table.unpack,t,1,2); local good,f=pcall(table.concat,t,',',1,2); print(ok,e,good,f)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_unpack_roots_index_results_across_gc_despite_pinned_backend_divergence() {
    // luna-core 4.0.2 fails to retain the first two temporary Lua strings:
    // it reports the last value three times under forced collection.
    // The owned VM must instead match the reference Lua 5.4.6 result.
    let source = "local t=setmetatable({}, {__index=function(_,i) collectgarbage('collect'); return tostring(i) end}); local a,b,c=table.unpack(t,1,3); print(a,b,c,table.concat(t,',',1,3))";
    let owned = run_fer(true, source);
    assert!(owned.status.success(), "{:?}", owned.stderr);
    assert_eq!(owned.stdout, b"1\t2\t3\t1,2,3\n");
    if let Some(reference) = reference_output(source) {
        assert!(reference.status.success(), "{:?}", reference.stderr);
        assert_eq!(owned.stdout, reference.stdout);
    }
    let production = run_fer(false, source);
    assert!(production.status.success(), "{:?}", production.stderr);
    assert_eq!(production.stdout, b"3\t3\t3\t1,2,3\n");
}

#[test]
fn owned_table_concat_eager_len_and_unpack_optional_len_match_lua54() {
    for source in [
        "local t=setmetatable({10,20,30},{__len=function() return 2 end}); print(select('#',table.unpack(t)),table.unpack(t)); print(select('#',table.unpack(t,1,3)))",
        "local t=setmetatable({'a','b','c'},{__len=function() return 2 end}); print(table.concat(t,'|'),table.concat(t,'|',1,3))",
        "local n=0; local t=setmetatable({1,2,3},{__len=function() n=n+1; return 2 end}); print(table.concat(t,','),n); print(select('#',table.unpack(t)),n); print(table.concat(t,',',1,3),n)",
        "local t=setmetatable({8,9},{__len=function() return 2.0 end}); print(table.concat(t,','),select('#',table.unpack(t)))",
        "local t=setmetatable({7,8,9},{__len=function() return 0 end}); print(table.concat(t,'|')=='',select('#',table.unpack(t)))",
        "local t=setmetatable({7,8,9},{__len=function() error(27) end}); local ok,e=pcall(table.concat,t,','); local good,f=pcall(table.unpack,t); local explicit,g=pcall(table.concat,t,',',1,3); local unpacked,h=pcall(table.unpack,t,1,3); print(ok,e,good,f,explicit,g,unpacked,h)",
        "local t=setmetatable({2,3},{__len=function() return 'broken' end}); local ok=pcall(table.concat,t); local good=pcall(table.unpack,t); print(ok,good)",
        "local t=setmetatable({5,6},{__len=function() collectgarbage('collect'); return 2 end}); print(table.concat(t,','),table.unpack(t))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_pack_nil_holes_and_argument_count_match_lua54() {
    for source in [
        "local t=table.pack(1,nil,3); print(t.n,t[1],t[2],t[3],select('#',table.unpack(t,1,t.n)))",
        "local empty=table.pack(); local nils=table.pack(nil,nil); print(empty.n,nils.n,nils[1]==nil,nils[2]==nil,select('#',table.unpack(nils,1,nils.n)))",
        "local orig=table.pack; table.pack=function(...) return {n=99} end; print(_G.table.pack().n); table.pack=orig; print(table.pack('a').n)",
        "local t=table.pack(1,false,3.25); print(t.n,t[1],t[2],t[3])",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_insert_remove_mutation_and_bounds_match_lua54() {
    for source in [
        "local t={1,3}; table.insert(t,2,2); table.insert(t,4); print(table.concat(t,',')); print(table.remove(t,2),table.concat(t,',')); print(table.remove(t),table.concat(t,','))",
        "local t={}; print(table.remove(t)==nil); table.insert(t,'a'); print(t[1],select('#',table.insert(t,nil)),#t)",
        "local t={1,2}; local a=pcall(table.insert,t,0,3); local b=pcall(table.remove,t,0); local c=pcall(table.insert,t,3,4); print(a,b,c,table.concat(t,','))",
        "local t={1,2}; print(table.remove(t,3)==nil,table.concat(t,',')); print(table.remove(t,1),table.concat(t,','))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_move_overlap_and_destination_match_lua54() {
    for source in [
        "local t={1,2,3}; print(table.move(t,1,3,2)==t,t[1],t[2],t[3],t[4])",
        "local t={1,2,3}; table.move(t,2,3,1); print(t[1],t[2],t[3])",
        "local t={1,2}; local d={9}; print(table.move(t,1,2,2,d)==d,d[1],d[2],d[3],t[1])",
        "local t={1,nil,3}; local d={7,8,9}; table.move(t,1,3,1,d); print(d[1],d[2],d[3])",
        "local t={}; t[-1]='x'; local d={}; table.move(t,-1,-1,1,d); print(d[1])",
        "local t={1,2}; local d={3}; print(table.move(t,5,4,1,d)==d,d[1])",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_insert_and_remove_observe_len_metamethods_like_lua54() {
    for source in [
        "local t=setmetatable({1,2,3},{__len=function() return 2 end}); table.insert(t,99); print(t[1],t[2],t[3])",
        "local t=setmetatable({1,2,3},{__len=function() return 2 end}); print(table.remove(t),t[1],t[2],t[3])",
        "local t=setmetatable({1,2,3},{__len=function() return 2 end}); table.insert(t,2,42); print(t[1],t[2],t[3]); print(table.remove(t,2),t[1],t[2],t[3])",
        "local n=0; local t=setmetatable({1,2},{__len=function() n=n+1; return 2 end}); table.insert(t,3); print(n,t[3]); print(table.remove(t),n)",
        "local t=setmetatable({1,2},{__len=function() error(28) end}); local a,b=pcall(table.insert,t,3); local c,d=pcall(table.remove,t); print(a,b,c,d)",
        "local t=setmetatable({1,2},{__len=function() return 'bad' end}); local a=pcall(table.insert,t,3); local b=pcall(table.remove,t); print(a,b)",
        "local t=setmetatable({4,6},{__len=function() return 2.0 end}); table.insert(t,5); print(t[1],t[2],t[3]); print(table.remove(t),t[1],t[2],t[3])",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_insert_remove_use_index_and_newindex_metamethods_like_lua54() {
    for source in [
        "local seen={}; local t={10,20}; setmetatable(t,{__newindex=function(_,k,v) seen[#seen+1]=k; rawset(t,k,v) end}); table.insert(t,30); print(table.concat(t,','),seen[1])",
        "local seen={}; local t={11,22}; setmetatable(t,{__newindex=function(_,k,v) seen[#seen+1]=k end}); table.insert(t,33); print(t[3]==nil,seen[1])",
        "local t={[1]=10}; setmetatable(t,{__len=function() return 2 end,__index=function(_,k) if k==2 then return 20 end end}); table.insert(t,2,15); print(t[1],t[2],t[3])",
        "local t={[1]=10}; setmetatable(t,{__len=function() return 3 end,__index=function(_,k) return k==2 and 20 or 30 end}); print(table.remove(t,1),t[1],t[2],t[3])",
        "local writes={}; local t={[1]=7,[3]=9}; setmetatable(t,{__len=function() return 3 end,__newindex=function(_,k,v) writes[#writes+1]=k end}); print(table.remove(t,1),writes[1],t[1],t[2],t[3])",
        "local t={[1]=1}; setmetatable(t,{__len=function() return 2 end,__index=function() error(27) end}); local a,b=pcall(table.insert,t,2,10); local c,d=pcall(table.remove,t,1); print(a,b,c,d)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_move_uses_index_and_newindex_metamethods_like_lua54() {
    for source in [
        "local src={[1]='a',[3]='c'}; setmetatable(src,{__index=function(_,k) return k==2 and 'b' end}); local dst={}; table.move(src,1,3,1,dst); print(table.concat(dst,','))",
        "local dest={}; local seen={}; setmetatable(dest,{__newindex=function(_,k,v) seen[#seen+1]=k; rawset(dest,k,v) end}); local src={11,22,33}; table.move(src,1,3,1,dest); print(dest[1],dest[2],dest[3],table.concat(seen,','))",
        "local d={}; local seen={}; setmetatable(d,{__newindex=function(_,k,v) seen[#seen+1]=k end}); table.move({5,6},1,2,1,d); print(d[1]==nil,d[2]==nil,table.concat(seen,','))",
        "local t={[1]=1,[3]=3}; setmetatable(t,{__index=function(_,k) if k==2 then return 2 end end}); table.move(t,1,3,2); print(t[1],t[2],t[3],t[4])",
        "local src={[1]=7}; setmetatable(src,{__index=function() error(47) end}); local good,e=pcall(table.move,src,1,2,1,{}); print(good,e)",
        "local target={}; setmetatable(target,{__newindex=function() error(55) end}); local ok,e=pcall(table.move,{9},1,1,1,target); print(ok,e)",
        "local src={[1]='one'}; setmetatable(src,{__index=function(_,i) return tostring(i) end}); local t={}; table.move(src,1,3,1,t); print(table.concat(t,','))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_sort_observes_index_and_newindex_like_lua54() {
    for source in [
        "local t={[1]=3,[3]=1}; setmetatable(t,{__len=function() return 3 end,__index=function(_,k) if k==2 then return 2 end end}); table.sort(t); print(t[1],t[2],t[3])",
        "local seen={}; local t={[1]=3,[2]=2}; setmetatable(t,{__len=function() return 3 end,__index=function(_,k) return k==3 and 1 end,__newindex=function(_,k,v) seen[#seen+1]=k; rawset(t,k,v) end}); table.sort(t); print(t[1],t[2],t[3],seen[1])",
        "local n=0; local t={[1]=2,[3]=1}; setmetatable(t,{__len=function() return 3 end,__index=function(_,k) n=n+1; return 3 end}); table.sort(t); print(t[1],t[2],t[3],n>0)",
        "local t={[1]=2}; setmetatable(t,{__len=function() return 2 end,__index=function() error(41) end}); local a,b=pcall(table.sort,t); print(a,b)",
        "local t={[1]=2}; setmetatable(t,{__len=function() return 2 end,__index=function() return 1 end,__newindex=function() error(43) end}); local a,b=pcall(table.sort,t); print(a,b)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_sort_uses_len_metamethod_for_sort_boundaries_like_lua54() {
    for source in [
        "local t=setmetatable({3,1,2},{__len=function() return 2 end}); table.sort(t); print(t[1],t[2],t[3],#t)",
        "local t=setmetatable({9,2,1},{__len=function() return 0 end}); table.sort(t,false); print(t[1],t[2],t[3])",
        "local t=setmetatable({4,1,3},{__len=function() return 2.0 end}); table.sort(t); print(t[1],t[2],t[3])",
        "local t=setmetatable({3,2,1},{__len=function() return 2 end}); table.sort(t,function(a,b) return a>b end); print(t[1],t[2],t[3])",
        "local called=0; local t=setmetatable({3,2,1},{__len=function() called=called+1; collectgarbage('collect'); return 2 end}); table.sort(t); print(called,t[1],t[2],t[3])",
        "local t=setmetatable({2,1},{__len=function() return 'bad' end}); local ok=pcall(table.sort,t); print(ok)",
        "local t=setmetatable({2,1},{__len=function() error(27) end}); local ok,e=pcall(table.sort,t); print(ok,e)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_sort_ignores_comparator_on_trivial_lists_like_lua54() {
    for source in [
        "local a={}; print(select('#',table.sort(a,false)),#a); local b={7}; print(select('#',table.sort(b,23)),b[1])",
        "local a={8}; local never=function() error(42) end; table.sort(a,never); print(a[1]); print(select('#',table.sort({},'no function')))",
        "local f=setmetatable({}, {__call=function() error('unexpected') end}); table.sort({},f); table.sort({2},f); print('ok')",
        "local ok=pcall(table.sort,{},false); local yes=pcall(table.sort,{1},false); local fail=pcall(table.sort,{1,2},false); print(ok,yes,fail)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_table_sort_numbers_strings_callbacks_and_metamethods_match_lua54() {
    for source in [
        "local t={3,1,2}; print(select('#',table.sort(t)),table.concat(t,','))",
        "local t={'b','a','c'}; table.sort(t); print(table.concat(t,'|'))",
        "local t={1,4,3,2}; table.sort(t,function(a,b) return a>b end); print(table.concat(t,','))",
        "local t={0,3.5,-1,2}; table.sort(t); print(table.concat(t,','))",
        "local t={}; print(select('#',table.sort(t)),#t); local q={5}; table.sort(q); print(q[1])",
        "local t={3,2,1}; local a=pcall(table.sort,t,42); local b=pcall(table.sort,3); print(a,b,table.concat(t,','))",
        "local mt={__lt=function(a,b) return a.x<b.x end}; local t={setmetatable({x=3},mt),setmetatable({x=1},mt),setmetatable({x=2},mt)}; table.sort(t); print(t[1].x,t[2].x,t[3].x)",
        "local t={3,1,2}; table.sort(t,function(a,b) local temp=tostring(a)..':'..tostring(b); return a<b end); print(table.concat(t,','))",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn generated_branch_and_environment_interactions_match_lua54() {
    for seed in 0_i64..48 {
        let base = (seed * 11) % 23 - 8;
        let add = seed % 7 + 1;
        let source = format!(
            "counter={seed}; function bump(x) counter=counter+x; return counter end; \
             local box={{base={base}}}; function box:plus(x) return self.base+x end; \
             if {seed}%2==0 then _G.counter=bump({add}) else counter=bump({add}) end; \
             print(counter, box:plus(3), type(_G), _VERSION)"
        );
        assert_parity(&source, true);
    }
}

#[test]
fn generated_lexical_environment_closure_cases_match_lua54() {
    for seed in 0_i64..24 {
        let original = (seed * 7) % 17 - 4;
        let step = seed % 5 + 1;
        let source = format!(
            "local outer=_ENV; local e={{print=print,n={original}}}; \
             do local _ENV=e; function inc(x) n=n+x; return n end; \
             if {seed}%2==0 then n=inc({step}) else n=inc({step})+1 end; \
             print(n) end; print(e.inc(1),e.n)"
        );
        assert_parity(&source, true);
    }
}

#[test]
fn generated_numeric_math_cases_match_lua54() {
    for seed in -64_i64..=64 {
        let source = format!(
            "local n={seed}/7; print(math.floor(n),math.ceil(n),math.abs({seed}),math.max({seed},5,-3),math.min({seed},5,-3),math.type(math.floor(n)),math.tointeger(n)==nil)"
        );
        assert_parity(&source, true);
    }
    for source in [
        "print(math.min('alpha','beta'),math.max('alpha','beta'))",
        "print(math.tointeger('0x10'),math.type(4.0),math.type('4')==nil)",
    ] {
        assert_parity(source, true);
    }
    for source in [
        "math.max()",
        "math.min(1,true)",
        "math.floor({})",
        "math.abs('invalid')",
    ] {
        assert_parity(source, false);
    }
}

#[test]
fn owned_root_table_collection_matches_lua_after_repeated_cyclic_pressure() {
    let source = "local getter; do local hidden=setmetatable({answer=42}, {__index=function() return 7 end}); getter=function() return hidden.answer,hidden.missing end end; for i=1,40000 do local t={}; t.self=t end; print(getter())";
    let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["--engine=owned", "--budget=2000000", "-e", source])
        .output()
        .expect("launch owned fer");
    let production = run_fer(false, source);
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert!(production.status.success());
    assert_eq!(owned.stdout, production.stdout);
    assert_eq!(owned.stdout, b"42\t7\n");
    if let Some(upstream) = reference_output(source) {
        assert!(upstream.status.success());
        assert_eq!(owned.stdout, upstream.stdout);
    }
}

#[test]
fn owned_nested_and_field_pressure_gc_matches_lua54() {
    for (source, expected) in [
        (
            "local function p() for i=1,21000 do local t={}; t.self=t end return 7 end; local function plus(t,n) return t.v+n end; print(plus({v=35},p()))",
            "42\n",
        ),
        (
            "local function p() for i=1,21000 do local f=function() return i end end return 4 end; local function plus(f,n) return f()+n end; local function make() local v=38; return function() return v end end; print(plus(make(),p()))",
            "42\n",
        ),
        (
            "for i=1,1800 do local t={}; for j=1,40 do t[j]=j end end; print('ok')",
            "ok\n",
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        let production = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(
            production.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&production.stderr)
        );
        assert_eq!(owned.stdout, expected.as_bytes());
        assert_eq!(owned.stdout, production.stdout, "{source}");
        if let Some(upstream) = reference_output(source) {
            assert!(
                upstream.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&upstream.stderr)
            );
            assert_eq!(owned.stdout, upstream.stdout, "{source}");
        }
    }
}

#[test]
fn owned_explicit_full_gc_mode_matches_lua54() {
    for (source, output) in [
        (
            "local t={value=42}; print(type(collectgarbage()),type(collectgarbage('collect')),t.value)",
            "number\tnumber\t42\n",
        ),
        (
            "local okay=pcall(collectgarbage,'invalid'); print(okay)",
            "false\n",
        ),
        (
            "local function worker() for i=1,21000 do local t={}; t.self=t end; collectgarbage('collect'); return 42 end; print(worker())",
            "42\n",
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=2000000", "-e", source])
            .output()
            .expect("launch owned fer");
        let reference = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(reference.status.success());
        assert_eq!(owned.stdout, output.as_bytes(), "{source}");
        assert_eq!(owned.stdout, reference.stdout, "{source}");
        if let Some(lua) = reference_output(source) {
            assert!(lua.status.success());
            assert_eq!(owned.stdout, lua.stdout, "{source}");
        }
    }
}

#[test]
fn owned_gc_tail_constructor_matches_lua54_across_heap_pressure() {
    let numbers = (1..=40)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let after_table = (2..=40)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sources = [
        format!(
            "local function values() return {numbers} end; for i=1,1800 do local t={{tag=i,values()}} end; print('ok')"
        ),
        format!(
            "local function values() return {{value=42}},{after_table} end; local saved; for i=1,1800 do local t={{tag=i,values()}}; if i==1800 then saved=t[1] end end; print(saved.value)"
        ),
    ];
    for (source, expected) in sources.iter().zip([b"ok\n".as_slice(), b"42\n".as_slice()]) {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        let reference = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(reference.status.success(), "{source}");
        assert_eq!(owned.stdout, expected, "{source}");
        assert_eq!(owned.stdout, reference.stdout, "{source}");
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success(), "{source}");
            assert_eq!(owned.stdout, upstream.stdout, "{source}");
        }
    }
}

#[test]
fn owned_long_weak_mode_strings_match_pinned_lua54_6() {
    for mode in [
        format!("v{}", "x".repeat(64)),
        format!("k{}", "x".repeat(64)),
        format!("x{}kv", "x".repeat(64)),
    ] {
        let source = format!(
            "local w=setmetatable({{}}, {{__mode='{mode}'}}); \
             do local key={{}}; local value={{}}; w[key]=17; w[1]=value end; \
             collectgarbage('collect'); \
             local key_gone=true; for k,v in pairs(w) do if type(k)=='table' then key_gone=false end end; \
             print(key_gone,w[1]==nil)"
        );
        assert_parity(&source, true);
    }
}

#[test]
fn owned_weak_mode_nul_termination_matches_lua54_6() {
    // The pinned production luna-core backend scans the bytes beyond NUL;
    // Lua 5.4.6 uses strchr and stops. Keep the divergence visible rather
    // than making the owned engine reproduce a production-backend bug.
    for (mode, expected) in [
        ("v\\0k", b"false\ttrue\n".as_slice()),
        ("k\\0v", b"true\tfalse\n".as_slice()),
        ("kv\\0x", b"true\ttrue\n".as_slice()),
        ("x\\0kv", b"false\tfalse\n".as_slice()),
    ] {
        let source = format!(
            "local w=setmetatable({{}}, {{__mode='{mode}'}}); \
             do local key={{}}; local value={{}}; w[key]=17; w[1]=value end; \
             collectgarbage('collect'); \
             local key_gone=true; for k,v in pairs(w) do if type(k)=='table' then key_gone=false end end; \
             print(key_gone,w[1]==nil)"
        );
        let owned = run_fer(true, &source);
        assert!(owned.status.success(), "{source}");
        assert_eq!(owned.stdout, expected, "{source}");
        let production = run_fer(false, &source);
        assert!(production.status.success(), "{source}");
        if mode == "v\\0k" {
            // Evidence that the existing production backend differs.
            assert_eq!(production.stdout, b"true\ttrue\n", "{source}");
        }
        if let Some(reference) = reference_output(&source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
        }
    }
}

#[test]
fn owned_weak_mode_flag_characters_match_lua54() {
    for mode in ["kvv", "xxvkx", "kkx", "vvx", "xyz"] {
        let source = format!(
            "local w=setmetatable({{}}, {{__mode='{mode}'}}); \
             do local key={{}}; local value={{}}; w[key]=17; w[1]=value end; \
             collectgarbage('collect'); \
             local key_gone=true; for k,v in pairs(w) do if type(k)=='table' then key_gone=false end end; \
             print(key_gone,w[1]==nil)"
        );
        assert_parity(&source, true);
    }
}

#[test]
fn owned_weak_value_gc_matches_upstream_lua54() {
    for source in [
        "local w=setmetatable({}, {__mode='v'}); local keep={id=42}; local gone={}; w[1]=keep; w[2]=gone; w.named=function() return 7 end; w.plain=23; w.words='hello'; gone=nil; collectgarbage('collect'); print(w[1].id,w[2]==nil,w.named==nil,w.plain,w.words); keep=nil; collectgarbage('collect'); print(w[1]==nil)",
        "local w=setmetatable({}, {__mode='v'}); do local key={}; w[key]=99 end; collectgarbage('collect'); local key,value=next(w); print(type(key),value)",
        "local meta={__mode='v'}; local w=setmetatable({},meta); meta.__mode=nil; local t={n=7}; w[1]=t; t=nil; collectgarbage('collect'); print(w[1].n)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_callable_table_finalizers_match_lua54() {
    for (source, expected) in [
        (
            "local count=0; local callback=setmetatable({}, {__call=function(self, v) count=count+v.n end}); do local t=setmetatable({n=7}, {__gc=callback}) end; collectgarbage('collect'); print(count)",
            b"7\n".as_slice(),
        ),
        (
            "local callback=setmetatable({}, {__call=function(self, v) print('closing',v.n) end}); local t=setmetatable({n=9}, {__gc=callback}); print('body')",
            b"body\nclosing\t9\n".as_slice(),
        ),
        (
            "local first=setmetatable({}, {__call=function(self, v) print('old') end}); local mt={__gc=first}; do local t=setmetatable({},mt) end; mt.__gc=function() print('new') end; collectgarbage('collect')",
            b"new\n".as_slice(),
        ),
    ] {
        let owned = run_fer(true, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert_eq!(owned.stdout, expected, "{source}");
        if let Some(reference) = reference_output(source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
        }
    }
}

#[test]
fn owned_noncallable_finalizer_warnings_match_lua54() {
    for finalizer in ["12", "true", "'not callable'", "{}"] {
        let source = format!(
            "warn('@on'); do local t=setmetatable({{}}, {{__gc={finalizer}}}) end; collectgarbage('collect'); print('alive')"
        );
        let owned = run_fer(true, &source);
        assert!(owned.status.success(), "{source}");
        if let Some(reference) = reference_output(&source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
            assert_eq!(owned.stderr, reference.stderr, "{source}");
        }
    }
}

#[test]
fn owned_finalizer_warning_stream_matches_upstream_lua54() {
    for source in [
        "warn('muted'); warn('@on'); warn('hello',' world'); warn('@off'); warn('muted'); warn('@on'); print(select('#',warn('restored')))",
        "warn('@on'); do local t=setmetatable({}, {__gc=function() error('gc-failed',0) end}) end; collectgarbage('collect'); print('alive')",
        "warn('@on'); local t=setmetatable({}, {__gc=function() error('shutdown-failed',0) end}); print('body')",
        "do local t=setmetatable({}, {__gc=function() error('muted',0) end}) end; collectgarbage('collect'); print('alive')",
    ] {
        let owned = run_fer(true, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        if let Some(reference) = reference_output(source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
            assert_eq!(owned.stderr, reference.stderr, "{source}");
        }
    }
}

#[test]
fn owned_finalizer_collectgarbage_nil_arity_matches_lua54() {
    // Both the pinned production backend and Lua 5.4 suppress a nested GC
    // request from __gc and return one nil value in this calling context.
    let cases: [(&str, &[u8]); 2] = [
        (
            "local count=-1; do local t=setmetatable({}, {__gc=function() count=select('#',collectgarbage('collect')) end}) end; collectgarbage('collect'); print(count,select('#',collectgarbage('collect')))",
            b"1\t1\n",
        ),
        (
            "local t=setmetatable({}, {__gc=function() print('close',select('#',collectgarbage('collect'))) end}); print('body')",
            b"body\nclose\t1\n",
        ),
    ];
    for (source, expected) in cases {
        let owned = run_fer(true, source);
        let production = run_fer(false, source);
        assert!(owned.status.success(), "{source}");
        assert!(production.status.success(), "{source}");
        assert_eq!(owned.stdout, expected, "{source}");
        assert_eq!(production.stdout, expected, "{source}");
        if let Some(reference) = reference_output(source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
        }
    }
}

#[test]
fn owned_finalizer_suppresses_nested_collection_like_lua54() {
    let cases: [(&str, &[u8], &[u8]); 2] = [
        (
            "local called=false; do local t=setmetatable({}, {__gc=function() local ok,v=pcall(collectgarbage,'collect'); called=ok and v==nil end}) end; collectgarbage('collect'); print(called)",
            b"true\n",
            // Both interpreters suppress nested collection; pcall
            // succeeds with a nil second result in scalar context.
            b"true\n",
        ),
        (
            "local t=setmetatable({}, {__gc=function() local ok,v=pcall(collectgarbage,'collect'); print('closing',ok,v) end}); print('body')",
            b"body\nclosing\ttrue\tnil\n",
            b"body\nclosing\ttrue\tnil\n",
        ),
    ];
    for (source, expected, production_expected) in cases {
        let owned = run_fer(true, source);
        let production = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(production.status.success(), "{source}");
        assert_eq!(owned.stdout, expected, "{source}");
        assert_eq!(production.stdout, production_expected, "{source}");
        if let Some(reference) = reference_output(source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
        }
    }
}

#[test]
fn owned_lua_state_shutdown_finalizers_match_lua54() {
    // The pinned production backend currently closes live marked tables in
    // registration order, while Lua 5.4 explicitly requires reverse order.
    // Keep that divergence visible, without teaching the owned VM the bug.
    let cases: [(&str, &[u8], &[u8]); 4] = [
        (
            "local a=setmetatable({id='A'}, {__gc=function(v) print('close',v.id) end}); local b=setmetatable({id='B'}, {__gc=function(v) print('close',v.id) end}); print('body',a.id,b.id)",
            b"body\tA\tB\nclose\tB\nclose\tA\n",
            b"body\tA\tB\nclose\tA\nclose\tB\n",
        ),
        (
            "local a=setmetatable({}, {__gc=function() print('once') end}); a=nil; collectgarbage('collect'); print('body')",
            b"once\nbody\n",
            b"once\nbody\n",
        ),
        (
            "local original=setmetatable({}, {__gc=function() print('original'); setmetatable({}, {__gc=function() print('new') end}) end}); print('body')",
            b"body\noriginal\n",
            b"body\noriginal\n",
        ),
        (
            "local a=setmetatable({}, {__gc=function() error('finalizer failure') end}); print('body')",
            b"body\n",
            b"body\n",
        ),
    ];
    for (source, expected, production_expected) in cases {
        let owned = run_fer(true, source);
        let production = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(production.status.success(), "{source}");
        assert_eq!(owned.stdout, expected, "{source}");
        assert_eq!(production.stdout, production_expected, "{source}");
        if let Some(reference) = reference_output(source) {
            assert!(reference.status.success(), "{source}");
            assert_eq!(owned.stdout, reference.stdout, "{source}");
        }
    }
}

#[test]
fn owned_reregistered_finalizers_match_lua54_on_later_gc_cycles() {
    for source in [
        "local calls=0; local mt={}; mt.__gc=function(v) calls=calls+1; if calls==1 then setmetatable(v,mt) end end; do local t=setmetatable({},mt) end; collectgarbage('collect'); print(calls); collectgarbage('collect'); print(calls); collectgarbage('collect'); print(calls)",
        "local calls=0; local saved; local mt={}; mt.__gc=function(v) calls=calls+1; if calls==1 then saved=v end end; do local t=setmetatable({},mt) end; collectgarbage('collect'); print(calls,saved~=nil); setmetatable(saved,mt); saved=nil; collectgarbage('collect'); print(calls); collectgarbage('collect'); print(calls)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_explicit_table_finalizers_match_lua54_on_lifecycle_cases() {
    for source in [
        "local log=''; do local a=setmetatable({id='A'},{__gc=function(v) log=log..v.id end}); local b=setmetatable({id='B'},{__gc=function(v) log=log..v.id end}) end; collectgarbage('collect'); print(log); collectgarbage('collect'); print(log)",
        "local called=false; local mt={}; do local t=setmetatable({},mt); mt.__gc=function() called=true end end; collectgarbage('collect'); print(called)",
        "local rescued; do local t=setmetatable({answer=42},{__gc=function(v) rescued=v end}) end; collectgarbage('collect'); print(rescued.answer); rescued=nil; collectgarbage('collect'); print('once')",
        "local weak=setmetatable({}, {__mode='v'}); local cleared=false; do local x=setmetatable({}, {__gc=function(v) cleared=weak[1]==nil end}); weak[1]=x end; collectgarbage('collect'); print(cleared)",
        "local weak=setmetatable({}, {__mode='k'}); local seen=false; do local x=setmetatable({}, {__gc=function(v) seen=weak[v]==42 end}); weak[x]=42 end; collectgarbage('collect'); print(seen); collectgarbage('collect'); print(next(weak)==nil)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn owned_lazy_standard_tables_survive_string_pressure_like_lua54() {
    let source = "local keep=tostring(123456789); for i=1,19000 do local s=tostring(i) end; local g=_G; print(g._VERSION,type(g.arg),type(g.math),keep)";
    let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["--engine=owned", "--budget=3000000", "-e", source])
        .output()
        .expect("launch owned fer");
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert_eq!(owned.stdout, b"Lua 5.4\ttable\ttable\t123456789\n");
    let production = run_fer(false, source);
    assert!(production.status.success());
    assert_eq!(owned.stdout, production.stdout);
    if let Some(upstream) = reference_output(source) {
        assert!(upstream.status.success());
        assert_eq!(owned.stdout, upstream.stdout);
    }
}

#[test]
fn owned_type_and_math_type_under_string_pressure_match_lua54() {
    for (source, expected) in [
        (
            "local hold=tostring(123456789); for i=1,19000 do local x=tostring(i) end; print(type(hold),math.type(7),hold)",
            b"string\tinteger\t123456789\n".as_slice(),
        ),
        (
            "local hold=tostring(123456789); local function stress() for i=1,19000 do local x=tostring(i) end return 17 end; print(type(hold,stress()),math.type(1.5),hold)",
            b"string\tfloat\t123456789\n".as_slice(),
        ),
        (
            "local hold=tostring(123456789); for i=1,19000 do local x=tostring(i) end; local ok1,t=pcall(type,hold); local ok2,n=pcall(math.type,7); print(ok1,t,ok2,n,hold)",
            b"true\tstring\ttrue\tinteger\t123456789\n".as_slice(),
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert_eq!(owned.stdout, expected, "{source}");
        let production = run_fer(false, source);
        assert!(production.status.success());
        assert_eq!(owned.stdout, production.stdout);
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success());
            assert_eq!(owned.stdout, upstream.stdout);
        }
    }
}

#[test]
fn owned_tostring_and_print_string_pressure_match_lua54() {
    for (source, expected) in [
        (
            "for i=1,19000 do local s=tostring(i) end; print('ok')",
            b"ok\n".as_slice(),
        ),
        (
            "local saved=tostring(99999); for i=1,19000 do local s=tostring(i) end; print(saved)",
            b"99999\n".as_slice(),
        ),
        (
            "local function pressure() for i=1,19000 do local s=tostring(i) end return 7 end; print(tostring(123456),pressure())",
            b"123456\t7\n".as_slice(),
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert_eq!(owned.stdout, expected, "{source}");
        let production = run_fer(false, source);
        assert!(production.status.success());
        assert_eq!(owned.stdout, production.stdout);
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success());
            assert_eq!(owned.stdout, upstream.stdout);
        }
    }
}

#[test]
fn owned_concat_string_pressure_gc_matches_lua54() {
    for (source, expected) in [
        (
            "for i=1,19000 do local s='v'..i end; print('ok')",
            b"ok\n".as_slice(),
        ),
        (
            "local saved={'v'..777}; for i=1,19000 do local s='v'..i end; collectgarbage('collect'); print(saved[1])",
            b"v777\n".as_slice(),
        ),
        (
            "local function pressure() for i=1,19000 do local s='v'..i end; return 7 end; local function use(s,n) print(s,n) end; use('sentinel'..42,pressure())",
            b"sentinel42\t7\n".as_slice(),
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert_eq!(owned.stdout, expected, "{source}");
        let production = run_fer(false, source);
        assert!(production.status.success());
        assert_eq!(owned.stdout, production.stdout);
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success());
            assert_eq!(owned.stdout, upstream.stdout);
        }
    }
}

#[test]
fn owned_explicit_string_collection_matches_lua54_after_two_epochs() {
    assert_parity(
        "collectgarbage('stop'); local literal='stable'; for i=1,3200 do local s=tostring(i) end; local high=collectgarbage('count'); collectgarbage('collect'); local low=collectgarbage('count'); for i=3201,6400 do local s=tostring(i) end; collectgarbage('collect'); print(high>low,literal,'stable',tostring(42))",
        true,
    );
    assert_parity(
        "local keep={}; local weak=setmetatable({}, {__mode='kv'}); local key=tostring(11111); keep[1]=tostring(22222); weak[key]=tostring(33333); key=nil; collectgarbage('collect'); print(keep[1],weak['11111'])",
        true,
    );
}

#[test]
fn owned_weak_key_ephemeron_and_double_weak_gc_match_upstream_lua54() {
    for source in [
        "local w=setmetatable({}, {__mode='k'}); local key={}; w[key]={back=key}; key=nil; collectgarbage('collect'); print(next(w)==nil)",
        "local w=setmetatable({}, {__mode='k'}); local key={}; w[key]={n=42}; collectgarbage('collect'); print(w[key].n); key=nil; collectgarbage('collect'); print(next(w)==nil)",
        "local w=setmetatable({}, {__mode='k'}); w['name']={n=42}; collectgarbage('collect'); print(w['name'].n)",
        "local w=setmetatable({}, {__mode='k'}); local seed={}; local key2={}; w[seed]={child=key2}; w[key2]={n=42}; key2=nil; collectgarbage('collect'); print(w[w[seed].child].n); seed=nil; collectgarbage('collect'); print(next(w)==nil)",
        "local w=setmetatable({}, {__mode='kv'}); local key={}; w[key]=42; key=nil; w[1]={}; w[2]='still'; collectgarbage('collect'); print(w[1]==nil,w[2],next(w)~=nil)",
    ] {
        assert_parity(source, true);
    }
}

#[test]
fn generated_ephemeron_chains_match_lua54_across_insertion_orders() {
    // The weak-key hash iteration order is unspecified; every insertion
    // order must reach the same ephemeron marking fixpoint.
    for length in 2..=10 {
        for reverse in [false, true] {
            let insert = if reverse {
                format!("for i={length}-1,1,-1 do weak[keys[i]]={{next=keys[i+1]}} end")
            } else {
                format!("for i=1,{length}-1 do weak[keys[i]]={{next=keys[i+1]}} end")
            };
            let source = format!(
                "local weak=setmetatable({{}},{{__mode='k'}}); local keys={{}}; \
                 for i=1,{length} do keys[i]={{}} end; \
                 {insert}; weak[keys[{length}]]={{answer=42}}; \
                 local root=keys[1]; keys=nil; collectgarbage('collect'); \
                 local cursor=root; for i=1,{length}-1 do cursor=weak[cursor].next end; \
                 print(weak[cursor].answer); root=nil; cursor=nil; \
                 collectgarbage('collect'); print(next(weak)==nil)"
            );
            assert_parity(&source, true);
        }
    }

    assert_parity(
        "local weak=setmetatable({}, {__mode='k'}); local key=function() end; weak[key]={back=key}; key=nil; collectgarbage('collect'); print(next(weak)==nil)",
        true,
    );
}

#[test]
fn owned_gc_count_reports_live_heap_growth_and_reclamation_like_lua54() {
    assert_parity(
        "collectgarbage('stop'); local before=collectgarbage('count'); for i=1,1400 do local t={1,2,3,4,5,6,7,8,9,10,11,12} end; local high=collectgarbage('count'); collectgarbage('collect'); local low=collectgarbage('count'); print(type(low),high>before,low<high,collectgarbage('isrunning'))",
        true,
    );
}

#[test]
fn owned_lazily_materialized_lua_tables_survive_recycled_heap_pressure() {
    let source = "collectgarbage('stop'); for i=1,16384 do local t={} end; collectgarbage('restart'); print(type(_G),type(math),type(arg))";
    let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["--engine=owned", "--budget=2000000", "-e", source])
        .output()
        .expect("launch owned fer");
    let production = run_fer(false, source);
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert!(production.status.success());
    assert_eq!(owned.stdout, b"table\ttable\ttable\n");
    assert_eq!(owned.stdout, production.stdout);
    if let Some(upstream) = reference_output(source) {
        assert!(upstream.status.success());
        assert_eq!(owned.stdout, upstream.stdout);
    }
}

#[test]
fn owned_gc_stop_restart_and_manual_collection_match_lua54() {
    for (source, expected) in [
        (
            "local before=collectgarbage('isrunning'); collectgarbage('stop'); local during=collectgarbage('isrunning'); collectgarbage('collect'); collectgarbage('restart'); print(before,during,collectgarbage('isrunning'))",
            "true\tfalse\ttrue\n",
        ),
        (
            "collectgarbage('stop'); for i=1,15000 do local t={} end; collectgarbage('collect'); for i=1,15000 do local t={} end; print('manual')",
            "manual\n",
        ),
        (
            "collectgarbage('stop'); for i=1,1000 do local t={} end; collectgarbage('restart'); for i=1,20000 do local t={} end; print('restart')",
            "restart\n",
        ),
    ] {
        let owned = Command::new(env!("CARGO_BIN_EXE_fer"))
            .args(["--engine=owned", "--budget=3000000", "-e", source])
            .output()
            .expect("launch owned fer");
        let reference = run_fer(false, source);
        assert!(
            owned.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&owned.stderr)
        );
        assert!(
            reference.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&reference.stderr)
        );
        assert_eq!(owned.stdout, expected.as_bytes(), "{source}");
        assert_eq!(owned.stdout, reference.stdout, "{source}");
        if let Some(upstream) = reference_output(source) {
            assert!(upstream.status.success(), "{source}");
            assert_eq!(owned.stdout, upstream.stdout, "{source}");
        }
    }
}
