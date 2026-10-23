-- Trusted-mode process API. No shell is introduced by Ferrua.
local bridge = assert(__fer_process_run, "process host bridge missing")
__fer_process_run = nil

local process = {}

local function checked_args(args)
    assert(type(args) == "table", "process args must be an array")
    local copy = json.array()
    for i, value in ipairs(args) do
        assert(type(value) == "string", "process argument #" .. i .. " must be a string")
        copy[i] = value
    end
    for key in pairs(args) do
        assert(type(key) == "number" and key >= 1 and key % 1 == 0 and key <= #copy,
            "process args must be a dense array")
    end
    return copy
end

--- Run program directly using argv. Returns result or (nil, spawn_error).
--- Nonzero exits are normal results with ok=false. Captured output is UTF-8 lossy.
function process.run(program, args, options)
    assert(type(program) == "string" and #program > 0,
        "process program must be a nonempty string")
    args = checked_args(args or {})
    options = options or {}
    assert(type(options) == "table", "process options must be a table")
    local permitted = { cwd=true, env=true, stdin=true, timeout_ms=true }
    for key in pairs(options) do
        assert(permitted[key], "unknown process option: " .. tostring(key))
    end
    local environment = json.object()
    if options.env ~= nil then
        assert(type(options.env) == "table", "process env must be a table")
        for name, value in pairs(options.env) do
            assert(type(name) == "string" and type(value) == "string",
                "process env needs string keys and values")
            environment[name] = value
        end
    end
    if options.cwd ~= nil then assert(type(options.cwd) == "string", "cwd must be a string") end
    if options.stdin ~= nil then assert(type(options.stdin) == "string", "stdin must be a string") end
    if options.timeout_ms ~= nil then
        assert(type(options.timeout_ms) == "number" and
            options.timeout_ms >= 1 and options.timeout_ms <= 300000 and
            options.timeout_ms % 1 == 0, "timeout_ms must be an integer from 1 to 300000")
    end
    local response = json.decode(bridge(json.encode(json.object({
        program = program,
        args = args,
        cwd = options.cwd,
        env = environment,
        stdin = options.stdin,
        timeout_ms = options.timeout_ms
    }))))
    if response.spawn_error ~= json.null then
        return nil, response.spawn_error
    end
    return response
end

--- Check success; raise a Lua error on spawn failure, timeout or nonzero status.
function process.check(program, args, options)
    local result, err = process.run(program, args, options)
    if not result then error(err, 2) end
    if not result.ok then
        error(("process %s failed (exit=%s, timeout=%s): %s"):format(
            program, tostring(result.code == json.null and "signal" or result.code),
            tostring(result.timed_out), result.stderr), 2)
    end
    return result
end

return process
