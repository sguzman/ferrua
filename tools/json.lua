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
                    if point >= 0xD800 and point <= 0xDFFF then
                        fail("surrogate escape not supported by this tooling decoder")
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

return json
