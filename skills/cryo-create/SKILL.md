---
name: cryo-create
description: Use when the user wants to create a new cryochamber application, set up a scheduled agent task, or scaffold a cryo project with plan.md and cryo.toml
---

# Creating a Cryochamber Application

## Overview

Guide users through creating a cryochamber application via conversational Q&A. Three phases: brainstorm the plan, configure cryo.toml, validate everything works.

Assumes cryo CLI is installed and on PATH.

## Phase 1: Brainstorm the Plan

Ask questions **one at a time**. Suggest answers based on the task. Multiple choice where possible.

### Q1. What's the task?

Open-ended: "What should the agent do each session?"

### Q2. Schedule pattern

Suggest based on Q1, then ask:
- **Periodic** (every N minutes/hours) — monitoring, scraping, reminders
- **Event-driven** (react to inbox messages) — responding to human input
- **Adaptive** (adjust pace based on activity) — correspondence games, polling
- **Interactive schedule table** — build a multi-step schedule collaboratively (e.g. "Day 1: send CFP, Day 7: remind reviewers, Day 14: collect results"). Good for conference organizing, editorial calendars, multi-phase projects. Co-create the table with the user row by row.

### Q3. External tools

Analyze the task and suggest likely tools (e.g. "this sounds like it needs a web scraper — would curl or a Python script work?"). **Skip if the task clearly needs no external tools.**

### Q4. Human interaction

Recommend based on task:
- **No interaction** (autonomous) — monitoring, automation
- **One-way** (agent sends reports) — scraping, summarization (Recommended for most tasks)
- **Two-way** (human sends commands/data) — games, collaborative planning

### Q5. Persistent state

What does the agent need to remember across sessions? Suggest based on task (counters, progress markers, data snapshots, timestamps). Warn: `cryo-agent note` is the **only** cross-session memory.

### Q6. Failure & retry strategy

What if the agent crashes or hangs? Suggest based on task:
- `max_retries`: how many retries (default 5). Note: after exhaustion, retries continue every 60s.
- `max_session_duration`: timeout in seconds. **Default 0 (no timeout) is dangerous for tasks that can hang.** Suggest 60–300s based on task complexity.

### Q7. AI agent & providers

Which agent command?
- **opencode** (default) — headless coding agent
- **claude** — Anthropic's Claude CLI
- **codex** — OpenAI's Codex CLI
- **custom** — any command

Then: do you have multiple API keys or providers to rotate between?
- If yes: walk through `[[providers]]` entries. Each needs a `name` and `env` map (e.g. `ANTHROPIC_API_KEY`). Suggest rotation strategy:
  - `quick-exit` (recommended) — rotate only on <5s exit (likely bad key)
  - `any-failure` — rotate on any crash
  - `never` (default)
- If no: skip, single provider is fine.

### Q8. Delayed wake reaction

If the machine was suspended and the agent wakes 5+ minutes late, how should it react? Suggest based on task:
- **Adjust and continue** (recommended for most) — recalculate deadlines, skip missed steps, catch up
- **Alert the human** — send a message about the delay, then proceed (recommended for time-sensitive tasks)
- **Abort the session** — exit with error, let human decide
- **Ignore** — treat as normal wake

### Q9. Notification & sync channel

How should the agent communicate with the user?
- **Zulip** (recommended) — rich web UI, bot support, persistent history. Walk through: zuliprc path, stream name, sync interval.
- **GitHub Discussions** — good for repo-centric workflows. Walk through: repo, discussion category.
- **Web UI only** — simplest, local browser via `cryo web`. Pick a port.
- **None** — agent runs silently, check logs manually.

### Q10. Periodic reports

Want daily/hourly health summaries as desktop notifications?
- If yes: set `report_time` (e.g. "09:00") and `report_interval` (hours, e.g. 24 for daily).
- If no: skip (disabled by default).

### Output

After all questions:
1. Draft `plan.md` with **Goal**, **Tasks**, **Configuration**, and **Notes** sections
2. For interactive schedule tables: embed the schedule as a markdown table in Tasks
3. Include delayed wake handling instructions in Tasks
4. Include persistent state strategy in Notes
5. Present draft to user for approval/edits
6. Write the file

Reference existing examples for plan.md structure:
- `examples/mr-lazy/plan.md` — simple periodic task
- `examples/chess-by-mail/plan.md` — adaptive event-driven task
