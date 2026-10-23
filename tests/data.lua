#!/usr/bin/env fer
local toml_source = [[
name = "Ferrua"
version = 1
[features]
json = true
]]
local t, err = toml.decode(toml_source)
assert(t, err)
assert(t.name == "Ferrua")
assert(t.features.json == true)
assert(t.version == 1)
local written = assert(toml.encode(t))
local round = assert(toml.decode(written))
assert(round.name == "Ferrua" and round.features.json == true)

local yaml_source = [[
name: Ferrua
features:
  - interpreter
  - automation
enabled: true
]]
local y, yerr = yaml.decode(yaml_source)
assert(y, yerr)
assert(y.name == "Ferrua" and y.features[2] == "automation")
assert(y.enabled == true)
local yaml_written = assert(yaml.encode(y))
local parsed = assert(yaml.decode(yaml_written))
assert(parsed.name == "Ferrua" and parsed.features[1] == "interpreter")

local bad, reason = toml.decode("something = [")
assert(bad == nil and type(reason) == "string")
local invalid, why = toml.encode({list=json.array({json.null})})
assert(invalid == nil and type(why) == "string")
print("TOML and YAML smoke passed")
