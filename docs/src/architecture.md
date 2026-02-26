# Architecture

## Core Loop

`cmd_start()` → spawn `cryo daemon` → event loop: spawn agent → listen on socket server for IPC commands → sleep until wake time or inbox event → run session → ...

## Binaries

| Binary | Purpose |
|--------|---------|
| `cryo` | Operator CLI — `init`, `start`, `status`, `cancel`, `log`, `watch`, `send`, `receive`, `wake`, `ps`, `restart`, `daemon` |
| `cryo-agent` | Agent IPC CLI — `hibernate`, `note`, `send`, `receive`, `alert`, `time` (sends commands to daemon via socket; `receive` and `time` are local) |
| `cryo-gh` | GitHub sync CLI — `init`, `pull`, `push`, `sync`, `unsync`, `status` (manages Discussion-based messaging via OS service) |

## Modules

| Module | Purpose |
|--------|---------|
| `socket` | Unix domain socket IPC — message types (`Request`/`Response`), client (`send_request`), server (`SocketServer`). |
| `config` | TOML persistence for project config (`cryo.toml`). `CryoConfig` struct, load/save, `apply_overrides` merges CLI overrides from state. |
| `state` | JSON persistence to `timer.json` — runtime-only state (session number, PID lock, CLI overrides). PID-based locking via `libc::kill(pid, 0)`. |
| `log` | Session log manager. Sessions delimited by `--- CRYO SESSION N ---` / `--- CRYO END ---`. `EventLogger` writes timestamped events (agent start, notes, hibernate, exit). |
| `protocol` | Loads templates from `templates/` via `include_str!` (protocol, plan, cryo.toml). Written by `init`/`start`. |
| `agent` | Builds lightweight prompt with task + session context, spawns agent subprocess (stdout/stderr redirected to `cryo-agent.log`). |
| `process` | Process management utilities: `send_signal`, `terminate_pid`, `spawn_daemon`. |
| `daemon` | Persistent event loop: socket server for agent IPC, watches `messages/inbox/` via `notify`, handles SIGUSR1 for forced wake, enforces session timeout, `EventLogger` for structured logs, retries with backoff (5s/15s/60s), executes fallback actions on deadline, and detects delayed wakes (e.g. after machine suspend). |
| `message` | File-based inbox/outbox message system. Inbox messages included in agent prompt on wake. |
| `fallback` | Dead-man switch: writes alerts to `messages/outbox/` for external delivery. |
| `channel` | Channel abstraction. Submodules: `file` (local inbox/outbox), `github` (Discussions via GraphQL). |
| `registry` | PID file registry for tracking running daemons. Uses `$XDG_RUNTIME_DIR/cryo/` (fallback `~/.cryo/daemons/`). Auto-cleans stale entries. |
| `service` | OS service management: install/uninstall launchd (macOS) or systemd (Linux) user services. Used by `cryo start` and `cryo-gh sync` for reboot-persistent daemons. `CRYO_NO_SERVICE=1` disables (falls back to direct spawn). |
| `web` | Axum-based web server with chat UI, REST API, and SSE for real-time updates. |
| `gh_sync` | GitHub Discussion sync state persistence (`gh-sync.json`). |

## Key Design Decisions

- **Daemon mode**: `cryo start` installs an OS service (launchd on macOS, systemd on Linux) that survives reboots. The daemon sleeps until the scheduled wake time, watches `messages/inbox/` for reactive wake, and enforces session timeout. Set `CRYO_NO_SERVICE=1` to fall back to direct background process spawn.
- **Socket-based IPC**: The agent communicates with the daemon via `cryo-agent` CLI subcommands (`hibernate`, `note`, `send`, `alert`), which send JSON messages over a Unix domain socket. `receive` and `time` are local (no daemon needed).
- **Fire-and-forget agent**: The daemon spawns the agent and redirects its stdout/stderr to `cryo-agent.log`. All structured communication flows through the socket.
- **SIGUSR1 wake**: `cryo wake` and `cryo send --wake` send SIGUSR1 to the daemon PID, which works regardless of `watch_inbox` setting. The daemon's signal-forwarding thread converts this into an `InboxChanged` event.
- **Config/state split**: `cryo.toml` is the project config (agent, retries, timeout, watch_inbox) created by `cryo init`. `timer.json` is runtime-only state (session number, PID, retry count, CLI overrides). CLI flags to `cryo start` are stored as optional overrides in `timer.json`.
- **Graceful degradation**: If the agent exits without calling `cryo-agent hibernate`, the daemon treats it as a crash and retries with backoff. EventLogger is always finalized even on error.
- **Default agent**: The CLI defaults to `opencode run` as the agent command (headless mode, not the TUI).

## Files Created at Runtime

| File | Purpose |
|------|---------|
| `timer.json` | Runtime state (session number, PID lock, retry count, CLI overrides) |
| `cryo.log` | Append-only structured event log |
| `cryo-agent.log` | Agent stdout/stderr (raw tool-call output) |
| `messages/inbox/` | Incoming messages for the agent |
| `messages/outbox/` | Outgoing messages (fallback alerts) |
| `messages/inbox/archive/` | Processed inbox messages |
| `.cryo/cryo.sock` | Unix domain socket for agent-daemon IPC |
| `gh-sync.json` | GitHub Discussion sync state (if configured) |
| `cryo-gh-sync.log` | GitHub sync daemon log output (if configured) |
