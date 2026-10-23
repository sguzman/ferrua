local function values()
    return 10, 20, 30
end
local a, b, c = values()
print(a, b, c, select("#", values()))
local collection = {values()}
print(#collection, collection[3])
