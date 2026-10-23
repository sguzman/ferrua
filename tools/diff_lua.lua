#!/usr/bin/env fer
-- Compare deterministic Lua behavior with the distribution's Lua 5.4 interpreter.
-- Both executions happen locally in GitHub Actions; no Python or external API.
local reference = "lua5.4"
local ours = "target/debug/fer"
local cases = {
    "01-control-flow", "02-closures", "03-tables", "04-string-patterns",
    "05-upvalues", "06-scopes", "07-coroutines", "08-metatables",
    "09-generic-iterators", "10-multiple-returns", "11-bitwise",
    "12-utf8"
}
local version = process.check(reference, {"-v"})
assert((version.stderr .. version.stdout):find("Lua 5.4.6", 1, true),
    "reference must report pinned upstream Lua 5.4.6")
print("Reference: " .. (version.stderr .. version.stdout):gsub("%s+$", ""))

for _, name in ipairs(cases) do
    local path = "tests/conformance/" .. name .. ".lua"
    local official, oerr = process.run(reference, {path}, {timeout_ms=10000})
    assert(official, oerr)
    assert(official.ok, name .. ": reference interpreter failed: " .. official.stderr)
    local current, cerr = process.run(ours, {"--lua=5.4", path}, {timeout_ms=10000})
    assert(current, cerr)
    assert(current.ok, name .. ": Ferrua interpreter failed: " .. current.stderr)
    assert(current.stdout == official.stdout,
        name .. " output differs from Lua 5.4\nexpected: " ..
        official.stdout .. "\nactual: " .. current.stdout)
    assert(current.stderr == "",
        name .. ": unexpected Ferrua stderr: " .. current.stderr)
    print(name .. " matched Lua 5.4")
end

print("Ferrua vs Lua 5.4 differential smoke passed")
