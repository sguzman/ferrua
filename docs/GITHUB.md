# 🐙 GitHub REST automation with Ferrua

The trusted-mode `github` Lua module is a thin wrapper around Ferrua's native HTTP client. It supplies required GitHub REST headers and optionally uses `GITHUB_TOKEN` from the process environment. It is not present under `fer --sandbox`.

```lua
local repo, err = github.repository("sguzman", "ferrua")
assert(repo, err)
print(repo.full_name)
```

For generic REST endpoints:

```lua
local response, err = github.request("GET", "/repos/sguzman/ferrua/actions/runs?per_page=5")
assert(response, err)
assert(response.ok, response.status)
local data = json.decode(response.body)
print(data.total_count)
```

Supported methods: GET, POST, PUT, PATCH, DELETE through `github.request(method, path, options?)`. The path must begin with one slash and cannot contain double-dot segments, whitespace, or an absolute URL. GitHub request options are `token`, `timeout_ms`, `base_url`, `headers`, `body` (raw string), or `json_body` (automatically serialized to JSON). `body` and `json_body` are mutually exclusive. Headers controlling authentication, host, and content length cannot be overridden. For JSON API responses, `github.request_json(method, path, options)` parses a successful response and otherwise returns `nil, error`. Explicit `token` overrides `GITHUB_TOKEN`. The default base URL is always `https://api.github.com`; a `http://127.0.0.1:PORT` override is allowed solely for offline test fixtures.

Use `env:` and `secrets.GITHUB_TOKEN`/the workflow token when calling from GitHub Actions. Do not commit real tokens or print request headers, which might contain credentials. The wrapper avoids echoing tokens and only returns response metadata. A server redirect, proxy or external dependency may affect where HTTP headers go: do not run untrusted Lua scripts in trusted mode and ensure any authenticated API target is appropriate. Ferrua does not audit the third-party HTTP client's redirect security policy.

The CI integration test starts a loopback HTTP fixture that requires a dummy bearer token and verifies the structured repository response. It invokes the compiled `fer` CLI and never contacts GitHub's public service.

`github.repository(owner, repo, options?)` parses a successful response into a table; failures return `nil, error`. `github.request(...)` returns an ordinary response for non-2xx HTTP statuses, with `ok=false`.

**Redirect safety:** Ferrua disables automatic HTTP redirects in its native client. Responses with 3xx status are surfaced to Lua instead of following their `Location` header; this prevents an API token from being silently forwarded to another origin. A loopback test verifies that no redirected connection is made. Never supply real credentials to an untrusted `base_url`; loopback overrides are intended solely for test fixtures.

Example creating an issue (requires a write-capable GitHub token and the appropriate repository permission):

```lua
local issue, err = github.request_json("POST", "/repos/OWNER/REPO/issues", {
    json_body = {title = "Automation example", body = "Created by Ferrua"}
})
assert(issue, err)
print(issue.number)
```
