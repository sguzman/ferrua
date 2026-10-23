local function iter(state, index)
    if index < #state then
        index = index + 1
        return index, state[index]
    end
end
local values = {2, 4, 6}
local result = 0
for i, x in iter, values, 0 do
    result = result + i * x
end
print(result)
