# Web UI improvements: stable message view + sync surfaces

**Date:** 2026-04-19
**Status:** Design

Two independent improvements bundled in one spec because they share the
same build/update DOM pattern in `templates/web_shell.html`.

- **Part A — Stable message view.** Fix the message box scrolling back to
  the top on every SSE tick and eliminate flicker of the header, todos,
  and lifecycle buttons.
- **Part B — Sync surfaces.** Detect and operate the GitHub Discussion
  and Zulip sync daemons per chamber from the web UI.

---

## Part A — Stable message view

### A.1 Problem

`renderDetail(id)` at `templates/web_shell.html:132` runs
`pane.innerHTML = ''` and rebuilds the whole pane on every SSE event.
The watcher at `src/web/watchers.rs:134-168` polls `timer.json` every
500 ms and emits `StatusChange` whenever the file content differs,
which happens constantly during an active session (retry count,
session number, etc.). Consequences:

- Messages scroll back to the top on every tick.
- Todo list, header, and log box flicker.
- Appended messages do not auto-scroll into view, because the rebuild
  resets `scrollTop` to 0.

### A.2 Goals

- The message box scroll position is stable across SSE ticks.
- New messages auto-scroll into view when the user is already at or
  near the bottom (chat semantics).
- No DOM work for status/todo/log ticks that leave list contents
  unchanged.

Non-goals:

- No framework introduction (stay vanilla JS).
- No SSE protocol changes on the server beyond what is strictly
  needed.

### A.3 Architecture — split `renderDetail` into build + update

Replace the monolithic `renderDetail(id)` with a two-phase pattern:

**`buildDetail(id)`** — called only when the selected chamber changes:
chamber switch, first load, or explicit Refresh button click. Clears
the pane and builds all sub-regions once, attaching them to cached
DOM references on a `view` object:

```js
view = {
  headerDotSession, headerNextWake, headerTaskLine, planCompleteBox,
  todosBox, msgBox, logBox, lifecycleBtns, syncBox,
  lastMsgKey: null,
  lastTodoSig: null,
  lastSyncSig: null,
  stickToBottom: true,
};
```

**`updateDetail(id, {status?, messages?, todos?, sync?})`** — called
on SSE ticks. Each sub-updater is idempotent and diff-aware:

- `updateHeader(status)` — overwrites `textContent` of the inline
  spans; assigning the same value is a no-op for the browser.
- `updateTodos(todos)` — compares a signature
  (`len:id|done|at,id|done|at,...`) to the last signature and rebuilds
  the inner list only if it changed.
- `updateMessages(messages)` — append-only; details in A.5.
- `updateLifecycleButtons(entry)` — compares current button set
  against desired; rebuilds only if different.
- `updateSyncBox(summaries)` — diff-aware like todos; details in
  Part B.
- Log lines remain append-only via the existing `log` event handler
  (already correct in today's code).

### A.4 SSE routing

- `message` event → refetch `/messages`, call `updateMessages`; also
  `loadChambers` for the sidebar unread counter.
- `status` event → refetch `/status`, `/todos`, and `/sync` in
  parallel, then call `updateHeader`, `updateTodos`,
  `updateLifecycleButtons`, `updateSyncBox`.
- `log` event → append to `logBox` (unchanged from today).
- `index` event → `loadChambers` only; do not touch the detail pane.

The watcher at `src/web/watchers.rs:134-168` emits `StatusChange`
only when `timer.json` actually changes, not on every 500 ms poll,
so the additional fetch of `/todos` per status tick is not
continuous. `todo.json` is not watched separately; refreshing todos
on `status` events is the simplest way to cover agent-initiated todo
mutations without adding another watcher thread. Diff-aware
`updateTodos` suppresses DOM work when nothing changed.

Chamber switch (`selectChamber`) runs `buildDetail`, which resets
`view` and fetches all payloads in parallel as today.

### A.5 Message identity and append-only semantics

`/api/chambers/:id/messages` returns all messages sorted by
timestamp with no server-assigned id (`src/web/routes/chamber.rs:
119-124`). For append-only updates we need a stable client-side key.

**Key:** `${direction}|${timestamp}|${from}|${hash32(body)}`

- `hash32` is a cheap rolling hash (e.g., FNV-1a or a 10-line
  JavaScript `cyrb53` variant). Full SHA would waste cycles; the
  triple plus a 32-bit body hash is unique enough in practice.
- Store `view.lastMsgKey` = key of the last rendered message.
- On `updateMessages`: iterate the server array, skip everything up
  to and including `lastMsgKey`, then append the remainder. Update
  `lastMsgKey`.
- If `lastMsgKey` is not found in the server array (rare: chamber
  reset archived the inbox), fall back to clearing and rebuilding the
  list, then scroll to bottom.

Alternative considered: expose a stable `message_id` (filename) in
the JSON payload. Cleaner, but widens the API for a client-only
concern. Rejected.

### A.6 Stick-to-bottom behavior

For `msgBox`:

1. Before any DOM mutation, compute
   `atBottom = (msgBox.scrollHeight - msgBox.scrollTop - msgBox.clientHeight) < 40`.
   The 40 px threshold tolerates sub-pixel rounding and small
   over-scroll.
2. Append new message rows.
3. After append, if `atBottom` was true before the append, set
   `msgBox.scrollTop = msgBox.scrollHeight`. Otherwise leave the
   scroll position alone (the user is reading history).

On `buildDetail`, after populating messages, unconditionally scroll
to bottom — the user just opened the chamber and wants the latest.

Edge cases:

- Empty `msgBox` — `scrollHeight == clientHeight`, so `atBottom` is
  true; first message auto-scrolls.
- Very long single message — if the user scrolled up within it,
  `atBottom` is false and new messages do not yank.
- Chamber switch — `buildDetail` resets `view`; initial scroll to
  bottom applies.
- Window resize — not handled; user can scroll manually.

For `logBox`, keep today's behavior (`scrollTop = scrollHeight` on
each new line). No stick-to-bottom detection needed since logs are
fire-and-forget diagnostic output.

The "preserve prior scroll" approach discussed during brainstorming
is not needed: the only remaining rebuilds after this change happen
on chamber switch and explicit Refresh, both of which should scroll
to bottom.

### A.7 Error handling

- Fetch failures in `updateDetail` toast the error and leave the
  existing view untouched. Partial failures are acceptable: if
  `/status` succeeds but `/messages` fails, the header updates and
  the message list stays stale rather than blanking.
- If SSE disconnects (background tab, network blip), `EventSource`
  auto-reconnects. No resync on reconnect for this change; user can
  hit Refresh. (Automatic resync on SSE `open` is a possible
  follow-up.)
- Missing `lastMsgKey` during append → full rebuild + scroll to
  bottom, as described above.

### A.8 Testing

Server-side behavior is unchanged; no new Rust tests are required.
`messages_json_sorted_by_timestamp` in
`src/web/routes/chamber.rs` already covers the property
`updateMessages` depends on.

Client-side JavaScript is tested manually via this checklist:

- [ ] Idle running chamber: scroll partway up in the message box,
      wait 30 s, scroll position does not change.
- [ ] Scrolled to bottom: send a message via another terminal,
      message appears and view auto-scrolls to it.
- [ ] Scrolled up: send a message; view does not yank; can scroll
      down manually to see it.
- [ ] Switch chambers: new chamber opens scrolled to the most recent
      message.
- [ ] Active session with frequent `timer.json` updates: todos list
      does not visibly flicker; header numbers update in place.
- [ ] Reset chamber: message list rebuilds cleanly and scrolls to
      bottom.

Automated browser tests (Playwright) are deferred; the JavaScript
diff is small and directly inspectable. Revisit if the web UI grows.

---

## Part B — Sync surfaces in web UI

### B.1 Problem

The web UI has no awareness of GitHub or Zulip sync.
`src/web/routes/chamber.rs:71-82` does not look at `gh-sync.json` or
`zulip-sync.json`, and no endpoint drives the `cryo-gh` or
`cryo-zulip` binaries. Operators who rely on external messaging must
drop to a terminal to check whether sync is configured, whether the
daemon is actually running, and to start or stop it.

### B.2 Goals

- Detect configured sync backends per chamber and display them.
- Show accurate running status — including the `CRYO_NO_SERVICE=1`
  fallback — not just "installed as a service".
- Allow start, stop, pull, and push from the UI for chambers that
  already have a sync state file.

Non-goals:

- No initialization from the UI. The user does `cryo-gh init` /
  `cryo-zulip init` in a terminal once; the web UI only operates
  already-initialized chambers.
- No secrets in the browser.
- No sync operations on external chambers. Match the existing
  `require_workspace` policy, which already restricts
  start/stop/restart/reset to workspace chambers. External chambers
  only get `wake`.
- No new runtime dependencies; reuse `service::is_installed`, simple
  filesystem checks, and shelling out to the existing sync CLIs.

### B.3 Running detection — pid files

Add a pid file per sync daemon so "is running" is a cheap local
check that works in both service mode and `CRYO_NO_SERVICE=1`
direct-spawn mode.

**In `src/bin/cryo_gh.rs`, `cmd_gh_sync_daemon`:**

- On startup: write `{dir}/cryo-gh-sync.pid` containing the current
  PID.
- On clean exit: remove the pid file.
- Register a Ctrl-C / SIGTERM handler (via `ctrlc` crate or a manual
  signal handler) to remove the pid file when stopped by launchd /
  systemd.

**In `src/bin/cryo_zulip.rs`, `cmd_sync_daemon`:** mirror the above
with `cryo-zulip-sync.pid`.

**New helpers** in `src/gh_sync.rs` and `src/zulip_sync.rs`:

```rust
pub fn sync_pid_path(dir: &Path) -> PathBuf
pub fn read_sync_pid(dir: &Path) -> Option<u32>
pub fn is_sync_running(dir: &Path) -> bool
```

`is_sync_running` reads the pid file and calls `libc::kill(pid, 0)`
— the same pattern `state::is_locked` uses. Stale pid files (process
dead) return false. No active cleanup needed; the next daemon start
overwrites the file.

Old daemons started before this change will not have a pid file, so
`is_sync_running` returns false for them. The UI will show
"configured but not running". This is acceptable since the daemon
restarts on any `cryo-gh sync` invocation, and we document the
requirement in the migration notes.

### B.4 Backend abstraction

Introduce `src/sync_common.rs` with a compact summary type and free
functions that delegate to the per-backend modules. Two backends
does not justify trait dispatch.

```rust
pub enum SyncBackend { Gh, Zulip }

pub struct SyncSummary {
    pub backend: SyncBackend,
    pub configured: bool,      // state file exists
    pub installed: bool,       // service::is_installed(...)
    pub running: bool,         // pid file alive
    pub target: String,        // "owner/repo#42" or "site · stream / topic"
    pub last_pushed_session: Option<u32>,
    pub log_tail_path: PathBuf,
}

pub fn summarize(backend: SyncBackend, dir: &Path) -> Option<SyncSummary>
pub fn summarize_all(dir: &Path) -> Vec<SyncSummary>    // configured only

pub fn start(backend: SyncBackend, dir: &Path) -> Result<()>
pub fn stop(backend: SyncBackend, dir: &Path)  -> Result<()>
pub fn pull(backend: SyncBackend, dir: &Path)  -> Result<()>
pub fn push(backend: SyncBackend, dir: &Path)  -> Result<()>
```

`summarize` inspects the appropriate state file, service install
state, and pid file. `summarize_all` returns only configured
backends.

**Per-backend adapters** are free functions in the existing modules:

- `gh_sync::summarize(dir) -> Option<SyncSummary>`
- `zulip_sync::summarize(dir) -> Option<SyncSummary>`

Lifecycle actions shell out to the existing binaries rather than
re-implementing them:

- `start` → `cryo-gh sync` / `cryo-zulip sync`
- `stop` → `cryo-gh unsync` / `cryo-zulip unsync`
- `pull` → `cryo-gh pull` / `cryo-zulip pull`
- `push` → `cryo-gh push` / `cryo-zulip push`

Each function returns `anyhow::Result<()>`; non-zero CLI exit is
converted into an error with stderr (truncated to ~500 chars)
attached.

**CLI binary resolution:** the helper tries, in order,
`$CRYO_GH_CLI` / `$CRYO_ZULIP_CLI` env vars (used by tests),
then a sibling of `std::env::current_exe()` (production install),
then PATH lookup. Unresolvable → `Err("cryo-gh binary not found")`.

Why shell out: the CLIs handle service install/uninstall, logging,
and error reporting. Duplicating that in the web layer doubles the
surface area. The ~100-200 ms per operator-initiated action is
acceptable.

### B.5 API surface

All endpoints are scoped under a chamber id.

```
GET  /api/chambers/:id/sync
POST /api/chambers/:id/sync/:backend/start
POST /api/chambers/:id/sync/:backend/stop
POST /api/chambers/:id/sync/:backend/pull
POST /api/chambers/:id/sync/:backend/push
```

- `:backend ∈ {"gh", "zulip"}`; any other value returns 400.
- All POST handlers call `require_workspace(&entry)` from
  `src/web/routes/chamber.rs:225` and return 409 for external
  chambers.
- Success responses return `{ok: true, message: "..."}` matching the
  existing lifecycle endpoints. CLI-failure responses return
  `{ok: false, message: "..."}` with the error text.
- `start` / `stop` install or uninstall the OS service. `pull` and
  `push` are synchronous one-shots: the handler blocks until the CLI
  exits and returns its result.
- After a successful `start` / `stop` / `pull` / `push`, the handler
  emits `SseEvent::StatusChange { chamber_id }` so connected
  browsers refetch `/sync` on the next tick.

**`ChamberEntry` extension** in `src/web/discovery.rs:39-51`:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncBadge { pub backend: String, pub running: bool }

// ChamberEntry gains:
pub sync: Vec<SyncBadge>,
```

`populate_runtime` calls `sync_common::summarize_all(dir)` and maps
configured entries into `sync`. Only `backend + running` is in the
overview; full detail is served by the `GET /sync` endpoint so the
chamber-list payload stays cheap.

### B.6 UI — the sync box

Placement: between the header and the todos region in the detail
pane. Rendered only when `summaries.length > 0`.

Per-backend row:

```
Sync
─────────────────────────────────────────
 gh    ● running   alice/cryo-notes#42
                   last push: session #3
                   [ stop ]  [ pull ]  [ push ]

 zulip ○ stopped   zulip.example.com · dev / cryo
                   [ start ]  [ pull ]  [ push ]
─────────────────────────────────────────
```

- Dot: `●` running, `○` stopped, `⚠` configured-but-service-missing
  (state file present, `is_installed` false).
- Target: `owner/repo#discussion` for gh; `site · stream / topic`
  for zulip. Truncated with ellipsis beyond ~50 characters.
- Action buttons: always `[start|stop]` (swapped by running state)
  plus `[pull]` and `[push]`. Pull/push are enabled regardless of
  daemon state because the CLIs re-load credentials themselves.
- `stop` opens a `window.confirm("Stop sync and uninstall
  service?")` matching the existing `reset` confirmation at
  `web_shell.html:249-254`. `start`, `pull`, `push` are
  single-click.
- Pull/push feedback: toast with the CLI's success or error
  message.

**Sidebar badge is deferred** — keep the sidebar focused on
presence, name, and unread count. Add per-chamber sync chips in a
follow-up if operators ask.

**Refresh flow:**

- `buildDetail` fetches `/sync` in parallel with `/status`,
  `/messages`, `/todos`. `buildSyncBox(summaries)` creates the
  section or omits it when empty.
- `updateDetail` refetches `/sync` on each `status` SSE tick and
  calls `updateSyncBox(summaries)`. Diff signature:
  `backend|running|target|last_pushed` per row, joined — skip DOM
  work if unchanged, matching the todo pattern from Part A.
- After a successful button press, the handler's explicit
  `StatusChange` emit triggers the next tick's refetch; no manual
  refresh needed.

CSS: extend `templates/web.css` with `.sync-box`, `.sync-row`,
`.sync-target`, `.sync-meta`, and a `.sync-dot` variant keyed by
status. Reuse the existing `.todos` layout and spacing.

### B.7 Error handling

- **Missing CLI binary:** handler returns
  `{ok: false, message: "cryo-gh binary not found"}`. UI toasts it.
- **CLI non-zero exit:** capture stderr, truncate to ~500 chars,
  return in `message`. Matches existing lifecycle error style.
- **Stale pid file:** `is_sync_running` returns false (process
  dead). The sync CLI re-installs the service on next start, which
  handles the cleanup path implicitly.
- **Concurrent start/stop clicks:** no serialization. Both CLIs are
  idempotent (sync installs even when installed; unsync removes
  even when absent). A rapid double-click is a harmless no-op.
- **External chamber hitting sync endpoint:** `require_workspace`
  returns 409; the UI does not render the sync box for external
  chambers either, so this is belt and suspenders.

### B.8 Testing

Rust tests (follows existing patterns):

1. `src/gh_sync.rs` and `src/zulip_sync.rs` — `sync_pid_path`,
   `read_sync_pid` (missing, present, invalid), `is_sync_running`
   (live PID and dead PID via a short-lived child).
2. `src/sync_common.rs` — `summarize` for each backend with mocked
   state files; `summarize_all` filters to configured only.
3. `src/web/routes/sync.rs` (new) —
   - `get_sync` returns `[]` when no state files exist.
   - `get_sync` returns entries matching disk state.
   - `post_sync_*` return 409 for external chambers (mirror
     `start_stop_restart_return_409_for_external` at
     `src/web/routes/chamber.rs:362-405`).
   - `post_sync_start` invokes a fake CLI via `CRYO_GH_CLI` /
     `CRYO_ZULIP_CLI` env vars so tests do not require real
     binaries on PATH.
4. `src/web/discovery.rs` — `populate_runtime` fills the new `sync`
   field when state files exist.
5. **Daemon pid file lifecycle** — integration test: start
   `cryo-gh sync-daemon` in a tempdir with a stubbed gh client,
   assert pid file appears; send SIGTERM, assert pid file removed.

Manual checklist for the UI is deferred to the implementation plan
alongside the Part A checklist.

Coverage target: ≥ 95 % per CLAUDE.md and project convention.
Achievable since all new logic is file IO plus subprocess shell-out,
both stubbable.

### B.9 Rollout order

1. Pid file instrumentation in the two `*_sync_daemon` commands
   plus the `is_sync_running` helpers.
2. `sync_common` module with `SyncSummary`, `summarize_all`, and
   the four lifecycle wrappers.
3. API endpoints — `GET /sync` plus four POSTs, wired into the
   existing `require_workspace` guard and SSE broadcast.
4. `ChamberEntry.sync` and `populate_runtime` extension.
5. UI `buildSyncBox` / `updateSyncBox` using Part A's build/update
   skeleton and the diff-signature pattern from `updateTodos`.
6. CSS additions in `templates/web.css`.

Each step is independently testable and leaves the codebase in a
working state.

---

## Sequencing of Parts A and B

Part A lands first. Part B's `buildSyncBox` / `updateSyncBox`
naturally extends Part A's `build`/`update` skeleton and reuses its
diff-signature pattern. The server-side work in Part B (pid files,
`sync_common`, API endpoints) is independent of Part A and can be
implemented in parallel, but the UI wiring in Part B assumes Part A
is already in place.
