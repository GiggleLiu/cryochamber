# Architecture

For users: see [Concepts](./concepts.md). This page is for contributors reading the source.

## Core Loop

`cmd_start()` → spawn `cryo daemon` → event loop: spawn agent → listen on socket server for IPC commands → sleep until wake time or inbox event → run session → ...

## Binaries

| Binary | Purpose |
|--------|---------|
| `cryo` | Operator CLI — `init`, `start`, `status`, `cancel`, `log`, `watch`, `send`, `receive`, `wake`, `ps`, `restart`, `daemon`. |
| `cryo-agent` | Agent IPC CLI — `hibernate`, `send`, `receive`, `time`, `todo`. Most commands send requests to the daemon via socket; only `time` is local. |
| `cryo-gh` | GitHub sync CLI — `init`, `pull`, `push`, `sync`, `unsync`, `status`. Manages Discussion-based messaging via an OS service. |
| `cryo-zulip` | Zulip sync CLI — `init`, `pull`, `push`, `sync`, `unsync`, `status`. Manages Zulip stream messaging via an OS service. |
| `cryohub` | Global web dashboard — `start`, `stop`, `restart`, `status`, `daemon`. Installs a launchd/systemd service that serves the hub UI over HTTP. |
| `cryo-mock` | Test-only mock agent for integration tests (`make check-mock`). |

## Modules

Modules live in `src/` and are re-exported via `lib.rs`. Entries list the module's purpose and a handful of representative types or functions — not an exhaustive API list. Read the source for full signatures.

### IPC and daemon lifecycle

| Module | Purpose | Key interfaces |
|--------|---------|----------------|
| `socket` | Unix domain socket IPC protocol. | `enum Request` (`Ping`, `Hello`, `Hibernate`, `Send`, `Receive`, `TodoAdd/Done/Remove`, `TodoList`), `struct Response`, `fn socket_path`, `fn send_request`. |
| `daemon_client` | Thin CLI → daemon IPC wrapper. | `fn send_request`, `fn send_checked_request`, `fn daemon_responding`, `fn signal_daemon_wake`. |
| `daemon` | Persistent event loop: socket server, inbox `notify` watcher, SIGUSR1 wake, timeout enforcement, TODO consumption / attempt-based rescheduling on crash, delayed-wake detection. | `enum DaemonEvent`, `trait Clock`, `trait EventSource`, `async fn run`, `fn main_loop`. |
| `daemon::effects` | Session I/O abstraction (inbox claim/finalize, reply posting, TODO mutation). | `trait SessionEffects`, `enum ReplyAuthor`, `struct FsSessionEffects`. |
| `daemon::request` | Request parsing and hibernate-decision logic. | `enum DaemonRequest`, `enum TodoRequest`, `struct HibernateDecision`, `fn resolve_hibernate_request`. |
| `daemon::schedule` | Wake-time scheduling and pure next-step decisions. | `fn compute_sleep_timeout`, `fn next_wake_from_todos`, `fn decide_next_step`. |
| `daemon::session` | Session runtime: process spawn, request/response loop, wait/terminate. | `trait SessionRuntime`, `struct ChildExitStatus`. |
| `lifecycle` | Session startup validation and chamber lifecycle operations. | `enum DaemonLaunchMode`, `struct StartOptions`, `struct PreparedStart`, `fn require_valid_project`, `fn require_live_daemon`, `fn prepare_start`, `fn validate_agent_command`. |
| `process` | Process management utilities. | `fn send_signal`, `fn terminate_pid`, `fn spawn_daemon`. |
| `registry` | User chamber registry under `$XDG_STATE_HOME/cryo/chambers/` (fallback `~/.cryo/chambers/`). Keeps stopped chambers, clears stale PIDs, and prunes entries whose chamber disappeared. | `struct DaemonEntry`, `fn remember_chamber`, `fn register`, `fn unregister`, `fn list`. |
| `service` | OS service management: launchd (macOS) / systemd (Linux) user services. Used by `cryo start`, `cryo-gh sync`, `cryo-zulip sync`. `CRYO_NO_SERVICE=1` disables. | `struct InstalledService`, `fn service_label`, `fn install`, `fn uninstall`, `fn list_installed`, `fn is_installed`. |

### Config, state, and persistence

| Module | Purpose | Key interfaces |
|--------|---------|----------------|
| `config` | TOML project config (`cryo.toml`), with CLI overrides merged from runtime state. | `struct CryoConfig`, `struct ProviderConfig`, `fn load_config`, `fn save_config`. |
| `state` | JSON runtime state (`timer.json`): session number, PID lock, CLI overrides. PID-based locking via `libc::kill(pid, 0)`. | `struct CryoState`, `fn load_state`, `fn save_state`, `fn is_locked`. |
| `todo` | Per-project TODO list (`todo.json`); mutated through daemon IPC so scheduling changes serialize with the session lifecycle. Also owns the claim, completion, and retry rescheduling logic used by the daemon around session runs. | `struct TodoFile`, `struct TodoItem`, `fn add`, `fn done`, `fn remove`, `fn items`, `fn display`, `fn next_wake_time`, `fn next_valid_wake`, `fn claim_due`, `fn complete_claimed`, `fn reschedule_claimed_after_crash`. |
| `protocol` | Loads templates from `templates/` via `include_str!`. `PROTOCOL_CONTENT` is embedded in each runtime prompt; scaffold helpers write chamber-owned files such as `plan.md` and `cryo.toml`. | `PROTOCOL_CONTENT`, `fn scaffold_chamber`, `fn write_template_plan`, `fn write_config_file`. |

### Agent, logging, and chamber status

| Module | Purpose | Key interfaces |
|--------|---------|----------------|
| `agent` | Resolves the agent command and builds the per-session prompt. | `enum AgentKind`, `struct AgentConfig`, `fn agent_program`, `fn build_prompt`. |
| `log` | Session log parsing. Sessions delimited by `--- CRYO SESSION N ---` / `--- CRYO END ---`. `EventLogger` writes timestamped events (agent start, hibernate, exit), and hub status derives daily digests from the same log. | `fn read_latest_session`, `fn read_current_session`, `fn read_recent_sessions`, `fn daily_digests`, `fn session_count`, `fn parse_latest_session_wake`, `fn parse_latest_session_task`. |
| `chamber_status` | Read model for status display — snapshots `timer.json`, logs, and message counts. | `struct ChamberStatus`, `struct ChamberMessage`, `struct ChamberOverview`, `struct ChamberSyncBadge`, `fn status`, `fn messages`, `fn next_wake`. |
| `session` | Legacy helper (`should_copy_plan`). Currently unused — `plan.md` must already exist in the working directory. | `fn should_copy_plan`. |

### Messaging and sync channels

| Module | Purpose | Key interfaces |
|--------|---------|----------------|
| `message` | Low-level markdown message parsing/rendering and file primitives used by the mailbox store. | `struct Message`, `fn write_message`, `fn read_inbox`, `fn list_inbox`, `fn archive_messages`, `fn format_inbox`, `fn parse_message`. |
| `channel` | Channel abstraction over messaging backends. | `trait MessageChannel` (read inbox, post reply). |
| `channel::store` | Local mailbox API over `messages/inbox/` + `messages/outbox/`, used by daemon, CLI, sync, hub, and status paths. | `struct MessageStore::new`, `fn send_in`, `fn send_out`, `fn read_inbox_named`, `fn read_and_archive_inbox`, `fn read_outbox_named`, `fn archive_outbox`. |
| `channel::github` | GitHub Discussions backend via `gh` CLI / GraphQL. | `fn whoami`, `fn gh_graphql`, `fn build_fetch_comments_query`, `fn build_post_comment_mutation`. |
| `channel::zulip` | Zulip REST API client. | `struct ZulipCredentials`, `struct ZulipClient`, `struct ZulipPullResult`, `fn from_zuliprc`. |
| `gh_sync` | GitHub Discussion sync state (`gh-sync.json`). | `struct GhSyncState`, `fn save_sync_state`, `fn load_sync_state`, `fn is_sync_running`, `fn summarize`. |
| `zulip_sync` | Zulip sync state (`zulip-sync.json`). | `struct ZulipSyncState`, `fn save_sync_state`, `fn load_sync_state`, `fn is_sync_running`, `fn summarize`. |
| `sync_common` | Shared types for sync backends (GitHub, Zulip). | `enum SyncBackend`, `struct SyncSummary`, `enum SyncLoopCommand`, `enum SyncCycleStatus`, `struct PidFile`. |
| `sync_control` | Orchestration and CLI dispatch for sync backends (`start`, `stop`, `pull`, `push`, `status`). | `fn start`, `fn stop`, `fn pull`, `fn push`, `fn summarize`, `fn summarize_all`, `fn is_running`. |

### Web dashboard

| Module | Purpose | Key interfaces |
|--------|---------|----------------|
| `hub` | `cryohub` dashboard: Axum router, registry-backed chamber discovery, SSE event stream, start/stop/restart handlers. Served by the `cryohub` binary. | `fn build_router`, `fn build_router_with_state`, `async fn serve`. |
| `hub::config` | Global hub config (`cryohub.toml`) for host, port, and dashboard-created chamber root. | `struct HubConfig`, `fn load_config`, `fn save_config`, `fn effective_config`. |
| `hub::state` | Shared app state, chamber index, SSE broadcast. | `struct AppState`, `enum SseEvent`, `fn resolve`, `fn refresh`. |
| `hub::discovery` | Registry-backed chamber discovery + URL-safe id encoding. Workspace scanning remains as a local test/helper mode. | `struct ChamberEntry`, `struct DiscoveryOptions`, `type ChamberIndex`, `fn encode_id`, `fn decode_id`, `fn discover_with_options`. |

## Key Design Decisions

- **Daemon mode.** `cryo start` installs an OS service (launchd on macOS, systemd on Linux) that survives reboots. The daemon sleeps until the scheduled wake time, watches `messages/inbox/` for reactive wake, and enforces session timeout. `CRYO_NO_SERVICE=1` falls back to direct background spawn.
- **Socket-based IPC.** The agent talks to the daemon via `cryo-agent` subcommands (`hibernate`, `send`, `receive`, `todo …`) which send JSON over a Unix domain socket. `cryo-agent` is for the spawned agent, not for operators; only `time` is purely local. TODO mutation and active-session inbox receive state are routed through the daemon so scheduling and reply obligations serialize with the session lifecycle.
- **Fire-and-forget agent.** The daemon spawns the agent and redirects stdout/stderr to `cryo-agent.log`. Stdout/stderr are diagnostic logs, not a human communication channel. All structured communication flows through `cryo-agent`.
- **SIGUSR1 wake.** `cryo wake` and `cryo send --wake` send SIGUSR1 to the daemon PID, which works regardless of the inbox-wake setting. The daemon's signal-forwarding thread converts this into an `InboxChanged` event.
- **Config / state split.** `cryo.toml` is the project config (agent, session timeout, inbox-wake behavior, provider env) created by `cryo init`. `timer.json` is runtime-only state (session number, PID, CLI overrides). CLI flags to `cryo start` are stored as optional overrides in `timer.json`.
- **Daemon-authored stand-in replies.** When a session ends without any agent-authored outbox message, the daemon writes a `from: cryochamber` message so operators always see at least one update per session.
- **Agent notes via `NOTES.md`.** The agent's persistent memory across sessions is a plain markdown file the agent reads and writes directly — no IPC roundtrip. Seeded by `cryo init`, surfaced in the hub's Notes tab.
- **Crash handling via TODO retry.** If the agent exits without ending the session cleanly, the daemon records the crash, marks each claimed TODO done, and adds a fresh attempt-suffixed retry TODO with an exponential delay (`2^k` minutes, capped at 1 day). The original claim is terminal — retries are always *new* items with fresh IDs. There is no in-daemon backoff-retry loop; rescheduling lives entirely in the TODO list, so it survives daemon restarts and is visible to both the agent and operators. `EventLogger` is still finalized on every outcome.
- **Daemon does not preview inbox, agent receives it.** Wake-time prompts do not include inbox contents. The daemon only notices that inbox files exist and surfaces that fact in the session prompt. During a session, agent-side `cryo-agent receive` goes back through daemon IPC, and the daemon immediately archives the current inbox batch to `messages/inbox/archive/`. The daemon remembers that batch only in the current session so the next successful agent `send` can count as its reply, or the daemon can fall back at session end. Operator `cryo receive` is unrelated; it reads the agent's outbox.
- **`cryohub` is global and registry-backed.** The web dashboard can start, restart, stop, and report status from any directory. Product discovery reads the user chamber registry only; it does not scan the current working directory. The hub config lives in `$XDG_CONFIG_HOME/cryo/cryohub.toml` (fallback `~/.config/cryo/cryohub.toml`) and stores host, port, and the root for dashboard-created chambers. The default chamber root is `~/.cryo/chambers`. `cryohub status` also lists legacy cwd-scoped services from older versions so users can remove them.
- **Default agent.** The CLI defaults to `opencode` (headless mode, not the TUI).
