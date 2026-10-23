# 🗃️ TOML and YAML conversions

Ferrua includes `toml` and `yaml` tables in **trusted mode** for common config and GitHub Actions scripts.

```lua
local doc = assert(toml.decode('name = "Ferrua"\nversion = 1'))
assert(doc.name == "Ferrua")
local yml = assert(yaml.encode({version=1, name=doc.name}))
local data = assert(yaml.decode(yml))
```

Each table implements `.decode(text)` and `.encode(value)`. Failures return `nil, error`, not partial results. The Rust host uses `toml` and `yaml_serde`, translating through `serde_json::Value` and Ferrua's `json` library. The YAML dependency is the actively maintained YAML organization fork; do not replace it with the deprecated, unsound `serde_yml` package. This version of Ferrua does not require Python.

This interoperability layer handles JSON-compatible maps, arrays, booleans, numbers and strings. It is **not** a comment-preserving editor: TOML/YAML comments, key formatting, original ordering, YAML custom tags and anchors are not retained as source syntax, and TOML datetimes may need special handling. TOML cannot directly represent null, so unsupported JSON null values generate serialization errors. The maximum input/output document is 4 MiB.

`tests/data.lua` exercises both formats with round trips, invalid input and unsupported values. The API is unavailable in `fer --sandbox`; use the standalone pure Lua `json` library there.
