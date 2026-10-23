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
    for key in pairs(options) do
        assert(key == "token" or key == "base_url" or key == "timeout_ms",
            "unknown GitHub option: " .. tostring(key))
    end
    local headers = {
        ["Accept"] = "application/vnd.github+json",
        ["User-Agent"] = "Ferrua-Lua-Automation",
        ["X-GitHub-Api-Version"] = "2022-11-28"
    }
    local token = token_from(options)
    if token and token ~= "" then
        headers["Authorization"] = "Bearer " .. token
    end
    return base_from(options), {headers=headers, timeout_ms=options.timeout_ms}
end

local function valid_path(path)
    return type(path) == "string" and path:sub(1,1) == "/" and
        path:sub(1,2) ~= "//" and not path:find("://",1,true) and
        not path:find("..",1,true) and not path:find("%s")
end

--- Generic REST request; returns response or nil, error.
--- Response fields: ok, status, headers, body (text).
function github.request(method, path, options)
    assert(type(method) == "string", "GitHub method must be text")
    assert(valid_path(path), "GitHub API path must be a relative /path without traversal")
    local base, request_options = options_from(options)
    return http.request(method, base .. path, request_options)
end

--- Fetch repository metadata and decode its JSON body.
--- Returns metadata on success, or nil and a concise error on failure.
function github.repository(owner, name, options)
    assert(type(owner) == "string" and owner:match("^[%w%-]+$"),
        "GitHub owner must be an ASCII-like identifier")
    assert(type(name) == "string" and name:match("^[%w_%.%-]+$") and
        name:sub(1,1) ~= "." and not name:find("..",1,true),
        "GitHub repository name contains invalid characters")
    local response, err = github.request("GET", "/repos/" .. owner .. "/" .. name, options)
    if not response then return nil, err end
    if not response.ok then
        return nil, "GitHub REST returned HTTP " .. tostring(response.status)
    end
    local ok, result = pcall(json.decode, response.body)
    if not ok then return nil, "GitHub returned invalid JSON: " .. tostring(result) end
    return result
end

return github
