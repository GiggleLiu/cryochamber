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
make example        # run an example (DIR=examples/chambers/mr-lazy or .../chess-by-mail)
make example-cancel # stop a running example (DIR=examples/chambers/...)
make example-hub    # start cryohub over examples/chambers/ (PORT=8765)
make example-clean  # remove auto-generated files from all examples
make run-plan       # execute a plan with Codex by default (RUNNER=claude for Claude)
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
| `cryo` | Operator CLI — `init`, `start`, `status`, `cancel`, `log`, `watch`, `send`, `receive`, `wake`, `ps`, `restart`, `daemon` |
| `cryo-agent` | Agent IPC CLI — `hibernate`, `send`, `reply`, `receive`, `alert`, `time`, `todo` (most commands send requests to the daemon via socket; `receive` and `time` are local) |
| `cryo-gh` | GitHub sync CLI — `init`, `pull`, `push`, `sync`, `unsync`, `status` (manages Discussion-based messaging via OS service) |
| `cryo-zulip` | Zulip sync CLI — `init`, `pull`, `push`, `sync`, `unsync`, `status` (manages Zulip stream messaging via OS service) |
| `cryohub` | Workspace-wide web dashboard — `start`, `stop`, `status`, `daemon` (installs a launchd/systemd service that serves the hub UI over HTTP). |

### Modules

| Module | Purpose |
|--------|---------|
| `socket` | Unix domain socket IPC — message types (`Request`/`Response`), client (`send_request`), server (`SocketServer`). |
| `config` | TOML persistence for project config (`cryo.toml`). `CryoConfig` struct, load/save, `apply_overrides` merges CLI overrides from state. |
| `state` | JSON persistence to `timer.json` — runtime-only state (session number, PID lock, CLI overrides). PID-based locking via `libc::kill(pid, 0)`. |
| `log` | Session log manager. Sessions delimited by `--- CRYO SESSION N ---` / `--- CRYO END ---`. `EventLogger` writes timestamped events (agent start, hibernate, exit). |
| `protocol` | Loads templates from `templates/` via `include_str!` (protocol, plan, cryo.toml). Written by `init`/`start`. |
| `agent` | Builds lightweight prompt with task + session context, spawns agent subprocess (stdout/stderr redirected to `cryo-agent.log`). |
| `process` | Process management utilities: `send_signal`, `terminate_pid`, `spawn_daemon`. |
| `session` | Legacy utility module (`should_copy_plan`). Currently unused — plan.md must exist in the working directory. |
| `daemon` | Persistent event loop: socket server for agent IPC, watches `messages/inbox/` via `notify`, handles SIGUSR1 for forced wake, enforces session timeout, `EventLogger` for structured logs, consumes past-due TODOs before each session, re-injects them with a `(attempt k)` suffix and `2^k`-minute delay (capped at 1 day) on crash, and detects delayed wakes (e.g. after machine suspend). It notices when inbox messages exist but never previews or archives them. |
| `message` | File-based inbox/outbox message system. `cryo-agent receive` reads and archives inbox messages into `messages/inbox/archive/`; the daemon only checks whether inbox files exist. |
| `channel` | Channel abstraction. Submodules: `file` (local inbox/outbox), `github` (Discussions via GraphQL), `zulip` (Zulip REST API). |
| `registry` | PID file registry for tracking running daemons. Uses `$XDG_RUNTIME_DIR/cryo/` (fallback `~/.cryo/daemons/`). Auto-cleans stale entries. |
| `report` | Periodic session summary reports. Parses log, counts sessions/failures, writes summary to `messages/outbox/` for sync delivery. |
| `service` | OS service management: install/uninstall launchd (macOS) or systemd (Linux) user services. Used by `cryo start` and `cryo-gh sync` for reboot-persistent daemons. `CRYO_NO_SERVICE=1` disables (falls back to direct spawn). |
| `gh_sync` | GitHub Discussion sync state persistence (`gh-sync.json`). |
| `todo` | Per-project TODO list persistence (`todo.json`). `TodoItem`/`TodoList` structs, load/save, add/done/remove. Mutated through daemon IPC so scheduling changes are serialized with the session lifecycle. |
| `zulip_sync` | Zulip sync state persistence (`zulip-sync.json`). |
| `hub` | Workspace-wide web dashboard: Axum router (`serve`, `build_router_with_state`), chamber discovery, SSE events, start/stop/restart handlers. Served by the `cryohub` binary. |

### Key Design Decisions

- **Daemon mode**: `cryo start` installs an OS service (launchd on macOS, systemd on Linux) that survives reboots. The daemon sleeps until the scheduled wake time, watches `messages/inbox/` for reactive wake, and enforces session timeout. Set `CRYO_NO_SERVICE=1` to fall back to direct background process spawn.
- **Socket-based IPC**: The agent communicates with the daemon via `cryo-agent` CLI subcommands (`hibernate`, `send`, `alert`, `todo`), which send JSON messages over a Unix domain socket. `receive` and `time` are local (no daemon needed).
- **Fire-and-forget agent**: The daemon spawns the agent and redirects its stdout/stderr to `cryo-agent.log`. Stdout/stderr are diagnostic logs, not a human communication channel. All structured communication flows through `cryo-agent`.
- **SIGUSR1 wake**: `cryo wake` and `cryo send --wake` send SIGUSR1 to the daemon PID, which works regardless of `watch_inbox` setting. The daemon's signal-forwarding thread converts this into an `InboxChanged` event.
- **Config/state split**: `cryo.toml` is the project config (agent, session timeout, watch_inbox, report interval, provider rotation) created by `cryo init`. `timer.json` is runtime-only state (session number, PID, CLI overrides). CLI flags to `cryo start` are stored as optional overrides in `timer.json`.
- **Chamber-authored messages**: All daemon-originated outbox messages use a single `from: cryochamber` sender — both per-session stand-in replies (when the agent exited without sending anything) and periodic reports. Agent-authored replies use `from: agent`.
- **Preflight validation**: `cryo start` checks that the agent command exists on PATH before spawning.
- **Crash handling via TODO re-injection**: If the agent exits without calling `cryo-agent hibernate`, the daemon records the crash and re-injects any TODOs it consumed for that wake with a `(attempt k)` suffix and an exponential delay (`2^k` min, capped at 1 day). There is no in-daemon backoff-retry loop; rescheduling lives entirely in `todo.json`, surviving daemon restarts and visible to both agent and operator. EventLogger is always finalized.
- **Daemon does not preview inbox, agent receives it**: Wake prompts do not include inbox contents. The daemon only checks whether inbox files exist so it can surface a notice in the session prompt. Only `cryo-agent receive` moves messages into `messages/inbox/archive/`. Crashed sessions therefore leave the inbox intact — no "check archive" recovery path is needed.
- **Default agent**: The CLI defaults to `opencode` as the agent command (headless mode, not the TUI).
- **Agent notes via `NOTES.md`**: The agent's persistent memory across sessions is a plain markdown file (`NOTES.md`) the agent reads and writes directly — no IPC roundtrip. Seeded by `cryo init`, surfaced in the hub's Notes drawer tab, and updated by the agent on its own. The removed `cryo-agent note` subcommand and `Request::Note` IPC variant are historical.
- **`cryo-agent time` input grammar**: Accepts three forms only — empty (current time), `+N minutes|hours|days|weeks` (relative offset), and ISO8601 (`2026-04-25T10:00` or date-only) as validated pass-through. Natural-language parsing is deliberately **not** supported: the agent is an LLM that can reason about "tomorrow 9am" itself, so the tool stays small and documentable. Unknown input prints the accepted forms.
- **`cryohub` is cwd-scoped**: The `cryohub` binary (not `cryo`) runs the web dashboard and always operates on the current directory — there is no `--dir` flag. The cwd must not itself be a chamber (no `cryo.toml` directly inside). Discovery scans `<cwd>/*` for chamber subdirectories. Host and port are CLI flags (`--host`, `--port`, defaults `127.0.0.1:8765`), not `cryo.toml` fields. The service label is `"hub"` (plist/unit `com.cryo.hub.<hash>`), and the log file is `cryohub.log`. `cryohub status`/`stop` operate on the cwd's service and additionally list every other `com.cryo.hub.*` service installed on the machine (via `service::list_installed`) so users can find services started from a different cwd. Per-chamber `web_host`/`web_port` fields are not part of `CryoConfig`.

### Files Created by `cryo init`

- `cryo.toml` — project configuration (agent, max_session_duration, watch_inbox)
- `CLAUDE.md` or `AGENTS.md` — cryochamber protocol for the agent
- `plan.md` — template plan file
- `NOTES.md` — agent's persistent memory across sessions (seeded from `templates/notes.md`; agent reads/writes directly)
- `README.md` — quickstart guide for the project (service commands, messaging channels)

### Files Created at Runtime (per project directory)

- `timer.json` — runtime state only (session number, PID lock, CLI overrides)
- `cryo.log` — append-only structured event log
- `cryo-agent.log` — agent stdout/stderr (raw tool-call output)
- `todo.json` — per-project TODO items for agent task tracking
- `messages/inbox/` — incoming messages for the agent
- `messages/outbox/` — outgoing messages (agent replies, daemon stand-in replies, periodic reports)
- `messages/inbox/archive/` — processed inbox messages
- `.cryo/cryo.sock` — Unix domain socket for agent-daemon IPC
- `gh-sync.json` — GitHub Discussion sync state (if configured)
- `cryo-gh-sync.log` — GitHub sync daemon log output (if configured)
- `zulip-sync.json` — Zulip sync state (if configured)
- `.cryo/zuliprc` — Zulip credentials copied from user's zuliprc (if configured). **Never sync, commit, or push this file** — it holds API credentials. Already gitignored; sync channels (`cryo-gh`, `cryo-zulip`) must never include it in any payload.
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
- `examples/` — Showcase examples. `chambers/` holds runnable chambers (e.g. `mr-lazy`, `chess-by-mail`, `personal-assistant`).

## Skills

- `.claude/skills/make-plan/SKILL.md` — Claude Code skill that guides users through creating a new cryochamber application (plan.md + cryo.toml) via conversational Q&A. Install with `claude skill install --path .claude/skills/make-plan`, invoke with `/make-plan`. Additional repo-local skills (`fix-pr`, `review-implementation`) live alongside it.

## Commit Convention

Conventional commits: `feat:`, `test:`, `docs:`, `chore:`, `fix:`

Do not commit implementation plans. Design documents should only be committed when they contain a key design decision.
