local total = 0
for i = 1, 20 do
    if i % 3 == 0 then
        total = total + i
    end
end
local function factorial(n)
    if n <= 1 then return 1 end
    return n * factorial(n - 1)
end
print(total, factorial(7))
