local source = "ferrua lua"
print(source:upper(), string.sub(source, 1, 6))
local result, n = source:gsub("lua", "rust")
print(result, n)
print(string.format("%04d", 42))
