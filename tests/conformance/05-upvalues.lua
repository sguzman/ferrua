local function outer(x)
    local function inner(y)
        x = x + y
        return x * 2
    end
    return inner
end
local f = outer(10)
print(f(1), f(4), f(-5))
