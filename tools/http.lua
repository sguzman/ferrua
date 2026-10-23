-- Direct trusted-mode HTTP adapter, with no implicit external commands.
local bridge = assert(__fer_http_call, "HTTP host bridge missing")
__fer_http_call = nil

local http = {}
local allowed = {GET=true, POST=true, PUT=true, PATCH=true, DELETE=true}

--- Returns response or (nil, transport_error). Non-2xx statuses are ordinary responses.
function http.request(method, url, options)
    assert(type(method) == "string" and allowed[method], "unsupported HTTP method")
    assert(type(url) == "string" and #url > 0, "HTTP URL must be nonempty text")
    options = options or {}
    assert(type(options) == "table", "HTTP options must be a table")
    local keys = {headers=true, body=true, timeout_ms=true}
    for key in pairs(options) do
        assert(keys[key], "unknown HTTP option: " .. tostring(key))
    end
    local headers = json.object()
    if options.headers ~= nil then
        assert(type(options.headers) == "table", "HTTP headers must be a table")
        for name, value in pairs(options.headers) do
            assert(type(name) == "string" and type(value) == "string",
                "HTTP header names and values must be strings")
            headers[name] = value
        end
    end
    if options.body ~= nil then assert(type(options.body) == "string", "HTTP body must be text") end
    if options.timeout_ms ~= nil then
        assert(type(options.timeout_ms) == "number" and
            options.timeout_ms % 1 == 0 and options.timeout_ms >= 1 and
            options.timeout_ms <= 120000, "timeout_ms must be from 1 to 120000")
    end
    local data = bridge(json.encode(json.object({
        method=method, url=url, headers=headers, body=options.body,
        timeout_ms=options.timeout_ms
    })))
    local result = json.decode(data)
    if result.error ~= json.null then return nil, result.error end
    return result.result
end

function http.get(url, options) return http.request("GET", url, options) end
function http.post(url, body, options)
    options = options or {}
    options.body = body
    return http.request("POST", url, options)
end
function http.patch(url, body, options)
    options = options or {}
    options.body = body
    return http.request("PATCH", url, options)
end

--- Throw if transport fails or status is outside 200-299.
function http.check(method, url, options)
    local response, err = http.request(method, url, options)
    if not response then error(err, 2) end
    if not response.ok then
        error("HTTP request returned status " .. tostring(response.status), 2)
    end
    return response
end

return http
