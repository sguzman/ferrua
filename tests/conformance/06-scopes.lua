local x = 10
do
    local x = 20
    for i = 1, 3 do
        local x = x + i
        print(x)
    end
end
print(x)
