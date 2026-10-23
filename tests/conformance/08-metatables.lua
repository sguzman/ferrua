local mt = {}
mt.__add = function(a, b)
    return setmetatable({x = a.x + b.x}, mt)
end
local a = setmetatable({x = 2}, mt)
local b = setmetatable({x = 3}, mt)
print((a + b).x)
local proxy = setmetatable({}, {__index = function(_, k) return k .. "!" end})
print(proxy.moon)
