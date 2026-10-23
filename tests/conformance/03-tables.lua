local values = {7, 1, 4, 3}
table.sort(values)
print(table.concat(values, ","))
local keys = {gamma = 3, alpha = 1, beta = 2}
local sorted = {}
for k in pairs(keys) do sorted[#sorted + 1] = k end
table.sort(sorted)
print(table.concat(sorted, ":"))
