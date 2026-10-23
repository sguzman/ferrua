-- GitHub REST helper for trusted Lua automation. API calls use native Ferrua HTTP.
local github = {}

local function token_from(options)
    local token = options.token
    if token == nil then token = os.getenv("GITHUB_TOKEN") end
    if token ~= nil then
        assert(type(token) == "string", "GitHub token must be text")
    end
    return token
end

local function base_from(options)
    local base = options.base_url or "https://api.github.com"
    -- Alternate base URL is only permitted for local, offline test fixtures.
    assert(base == "https://api.github.com" or
        base:match("^http://127%.0%.0%.1:%d+$"),
        "GitHub base_url must be the official API or a loopback test server")
    return base
end

local function options_from(options)
    options = options or {}
    assert(type(options) == "table", "GitHub options must be a table")
    local allowed = {
        token = true, base_url = true, timeout_ms = true, headers = true,
        body = true, json_body = true
    }
    for key in pairs(options) do
        assert(allowed[key], "unknown GitHub option: " .. tostring(key))
    end
    local headers = {
        ["Accept"] = "application/vnd.github+json",
        ["User-Agent"] = "Ferrua-Lua-Automation",
        ["X-GitHub-Api-Version"] = "2022-11-28"
    }
    if options.headers ~= nil then
        assert(type(options.headers) == "table", "GitHub headers must be a table")
        for name, value in pairs(options.headers) do
            assert(type(name) == "string" and type(value) == "string",
                "GitHub headers must contain string keys and values")
            local lower = name:lower()
            assert(lower ~= "authorization" and lower ~= "host" and
                lower ~= "content-length" and lower ~= "proxy-authorization",
                "GitHub authentication and transport headers are managed internally")
            headers[name] = value
        end
    end
    local token = token_from(options)
    if token and token ~= "" then
        headers["Authorization"] = "Bearer " .. token
    end
    assert(options.body == nil or options.json_body == nil,
        "use either body or json_body, not both")
    local body = options.body
    if options.json_body ~= nil then
        body = json.encode(options.json_body)
        headers["Content-Type"] = "application/json"
    end
    if body ~= nil then assert(type(body) == "string", "body must be text") end
    return base_from(options), {
        headers=headers, body=body, timeout_ms=options.timeout_ms
    }
end

local function valid_path(path)
    return type(path) == "string" and path:sub(1,1) == "/" and
        path:sub(1,2) ~= "//" and not path:find("://",1,true) and
        not path:find("..",1,true) and not path:find("[%c%s]") and not path:find("#",1,true)
end

--- Generic REST request; returns response or nil, error.
--- Response fields: ok, status, headers, body (text). No redirects followed.
--- Supply body or json_body for POST/PUT/PATCH, with headers as needed.
function github.request(method, path, options)
    assert(type(method) == "string", "GitHub method must be text")
    assert(valid_path(path), "GitHub API path must be a relative /path without traversal")
    local base, request_options = options_from(options)
    return http.request(method, base .. path, request_options)
end

--- Generic JSON-decoding variant for successful API operations.
function github.request_json(method, path, options)
    local response, err = github.request(method, path, options)
    if not response then return nil, err end
    if not response.ok then
        return nil, "GitHub REST returned HTTP " .. tostring(response.status)
    end
    local ok, result = pcall(json.decode, response.body)
    if not ok then return nil, "GitHub returned invalid JSON: " .. tostring(result) end
    return result
end

--- Fetch repository metadata and decode its JSON body.
--- Returns metadata on success, or nil and a concise error on failure.
function github.repository(owner, name, options)
    assert(type(owner) == "string" and owner:match("^[%w%-]+$"),
        "GitHub owner must be an ASCII-like identifier")
    assert(type(name) == "string" and name:match("^[%w_%.%-]+$") and
        name:sub(1,1) ~= "." and not name:find("..",1,true),
        "GitHub repository name contains invalid characters")
    return github.request_json("GET", "/repos/" .. owner .. "/" .. name, options)
end

return github
