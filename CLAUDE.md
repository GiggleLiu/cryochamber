# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is Cryochamber

Cryochamber is a long-term AI agent task scheduler. It runs an AI agent session, parses structured `[CRYO:*]` markers from the agent's output, then uses OS-level timers (launchd on macOS, systemd on Linux) to hibernate and wake the agent at a future time. This creates a daemon + agent architecture where the daemon orchestrates multi-session plans over hours or days.

## Build & Test Commands

```bash
cargo build                          # build
cargo test                           # run all tests
cargo test marker_tests              # run a single test module
cargo test test_parse_exit_marker    # run a single test by name
cargo fmt --all                      # format
cargo clippy --all-targets -- -D warnings  # lint (warnings are errors)
make check                           # fmt-check + clippy + test in sequence
```

## Architecture

### Core Loop

`cmd_start()` → `run_session()` → parse markers → validate → schedule timer → hibernate → (OS timer fires) → `cmd_wake()` → `run_session()` → ...

### Log-Based Protocol

Agent and daemon communicate through `[CRYO:*]` markers embedded in the agent's stdout, written to `cryo.log`:

- `[CRYO:EXIT 0|1|2]` — session result (Success/Partial/Failure)
- `[CRYO:WAKE 2026-02-23T09:00]` — next wake time
- `[CRYO:CMD ...]` — next task instruction
- `[CRYO:PLAN ...]` — plan progress note
- `[CRYO:FALLBACK email|webhook target "message"]` — dead-man switch

### Modules

| Module | Purpose |
|--------|---------|
| `marker` | Regex-based parser for `[CRYO:*]` markers. Uses `fallback::FallbackAction` for parsed fallback data. |
| `state` | JSON persistence to `timer.json` with PID-based locking via `libc::kill(pid, 0)`. |
| `log` | Session log manager. Sessions delimited by `--- CRYO SESSION N ---` / `--- CRYO END ---`. |
| `protocol` | Static protocol text for agent `CLAUDE.md`/`AGENTS.md`. Contains `PROTOCOL_CONTENT` constant and helpers for `init`/`start`. |
| `agent` | Builds lightweight prompt with task + session context, spawns agent subprocess. Agent reads `plan.md` and protocol file directly. |
| `validate` | Pre-hibernate checks: requires EXIT marker + WAKE time (unless plan complete). Refuses to hibernate on failure. |
| `timer` | `CryoTimer` trait with `launchd` (plist + `launchctl`) and `systemd` (unit files + `systemctl --user`) implementations. Platform selected at compile time via `cfg!(target_os)`. |
| `message` | File-based inbox/outbox message system. Inbox messages are read on wake and included in agent prompt; fallback alerts are written to outbox. |
| `fallback` | Dead-man switch: writes alerts to `messages/outbox/` for external runners to deliver. |

### Key Design Decisions

- **Graceful degradation**: Validation failures prevent hibernation rather than risking silent failures.
- **Default agent**: The CLI defaults to `opencode` as the agent command (not `claude`).

### Files Created at Runtime

- `timer.json` — serialized `CryoState` (plan path, session number, timer IDs, PID lock)
- `cryo.log` — append-only session log
- `plan.md` — copy of the plan file in the working directory
- `CLAUDE.md` or `AGENTS.md` — cryochamber protocol for the agent (written by `init` or auto-created by `start`)
- `messages/inbox/` — incoming messages for the agent (from humans, bots, webhooks)
- `messages/outbox/` — outgoing messages (fallback alerts, status updates)
- `messages/inbox/archive/` — processed inbox messages

## Commit Convention

Conventional commits: `feat:`, `test:`, `docs:`, `chore:`, `fix:`
