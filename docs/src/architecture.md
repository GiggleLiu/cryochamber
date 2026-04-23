# Architecture

## Core Loop

`cmd_start()` → spawn `cryo daemon` → event loop: spawn agent → listen on socket server for IPC commands → sleep until wake time or inbox event → run session → ...

## Binaries

| Binary | Purpose |
|--------|---------|
| `cryo` | Operator CLI — `init`, `start`, `status`, `cancel`, `log`, `watch`, `send`, `receive`, `wake`, `ps`, `restart`, `daemon`. |
| `cryo-agent` | Agent IPC CLI — `hibernate`, `send`, `reply`, `receive`, `time`, `todo`. Most commands send requests to the daemon via socket; `receive` and `time` are local. |
| `cryo-gh` | GitHub sync CLI — `init`, `pull`, `push`, `sync`, `unsync`, `status`. Manages Discussion-based messaging via an OS service. |
| `cryo-zulip` | Zulip sync CLI — `init`, `pull`, `push`, `sync`, `unsync`, `status`. Manages Zulip stream messaging via an OS service. |
| `cryohub` | Workspace-wide web dashboard — `start`, `stop`, `status`, `daemon`. Installs a launchd/systemd service that serves the hub UI over HTTP. |
| `cryo-mock` | Test-only mock agent for integration tests (`make check-mock`). |

## Modules

Modules live in `src/` and are re-exported via `lib.rs`. Entries list the module's purpose and a handful of representative types or functions — not an exhaustive API list. Read the source for full signatures.

### IPC and daemon lifecycle

| Module | Purpose | Key interfaces |
|--------|---------|----------------|
| `socket` | Unix domain socket IPC protocol. | `enum Request` (`Ping`, `Hello`, `Hibernate`, `Reply`, `TodoAdd/Done/Remove`, `TodoList`), `struct Response`, `fn socket_path`, `fn send_request`. |
| `daemon_client` | Thin CLI → daemon IPC wrapper. | `fn send_request`, `fn send_checked_request`, `fn daemon_responding`, `fn signal_daemon_wake`. |
| `daemon` | Persistent event loop: socket server, inbox `notify` watcher, SIGUSR1 wake, timeout enforcement, TODO consumption / attempt-based rescheduling on crash, delayed-wake detection. | `enum DaemonEvent`, `trait Clock`, `trait EventSource`, `async fn run`, `fn main_loop`. |
| `daemon::effects` | Session I/O abstraction (inbox reads, reply posting, TODO mutation). | `trait SessionEffects`, `enum ReplyAuthor`, `struct FsSessionEffects`. |
| `daemon::request` | Request parsing and hibernate-decision logic. | `enum DaemonRequest`, `enum TodoRequest`, `struct HibernateDecision`, `fn resolve_hibernate_request`. |
| `daemon::schedule` | Provider rotation and wake-time scheduling. | `struct RetryState`, `fn rotate_provider`, `fn next_wake_from_todos`. |
| `daemon::session` | Session runtime: process spawn, request/response loop, wait/terminate. | `trait SessionRuntime`, `struct ChildExitStatus`. |
| `lifecycle` | Session startup validation and chamber lifecycle operations. | `enum DaemonLaunchMode`, `struct StartOptions`, `struct PreparedStart`, `fn require_valid_project`, `fn require_live_daemon`, `fn prepare_start`, `fn validate_agent_command`. |
| `process` | Process management utilities. | `fn send_signal`, `fn terminate_pid`, `fn spawn_daemon`. |
| `registry` | PID file registry under `$XDG_RUNTIME_DIR/cryo/` (fallback `~/.cryo/daemons/`). Auto-cleans stale entries. | `struct DaemonEntry`, `fn register`, `fn unregister`, `fn list`. |
| `service` | OS service management: launchd (macOS) / systemd (Linux) user services. Used by `cryo start`, `cryo-gh sync`, `cryo-zulip sync`. `CRYO_NO_SERVICE=1` disables. | `struct InstalledService`, `fn service_label`, `fn install`, `fn uninstall`, `fn list_installed`, `fn is_installed`. |

### Config, state, and persistence

| Module | Purpose | Key interfaces |
|--------|---------|----------------|
| `config` | TOML project config (`cryo.toml`), with CLI overrides merged from runtime state. | `struct CryoConfig`, `struct ProviderConfig`, `enum RotateOn`, `fn load_config`, `fn save_config`. |
| `state` | JSON runtime state (`timer.json`): session number, PID lock, CLI overrides. PID-based locking via `libc::kill(pid, 0)`. | `struct CryoState`, `fn load_state`, `fn save_state`, `fn is_locked`. |
| `todo` | Per-project TODO list (`todo.json`); mutated through daemon IPC so scheduling changes serialize with the session lifecycle. Also owns the retry rescheduling logic used by the daemon when re-injecting consumed TODOs after a crash. | `struct TodoFile`, `struct TodoItem`, `fn add`, `fn done`, `fn remove`, `fn items`, `fn display`, `fn next_wake_time`, `fn next_valid_wake`, `fn consume_past_due`, `fn reschedule_consumed`. |
| `protocol` | Loads templates from `templates/` via `include_str!` and writes them into the project. | `enum ProtocolFile`, `fn protocol_filename`, `fn find_protocol_file`, `fn write_protocol_file`, `fn write_template_plan`, `fn write_config_file`. |

### Agent, logging, and chamber status

| Module | Purpose | Key interfaces |
|--------|---------|----------------|
| `agent` | Resolves the agent command and builds the per-session prompt. | `enum AgentKind`, `struct AgentConfig`, `fn agent_program`, `fn build_prompt`. |
| `log` | Session log parsing. Sessions delimited by `--- CRYO SESSION N ---` / `--- CRYO END ---`. `EventLogger` writes timestamped events (agent start, hibernate, exit). | `fn read_latest_session`, `fn read_current_session`, `fn read_recent_sessions`, `fn session_count`, `fn parse_latest_session_wake`, `fn parse_latest_session_task`. |
| `chamber_status` | Read model for status display — snapshots `timer.json`, logs, and message counts. | `struct ChamberStatus`, `struct ChamberMessage`, `struct ChamberOverview`, `struct ChamberSyncBadge`, `fn status`, `fn messages`, `fn next_wake`. |
| `session` | Legacy helper (`should_copy_plan`). Currently unused — `plan.md` must already exist in the working directory. | `fn should_copy_plan`. |

### Messaging, sync channels, and reports

| Module | Purpose | Key interfaces |
|--------|---------|----------------|
| `message` | File-based inbox/outbox I/O. `cryo-agent receive` archives inbox messages; the daemon only checks whether inbox files exist. | `struct Message`, `fn ensure_dirs`, `fn write_message`, `fn read_inbox`, `fn list_inbox`, `fn archive_messages`. |
| `channel` | Channel abstraction over messaging backends. | `trait MessageChannel` (read inbox, post reply). |
| `channel::file` | Local `messages/inbox/` + `messages/outbox/` backend. | `struct FileChannel::new`. |
| `channel::github` | GitHub Discussions backend via `gh` CLI / GraphQL. | `fn whoami`, `fn gh_graphql`, `fn build_fetch_comments_query`, `fn build_post_comment_mutation`. |
| `channel::zulip` | Zulip REST API client. | `struct ZulipCredentials`, `struct ZulipClient`, `struct ZulipPullResult`, `fn from_zuliprc`. |
| `gh_sync` | GitHub Discussion sync state (`gh-sync.json`). | `struct GhSyncState`, `fn save_sync_state`, `fn load_sync_state`, `fn is_sync_running`, `fn summarize`. |
| `zulip_sync` | Zulip sync state (`zulip-sync.json`). | `struct ZulipSyncState`, `fn save_sync_state`, `fn load_sync_state`, `fn is_sync_running`, `fn summarize`. |
| `sync_common` | Shared types for sync backends (GitHub, Zulip). | `enum SyncBackend`, `struct SyncSummary`, `enum SyncLoopCommand`, `enum SyncCycleStatus`, `struct PidFile`. |
| `sync_control` | Orchestration and CLI dispatch for sync backends (`start`, `stop`, `pull`, `push`, `status`). | `fn start`, `fn stop`, `fn pull`, `fn push`, `fn summarize`, `fn summarize_all`, `fn is_running`. |
| `report` | Periodic session summary reports written to `messages/outbox/`. | `struct ReportSummary`, `fn generate_report`, `fn write_report_to_outbox`, `fn compute_next_report_time`. |

### Web dashboard

| Module | Purpose | Key interfaces |
|--------|---------|----------------|
| `hub` | `cryohub` dashboard: Axum router, chamber discovery, SSE event stream, start/stop/restart handlers. Served by the `cryohub` binary; always operates on the current directory. | `fn build_router`, `fn build_router_with_state`, `async fn serve`. |
| `hub::state` | Shared app state, chamber index, SSE broadcast. | `struct AppState`, `enum SseEvent`, `fn resolve`, `fn refresh`. |
| `hub::discovery` | Chamber discovery + URL-safe id encoding over a workspace directory. | `struct ChamberEntry`, `type ChamberIndex`, `fn encode_id`, `fn decode_id`, `fn scan_workspace`. |

## Key Design Decisions

- **Daemon mode.** `cryo start` installs an OS service (launchd on macOS, systemd on Linux) that survives reboots. The daemon sleeps until the scheduled wake time, watches `messages/inbox/` for reactive wake, and enforces session timeout. `CRYO_NO_SERVICE=1` falls back to direct background spawn.
- **Socket-based IPC.** The agent talks to the daemon via `cryo-agent` subcommands (`hibernate`, `send`, `reply`, `todo …`) which send JSON over a Unix domain socket. `receive` and `time` are local (no daemon needed). TODO mutation is routed through the daemon so scheduling changes serialize with the session lifecycle.
- **Fire-and-forget agent.** The daemon spawns the agent and redirects stdout/stderr to `cryo-agent.log`. Stdout/stderr are diagnostic logs, not a human communication channel. All structured communication flows through `cryo-agent`.
- **SIGUSR1 wake.** `cryo wake` and `cryo send --wake` send SIGUSR1 to the daemon PID, which works regardless of `watch_inbox`. The daemon's signal-forwarding thread converts this into an `InboxChanged` event.
- **Config / state split.** `cryo.toml` is the project config (agent, session timeout, watch_inbox, report interval, provider rotation) created by `cryo init`. `timer.json` is runtime-only state (session number, PID, CLI overrides). CLI flags to `cryo start` are stored as optional overrides in `timer.json`.
- **Daemon-authored stand-in replies.** When a session ends without any agent-authored outbox message, the daemon writes a `from: cryochamber` message so operators always see at least one update per session. All chamber-level messages (stand-in replies, periodic reports) share the single `cryochamber` sender.
- **Agent notes via `NOTES.md`.** The agent's persistent memory across sessions is a plain markdown file the agent reads and writes directly — no IPC roundtrip. Seeded by `cryo init`, surfaced in the hub's Notes tab.
- **Crash handling via TODO re-injection.** If the agent exits without calling `cryo-agent hibernate`, the daemon records the crash and re-injects any TODOs it consumed for that wake with a ` (attempt k)` suffix and an exponential delay (`2^k` minutes, capped at 1 day). There is no in-daemon backoff-retry loop — rescheduling is expressed entirely through the TODO list so it survives daemon restarts and is visible to both the agent and operators. `EventLogger` is still finalized on every outcome.
- **Daemon does not preview inbox, agent receives it.** Wake-time prompts do not include inbox contents. The daemon only notices that inbox files exist and surfaces that fact in the session prompt. Only `cryo-agent receive` moves them into `messages/inbox/archive/`. A crashed session therefore leaves its inbox intact, and the next wake sees exactly the same messages — no special "check archive" recovery step is needed.
- **`cryohub` is cwd-scoped.** The web dashboard binary always operates on the current directory — no `--dir` flag. The cwd must not itself be a chamber. Discovery scans `<cwd>/*` for chamber subdirectories. The service label is `com.cryo.hub.<hash>`; `cryohub status` additionally lists every other `com.cryo.hub.*` service installed on the machine.
- **Default agent.** The CLI defaults to `opencode` (headless mode, not the TUI).

## TODO and Message Flow

### TODO lifecycle

- `cryo-agent todo add|done|remove|list` sends socket requests to the daemon rather than mutating `todo.json` directly. The daemon handles those requests both while idle and during an active session, so TODO changes are serialized through a single owner.
- `todo.json` is the scheduler's source of truth. The daemon computes the next wake from the earliest pending TODO whose `at` field parses as `%Y-%m-%dT%H:%M`; invalid timestamps are skipped with a warning instead of breaking the loop.
- Right before a session starts, the daemon consumes every pending TODO whose `at` timestamp parses successfully and is already due, then marks it done. Pending TODOs with empty or invalid `at` values are left untouched. That prevents the same wake from firing again immediately while the current session is already handling that work.
- If the session crashes, the daemon re-injects those consumed TODOs as fresh items with a ` (attempt k)` suffix and a `2^k`-minute delay capped at one day. The backoff therefore lives in `todo.json`, survives daemon restarts, and stays visible to operators.
- `cryo-agent hibernate` is refused unless the chamber still has at least one pending TODO with a valid wake time, unless the agent declares `--complete` because the plan is genuinely finished.

### Message lifecycle

- Incoming messages are plain markdown files in `messages/inbox/`. They may be written by `cryo send`, `cryo wake`, `cryo-gh pull/sync`, or `cryo-zulip pull/sync`.
- The daemon watches `messages/inbox/` for new files and wakes on create events, but it does not parse message bodies or archive inbox files on the agent's behalf.
- Session prompts only tell the agent that inbox mail is waiting. The agent must run `cryo-agent receive` to print the inbox contents and move those files into `messages/inbox/archive/`.
- Agent-authored replies flow through the daemon: `cryo-agent send` / `cryo-agent reply` sends a socket request, and the daemon writes the corresponding markdown file into `messages/outbox/`.
- When a session ends, the daemon re-checks unread inbox filenames. If messages arrived and the agent never sent a reply, the daemon writes a stand-in `from: cryochamber` outbox reply; if the session produced no outbox message at all, it writes a chamber-status message instead.
- Operators can inspect the outbox locally with `cryo receive`. GitHub and Zulip sync daemons separately watch `messages/outbox/`, post new files to their remote channels, and archive each outbox message after a successful push.

## Files Created at Runtime

| File | Purpose |
|------|---------|
| `timer.json` | Runtime state (session number, PID lock, CLI overrides). |
| `todo.json` | Per-project TODO items for agent task tracking. |
| `cryo.log` | Append-only structured event log. |
| `cryo-agent.log` | Agent stdout/stderr (raw tool-call output). |
| `messages/inbox/` | Incoming messages for the agent. |
| `messages/outbox/` | Outgoing messages (agent replies, daemon stand-in replies, periodic reports). |
| `messages/inbox/archive/` | Processed inbox messages. |
| `.cryo/cryo.sock` | Unix domain socket for agent-daemon IPC. |
| `gh-sync.json` | GitHub Discussion sync state (if configured). |
| `cryo-gh-sync.log` | GitHub sync daemon log output (if configured). |
| `zulip-sync.json` | Zulip sync state (if configured). |
| `.cryo/zuliprc` | Zulip credentials (if configured). **Never sync, commit, or push this file** — it holds API credentials. |
| `cryo-zulip-sync.log` | Zulip sync daemon log output (if configured). |
| `~/Library/LaunchAgents/com.cryo.*.plist` | macOS launchd service files (auto-managed). |
| `~/.config/systemd/user/com.cryo.*.service` | Linux systemd service files (auto-managed). |
