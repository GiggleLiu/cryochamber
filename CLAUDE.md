# CLAUDE.md

Developer guidance for Claude Code (claude.ai/code) when working on this repository.
For project overview and usage, see `README.md`.

## Build & Test

```bash
cargo build                          # build
cargo test                           # run all tests
cargo test marker_tests              # run a single test module
cargo test test_parse_exit_marker    # run a single test by name
cargo fmt --all                      # format
cargo clippy --all-targets -- -D warnings  # lint (warnings are errors)
```

## Make Targets

```bash
make check       # fmt-check + clippy + test in sequence
make build       # cargo build
make test        # cargo test
make fmt         # cargo fmt
make clippy      # cargo clippy (warnings are errors)
make coverage    # generate coverage report (auto-installs cargo-llvm-cov)
make logo        # compile logo with typst
make chess       # build and run the chess-by-mail example
make run-plan    # execute a plan with Claude headless (see Makefile for options)
```

## Architecture

### Core Loop

`cmd_start()` → `run_session()` → parse markers → validate → schedule timer → hibernate → (OS timer fires) → `cmd_wake()` → `run_session()` → ...

### Modules

| Module | Purpose |
|--------|---------|
| `marker` | Regex-based parser for `[CRYO:*]` markers. Uses `fallback::FallbackAction` for parsed fallback data. |
| `state` | JSON persistence to `timer.json` with PID-based locking via `libc::kill(pid, 0)`. |
| `log` | Session log manager. Sessions delimited by `--- CRYO SESSION N ---` / `--- CRYO END ---`. |
| `protocol` | Static protocol text, agent Makefile template, and template plan. Written by `init`/`start`. |
| `agent` | Builds lightweight prompt with task + session context, spawns agent subprocess. |
| `validate` | Pre-hibernate checks: requires EXIT marker + WAKE time (unless plan complete). |
| `timer` | `CryoTimer` trait with `launchd` and `systemd` implementations. Platform selected via `cfg!(target_os)`. |
| `message` | File-based inbox/outbox message system. Inbox messages included in agent prompt on wake. |
| `fallback` | Dead-man switch: writes alerts to `messages/outbox/` for external delivery. |
| `channel` | Channel abstraction. Submodules: `file` (local inbox/outbox), `github` (Discussions via GraphQL). |
| `gh_sync` | GitHub Discussion sync state persistence (`gh-sync.json`). |

### Key Design Decisions

- **Agent never calls `cryo`**: The agent communicates via stdout markers only. Time utilities are exposed through `make time` in a per-project Makefile.
- **Graceful degradation**: Validation failures prevent hibernation rather than risking silent failures.
- **Default agent**: The CLI defaults to `opencode` as the agent command (not `claude`).

### Files Created at Runtime (per project directory)

- `timer.json` — serialized `CryoState` (plan path, session number, timer IDs, PID lock)
- `cryo.log` — append-only session log
- `plan.md` — copy of the plan file in the working directory
- `Makefile` — agent utility targets (`make time`, etc.)
- `CLAUDE.md` or `AGENTS.md` — cryochamber protocol for the agent
- `messages/inbox/` — incoming messages for the agent
- `messages/outbox/` — outgoing messages (fallback alerts)
- `messages/inbox/archive/` — processed inbox messages
- `gh-sync.json` — GitHub Discussion sync state (if configured)

## Documentation

- `README.md` — Project overview, quickstart, markers, and admin CLI
- `cryo-skill.md` — Cryochamber skill definition for AI agents
- `docs/plans/` — Design documents and implementation plans
- `docs/reports/` — Code review reports
- `examples/` — Showcase examples (chess-by-mail, conference-chair, mars-mission)

## Commit Convention

Conventional commits: `feat:`, `test:`, `docs:`, `chore:`, `fix:`
