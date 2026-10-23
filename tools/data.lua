-- Trusted-mode TOML and YAML conversion, with Rust parsing and serialization.
local bridge = assert(__fer_data_call, "data conversion bridge missing")
__fer_data_call = nil

local function request(operation, text, value)
    local input = json.object({operation=operation, text=text, value=value})
    local result = json.decode(bridge(json.encode(input)))
    if result.error ~= json.null then return nil, result.error end
    return result.value
end

local function decode(kind, text)
    assert(type(text) == "string", kind .. ".decode expects text")
    return request(kind .. "_decode", text)
end

local function encode(kind, value)
    assert(value ~= nil, kind .. ".encode expects a value")
    return request(kind .. "_encode", nil, value)
end

local toml = {}
local yaml = {}
function toml.decode(text) return decode("toml", text) end
function toml.encode(value) return encode("toml", value) end
function yaml.decode(text) return decode("yaml", text) end
function yaml.encode(value) return encode("yaml", value) end
return toml, yaml
