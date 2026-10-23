# 📂 Ferrua filesystem API

Ferrua exposes a trusted-mode `fs` table. It uses Rust's standard filesystem library; the `fs` table and native bridge are intentionally absent with `fer --sandbox`.

| API | Result |
|---|---|
| `fs.exists(path)` | Boolean indicating whether the path exists |
| `fs.read(path)` | UTF-8 text |
| `fs.write(path, text)` | Number of UTF-8 bytes written; overwrites the file |
| `fs.mkdir(path)` | Creates a directory and missing parents; returns true |
| `fs.copy(source, dest)` | Bytes copied; overwrites an existing destination file |
| `fs.list(path)` | Sorted array of immediate entries |
| `fs.walk(path)` | Recursive array of entries |

All operations return `nil, error` when the underlying filesystem operation fails. Entry records contain `name`, `path` and `type` (`file`, `directory`, `symlink` or `other`). Traversal never follows symbolic links; it stops at 64 levels or 10,000 total entries. A single directory is limited to 10,000 entries. Read and write text operations are capped at 8 MiB each. Binary files and very large files should use future streaming APIs instead.

```lua
local items = assert(fs.list("."))
for _, item in ipairs(items) do
    print(item.type, item.name)
end
```

The API uses the caller's filesystem permissions without adding permissions or isolation. Scripts can overwrite files only when explicitly asking for `fs.write` or `fs.copy`. There is no recursive deletion API. Paths can be absolute or relative to the process working directory.

The CI regression `fer examples/fs.lua SCRATCH_DIRECTORY` uses a disposable temporary directory and tests read/write, copy, traversal, errors and output contracts.
