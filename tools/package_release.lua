#!/usr/bin/env fer
-- Dogfood Ferrua to assemble its own versioned Linux release archive.
-- Build the binary first with cargo build --locked --release.
local manifest = assert(fs.read("Cargo.toml"))
local version = assert(manifest:match('\nversion%s*=%s*"([^"]+)"'),
    "could not determine Cargo package version")
local tag = "v" .. version
if arg[1] ~= nil then
    assert(arg[1] == tag,
        "release tag mismatch: expected " .. tag .. ", got " .. tostring(arg[1]))
end
assert(version:match("^%d+%.%d+%.%d+[%w%.%-]*$"), "invalid release version")
assert(fs.exists("target/release/fer"),
    "release executable absent: build with cargo build --locked --release")
local archive = "fer-" .. tag .. "-linux-x86_64.tar.gz"
process.check("tar", {
    "-czf", archive, "-C", "target/release", "fer", "-C", "../..",
    "README.md", "LICENSE"
})
local digest = process.check("sha256sum", {archive}).stdout
assert(digest:match("^[0-9a-f]+%s"), "sha256sum returned unexpected text")
assert(fs.write(archive .. ".sha256", digest) == #digest)
process.check("sha256sum", {"-c", archive .. ".sha256"})
local listed = process.check("tar", {"-tzf", archive}).stdout
assert(listed:find("fer\n", 1, true), "archive omitted fer")
assert(listed:find("README.md\n", 1, true), "archive omitted README")
assert(listed:find("LICENSE\n", 1, true), "archive omitted LICENSE")
print(archive)
