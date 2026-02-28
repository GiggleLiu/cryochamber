---
name: cryo-create
description: Use when the user wants to create a new cryochamber application, set up a scheduled agent task, or scaffold a cryo project. Also when user says "cryo init", "new cryo app", or asks about plan.md and cryo.toml setup
---

# Creating a Cryochamber Application

## Overview

Guide users through creating a cryo app via conversational Q&A. Three phases: brainstorm the plan, configure cryo.toml, validate everything works. Assumes cryo CLI is on PATH.

## Process

1. Brainstorm (Q1-Q10) -> Draft plan.md -> User approves (or revise)
2. Generate cryo.toml -> User approves (or revise)
3. Validate: Files -> Tools -> Smoke test -> Ready

## Phase 1: Brainstorm the Plan

Ask questions **one at a time**. Suggest answers based on the task. Multiple choice where possible.

**Q1. What's the task?** Open-ended: "What should the agent do each session?"

**Q2. Schedule pattern.** Suggest based on Q1:
- **Periodic** — every N minutes/hours (monitoring, scraping)
- **Event-driven** — react to inbox (human input)
- **Adaptive** — adjust pace based on activity (games, polling)
- **Interactive schedule table** — co-create a multi-step schedule row by row (conferences, editorial calendars)

**Q3. External tools.** Suggest likely tools based on task. Skip if clearly none needed.

**Q4. Human interaction.** Recommend based on task: No interaction (autonomous) | One-way (agent sends reports, recommended) | Two-way (human sends data)

**Q5. Persistent state.** What to remember across sessions? Suggest counters, progress, timestamps. Warn: `cryo-agent note` is the only cross-session memory.

**Q6. Failure & retry.** Suggest `max_retries` (default 5, retries continue every 60s after exhaustion) and `max_session_duration` (default 0 is dangerous — suggest 60-300s).

**Q7. AI agent & providers.** Agent command: opencode (default) | claude | codex | custom. Multiple API keys? Walk through `[[providers]]` entries + rotation strategy: `quick-exit` (recommended) | `any-failure` | `never`.

**Q8. Delayed wake.** If agent wakes 5+ min late, how to react? Suggest: Adjust and continue (recommended) | Alert human (time-sensitive) | Abort | Ignore.

**Q9. Sync channel.** Zulip (recommended, walk through zuliprc + stream) | GitHub Discussions (repo + category) | Web UI (`cryo web`, pick port) | None.

**Q10. Periodic reports.** Daily summaries? Set `report_time` + `report_interval`, or skip.

### Output

Draft `plan.md` with Goal, Tasks, Configuration, Notes. Include schedule table if applicable, delayed wake handling in Tasks, persistent state in Notes. Present for approval, then write.

Reference: `examples/mr-lazy/plan.md` (periodic), `examples/chess-by-mail/plan.md` (adaptive).

## Phase 2: Configure cryo.toml

Map Phase 1 answers directly — no new questions.

| Answer | cryo.toml field |
|---|---|
| Agent (Q7) | `agent` |
| Retry (Q6) | `max_retries`, `max_session_duration` |
| Interaction (Q4) | `watch_inbox` (two-way -> true) |
| Channel (Q9) | `web_host`, `web_port` |
| Reports (Q10) | `report_time`, `report_interval` |
| Providers (Q7) | `rotate_on`, `[[providers]]` |
| Delayed wake (Q8) | in plan.md, not cryo.toml |

Present config with commented explanations. Note if sync init needed in Phase 3. Write the file.

## Phase 3: Validate

Three layers, in order. On failure: stop, suggest fixes, retry that layer.

1. **Files** — plan.md has Goal + Tasks, cryo.toml parses, agent on PATH
2. **Tools** — smoke test each external tool in plan.md (scripts run, APIs reachable, env vars set, provider keys valid)
3. **Smoke test** — `cryo init && cryo start`, wait for first session (check cryo.log), init sync channel if configured, `cryo cancel`

On success: "Your cryo application is ready. Run `cryo start` to begin."

## Common Mistakes

| Mistake | Fix |
|---|---|
| No `max_session_duration` — hangs forever | Set timeout (60-300s) |
| Hardcoded timestamps | Use `cryo-agent time "+N minutes"` |
| No persistent state — forgets everything | Use `cryo-agent note` |
| Missing hibernation — treated as crash | Every path ends with `cryo-agent hibernate` |
| `watch_inbox = false` with two-way | Set `watch_inbox = true` |
| Provider env vars not set | Validate in Phase 3 |
