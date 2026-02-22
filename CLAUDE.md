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
| `marker` | Regex-based parser for `[CRYO:*]` markers. Has its own `FallbackAction` type (separate from `fallback::FallbackAction`). |
| `state` | JSON persistence to `timer.json` with PID-based locking via `libc::kill(pid, 0)`. |
| `log` | Session log manager. Sessions delimited by `--- CRYO SESSION N ---` / `--- CRYO END ---`. |
| `agent` | Builds prompt from plan + last session log, spawns agent subprocess. |
| `validate` | Pre-hibernate checks: requires EXIT marker + WAKE time (unless plan complete). Refuses to hibernate on failure. |
| `timer` | `CryoTimer` trait with `launchd` (plist + `launchctl`) and `systemd` (unit files + `systemctl --user`) implementations. Platform selected at compile time via `cfg!(target_os)`. |
| `fallback` | Email (via `lettre`) and webhook execution for dead-man switch alerts. |

### Key Design Decisions

- **Two `FallbackAction` types**: `marker::FallbackAction` (parsed from output) and `fallback::FallbackAction` (execution). They are structurally identical but not the same type — fields are manually copied in `main.rs`.
- **Graceful degradation**: Validation failures prevent hibernation rather than risking silent failures.
- **Default agent**: The CLI defaults to `opencode` as the agent command (not `claude`).

### Files Created at Runtime

- `timer.json` — serialized `CryoState` (plan path, session number, timer IDs, PID lock)
- `cryo.log` — append-only session log
- `plan.md` — copy of the plan file in the working directory

## Commit Convention

Conventional commits: `feat:`, `test:`, `docs:`, `chore:`, `fix:`
