#!/usr/bin/env fer
-- Example: useful CI automation without Python or shell interpolation.
local result = process.check("printf", {"%s", "Ferrua automation works"})
assert(result.stdout == "Ferrua automation works")
assert(result.stderr == "")
assert(result.code == 0)
local failure = assert(process.run("sh", {"-c", "exit 7"}))
assert(not failure.ok and failure.code == 7)
local missing, reason = process.run("fer-intentionally-missing-command", {})
assert(missing == nil and type(reason) == "string")
local input = process.check("cat", {}, {stdin = "hello from stdin"})
assert(input.stdout == "hello from stdin")
local env = process.check("sh", {"-c", "printf %s \"$FER_SAMPLE\""},
    {env = {FER_SAMPLE = "environment forwarded"}})
assert(env.stdout == "environment forwarded")
local timed = assert(process.run("sleep", {"2"}, {timeout_ms = 50}))
assert(not timed.ok and timed.timed_out)
print("process module smoke passed")
