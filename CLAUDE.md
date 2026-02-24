# CLAUDE.md

Developer guidance for Claude Code (claude.ai/code) when working on this repository.
For project overview and usage, see `README.md`.

## Build & Test

```bash
cargo build                          # build
cargo test                           # run all tests
cargo test daemon::tests             # run a single test module
cargo test test_event_logger         # run a single test by name
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
make cli         # cargo install --path .
make run-plan    # execute a plan with Claude headless (see Makefile for options)
```

## Architecture

### Core Loop

`cmd_start()` → spawn `cryo daemon` → event loop: spawn agent → listen on socket server for IPC commands → sleep until wake time or inbox event → run session → ...

### Modules

| Module | Purpose |
|--------|---------|
| `socket` | Unix domain socket IPC — message types (`Request`/`Response`), client (`send_request`), server (`SocketServer`). |
| `state` | JSON persistence to `timer.json` with PID-based locking via `libc::kill(pid, 0)`. |
| `log` | Session log manager. Sessions delimited by `--- CRYO SESSION N ---` / `--- CRYO END ---`. `EventLogger` writes timestamped events (agent start, notes, hibernate, exit). |
| `protocol` | Static protocol text with CLI commands, agent Makefile template, and template plan. Written by `init`/`start`. |
| `agent` | Builds lightweight prompt with task + session context, spawns agent subprocess (fire-and-forget, no stdout capture). |
| `session` | Pure utility: `should_copy_plan` checks whether to copy the plan file. |
| `daemon` | Persistent event loop: socket server for agent IPC, watches `messages/inbox/` via `notify`, enforces session timeout, `EventLogger` for structured logs, retries with backoff (5s/15s/60s), and executes fallback actions on deadline. |
| `message` | File-based inbox/outbox message system. Inbox messages included in agent prompt on wake. |
| `fallback` | Dead-man switch: writes alerts to `messages/outbox/` for external delivery. |
| `channel` | Channel abstraction. Submodules: `file` (local inbox/outbox), `github` (Discussions via GraphQL). |
| `registry` | PID file registry for tracking running daemons. Uses `$XDG_RUNTIME_DIR/cryo/` (fallback `~/.cryo/daemons/`). Auto-cleans stale entries. |
| `gh_sync` | GitHub Discussion sync state persistence (`gh-sync.json`). |

### Key Design Decisions

- **Daemon mode**: `cryo start` launches a persistent background process. The daemon sleeps until the scheduled wake time, watches `messages/inbox/` for reactive wake, and enforces session timeout.
- **Socket-based IPC**: The agent communicates with the daemon via `cryo` CLI subcommands (`hibernate`, `note`, `reply`, `alert`), which send JSON messages over a Unix domain socket. This replaces the old stdout marker-parsing approach.
- **Fire-and-forget agent**: The daemon spawns the agent without capturing stdout/stderr. All structured communication flows through the socket.
- **Preflight validation**: `cryo start` checks that the agent command exists on PATH before spawning.
- **Graceful degradation**: If the agent exits without calling `cryo hibernate`, the daemon treats it as a crash and retries with backoff. EventLogger is always finalized even on error.
- **Default agent**: The CLI defaults to `opencode run` as the agent command (headless mode, not the TUI).

### Files Created at Runtime (per project directory)

- `timer.json` — serialized `CryoState` (plan path, session number, PID lock, daemon mode, session timeout, retry config)
- `cryo.log` — append-only session log
- `plan.md` — copy of the plan file in the working directory
- `Makefile` — agent utility targets (`make time`, etc.)
- `CLAUDE.md` or `AGENTS.md` — cryochamber protocol for the agent
- `messages/inbox/` — incoming messages for the agent
- `messages/outbox/` — outgoing messages (fallback alerts)
- `messages/inbox/archive/` — processed inbox messages
- `.cryo/cryo.sock` — Unix domain socket for agent-daemon IPC
- `gh-sync.json` — GitHub Discussion sync state (if configured)

## Documentation

- `README.md` — Project overview, quickstart, CLI commands, and admin CLI
- `cryo-skill.md` — Cryochamber skill definition for AI agents
- `docs/plans/` — Design documents and implementation plans
- `docs/reports/` — Code review reports
- `examples/` — Showcase examples (chess-by-mail, conference-chair, mars-mission)

## Commit Convention

Conventional commits: `feat:`, `test:`, `docs:`, `chore:`, `fix:`
