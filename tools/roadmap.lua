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
-- Cross-milestone graduation invariants supplement per-task check arithmetic.
-- Narrow smoke suites cannot authorize a production engine switch.
local function validate_graduation()
    local graduation = assert(lookup.T304, "missing owned-engine graduation gate")
    local release = assert(lookup.T107, "missing distribution acceptance gate")
    local packaging = assert(lookup.T204, "missing tagged-release acceptance gate")
    local adoption = assert(lookup.T208, "missing pinned-consumer acceptance gate")
    if graduation.checks[2].passed then
        assert(graduation.checks[1].passed,
            "cannot switch production interpreter before criteria are documented")
        for _, id in ipairs({ "T301", "T302", "T303" }) do
            assert(lookup[id] and lookup[id].progress == 100,
                "cannot graduate owned VM while " .. id .. " remains unaccepted")
        end
    end
    if graduation.checks[3].passed then
        assert(graduation.checks[2].passed,
            "cannot release the stable owned runtime before engine cutover")
        assert(release.checks[2].passed,
            "cannot release the owned runtime before native EndeavourOS acceptance")
    end
    if release.checks[3].passed then
        assert(release.checks[2].passed,
            "cannot accept stable fer release without native EndeavourOS QA")
        assert(packaging.checks[1].passed,
            "cannot accept stable fer release without verified tagged artifact")
        assert(adoption.checks[1].passed and adoption.checks[3].passed,
            "cannot accept stable fer release before checksums and pinned downstream QA")
    end
end

-- Release workflow must check a requested tag *before* building/publishing.
-- A v0.x tag is always a GitHub prerelease; a v1+ tag is blocked until all
-- native, downstream and owned-engine preconditions have been accepted.
local function validate_release_tag(tag)
    assert(type(tag) == "string", "release tag is required")
    local major, minor, patch, suffix = tag:match("^v(%d+)%.(%d+)%.(%d+)(.*)$")
    assert(major and minor and patch and
        (suffix == "" or suffix:match("^%-[%w%.%-]+$")),
        "release tag must be explicit semver (vMAJOR.MINOR.PATCH[-PRERELEASE])")
    local source_version = assert(read("Cargo.toml"):match('\nversion%s*=%s*"([^"]+)"'),
        "missing Cargo package version")
    assert(tag == "v" .. source_version,
        "release tag must match the Cargo package version")
    if tonumber(major) >= 1 then
        validate_graduation()
        assert(lookup.T107.checks[2].passed,
            "stable release blocked: native EndeavourOS acceptance missing")
        assert(lookup.T204.checks[1].passed,
            "stable release blocked: verified tagged preview release missing")
        assert(lookup.T208.checks[1].passed and lookup.T208.checks[3].passed,
            "stable release blocked: versioned checksums or pinned downstream QA missing")
        assert(lookup.T301.progress == 100 and lookup.T302.progress == 100 and
            lookup.T303.progress == 100 and lookup.T304.checks[2].passed,
            "stable release blocked: native owned-engine parity and cutover unaccepted")
    end
    return tag
end

if arg[1] == "--self-test-gates" then
    validate_graduation()
    local function must_reject(id, check_index)
        local check = assert(lookup[id], "unknown graduation test task").checks[check_index]
        assert(not check.passed, "graduation self-test expects incomplete baseline")
        check.passed = true
        local ok = pcall(validate_graduation)
        check.passed = false
        assert(not ok, "graduation guard failed to reject premature " .. id)
    end
    must_reject("T304", 2)
    must_reject("T304", 3)
    must_reject("T107", 3)
    -- Current pre-1.0 development tag can be verified without falsely
    -- granting stable-release or native EndeavourOS acceptance.
    local source_version = assert(read("Cargo.toml"):match('\nversion%s*=%s*"([^"]+)"'))
    local current_tag = "v" .. source_version
    local current_major = tonumber(source_version:match("^(%d+)"))
    assert(not pcall(validate_release_tag, "invalid"))
    assert(not pcall(validate_release_tag, "v999.999.999"))
    if current_major == 0 then
        assert(pcall(validate_release_tag, current_tag),
            "pre-1.0 prerelease must remain eligible")
    elseif not lookup.T107.checks[2].passed then
        assert(not pcall(validate_release_tag, current_tag),
            "stable release must not bypass native EndeavourOS QA")
    end
    print("Ferrua graduation guards passed")
    return
end

validate_graduation()
if arg[1] == "--release-gate" then
    local tag = validate_release_tag(arg[2])
    print("Ferrua release gate accepted: " .. tag)
    return
end

local incomplete, unchecked = {}, 0
for _, task in ipairs(tasks) do
    if task.progress < 100 then
        incomplete[#incomplete + 1] = task.id .. " (" .. task.progress .. "%)"
        for _, check in ipairs(task.checks) do
            if not check.passed then unchecked = unchecked + 1 end
        end
    end
end
local readiness = #incomplete == 0 and "ACCEPTANCE CHECKS COMPLETE" or "BLOCKED"
local lines = {
    "# 🗺️ Ferrua roadmap", "",
    "**Source of truth:** [progress.json](progress.json). Progress is evidence-backed atomic acceptance checks averaged by phase, milestone and feature. No unverified CI claim reaches 100%.",
    "",
    string.format("**Checklist coverage (not work completion):** `%s %d%%` across %d tracked tasks. This is not an estimate of engineering effort, time remaining, or Lua compatibility.",
        bar(mean(tasks)), mean(tasks), #tasks), "",
    string.format("**Delivery status:** **%s** — %d incomplete gate tasks, %d unaccepted checks: %s. These are acceptance counts, not an estimate of remaining engineering effort.",
        readiness, #incomplete, unchecked, table.concat(incomplete, ", ")), ""
}
for _, milestone in ipairs(ledger.milestones) do
    local m_tasks = {}
    for _, phase in ipairs(milestone.phases) do
        for _, task in ipairs(phase.tasks) do m_tasks[#m_tasks + 1] = task end
    end
    lines[#lines + 1] = string.format("## %s %s — %s — `%s %d%%`",
        milestone.emoji, milestone.id, milestone.name, bar(mean(m_tasks)), mean(m_tasks))
    lines[#lines + 1] = ""
    lines[#lines + 1] = "**Acceptance gate:** " .. milestone.gate
    lines[#lines + 1] = ""
    for _, phase in ipairs(milestone.phases) do
        lines[#lines + 1] = string.format("### %s %s — %s — `%s %d%%`",
            phase.emoji, phase.id, phase.name, bar(mean(phase.tasks)), mean(phase.tasks))
        lines[#lines + 1] = ""
        for _, task in ipairs(phase.tasks) do
            lines[#lines + 1] = string.format("- %s **%s** %s — `%s %d%%`",
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
        lines[#lines + 1] = string.format("- %s %s — `%s %d%%`",
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
