local co = coroutine.create(function()
    coroutine.yield(3)
    coroutine.yield(4)
    return 5
end)
while coroutine.status(co) ~= "dead" do
    local ok, value = coroutine.resume(co)
    assert(ok)
    print(value)
end
