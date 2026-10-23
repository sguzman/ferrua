print(utf8.len("🌙✨"))
print(utf8.codepoint("🌙"))
local chars = {}
for _, code in utf8.codes("Café") do
    chars[#chars + 1] = code
end
print(table.concat(chars, ","))
