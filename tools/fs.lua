-- Trusted-mode filesystem helpers; no shell or Python needed.
local bridge = assert(__fer_fs_call, "fs host bridge missing")
__fer_fs_call = nil
local fs = {}

local function execute(op, path, data, destination)
    assert(type(path) == "string" and #path > 0, "fs path must be nonempty text")
    if data ~= nil then assert(type(data) == "string", "fs data must be text") end
    if destination ~= nil then
        assert(type(destination) == "string" and #destination > 0,
            "fs destination must be nonempty text")
    end
    local request = json.object({op=op, path=path, data=data, destination=destination})
    local result = json.decode(bridge(json.encode(request)))
    if not result.ok then return nil, result.error end
    return result.value
end

function fs.exists(path) return execute("exists", path) end
function fs.read(path) return execute("read", path) end
function fs.write(path, text) return execute("write", path, text) end
function fs.mkdir(path) return execute("mkdir", path) end
function fs.copy(from, to) return execute("copy", from, nil, to) end
function fs.list(path) return execute("list", path) end
function fs.walk(path) return execute("walk", path) end

return fs
