#!/usr/bin/env fer
-- Execute release-critical scripting checks in every Luna compatibility dialect.
-- Use the already-built fer binary; no Python, external Lua VM, or network required.
local dialects = {"5.1", "5.2", "5.3", "5.4", "5.5"}
local interpreter = "target/debug/fer"

local function check(args, label)
    local result = process.check(interpreter, args, {timeout_ms = 30000})
    assert(result.code == 0, label)
    return result.stdout
end

for _, version in ipairs(dialects) do
    local flag = "--lua=" .. version
    local json_report = check({flag, "--sandbox", "tests/json_spec.lua"}, version .. " JSON")
    assert(json_report:find("JSON conformance smoke passed", 1, true),
        version .. " JSON fixture did not complete")

    local data_report = check({flag, "tests/data.lua"}, version .. " structured data")
    assert(data_report:find("TOML and YAML smoke passed", 1, true),
        version .. " structured data fixture did not complete")

    local script_report = check({flag, "examples/count.lua"}, version .. " basic scripting")
    assert(script_report:match("^55%s*$"), version .. " script mismatch")

    check({flag, "--check", "examples/count.lua"}, version .. " syntax check")

    local sandbox_report = check({
        flag, "--sandbox", "-e",
        "assert(process == nil and fs == nil and http == nil and github == nil)"
    }, version .. " sandbox isolation")
    assert(sandbox_report == "")

    print("Lua " .. version .. " accepted")
end

print("Ferrua Lua dialect matrix passed")
