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
make check          # fmt-check + clippy + test in sequence
make build          # cargo build
make test           # cargo test
make fmt            # cargo fmt
make clippy         # cargo clippy (warnings are errors)
make coverage       # generate coverage report (auto-installs cargo-llvm-cov)
make cli            # cargo install --path .
make logo           # compile logo with typst
make example        # run an example (DIR=examples/mr-lazy or DIR=examples/chess-by-mail)
make example-cancel # stop a running example (DIR=examples/...)
make example-clean  # remove auto-generated files from all examples
make run-plan       # execute a plan with Claude headless (see Makefile for options)
make check-agent    # quick agent smoke test (AGENT=opencode|claude)
make check-round-trip # full round-trip test with mr-lazy
make check-gh       # verify GitHub Discussion sync (REPO=owner/repo)
make check-service  # verify OS service install/uninstall lifecycle (launchd/systemd)
make check-mock     # run mock agent integration tests (no external agent required)
make book           # build mdbook documentation (auto-installs mdbook)
make book-serve     # serve mdbook locally with live reload
make book-deploy    # deploy mdbook to GitHub Pages (gh-pages branch)
make copilot-review # request Copilot code review on current PR
make release V=x.y.z # tag and push a release (triggers CI publish to crates.io)
```

## Architecture

### Core Loop

`cmd_start()` → spawn `cryo daemon` → event loop: spawn agent → listen on socket server for IPC commands → sleep until wake time or inbox event → run session → ...

### Binaries

| Binary | Purpose |
|--------|---------|
| `cryo` | Operator CLI — `init`, `start`, `status`, `cancel`, `log`, `watch`, `send`, `receive`, `wake`, `ps`, `restart`, `web`, `daemon` |
| `cryo-agent` | Agent IPC CLI — `hibernate`, `note`, `send`, `reply`, `receive`, `alert`, `time`, `todo` (sends commands to daemon via socket; `receive`, `time`, and `todo` are local) |
| `cryo-gh` | GitHub sync CLI — `init`, `pull`, `push`, `sync`, `unsync`, `status` (manages Discussion-based messaging via OS service) |
| `cryo-zulip` | Zulip sync CLI — `init`, `pull`, `push`, `sync`, `unsync`, `status` (manages Zulip stream messaging via OS service) |

### Modules

| Module | Purpose |
|--------|---------|
| `socket` | Unix domain socket IPC — message types (`Request`/`Response`), client (`send_request`), server (`SocketServer`). |
| `config` | TOML persistence for project config (`cryo.toml`). `CryoConfig` struct, load/save, `apply_overrides` merges CLI overrides from state. |
| `state` | JSON persistence to `timer.json` — runtime-only state (session number, PID lock, CLI overrides). PID-based locking via `libc::kill(pid, 0)`. |
| `log` | Session log manager. Sessions delimited by `--- CRYO SESSION N ---` / `--- CRYO END ---`. `EventLogger` writes timestamped events (agent start, notes, hibernate, exit). |
| `protocol` | Loads templates from `templates/` via `include_str!` (protocol, plan, cryo.toml). Written by `init`/`start`. |
| `agent` | Builds lightweight prompt with task + session context, spawns agent subprocess (stdout/stderr redirected to `cryo-agent.log`). |
| `process` | Process management utilities: `send_signal`, `terminate_pid`, `spawn_daemon`. |
| `session` | Legacy utility module (`should_copy_plan`). Currently unused — plan.md must exist in the working directory. |
| `daemon` | Persistent event loop: socket server for agent IPC, watches `messages/inbox/` via `notify`, handles SIGUSR1 for forced wake, enforces session timeout, `EventLogger` for structured logs, retries with backoff (5s/15s/60s), executes fallback actions on deadline, and detects delayed wakes (e.g. after machine suspend). |
| `message` | File-based inbox/outbox message system. Inbox messages included in agent prompt on wake. |
| `fallback` | Dead-man switch: writes alerts to `messages/outbox/` for external delivery. |
| `channel` | Channel abstraction. Submodules: `file` (local inbox/outbox), `github` (Discussions via GraphQL), `zulip` (Zulip REST API). |
| `registry` | PID file registry for tracking running daemons. Uses `$XDG_RUNTIME_DIR/cryo/` (fallback `~/.cryo/daemons/`). Auto-cleans stale entries. |
| `report` | Periodic session summary reports. Parses log, counts sessions/failures, sends desktop notification via notify-rust. |
| `service` | OS service management: install/uninstall launchd (macOS) or systemd (Linux) user services. Used by `cryo start` and `cryo-gh sync` for reboot-persistent daemons. `CRYO_NO_SERVICE=1` disables (falls back to direct spawn). |
| `gh_sync` | GitHub Discussion sync state persistence (`gh-sync.json`). |
| `todo` | Per-project TODO list persistence (`todo.json`). `TodoItem`/`TodoList` structs, load/save, add/done/remove. Local only (no daemon IPC). |
| `zulip_sync` | Zulip sync state persistence (`zulip-sync.json`). |

### Key Design Decisions

- **Daemon mode**: `cryo start` installs an OS service (launchd on macOS, systemd on Linux) that survives reboots. The daemon sleeps until the scheduled wake time, watches `messages/inbox/` for reactive wake, and enforces session timeout. Set `CRYO_NO_SERVICE=1` to fall back to direct background process spawn.
- **Socket-based IPC**: The agent communicates with the daemon via `cryo-agent` CLI subcommands (`hibernate`, `note`, `send`, `alert`), which send JSON messages over a Unix domain socket. `receive` and `time` are local (no daemon needed).
- **Fire-and-forget agent**: The daemon spawns the agent and redirects its stdout/stderr to `cryo-agent.log`. All structured communication flows through the socket.
- **SIGUSR1 wake**: `cryo wake` and `cryo send --wake` send SIGUSR1 to the daemon PID, which works regardless of `watch_inbox` setting. The daemon's signal-forwarding thread converts this into an `InboxChanged` event.
- **Config/state split**: `cryo.toml` is the project config (agent, retries, timeout, watch_inbox) created by `cryo init`. `timer.json` is runtime-only state (session number, PID, retry count, CLI overrides). CLI flags to `cryo start` are stored as optional overrides in `timer.json`.
- **Preflight validation**: `cryo start` checks that the agent command exists on PATH before spawning.
- **Graceful degradation**: If the agent exits without calling `cryo-agent hibernate`, the daemon treats it as a crash and retries with backoff. EventLogger is always finalized even on error.
- **Default agent**: The CLI defaults to `opencode run` as the agent command (headless mode, not the TUI).

### Files Created by `cryo init`

- `cryo.toml` — project configuration (agent, max_retries, max_session_duration, watch_inbox)
- `CLAUDE.md` or `AGENTS.md` — cryochamber protocol for the agent
- `plan.md` — template plan file
- `README.md` — quickstart guide for the project (service commands, messaging channels)

### Files Created at Runtime (per project directory)

- `timer.json` — runtime state only (session number, PID lock, retry count, CLI overrides)
- `cryo.log` — append-only structured event log
- `cryo-agent.log` — agent stdout/stderr (raw tool-call output)
- `todo.json` — per-project TODO items for agent task tracking
- `messages/inbox/` — incoming messages for the agent
- `messages/outbox/` — outgoing messages (fallback alerts)
- `messages/inbox/archive/` — processed inbox messages
- `.cryo/cryo.sock` — Unix domain socket for agent-daemon IPC
- `gh-sync.json` — GitHub Discussion sync state (if configured)
- `cryo-gh-sync.log` — GitHub sync daemon log output (if configured)
- `zulip-sync.json` — Zulip sync state (if configured)
- `.cryo/zuliprc` — Zulip credentials copied from user's zuliprc (if configured)
- `cryo-zulip-sync.log` — Zulip sync daemon log output (if configured)
- `~/Library/LaunchAgents/com.cryo.*.plist` — macOS launchd service files (auto-managed)
- `~/.config/systemd/user/com.cryo.*.service` — Linux systemd service files (auto-managed)

## Documentation

Main documentation lives in the mdbook at `docs/src/` (published to [giggleliu.github.io/cryochamber](https://giggleliu.github.io/cryochamber/)). Keep `README.md` lean — detailed guides belong in the mdbook.

- `README.md` — Project overview and quickstart only
- `docs/src/` — mdbook source: user guide, command reference, sync channels, examples, architecture
- `Makefile` — Dev targets (`check`, `build`, `test`, `run-plan`, `check-round-trip`, etc.)
- `templates/` — Single source of truth for agent protocol, template plan, and cryo.toml config template
- `docs/plans/` — Design documents (key design decisions only)
- `docs/reports/` — Code review reports
- `examples/` — Showcase examples (chess-by-mail, mr-lazy)

## Skills

- `skills/make-plan/SKILL.md` — Claude Code skill that guides users through creating a new cryochamber application (plan.md + cryo.toml) via conversational Q&A. Install with `claude skill install --path skills/make-plan`, invoke with `/make-plan`.

## Commit Convention

Conventional commits: `feat:`, `test:`, `docs:`, `chore:`, `fix:`

Do not commit implementation plans. Design documents should only be committed when they contain a key design decision.
