# CLAUDE.md

Developer guidance for Claude Code (claude.ai/code) when working on this repository.
For project overview and usage, see `README.md`.

> **Scope note:** Most of this document's normative content is duplicated where its real audiences can see it: `templates/protocol.md` (for in-chamber agents — injected into every wake prompt) and `docs/src/explanation/` (for end users). What lives only here is maintainer-facing design rationale — *why* a rule exists, not *how* to follow it. When changing an operational contract, land the change in those user-visible surfaces first, then mirror it here.

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
make example-start-all # start all example chambers (AGENT=opencode|claude)
make example-cancel # stop a running example (DIR=examples/chambers/...)
make example-hub    # start global cryohub in foreground (PORT=8765)
make example-clean  # remove auto-generated files from all examples
make run-plan       # execute a plan with Codex by default (RUNNER=claude for Claude)
make check-agent    # quick agent smoke test (AGENT=opencode|claude)
make check-round-trip # full round-trip test with mr-lazy
make check-service  # verify OS service install/uninstall lifecycle (launchd/systemd)
make check-mock     # run mock agent integration tests (no external agent required)
make book           # build mdbook documentation (English + Chinese)
make book-serve     # serve the built book (both languages) at :3000
make book-serve-live # mdbook serve with live reload (English only)
make book-deploy    # deploy mdbook to GitHub Pages (gh-pages branch)
make copilot-review # request Copilot code review on current PR
make release V=x.y.z # tag and push a release (triggers CI publish to crates.io)
```

## Codex CLI (for `make run-plan`)

`make run-plan` invokes `codex exec` on the latest plan in `docs/plans/` (default `RUNNER=codex`).
Auth: `codex login` (ChatGPT) or `printenv OPENAI_API_KEY | codex login --with-api-key`.
Verify with `codex login status`. Override model via `CODEX_MODEL=gpt-5.5 make run-plan`,
switch runner via `AGENT_TYPE=claude make run-plan`. Output goes to `run-plan-output.log`.

## Architecture

### Core Loop

`cmd_start()` → spawn `cryo daemon` → event loop: spawn agent → listen on socket server for IPC commands → sleep until wake time or inbox event → run session → ...

### Binaries

| Binary | Purpose |
|--------|---------|
| `cryo` | Operator CLI — `init`, `start`, `status`, `cancel`, `clean`, `log`, `watch`, `send`, `receive`, `ps`, `restart`, `daemon` |
| `cryo-agent` | Agent-side IPC/utility CLI — `hibernate`, `send`, `receive`, `dialog`, `time`, `todo` (used by the spawned agent, not by operators; most commands send requests to the daemon via socket, while `time` is local) |
| `cryo-zulip` | Zulip sync CLI — `init`, `pull`, `push`, `sync`, `unsync`, `status` (manages Zulip stream messaging via OS service) |
| `cryohub` | Global web dashboard — `start [--host --port --foreground --chamber-root]`, `stop`, `status`, `daemon` (`daemon` is internal — `start` installs a launchd/systemd service that serves the hub UI over HTTP). |

### Modules

| Module | Purpose |
|--------|---------|
| `socket` | Unix domain socket IPC — message types (`Request`/`Response`), client (`send_request`), server (`SocketServer`). |
| `config` | TOML persistence for project config (`cryo.toml`). `CryoConfig` struct, load/save, `apply_overrides` merges CLI overrides from state. |
| `state` | JSON persistence to `timer.json` — runtime-only state (session number, PID lock, CLI overrides). PID-based locking via `libc::kill(pid, 0)`. |
| `log` | Session log manager. Sessions delimited by `--- CRYO SESSION N ---` / `--- CRYO END ---`. `EventLogger` writes timestamped events (agent start, hibernate, exit), and hub status derives daily digests from the same log. |
| `protocol` | Loads templates from `templates/` via `include_str!` (protocol, plan, cryo.toml). Written by `init`/`start`. |
| `agent` | Builds lightweight prompt with task + session context, spawns agent subprocess (stdout/stderr redirected to `cryo-agent.log`). |
| `process` | Process management utilities: `send_signal`, `terminate_pid`, `spawn_daemon`. |
| `session` | Legacy utility module (`should_copy_plan`). Currently unused — plan.md must exist in the working directory. |
| `daemon` | Persistent event loop: socket server for agent IPC, watches `messages/inbox/` via `notify`, enforces session timeout, `EventLogger` for structured logs, consumes past-due TODOs before each session, re-injects them with a `(attempt k)` suffix and `2^k`-minute delay (capped at 1 day) on crash, detects delayed wakes (e.g. after machine suspend), and coordinates the active-session inbox claim/send/fallback lifecycle. It notices when inbox messages exist but never previews bodies in the wake prompt. |
| `message` | File-based inbox/outbox message system. Agent-side `cryo-agent receive` goes through daemon IPC and archives the current inbox batch into `messages/inbox/archive/` immediately via `MessageStore`. Any “awaiting reply” state for that batch lives only in the daemon's current session. Operator `cryo receive` is separate: it reads messages from `messages/outbox/`. |
| `channel` | Channel abstraction. Submodules: `store` (local inbox/outbox), `zulip` (Zulip REST API). Attachments cross the boundary symmetrically. On pull, `localize_upload_links` downloads `/user_uploads/` files (authenticated, 25 MB cap) into `messages/attachments/` and rewrites inbox links to those local paths, so vision agents can read uploaded images. On push, `externalize_local_links` uploads chamber-local files an agent linked to and rewrites the link to the returned `/user_uploads/` path. Both directions are best-effort: a failure leaves the link untouched and never fails the sync cycle. |
| `registry` | User chamber registry for Cryohub discovery. Uses `$XDG_STATE_HOME/cryo/chambers/` (fallback `~/.cryo/chambers/`), keeps stopped chambers, clears stale PIDs, and prunes entries whose chamber disappeared. |
| `service` | OS service management: install/uninstall launchd (macOS) or systemd (Linux) user services. Used by `cryo start` and `cryo-zulip sync` for reboot-persistent daemons. `CRYO_NO_SERVICE=1` disables (falls back to direct spawn). |
| `todo` | Per-project TODO list persistence (`todo.json`). `TodoItem`/`TodoFile` structs plus retry rescheduling logic for crashed sessions. Mutated through daemon IPC so scheduling changes are serialized with the session lifecycle. |
| `zulip_sync` | Zulip sync state persistence (`zulip-sync.json`). |
| `hub` | Global web dashboard: Axum router (`serve`, `build_router_with_state`), registry-backed chamber discovery, SSE events, start/stop/restart handlers. Served by the `cryohub` binary. |

### Chamber Invariants

Four guarantees the daemon must preserve. Every change to the session
lifecycle, inbox handling, or TODO scheduling must be checked against
them. If a proposed change would violate one, it is wrong by default
and needs an explicit justification.

1. **Every wake produces at least one visible message to the user.** If
   the agent exits without calling `cryo-agent send`, the daemon writes
   a `from: cryochamber` fallback so the operator always has
   *something* to look at for that wake (`daemon_missing_outbound_text`
   in `src/daemon.rs`). Retryable agent-runner failures before any
   outbound message or inbox claim may defer this fallback across the
   bounded in-daemon retry loop; after retries are exhausted the
   fallback is mandatory. Silent finalized wakes are a bug.
   *Scope:* a "wake" here is a wake that starts a session. A scheduled
   fire that claims no due TODO and finds a verified-empty inbox starts
   no session (demand-driven wakes) — there is no work to report and no
   claimed TODO to honour; the skip itself is logged to `cryo.log`.
   Bootstrap and inbox wakes always start a session.
2. **Every inbox message is answered — by the agent or by the
   chamber.** An agent crash mid-processing is acceptable; leaving
   the sender with no reply is not. If a session ends with a received
   batch still unanswered, the daemon writes the fallback reply from
   `daemon_unanswered_reply_text` for that batch
   (`finalize_human_replies`). This guarantee is bounded by daemon
   liveness: the reply obligation lives in the daemon's in-memory
   session state (`SessionInboxState`), so it holds across agent
   crashes, timeouts, and graceful shutdown, but a hard kill of the
   *daemon process* (SIGKILL / OOM / power loss) after `cryo-agent
   receive` archived a batch and before the reply is written strands
   that batch — the sender must resend. This is an accepted limitation:
   making the obligation durable would require a file-backed pending
   flow, which the inbox contract deliberately avoids. The hibernate
   quietness gate enforces the front half of this: a session cannot end
   in a clean hibernate while unread mail exists, so mail present at
   hibernate time is answered by the live agent when the agent behaves —
   a crash, failure report (`--exit N`), timeout, or shutdown still ends
   the session and leaves that mail unread for a future session.
3. **Every TODO is honoured, and every failure is reported.** When a
   TODO's `at` time arrives the daemon claims it and runs a session.
   The claimed item stays visible as `[~]` in `## TODO List` while the
   session is active, but the scheduler ignores it so the same wake
   does not loop. If the session succeeds, the daemon marks the claimed
   item done. If that session crashes, `reschedule_claimed_after_crash`
   marks the original done and creates a *new* TODO with an
   `(attempt k)` suffix and a `2^k`-minute delay capped at 1 day, so
   the task keeps being retried and the operator can see how many
   attempts have been made. A manually-completed TODO
   (`cryo-agent todo done`) is the only way to stop the retry cycle
   short of a successful session.
4. **Claim/consumption is terminal — a picked-up message or TODO
   never returns to pending, regardless of whether processing
   succeeded.** An
   inbox message read by `cryo-agent receive` is archived into
   `messages/inbox/archive/` immediately and is never re-delivered or
   restored to the inbox. A TODO claimed for a session is never moved
   back to plain pending: successful sessions mark it done, and failed
   sessions mark it done plus add a fresh retry item. Retries are
   always implemented as *new* items with fresh IDs, not by reopening
   the original. This prevents the same trigger from re-spawning the
   agent on every subsequent wake tick and keeps the retry count
   honest. If a human still wants action on an unanswered message,
   they resend.

### Key Design Decisions

- **Daemon mode**: `cryo start` installs an OS service (launchd on macOS, systemd on Linux) that survives reboots. The daemon sleeps until the scheduled wake time, watches `messages/inbox/` for reactive wake, and enforces session timeout. Set `CRYO_NO_SERVICE=1` to fall back to direct background process spawn.
- **Socket-based IPC**: The agent communicates with the daemon via `cryo-agent` CLI subcommands (`hibernate`, `send`, `receive`, `todo`), which send JSON messages over a Unix domain socket. Only `time` is purely local.
- **Fire-and-forget agent**: The daemon spawns the agent and redirects its stdout/stderr to `cryo-agent.log`. Stdout/stderr are diagnostic logs, not a human communication channel. All structured communication flows through `cryo-agent`.
- **No forced-wake command**: there is no `cryo wake` and no SIGUSR1 wake path. A chamber wakes for its schedule (`todo.json`), for mail (the `watch_dirs` inbox watcher), or at daemon start. `cryo send` is how an operator reaches the agent — it carries intent and is guaranteed a reply (invariant 2); `cryo restart` covers the message-less "kick it now" case via the bootstrap session. Do not reintroduce a bare wake signal.
- **Reactive wake via `watch_dirs`**: `cryo.toml` carries a list `watch_dirs` (default `["messages/inbox"]`) of directories the daemon attaches a notify watcher to. New files in any watched directory wake the agent, equivalent to inbox-triggered wake. Empty list disables reactive wake. Do not reintroduce `watch_inbox` compatibility.
- **Config/state split**: `cryo.toml` is the project config (agent, session timeout, watch_dirs, provider env) created by `cryo init`. `timer.json` is runtime-only state (session number, PID, CLI overrides). CLI flags to `cryo start` are stored as optional overrides in `timer.json`.
- **Provider config**: Cryochamber supports a single active provider profile. The canonical TOML shape is `[provider]` with an `env = { ... }` map, injected into every spawned agent session. Legacy `[[providers]]` arrays are accepted only for backward compatibility: loading them emits a deprecation warning, the first entry is used, and saving canonicalizes back to `[provider]`. Provider rotation has been removed; do not reintroduce `provider_index`, `rotate_on`, or multi-provider retry behavior.
- **Chamber-authored messages**: All daemon-originated outbox messages use a single `from: cryochamber` sender for fallback replies when the agent never sends a human-visible message after claiming inbox, or crashes after retry exhaustion. Agent-authored human-visible messages use `from: agent`.
- **Preflight validation**: `cryo start` checks that the agent command exists on PATH before spawning.
- **Chamber resolution via `CRYO_CHAMBER_DIR`**: all binaries resolve the chamber from `CRYO_CHAMBER_DIR` when set, else cwd (`work_dir()` in `src/lib.rs`). The daemon injects the variable into every spawned agent session, so `cryo-agent` reaches the right chamber no matter what cwd the agent's shell tool uses (a wrong cwd used to route IPC to another chamber's socket). `cryo-agent` additionally preflights `ensure_chamber_dir` for every command except the local-only `time`, so a non-chamber directory fails with the real reason instead of a "Daemon instance mismatch" red herring.
- **Crash handling via retry and TODO re-injection**: Retryable agent-runner exits before any outbound message or inbox claim get an in-daemon retry loop: up to 10 retries after the first attempt, with exponentially increasing gaps. If the final attempt still fails, or if the agent exits without calling `cryo-agent hibernate` after consuming work, the daemon records the crash and re-injects any TODOs it claimed for that wake with a `(attempt k)` suffix and an exponential delay (`2^k` min, capped at 1 day). TODO rescheduling lives in `todo.json`, surviving daemon restarts and visible to both agent and operator. EventLogger is always finalized for each attempt.
- **Attachments are a channel concern, not an agent concern**: the agent only ever deals in chamber-local file paths — it reads pulled attachments from `messages/attachments/` and links to local files when sending. Uploading, downloading, and per-server markdown quirks live in the sync binary. Notably `externalize_local_links` rewrites upload links to **absolute** URLs and strips the `!` from `![alt](...)`; both are required for an inline preview (verified on Zulip 11.4): the preview pass only matches absolute upload URLs, and CommonMark image syntax renders literally before server 12.0 (feature level 437). Do not push channel-specific syntax into `cryo-agent send` or the protocol; do not let the sync upload anything outside the chamber or under `.cryo/` (it holds the bot API key).
- **Inbox contract**: There is no `cryo-agent reply`. Wake prompts do not include inbox contents; the daemon only checks whether inbox files exist so it can surface a notice in the session prompt. During an agent session, `cryo-agent receive` asks the daemon to read and archive the current inbox batch immediately; it is not an operator-facing command. The next successful `cryo-agent send` is the reply by definition for that received batch. If the agent exits or crashes before such a `send`, the daemon writes the fallback message for that batch. Inbox messages are never retried; only TODOs have retry semantics. If the human still wants action, they resend.
- **Default agent**: The CLI defaults to `opencode` as the agent command (headless mode, not the TUI).
- **Agent notes via `NOTES.md`**: The agent's persistent memory across sessions is a plain markdown file (`NOTES.md`) the agent reads and writes directly — no IPC roundtrip. Seeded by `cryo init`, surfaced in the hub's Notes drawer tab, and updated by the agent on its own. The removed `cryo-agent note` subcommand and `Request::Note` IPC variant are historical.
- **`cryo-agent time` input grammar**: Accepts three forms only — empty (current time), `+N minutes|hours|days|weeks` (relative offset), and ISO8601 (`2026-04-25T10:00` or date-only) as validated pass-through. Natural-language parsing is deliberately **not** supported: the agent is an LLM that can reason about "tomorrow 9am" itself, so the tool stays small and documentable. Unknown input prints the accepted forms.
- **`cryohub` is global and registry-backed**: The `cryohub` binary (not `cryo`) runs one global web dashboard per user and can start, stop, and report status from any directory. Product discovery reads the user chamber registry only; it does not scan the current working directory. Host, port, and the dashboard-created chamber root live in `hub::config::HubConfig` at `$XDG_CONFIG_HOME/cryo/cryohub.toml` (fallback `~/.config/cryo/cryohub.toml`); default host/port are `127.0.0.1:8765`, and the default chamber root is `~/.cryo/chambers`. `cryohub start --host/--port` updates the saved hub config. The service label is `"hub"` anchored to `hub::paths::hub_service_dir()` so `cryohub stop` works from arbitrary directories. The log file lives under the user-level Cryo state/log directory via `hub::paths::hub_log_path()`. `cryohub status` also lists legacy cwd-scoped `com.cryo.hub.*` services from older versions. Per-chamber `web_host`/`web_port` fields are not part of `CryoConfig`.
- **Hub chamber creation**: The New Chamber modal scaffolds opencode chambers. The optional API key provider section is folded and writes only `cryo.toml`; do not create per-chamber `opencode.json`. Provider and model controls are combobox inputs backed by browser-populated `<datalist>` elements. The browser fetches the provider/model catalog dynamically from `https://models.dev/api.json`, but both fields must continue to allow manual input for custom providers/models and network failures. Server-side code should validate provider ids and map provider ids to API-key environment variable names, but should not carry a hardcoded model catalog.
- **The reply window is the only waiting mechanism**: there is no agent-chosen wait (`cryo-agent receive --wait` and `Request::ReceiveWait` are gone). The agent never blocks on the operator; it finishes, asks to sleep, and the daemon decides when the sleep happens. A conversation is carried by the loop *hibernate refused/rejected → `receive` → `send` → `hibernate`*, which re-arms the window each round. Long-gap conversations legitimately span sessions and are reconstructed from the dialog archive (`cryo-agent dialog`), not held open by a blocked process.
- **Hibernate quietness gate & reply window**: hibernate is granted only when the chamber is quiet — unconditionally refused while unread inbox mail exists (`--complete` additionally while a TODO is due; `--exit N` failure reports are never gated) — and, when `reply_window` (cryo.toml, seconds, default 300; 0 disables) opens a window, the daemon parks the accepted hibernate (holding the operator-facing socket responder; hibernate is the only call that parks). Mail arrival rejects the parked call back into the same agent process (notice only — the agent still claims via `receive`; delivery clears `hibernate_outcome` so a later crash is a real crash); a due TODO or window expiry grants the sleep (TODOs always run in fresh sessions — never delivered into a linger, keeping invariant 3's one-session-one-attempt accounting). Every failure mode (runner shell-timeout kills the parked client, daemon restart, crash mid-window) degrades to today's fresh-session behavior. See issue #71.
- **Scheduled wakes are demand-driven**: the daemon's cached `next_wake` only decides *when to re-check* `todo.json`; whether a session runs is decided by what the wake actually claims. A scheduled wake that claims no due TODO and finds an empty inbox runs nothing — the daemon resyncs `next_wake` from disk and sleeps (pacing retries via `TODO_CLAIM_RETRY_DELAY` if disk still shows past-due work that `claim_due` failed to persist). Bootstrap and inbox wakes are explicit demand and always run. This makes a stale wake-time cache cost a file read instead of an agent session; the 2026-04-21 incident (a pre-fix binary spun ~678 spurious sessions in 9 hours) is the motivating failure shape, reproduced in `test_stale_scheduled_wake_with_no_due_work_skips_session_and_resyncs`.
- **Bounded TODO list in the wake prompt**: the per-session prompt renders TODOs via `TodoFile::display_for_prompt`, which always shows pending/claimed items but folds all except the last 3 completed items (in list order ≈ creation order; completion times are not tracked) behind a count — done items are never deleted, so the full list grows with chamber age and would be re-injected on every wake. `cryo-agent todo list` still returns the full list.

### Primary APIs and Ownership

- **API to invoke the external agent process**: `src/agent.rs`.
  - `agent_program` is the preflight executable resolver.
  - `AgentConfig` + `build_prompt` define the session prompt contract.
  - `build_command` and `spawn_agent` are the execution APIs.
  - The daemon invokes these from `ProcessSessionLauncher::run_session` in `src/daemon/session.rs`.
  - `cryo-agent` is **not** the external model runner; it is the agent-side IPC/utility CLI the spawned agent uses to talk back to the daemon.

- **API to manipulate TODOs**: `src/todo.rs::TodoFile`.
  - `TodoFile` is the single file-backed API for `todo.json`: `add`, `done`, `remove`, `items`, `display`, `next_wake_time`, `next_valid_wake`, `claim_due`, `complete_claimed`, `reschedule_claimed_after_crash`.
  - `cryo-agent todo ...` does not touch `todo.json` directly; it sends `socket::Request::{TodoAdd, TodoDone, TodoRemove, TodoList}` through `daemon_client::send_checked_request`.
  - Daemon request handling goes through `SessionEffects` / `FsSessionEffects`, which delegate to `TodoFile`.
  - Scheduler-side daemon logic also uses `TodoFile` directly for wake computation and crash retry requeue.

- **API to manipulate local message files**: `src/channel/store.rs::MessageStore`.
  - `MessageStore` is the local mailbox API for directory setup, inbox/outbox reads, and archiving. The intended human-message lifecycle is simple: `read_and_archive_inbox` -> next agent `send` or daemon fallback in the same session.
  - Agent-side `cryo-agent receive` sends `socket::Request::Receive` through `daemon_client::send_checked_request`; during an active session, the daemon then reads and archives the inbox batch through `MessageStore` and records the reply obligation in session memory.
  - `cryo-agent send` must **not** write outbox files directly; it sends `socket::Request::Send` through `daemon_client::send_checked_request` so the daemon can both write the outbox message and resolve any claimed inbox batch correctly.
  - Do not design new file-backed pending/recovery flows for inbox messages; archive-on-receive plus daemon fallback is the contract.
  - Daemon, operator CLI, sync daemons, hub routes, and status/read-model code should all use `MessageStore` for local mailbox file access.
  - Low-level markdown parsing / rendering / archiving primitives still live in `src/message.rs`; `MessageStore` composes them instead of reimplementing them.

- **How the daemon manipulates TODOs and messages**:
  - Request-time side effects go through `src/daemon/effects.rs`: `SessionEffects` is the boundary; `FsSessionEffects` is the filesystem-backed implementation.
  - Daemon TODO mutation goes through `FsSessionEffects` → `TodoFile`.
  - Daemon message-file mutation goes through `FsSessionEffects` → `MessageStore`.
  - Session startup only lists unread inbox filenames; the daemon does not preview inbox bodies.
  - `DaemonRequest::Send` writes the agent-authored outbox message and, if a claimed inbox batch exists, also finalizes that batch.
  - Session finalization may write a fallback `from: cryochamber` reply or status update if the agent did not send a human-visible response. Fallback reply obligation applies only to the batch this session already received; unread inbox files stay unread for a future session.
  - Past-due TODOs are claimed when a session starts; on success the daemon marks them done, and on crash it marks them done while creating fresh retry TODOs rather than "un-claiming" the originals.

### Files Created by `cryo init`

- `cryo.toml` — project configuration (agent, max_session_duration, watch_dirs)
- `plan.md` — template plan file
- `NOTES.md` — agent's persistent memory across sessions (seeded from `templates/notes.md`; agent reads/writes directly)
- `README.md` — quickstart guide for the project (service commands, messaging channels)

### Files Created at Runtime (per project directory)

- `timer.json` — runtime state only (session number, PID lock, CLI overrides)
- `cryo.log` — append-only structured event log
- `cryo-agent.log` — agent stdout/stderr (raw tool-call output)
- `todo.json` — per-project TODO items for agent task tracking
- `messages/inbox/` — incoming messages for the agent
- `messages/outbox/` — outgoing messages (agent replies, daemon stand-in replies)
- `messages/inbox/archive/` — processed inbox messages
- `messages/attachments/` — files downloaded from remote uploads (e.g. Zulip images); inbox message links are rewritten to point here
- `.cryo/cryo.sock` — Unix domain socket for agent-daemon IPC
- `zulip-sync.json` — Zulip sync state (if configured)
- `.cryo/zuliprc` — Zulip credentials copied from user's zuliprc (if configured). **Never sync, commit, or push this file** — it holds API credentials. Already gitignored; the `cryo-zulip` sync channel must never include it in any payload.
- `cryo-zulip-sync.log` — Zulip sync daemon log output (if configured)
- `~/Library/LaunchAgents/com.cryo.*.plist` — macOS launchd service files (auto-managed)
- `~/.config/systemd/user/com.cryo.*.service` — Linux systemd service files (auto-managed)

## Documentation

Main documentation lives in the mdbook at `docs/src/` (published to [giggleliu.github.io/cryochamber](https://giggleliu.github.io/cryochamber/)). Keep `README.md` lean — detailed guides belong in the mdbook.

- `README.md` — Project overview and quickstart only
- `docs/src/` — mdbook source: introduction (pitch + quickstart), CLI reference, configuration
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
