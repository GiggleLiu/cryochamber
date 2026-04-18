# Multi-chamber `cryo web` — Design

**Date:** 2026-04-18
**Status:** Draft (pending user review)
**Scope:** Replace single-chamber `cryo web` with a workspace-scoped web UI that manages multiple cryochamber applications ("chambers").

## Problem

Today `cryo web` serves exactly one chamber — the directory it was launched from. A user running multiple chambers (e.g. chess-by-mail, mr-lazy, reports) needs one browser tab per chamber, one `cryo web` process per chamber, and no cross-chamber overview. There is no single pane of glass for "what are all my chambers doing right now."

## Goals

- One `cryo web` process serves N chambers, on one HTTP port.
- See all chambers at a glance: status, next wake, unread messages.
- Click into a chamber to get the same detail UI we already have (status, log, messages, send, wake) — plus lifecycle control.
- Lifecycle actions from the UI: `start`, `stop`, `restart`.
- Zero changes to `cryo start` / `cryo-agent` / daemon internals / per-chamber file layout.

## Non-goals (v1)

- Auth / HTTPS / remote access. Bind to `127.0.0.1` by default; `--host 0.0.0.0` still works but prints a warning because lifecycle actions now exist.
- Editing `cryo.toml`, managing TODO items, installing sync services (gh/zulip) from the UI. Those stay CLI.
- Serving multiple workspaces from one server.
- Authentication / token gating.

## Model

### Workspace layout

```
~/my-cryo-workspace/
  chambers/
    chess-by-mail/     <- cryo.toml here
    mr-lazy/           <- cryo.toml here
    reports/           <- cryo.toml here
```

A **chamber** is a directory containing a `cryo.toml`. A **workspace** is a directory containing a `chambers/` subdirectory. `cryo web` is run from the workspace dir.

Symlinks under `chambers/*` are followed and path-canonicalized so a symlinked chamber doesn't appear twice (once as "workspace", once as "external").

### Discovery

On server startup and on `POST /api/chambers/refresh`:

1. Scan `./chambers/*/cryo.toml` under the workspace (cwd where `cryo web` was started).
2. Call `registry::list()` — any running daemon whose path is NOT under `./chambers/` appears as an **external** chamber.
3. Merge keyed by canonicalized absolute path. Each entry is:

   ```
   ChamberEntry {
     id: String,              // URL-escaped canonicalized absolute path
     name: String,            // dir basename
     path: PathBuf,           // canonicalized
     source: Workspace | External,
     config_error: Option<String>,  // if cryo.toml parse failed
     running: bool,
     session: Option<u32>,
     next_wake: Option<String>,
     unread: usize,
   }
   ```

Discovery tolerates parse failures — a workspace chamber with a broken `cryo.toml` still shows up with `config_error: Some("...")`. The "start" button is disabled for those rows.

### Process model

One HTTP server. One port (default 8765). N chambers. Per-chamber daemons stay exactly as they are — the web server reads each chamber's `cryo.toml` / `timer.json` / `cryo.log` / `messages/` and talks to sockets / sends SIGUSR1 / etc. No changes to daemon code.

File watchers are spawned **lazily** per chamber the first time discovery sees it. One shared broadcast channel feeds the SSE stream. Watchers are dropped when a chamber disappears from the index.

## HTTP surface

All chamber endpoints are namespaced by `:id` (URL-escaped canonicalized absolute path).

**Discovery / list:**
- `GET  /api/chambers` → `[ChamberEntry, ...]`
- `POST /api/chambers/refresh` → re-run discovery, return fresh list.

**Per-chamber (mirrors today's endpoints, prefixed by chamber):**
- `GET  /api/chambers/:id/status`
- `GET  /api/chambers/:id/messages`
- `POST /api/chambers/:id/send`
- `POST /api/chambers/:id/wake`
- `POST /api/chambers/:id/start`    (new — workspace chambers only)
- `POST /api/chambers/:id/stop`     (new — workspace chambers only)
- `POST /api/chambers/:id/restart`  (new — workspace chambers only)

External chambers support everything except start/stop/restart — those are disabled in the UI and return HTTP 409 from the API.

**SSE (single connection, multiplexed):**
- `GET /api/events` → one stream for the whole UI. Each event carries `chamber_id` so the client routes updates to the sidebar and/or the current detail pane.

**Pages:**
- `GET /` → app shell with empty detail pane (shows workspace summary).
- `GET /c/:id` → app shell with chamber `:id` selected on load.
- `GET /assets/web.css` → shared stylesheet.

## Rust code layout

Split `src/web.rs` (currently ~680 lines) into a module:

```
src/web/
  mod.rs          – pub fn serve(), pub fn build_router()
  state.rs        – AppState { workspace_dir, chambers: Arc<RwLock<ChamberIndex>>, tx }
  discovery.rs    – scan chambers/*, merge with registry::list(), produce ChamberIndex
  routes/
    chambers.rs   – GET /api/chambers, POST /api/chambers/refresh
    chamber.rs    – status, messages, send, wake, start, stop, restart
    events.rs     – GET /api/events (SSE)
    pages.rs      – GET /, GET /c/:id (static HTML shell)
  watchers.rs     – spawn inotify watchers per chamber, feed the broadcast channel
  lifecycle.rs    – start_chamber / stop_chamber / restart_chamber
```

**Reuse, not rewrite.** The existing per-chamber helpers (status computation, message parsing, SSE event shape) move into `routes/chamber.rs` with a `dir: &Path` argument instead of implicit "the chamber." No behavior changes — just parameterize.

**Lifecycle.** New functions wrap the same service/process paths `cryo start` and `cryo restart` use today (`service::install`, `process::terminate_pid`, `state::save_state`). No shelling out to the `cryo` binary.

**Templates.** Split `templates/web.html` into `templates/web_shell.html` (the single-page app shell) and `templates/web.css` (shared CSS served at `/assets/web.css`).

## UI

**Single layout, always sidebar + main pane.** No separate list page vs detail page.

```
┌────────────────────────────┬──────────────────────────────────────────────┐
│ CRYOCHAMBER                │  chess-by-mail       [●] session #42         │
│ ──────────────────────────│  ───────────────────────────────────────────── │
│ [●] chess-by-mail     (0)  │  Task: ...      Next wake: in 1h 20m         │
│ [●] mr-lazy           (2)  │  [wake] [stop] [restart]                     │
│ [○] reports                │                                              │
│ [⚠] external-ext      (1)  │  ── Messages ──────────────────────────────  │
│                            │  inbox  human       Hello                    │
│ ──────────────────────────│  outbox agent       Got it                    │
│ [⟳ refresh]                │                                              │
│                            │  [send ▸]                                    │
│                            │                                              │
│                            │  ── Log ──────────────────────────────────── │
│                            │  [10:00:05] hibernate: ...                   │
└────────────────────────────┴──────────────────────────────────────────────┘
```

**Sidebar (left, always visible):** every chamber. Sort order running → stopped → external. Each row shows status dot, name, unread badge. Active row is highlighted. Footer has a refresh button and the workspace path.

- `●` green = daemon running
- `○` gray  = daemon stopped (workspace chamber)
- `⚠` blue  = external (running daemon outside `./chambers/`)
- `✗` red   = workspace chamber with `config_error`

**Main pane:** detail for the selected chamber — status / task / next wake / notes / messages / log / send widget, same widgets we have today, just scoped to the selected chamber. Header contains the chamber name, status dot, session number, and lifecycle buttons (`[start]` if stopped, `[stop] [restart]` if running, nothing for external).

When no chamber is selected (URL `/`) the main pane shows a workspace summary: counts of running / stopped / external and a prompt to pick a chamber.

**SSE dispatch (client side).** One `EventSource` connection. Events carry `chamber_id`. The sidebar listens to all events (to update status dots and unread badges). The main pane filters to `chamber_id == current`. Navigating between chambers does not reopen the connection; it just swaps the filter.

**Error surfaces.**
- Lifecycle action fails → toast with the error from the service/process layer; sidebar row status re-queries within 1s.
- Start on a chamber whose `cryo.toml` references a missing agent → surface preflight error in the toast (same check `cmd_start` already does).
- Workspace has no `chambers/` → main pane shows onboarding text (`mkdir chambers && move existing chamber under chambers/`) plus any external running daemons in the sidebar.

## CLI migration

- `cryo web` now requires a workspace dir (cwd containing `chambers/`, or at least a cwd that is not itself a chamber).
- If cwd *is* a chamber (has `cryo.toml`) but no `chambers/`, print a helpful error:
  > "cryo web now runs in workspace mode — cd into the parent of your chambers/ directory, or create chambers/ here. See docs/src/web.md for the migration path."
- `cryo web --stop` and service install/uninstall flow stay as-is, just keyed on the workspace dir. The service filename (`com.cryo.web.<hash>.plist`) derives from the workspace dir — different hash input, same mechanism.
- All other commands (`cryo start`, `cryo status`, `cryo-agent`, etc.) are untouched. Per-chamber file layout is unchanged.

**Migration for existing users** running `cryo web` inside a chamber:
```
mkdir -p ~/cryo-workspace/chambers
ln -s $(pwd) ~/cryo-workspace/chambers/<name>
cd ~/cryo-workspace && cryo web
```

The README and the mdbook will document this.

## Testing

**Unit**
- `discovery`: scan `chambers/*` in a tempdir, merge with a faked registry, assert external badging and canonicalization (symlink dedup).
- `routes/chamber`: handlers operate on a `&Path` parameter, not global state. Test against tempdirs.
- `lifecycle::start_chamber`, `stop_chamber`, `restart_chamber`: invoke the right service paths using a mock service backend (the existing `CRYO_NO_SERVICE` env var path works for this).

**Integration**
- Spin up two tempdirs as `chambers/a` and `chambers/b`. Run `build_router()` against a fake workspace. Hit `/api/chambers` and assert both appear. Hit `/api/chambers/:a/status` and assert expected JSON. `POST /api/chambers/:a/send`, read the inbox dir, assert the message file was created.
- SSE: write a file to `chambers/a/messages/inbox/`, assert a subscribed client receives an event with `chamber_id == a`.

**CI gates**
- `make check` (fmt + `clippy -D warnings` + tests) must pass.
- `make check-mock` extended with a multi-chamber lifecycle happy-path.

## Risks

1. **Lifecycle actions over `0.0.0.0` are a footgun.** Mitigation: print a loud warning on non-loopback bind; document in the mdbook. v2 can add token auth.
2. **Stopped workspace chambers with broken `cryo.toml`.** Mitigation: discovery tolerates parse failures; row shows `config_error`, start button disabled.
3. **Watcher fan-out on many chambers.** Mitigation: lazy per-chamber spawn, drop on disappearance, one shared broadcast channel.
4. **Symlinked chambers double-count.** Mitigation: canonicalize all paths before merging with the registry. Use the canonical path as the chamber id.
5. **Existing user muscle memory** — someone types `cryo web` in a chamber dir and gets an error. Mitigation: error message is a one-paragraph migration recipe; mdbook has a dedicated page.

## Out of scope for this design (future work)

- Token auth for remote / LAN usage.
- Editing `cryo.toml` from the UI.
- Managing TODO items from the UI.
- Installing sync services (gh/zulip) from the UI.
- Multi-workspace in one server.
- A `cryo web init` scaffolder that creates the workspace layout for you.
