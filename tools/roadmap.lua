#!/usr/bin/env fer
-- No Python, and no external Lua modules: fer runs its own roadmap generator.
local json = require("tools.json")
local function read(path)
    local f, err = io.open(path, "rb")
    assert(f, err)
    local s = assert(f:read("*a"))
    assert(f:close())
    return s
end
local function write(path, s)
    local f, err = io.open(path, "wb")
    assert(f, err)
    assert(f:write(s))
    assert(f:close())
end
local ledger = json.decode(read("docs/progress.json"))
assert(ledger.schema == 1, "unexpected ledger schema")
local function round(n) return math.floor(n + 0.5) end
local function mean(tasks)
    if #tasks == 0 then return 0 end
    local total = 0
    for _, task in ipairs(tasks) do total = total + task.progress end
    return round(total / #tasks)
end
local function bar(n)
    local blocks = math.min(10, round(n / 10))
    return string.rep("█", blocks) .. string.rep("░", 10 - blocks)
end
local tasks, lookup, ids = {}, {}, {}
for _, milestone in ipairs(ledger.milestones) do
    for _, phase in ipairs(milestone.phases) do
        for _, task in ipairs(phase.tasks) do
            assert(not ids[task.id], "duplicate task ID: " .. task.id)
            ids[task.id] = true
            lookup[task.id] = task
            tasks[#tasks + 1] = task
            assert(#task.checks > 0, "task needs checks: " .. task.id)
            local accepted = 0
            for _, check in ipairs(task.checks) do
                assert(type(check.passed) == "boolean", "non-boolean pass value: " .. task.id)
                if check.passed then
                    accepted = accepted + 1
                    assert(type(check.evidence) == "string" and #check.evidence > 0,
                        "missing acceptance evidence: " .. task.id)
                end
            end
            assert(task.progress == round(100 * accepted / #task.checks),
                "stale progress for " .. task.id)
        end
    end
end
local lines = {
    "# 🗺️ Ferrua roadmap", "",
    "**Source of truth:** [progress.json](progress.json). Progress is evidence-backed atomic acceptance checks averaged by phase, milestone and feature. No unverified CI claim reaches 100%.",
    "",
    string.format("**Overall:** \`%s %d%%\` across %d atomic tasks. This describes recorded work, not full Lua compatibility.",
        bar(mean(tasks)), mean(tasks), #tasks), ""
}
for _, milestone in ipairs(ledger.milestones) do
    local m_tasks = {}
    for _, phase in ipairs(milestone.phases) do
        for _, task in ipairs(phase.tasks) do m_tasks[#m_tasks + 1] = task end
    end
    lines[#lines + 1] = string.format("## %s %s — %s — \`%s %d%%\`",
        milestone.emoji, milestone.id, milestone.name, bar(mean(m_tasks)), mean(m_tasks))
    lines[#lines + 1] = ""
    lines[#lines + 1] = "**Acceptance gate:** " .. milestone.gate
    lines[#lines + 1] = ""
    for _, phase in ipairs(milestone.phases) do
        lines[#lines + 1] = string.format("### %s %s — %s — \`%s %d%%\`",
            phase.emoji, phase.id, phase.name, bar(mean(phase.tasks)), mean(phase.tasks))
        lines[#lines + 1] = ""
        for _, task in ipairs(phase.tasks) do
            lines[#lines + 1] = string.format("- %s **%s** %s — \`%s %d%%\`",
                task.emoji, task.id, task.name, bar(task.progress), task.progress)
            for _, check in ipairs(task.checks) do
                lines[#lines + 1] = string.format("  - [%s] %s%s", check.passed and "x" or " ",
                    check.text, check.passed and (" — " .. check.evidence) or "")
            end
        end
        lines[#lines + 1] = ""
    end
    lines[#lines + 1] = "**Feature inventory** (computed from associated atomic task progress):"
    lines[#lines + 1] = ""
    for _, feature in ipairs(milestone.features) do
        local f_tasks = {}
        for _, id in ipairs(feature.task_ids) do
            assert(lookup[id], "unknown feature task: " .. id)
            f_tasks[#f_tasks + 1] = lookup[id]
        end
        lines[#lines + 1] = string.format("- %s %s — \`%s %d%%\`",
            feature.emoji, feature.name, bar(mean(f_tasks)), mean(f_tasks))
    end
    lines[#lines + 1] = ""
end
local content = table.concat(lines, "\n") .. "\n"
if arg[1] == "--check" then
    assert(content == read("docs/ROADMAP.md"), "stale ROADMAP.md; run fer tools/roadmap.lua")
    print(string.format("Ferrua roadmap verified: %d tasks, %d%%", #tasks, mean(tasks)))
else
    write("docs/ROADMAP.md", content)
    print(string.format("Ferrua roadmap generated: %d tasks, %d%%", #tasks, mean(tasks)))
end
