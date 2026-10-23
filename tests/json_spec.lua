#!/usr/bin/env fer
-- Generic JSON conformance regressions; runnable in every supported Lua mode.
local function valid(source, expected)
    local value = json.decode(source)
    if expected ~= nil then assert(value == expected, source) end
    return value
end
local function invalid(source)
    local ok, result = pcall(json.decode, source)
    assert(not ok, "accepted invalid JSON: " .. source)
    assert(type(result) == "string")
end

assert(json.encode(json.decode('{"a":1,"b":[true,false,null]}')) ==
    '{"a":1,"b":[true,false,null]}')
assert(json.encode(valid("{}")) == "{}")
assert(json.encode(valid("[]")) == "[]")
assert(json.encode(json.object()) == "{}")
assert(json.encode(json.array()) == "[]")
assert(json.decode("null") == json.null)
assert(json.encode(json.null) == "null")
assert(valid("0") == 0)
assert(valid("-0") == 0)
assert(valid("1.5") == 1.5)
assert(valid("-1.25") == -1.25)
assert(valid("1E+2") == 100)
assert(valid("2e-2") == 0.02)
assert(valid(" \t\r\n true \n ") == true)
assert(valid('"\\u0041"') == "A")
assert(valid('"\\uD83C\\uDF19"') == "🌙")
assert(json.encode(valid('"\\u0000"')) == '"\\u0000"')
assert(valid('"\\n"') == "\n")
assert(valid('"\\\\"') == "\\")
assert(valid('"\\/"') == "/")
for _, source in ipairs({
    "", " ", "00", "01", "-01", "1.", "1e", "1e+", "-", "1.2.3",
    "1e309", "-1e309", "[1,]", '{"a":1,}', '{"a"}',
    '{"a": 1} trailing', "[", "{", '"\\x"', '"\\uD800"',
    '"\\uDC00"', '"\\uD800\\u0041"', "nil", "+1",
    string.char(11) .. "true",
    '"raw' .. string.char(10) .. 'line"'
}) do invalid(source) end
local self_ref = {}
self_ref.itself = self_ref
assert(not pcall(json.encode, self_ref))
assert(not pcall(json.encode, math.huge))
assert(not pcall(json.encode, 0/0))
assert(not pcall(json.encode, { [1]="one", [3]="three" }))
assert(json.encode(json.array({1,2,3})) == "[1,2,3]")
assert(json.encode(json.object({answer=42})) == '{"answer":42}')
invalid(string.rep("[", 130) .. "0" .. string.rep("]", 130))
local nested, root = {}, {}
root = nested
for _ = 1, 130 do
    local child = {}
    nested[1] = child
    nested = child
end
assert(not pcall(json.encode, root))
print("JSON conformance smoke passed")
