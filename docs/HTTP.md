# 🌐 Ferrua HTTP API

The trusted-mode `http` table is built into `fer`. It uses the Rust `ureq` blocking client with TLS via rustls. No Python, curl, shell execution or network subprocess is required.

```lua
local result = http.check("GET", "https://api.github.com/repos/sguzman/ferrua", {
    headers = {
        ["Accept"] = "application/vnd.github+json",
        ["User-Agent"] = "Ferrua-Script"
    }
})
local repository = json.decode(result.body)
print(repository.full_name)
```

Supported functions:

- `http.request(method, url, options?)`: GET, POST, PUT, PATCH, DELETE
- `http.get(url, options?)`: GET shortcut
- `http.post(url, body, options?)` and `http.patch(url, body, options?)`
- `http.check(method, url, options?)`: like `request` but throws if status is not 2xx

An ordinary response has `ok` (2xx), `status`, `headers` (mapping of textual response header values), and `body` (UTF-8). 404/500 responses are returned with `ok=false`, not treated as transport errors. Transport or validation failures return `nil, error` from `request`, `get`, `post` and `patch`.

Options: `headers = {["Authorization"] = "Bearer ..."}`, `body = "text"`, `timeout_ms = 30000` (1–120000). Requests default to a 30-second overall timeout, including response retrieval. Responses are limited to 4 MiB; larger or invalid UTF-8 bodies return an error. The client follows its built-in redirect policy. HTTP URLs and loopback endpoints are allowed in trusted mode; never run scripts fetched from untrusted sources in trusted mode.

For authenticated GitHub automation, read a token from environment variables at runtime (e.g. `os.getenv("GITHUB_TOKEN")`). Do not hard-code secrets, print tokens or include them in public fixtures.

The CI integration test starts a temporary server on `127.0.0.1`, invokes the built `fer` interpreter, asserts HTTP headers/body/status, and shuts down. It needs no public network, credentials or Python. The API is intentionally unavailable under `fer --sandbox`.

HTTP shortcut methods copy supplied options rather than mutating the caller's table. The local CI fixtures cover successful GET, non-2xx GET, POST with headers and request body, and sandbox exclusion.
