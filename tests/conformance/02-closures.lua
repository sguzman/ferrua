local function counter(start)
    return function(delta)
        start = start + delta
        return start
    end
end
local a, b = counter(5), counter(100)
print(a(3), b(1), a(-2), b(9), a(0))
