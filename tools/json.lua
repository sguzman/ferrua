-- Minimal first-party JSON decoder for Ferrua's progress tooling.
-- Supports RFC 8259 objects, arrays, numbers, strings and booleans.
-- Null is represented by json.null to retain array/object slots.
local json = {null = {}}

function json.decode(source)
    local position, length = 1, #source
    local parse_value

    local function fail(message)
        error("JSON at byte " .. position .. ": " .. message, 0)
    end
    local function whitespace()
        while position <= length and source:sub(position, position):match("%s") do
            position = position + 1
        end
    end
    local function string_value()
        if source:sub(position, position) ~= '"' then fail("expected string") end
        position = position + 1
        local chunks = {}
        while position <= length do
            local char = source:sub(position, position)
            position = position + 1
            if char == '"' then return table.concat(chunks) end
            if char ~= "\\" then
                if char:byte() < 32 then fail("unescaped control character") end
                chunks[#chunks + 1] = char
            else
                local escape = source:sub(position, position)
                position = position + 1
                local common = {['"']='"', ['\\']='\\', ['/']='/', b='\b',
                    f='\f', n='\n', r='\r', t='\t'}
                if common[escape] then
                    chunks[#chunks + 1] = common[escape]
                elseif escape == 'u' then
                    local hex = source:sub(position, position + 3)
                    if not hex:match("^%x%x%x%x$") then fail("invalid unicode escape") end
                    position = position + 4
                    local point = tonumber(hex, 16)
                    if point >= 0xD800 and point <= 0xDBFF then
                        if source:sub(position, position + 1) ~= "\\u" then
                            fail("missing low surrogate")
                        end
                        position = position + 2
                        local low = source:sub(position, position + 3)
                        if not low:match("^%x%x%x%x$") then fail("invalid low surrogate") end
                        position = position + 4
                        low = tonumber(low, 16)
                        if low < 0xDC00 or low > 0xDFFF then fail("invalid low surrogate") end
                        point = 0x10000 + (point - 0xD800) * 0x400 + (low - 0xDC00)
                    elseif point >= 0xDC00 and point <= 0xDFFF then
                        fail("unexpected low surrogate")
                    end
                    chunks[#chunks + 1] = utf8.char(point)
                else
                    fail("invalid string escape")
                end
            end
        end
        fail("unterminated string")
    end
    local function array_value()
        position = position + 1
        local result = {}
        whitespace()
        if source:sub(position, position) == "]" then
            position = position + 1
            return result
        end
        while true do
            result[#result + 1] = parse_value()
            whitespace()
            local char = source:sub(position, position)
            if char == "]" then
                position = position + 1
                return result
            end
            if char ~= "," then fail("expected comma or closing bracket") end
            position = position + 1
        end
    end
    local function object_value()
        position = position + 1
        local result = {}
        whitespace()
        if source:sub(position, position) == "}" then
            position = position + 1
            return result
        end
        while true do
            whitespace()
            local key = string_value()
            whitespace()
            if source:sub(position, position) ~= ":" then fail("expected colon") end
            position = position + 1
            result[key] = parse_value()
            whitespace()
            local char = source:sub(position, position)
            if char == "}" then
                position = position + 1
                return result
            end
            if char ~= "," then fail("expected comma or closing brace") end
            position = position + 1
        end
    end
    function parse_value()
        whitespace()
        local char = source:sub(position, position)
        if char == '"' then return string_value() end
        if char == '[' then return array_value() end
        if char == '{' then return object_value() end
        for _, literal in ipairs({"true", "false", "null"}) do
            if source:sub(position, position + #literal - 1) == literal then
                position = position + #literal
                if literal == "true" then return true end
                if literal == "false" then return false end
                return json.null
            end
        end
        local part = source:sub(position):match("^%-?%d+%.?%d*[eE]?[+%-]?%d*")
        if part and #part > 0 then
            local result = tonumber(part)
            if not result then fail("invalid number") end
            position = position + #part
            return result
        end
        fail("unexpected token")
    end
    local result = parse_value()
    whitespace()
    if position <= length then fail("trailing data") end
    return result
end


-- Deterministic first-party encoder. Cyclic or unsupported values are rejected.
local escapes = {['"']='\\"', ['\\']='\\\\', ['\b']='\\b',
    ['\f']='\\f', ['\n']='\\n', ['\r']='\\r', ['\t']='\\t'}
local function quote(value)
    return '"' .. value:gsub('[%z\1-\31\\"]', function(char)
        return escapes[char] or string.format("\\u%04x", char:byte())
    end) .. '"'
end

function json.encode(value)
    local visited = {}
    local function encode(item)
        if item == json.null or item == nil then return "null" end
        local kind = type(item)
        if kind == "string" then return quote(item) end
        if kind == "boolean" then return item and "true" or "false" end
        if kind == "number" then
            assert(item == item and item ~= math.huge and item ~= -math.huge,
                "JSON does not support non-finite numbers")
            return tostring(item)
        end
        assert(kind == "table", "JSON cannot encode " .. kind)
        assert(not visited[item], "JSON cannot encode cycles")
        visited[item] = true
        local keys, count, max_index, array = {}, 0, 0, true
        for key in pairs(item) do
            count = count + 1
            keys[#keys + 1] = key
            if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then
                array = false
            elseif key > max_index then
                max_index = key
            end
        end
        local elements = {}
        if array and count == max_index then
            for index = 1, max_index do elements[#elements + 1] = encode(item[index]) end
            visited[item] = nil
            return "[" .. table.concat(elements, ",") .. "]"
        end
        for _, key in ipairs(keys) do
            assert(type(key) == "string", "JSON object keys must be strings")
        end
        table.sort(keys)
        for _, key in ipairs(keys) do
            elements[#elements + 1] = quote(key) .. ":" .. encode(item[key])
        end
        visited[item] = nil
        return "{" .. table.concat(elements, ",") .. "}"
    end
    return encode(value)
end

return json
