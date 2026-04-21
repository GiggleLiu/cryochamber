# Web UI improvements: stable message view + sync surfaces

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the message box scrolling back to the top on every SSE tick (Part A) and add UI surfaces to detect and operate the `cryo-gh` / `cryo-zulip` sync daemons per chamber (Part B).

**Architecture:** Split the monolithic `renderDetail(id)` in `templates/web_shell.html` into a `buildDetail` / `updateDetail` pair with diff-aware sub-updaters and chat-style stick-to-bottom for the message box. Add pid files to the two sync daemons so "running" is a cheap local check, introduce `src/sync_common.rs` for a unified summary + four lifecycle wrappers that shell out to the existing CLIs, expose five new HTTP endpoints under `/api/chambers/:id/sync`, and render a per-backend row between the chamber header and todos.

**Tech Stack:** Rust (axum 0.8, libc for pid liveness check), vanilla JS in `templates/web_shell.html`, existing `signal_hook` crate for daemon shutdown handling.

**Spec:** `docs/superpowers/specs/2026-04-19-web-improvements-design.md`

---

## File Structure

**Create:**
- `src/sync_common.rs` — `SyncBackend` enum, `SyncSummary`, `summarize` / `summarize_all`, and `start` / `stop` / `pull` / `push` wrappers that shell out to `cryo-gh` / `cryo-zulip`.
- `src/web/routes/sync.rs` — `get_sync`, `post_sync_start`, `post_sync_stop`, `post_sync_pull`, `post_sync_push` handlers.

**Modify:**
- `src/gh_sync.rs` — add `sync_pid_path`, `read_sync_pid`, `is_sync_running`, and `summarize(dir) -> Option<SyncSummary>` helpers.
- `src/zulip_sync.rs` — mirror the gh_sync helpers.
- `src/bin/cryo_gh.rs` — `cmd_gh_sync_daemon` writes pid file on startup and removes it on exit.
- `src/bin/cryo_zulip.rs` — `cmd_sync_daemon` writes pid file on startup and removes it on exit.
- `src/lib.rs` — `pub mod sync_common;` (if not auto-picked by `src/web/routes/mod.rs`, also re-export).
- `src/web/discovery.rs` — `ChamberEntry` gains `sync: Vec<SyncBadge>`; `populate_runtime` fills it.
- `src/web/routes/mod.rs` — `pub mod sync;`.
- `src/web/mod.rs` — register five new routes in `build_router_with_state`.
- `templates/web_shell.html` — full JS refactor: `buildDetail` / `updateDetail` split, `updateMessages` append-only + stick-to-bottom, `buildSyncBox` / `updateSyncBox`.
- `templates/web.css` — add `.sync-box`, `.sync-row`, `.sync-dot`, `.sync-target`, `.sync-meta`, `.sync-actions` rules.

**Delete:** none.

---

## Part A — Stable message view

### Task A1: Refactor `renderDetail` into `buildDetail` + `updateDetail` skeleton

**Files:**
- Modify: `templates/web_shell.html:123-247` (the current `renderDetail` function)

- [ ] **Step 1: Read current `renderDetail` to understand the layout.**

The current function at `templates/web_shell.html:123` builds, in order:
header (line 134), todo box (169), message box (199), send box (215), log box (236). We keep the same DOM structure but split the build path from the update path.

- [ ] **Step 2: Introduce a `view` object and `buildDetail` / `updateDetail` functions alongside the existing `renderDetail`.**

Do **not** modify or delete `renderDetail`. Add the new helpers next to it so the UI keeps working unchanged through Tasks A2–A4 (the SSE handlers and lifecycle call sites still invoke `renderDetail`). Task A5 swaps them over once all helpers are implemented.

Insert the following inside the IIFE, immediately before the existing `renderDetail` definition:

```js
  // Per-chamber view state. Reset on every buildDetail.
  let view = null;

  function newView() {
    return {
      chamberId: null,
      headerName: null,
      headerMeta: null,       // "● session #N · Next wake: ..."
      headerTaskLine: null,
      planCompleteBox: null,
      lifecycleBtns: null,
      syncBox: null,          // container (may be empty)
      todosBox: null,
      msgBox: null,
      logBox: null,
      lastMsgKey: null,
      lastTodoSig: null,
      lastSyncSig: null,
      lastLifecycleSig: null,
    };
  }

  async function buildDetail(id) {
    const entry = state.chambers.find(c => c.id === id);
    if (!entry) return;
    const pane = document.getElementById('pane');
    const [status, messages, todos, sync] = await Promise.all([
      fetchJSON(`/api/chambers/${id}/status`),
      fetchJSON(`/api/chambers/${id}/messages`),
      fetchJSON(`/api/chambers/${id}/todos`),
      fetchJSON(`/api/chambers/${id}/sync`),
    ]);
    pane.innerHTML = '';
    view = newView();
    view.chamberId = id;

    buildHeader(pane, entry, status);
    view.syncBox = document.createElement('div');
    pane.appendChild(view.syncBox);
    buildSyncBox(sync);
    view.todosBox = document.createElement('div');
    pane.appendChild(view.todosBox);
    buildTodos(todos);
    buildMessages(messages);
    buildSendBar(pane, id);
    buildLogBox(pane, status);
    scrollMessagesToBottom();
  }

  async function updateDetail(id, parts) {
    if (!view || view.chamberId !== id) return;
    const fetches = {};
    if (parts.status) fetches.status = fetchJSON(`/api/chambers/${id}/status`);
    if (parts.messages) fetches.messages = fetchJSON(`/api/chambers/${id}/messages`);
    if (parts.todos) fetches.todos = fetchJSON(`/api/chambers/${id}/todos`);
    if (parts.sync) fetches.sync = fetchJSON(`/api/chambers/${id}/sync`);
    const entries = await Promise.allSettled(Object.entries(fetches).map(async ([k, p]) => [k, await p]));
    const data = {};
    for (const r of entries) {
      if (r.status === 'fulfilled') data[r.value[0]] = r.value[1];
    }
    const entry = state.chambers.find(c => c.id === id);
    if (entry && data.status) updateHeader(entry, data.status);
    if (entry && data.status) updateLifecycleButtons(entry);
    if (data.sync) updateSyncBox(data.sync);
    if (data.todos) updateTodos(data.todos);
    if (data.messages) updateMessages(data.messages);
  }

```

- [ ] **Step 3: Confirm the file still parses and nothing visible changed.**

`buildDetail` is defined but never called yet — the existing `renderDetail` still drives every path (SSE handlers, lifecycle actions, chamber switch, Refresh). Reload `make example-web` and confirm the browser console has no errors and the detail pane behaves exactly as before. Task A5 will swap call sites and delete the old `renderDetail` body once all helpers are implemented.

- [ ] **Step 4: Commit the skeleton.**

```
git add templates/web_shell.html
git commit -m "refactor(web): scaffold buildDetail/updateDetail skeleton"
```

---

### Task A2: Implement `buildHeader` + `updateHeader` + `updateLifecycleButtons`

**Files:**
- Modify: `templates/web_shell.html` (inside the IIFE)

- [ ] **Step 1: Add `buildHeader` that constructs the header once with cached span refs.**

Insert after `newView()`:

```js
  function buildHeader(pane, entry, status) {
    const header = document.createElement('div');
    header.style.padding = '12px 20px';
    header.style.borderBottom = '1px solid var(--border)';
    const top = document.createElement('div');
    top.style.display = 'flex';
    top.style.alignItems = 'center';
    top.style.justifyContent = 'space-between';

    const left = document.createElement('div');
    const name = document.createElement('strong');
    name.style.color = 'var(--accent)';
    name.textContent = entry.name;
    const meta = document.createElement('span');
    meta.style.color = 'var(--text-dim)';
    meta.style.marginLeft = '8px';
    left.appendChild(name);
    left.appendChild(meta);

    const btns = document.createElement('div');
    btns.className = 'lifecycle-buttons';

    top.appendChild(left);
    top.appendChild(btns);
    header.appendChild(top);

    const taskLine = document.createElement('div');
    taskLine.style.marginTop = '6px';
    taskLine.style.color = 'var(--text-dim)';
    taskLine.style.fontSize = '12px';
    header.appendChild(taskLine);

    const planBox = document.createElement('div');
    planBox.className = 'plan-complete';
    planBox.style.display = 'none';
    header.appendChild(planBox);

    pane.appendChild(header);
    view.headerName = name;
    view.headerMeta = meta;
    view.headerTaskLine = taskLine;
    view.planCompleteBox = planBox;
    view.lifecycleBtns = btns;

    updateHeader(entry, status);
    updateLifecycleButtons(entry);
  }
```

- [ ] **Step 2: Add `updateHeader` (string-only, no DOM rebuild).**

```js
  function updateHeader(entry, status) {
    const dot = statusDot(entry);
    const wake = status.next_wake ? ` · Next wake: ${status.next_wake}` : '';
    view.headerName.textContent = entry.name;
    view.headerMeta.textContent = `${dot} session #${status.session}${wake}`;
    if (status.completed) {
      view.planCompleteBox.style.display = '';
      view.planCompleteBox.textContent = `✓ Plan complete${status.completion_summary ? ': ' + status.completion_summary : ''}`;
      view.headerTaskLine.textContent = '';
    } else {
      view.planCompleteBox.style.display = 'none';
      view.headerTaskLine.textContent = status.task ? `Task: ${status.task}` : '';
    }
  }
```

- [ ] **Step 3: Add `updateLifecycleButtons` with a signature-diff guard.**

```js
  function updateLifecycleButtons(entry) {
    const sig = `${entry.source}|${entry.running}|${!!entry.config_error}`;
    if (sig === view.lastLifecycleSig) return;
    view.lastLifecycleSig = sig;
    const btns = view.lifecycleBtns;
    btns.innerHTML = '';
    if (entry.source === 'workspace') {
      if (entry.running) {
        btns.appendChild(btn('wake', () => lifecycle(entry.id, 'wake')));
        btns.appendChild(btn('stop', () => lifecycle(entry.id, 'stop')));
        btns.appendChild(btn('restart', () => lifecycle(entry.id, 'restart')));
      } else if (!entry.config_error) {
        btns.appendChild(btn('start', () => lifecycle(entry.id, 'start')));
      }
      if (!entry.config_error) {
        const resetBtn = btn('reset', () => confirmReset(entry.id, entry.name));
        resetBtn.classList.add('danger');
        btns.appendChild(resetBtn);
      }
    } else {
      btns.appendChild(btn('wake', () => lifecycle(entry.id, 'wake')));
    }
  }
```

- [ ] **Step 4: Commit.**

```
git add templates/web_shell.html
git commit -m "feat(web): buildHeader with diff-aware updateHeader/lifecycle buttons"
```

---

### Task A3: Implement `buildTodos` + `updateTodos` with signature diff

**Files:**
- Modify: `templates/web_shell.html` (inside the IIFE)

- [ ] **Step 1: Add `buildTodos`.**

```js
  function buildTodos(todos) {
    view.todosBox.className = 'todos';
    view.todosBox.style.display = 'none';
    updateTodos(todos);
  }
```

- [ ] **Step 2: Add `updateTodos` with a signature check.**

```js
  function todoSignature(todos) {
    if (!todos || !todos.length) return '0:';
    return todos.length + ':' + todos.map(t => `${t.id}|${t.done ? 1 : 0}|${t.at || ''}`).join(',');
  }

  function updateTodos(todos) {
    const sig = todoSignature(todos);
    if (sig === view.lastTodoSig) return;
    view.lastTodoSig = sig;
    const box = view.todosBox;
    box.innerHTML = '';
    if (!todos || !todos.length) {
      box.style.display = 'none';
      return;
    }
    box.style.display = '';
    const title = document.createElement('div');
    title.className = 'todos-title';
    const pending = todos.filter(t => !t.done).length;
    title.textContent = `Todos (${pending} pending / ${todos.length} total)`;
    box.appendChild(title);
    for (const t of todos) {
      const row = document.createElement('div');
      row.className = 'todo-row' + (t.done ? ' done' : '');
      const check = document.createElement('span');
      check.className = 'todo-check';
      check.textContent = t.done ? '☑' : '☐';
      const text = document.createElement('span');
      text.className = 'todo-text';
      text.textContent = t.text;
      row.appendChild(check);
      row.appendChild(text);
      if (t.at) {
        const when = document.createElement('span');
        when.className = 'todo-at';
        when.textContent = t.at;
        row.appendChild(when);
      }
      box.appendChild(row);
    }
  }
```

- [ ] **Step 3: Commit.**

```
git add templates/web_shell.html
git commit -m "feat(web): diff-aware updateTodos with signature guard"
```

---

### Task A4: Implement `buildMessages` + `updateMessages` (append-only + stick-to-bottom)

**Files:**
- Modify: `templates/web_shell.html` (inside the IIFE)

- [ ] **Step 1: Add the key + hash helpers and `buildMessages`.**

```js
  // Small 32-bit FNV-1a. Enough to disambiguate same-timestamp messages.
  function hash32(s) {
    let h = 0x811c9dc5 >>> 0;
    for (let i = 0; i < s.length; i++) {
      h ^= s.charCodeAt(i);
      h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h.toString(36);
  }

  function messageKey(m) {
    return `${m.direction}|${m.timestamp}|${m.from}|${hash32(m.body || '')}`;
  }

  function buildMessageRow(m) {
    const row = document.createElement('div');
    row.style.marginBottom = '10px';
    row.style.padding = '8px';
    row.style.background = m.direction === 'inbox' ? 'var(--inbox-bg)' : 'var(--outbox-bg)';
    row.style.borderRadius = '3px';
    row.dataset.key = messageKey(m);
    const meta = document.createElement('div');
    meta.style.fontSize = '11px';
    meta.style.color = 'var(--text-dim)';
    meta.textContent = `${m.direction} · ${m.from} · ${m.timestamp}`;
    const body = document.createElement('div');
    body.style.marginTop = '4px';
    body.style.whiteSpace = 'pre-wrap';
    body.textContent = m.body;
    row.appendChild(meta);
    row.appendChild(body);
    return row;
  }

  function buildMessages(messages) {
    const box = document.createElement('div');
    box.id = 'msg-box';
    box.style.flex = '1';
    box.style.overflowY = 'auto';
    box.style.padding = '12px 20px';
    document.getElementById('pane').appendChild(box);
    view.msgBox = box;
    view.lastMsgKey = null;
    for (const m of messages || []) {
      box.appendChild(buildMessageRow(m));
      view.lastMsgKey = messageKey(m);
    }
  }
```

- [ ] **Step 2: Add the stick-to-bottom helper and `updateMessages`.**

```js
  function isAtBottom(el) {
    return (el.scrollHeight - el.scrollTop - el.clientHeight) < 40;
  }

  function scrollMessagesToBottom() {
    if (view && view.msgBox) view.msgBox.scrollTop = view.msgBox.scrollHeight;
  }

  function updateMessages(messages) {
    if (!view.msgBox) return;
    const box = view.msgBox;
    const wasAtBottom = isAtBottom(box);

    // Locate index immediately after lastMsgKey in the new array.
    let startAt = 0;
    if (view.lastMsgKey) {
      const idx = messages.findIndex(m => messageKey(m) === view.lastMsgKey);
      if (idx < 0) {
        // Fallback: full rebuild (e.g. chamber reset archived history).
        box.innerHTML = '';
        for (const m of messages) box.appendChild(buildMessageRow(m));
        view.lastMsgKey = messages.length ? messageKey(messages[messages.length - 1]) : null;
        box.scrollTop = box.scrollHeight;
        return;
      }
      startAt = idx + 1;
    }
    for (let i = startAt; i < messages.length; i++) {
      box.appendChild(buildMessageRow(messages[i]));
      view.lastMsgKey = messageKey(messages[i]);
    }
    if (wasAtBottom) box.scrollTop = box.scrollHeight;
  }
```

- [ ] **Step 3: Add `buildSendBar` and `buildLogBox` so the pane is complete.**

These replace the tail of the old `renderDetail`. They are build-only (send bar is stateless; log box already has append-only behavior on `log` SSE events).

```js
  function buildSendBar(pane, id) {
    const send = document.createElement('div');
    send.style.padding = '10px 20px';
    send.style.borderTop = '1px solid var(--border)';
    send.innerHTML = `
      <input id="send-body" placeholder="Message..." style="width:70%;padding:6px;background:var(--surface2);border:1px solid var(--border);color:var(--text);font-family:inherit;">
      <button id="send-btn" style="padding:6px 12px;background:var(--accent-dim);color:var(--accent);border:none;cursor:pointer;font-family:inherit;">send</button>
    `;
    send.querySelector('#send-btn').addEventListener('click', async () => {
      const body = send.querySelector('#send-body').value.trim();
      if (!body) return;
      try {
        await fetchJSON(`/api/chambers/${id}/send`, {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({body}),
        });
        send.querySelector('#send-body').value = '';
      } catch (e) { toast(e.message, 'error'); }
    });
    pane.appendChild(send);
  }

  function buildLogBox(pane, status) {
    const logBox = document.createElement('pre');
    logBox.id = 'log-box';
    logBox.style.background = 'var(--bg)';
    logBox.style.color = 'var(--text-dim)';
    logBox.style.padding = '10px 20px';
    logBox.style.maxHeight = '200px';
    logBox.style.overflowY = 'auto';
    logBox.style.fontSize = '11px';
    logBox.style.borderTop = '1px solid var(--border)';
    logBox.textContent = status.log_tail || '';
    pane.appendChild(logBox);
    view.logBox = logBox;
  }
```

- [ ] **Step 4: Commit.**

```
git add templates/web_shell.html
git commit -m "feat(web): append-only updateMessages with stick-to-bottom"
```

---

### Task A5: Rewire SSE routing to call `updateDetail` (not `renderDetail`)

**Files:**
- Modify: `templates/web_shell.html` (the SSE handlers at the bottom of the IIFE)

- [ ] **Step 1: Replace the SSE handler block.**

Find the block that starts with `const evt = new EventSource('/api/events');` (around the original line 276 in the pre-refactor file, now just below your new build/update helpers). Replace it with:

```js
  const evt = new EventSource('/api/events');
  evt.addEventListener('message', async e => {
    const d = JSON.parse(e.data);
    if (state.currentId === d.chamber_id) {
      await updateDetail(state.currentId, { messages: true });
    }
    await loadChambers();
  });
  evt.addEventListener('status', async e => {
    const d = JSON.parse(e.data);
    if (state.currentId === d.chamber_id) {
      await updateDetail(state.currentId, { status: true, todos: true, sync: true });
    }
    await loadChambers();
  });
  evt.addEventListener('log', e => {
    const d = JSON.parse(e.data);
    if (state.currentId === d.chamber_id && view && view.logBox) {
      view.logBox.textContent += (view.logBox.textContent ? '\n' : '') + d.line;
      view.logBox.scrollTop = view.logBox.scrollHeight;
    }
  });
  evt.addEventListener('index', async () => { await loadChambers(); });
```

- [ ] **Step 2: Update the `selectChamber`, refresh, and lifecycle call sites to use `buildDetail` and delete the old `renderDetail` function.**

Find every `renderDetail(` call remaining in the file after Step 1:

- `selectChamber` (line ~109 of the original file): `await renderDetail(id)` → `await buildDetail(id)`
- `lifecycle` (line ~261): `if (state.currentId === id) await renderDetail(id);` → `... await buildDetail(id);`
- Refresh button handler (line ~272): `if (state.currentId) await renderDetail(state.currentId);` → `... await buildDetail(state.currentId);`

Then delete the entire `async function renderDetail(id) { ... }` definition (the old body that spans ~125 lines). `buildDetail` now owns the rebuild path; the old function is unused.

- [ ] **Step 3: Manual smoke test.**

```
make example-web
```

Visit `http://127.0.0.1:8765/`. The `/api/chambers/:id/sync` calls in `buildDetail` will 404 in the dev console because the endpoint isn't live yet. Add a temporary defensive guard **only inside `buildDetail`** so Part A is exercisable without Part B:

```js
fetchJSON(`/api/chambers/${id}/sync`).catch(() => []),
```

Replace the existing `fetchJSON(`/api/chambers/${id}/sync`)` inside `buildDetail`'s `Promise.all` with the `.catch(() => [])` wrapped version. Remove this guard at the end of Task B9 (after the endpoint exists).

Smoke test:

- Select a chamber. The detail pane builds.
- Send a message in another terminal (`cryo-agent send --body hello`). You should see it appended and auto-scrolled.
- Scroll up a few messages. Send another message. The new message should append but your scroll position should not jump.

- [ ] **Step 4: Commit.**

```
git add templates/web_shell.html
git commit -m "feat(web): route SSE events to updateDetail (incremental DOM)"
```

---

### Task A6: Manual validation checklist for Part A

**Files:** none modified.

- [ ] **Step 1: Run the checklist from `docs/superpowers/specs/2026-04-19-web-improvements-design.md` section A.8.**

Start the example workspace and walk through:

1. Idle running chamber: scroll partway up in the message box; wait 30 s; scroll position unchanged.
2. Scrolled to bottom: send a message via another terminal; message appears and auto-scrolls.
3. Scrolled up: send a message; view does not yank.
4. Switch chambers: new chamber opens scrolled to most recent message.
5. Active session with frequent `timer.json` updates: todos list does not visibly flicker; header numbers update in place.
6. Reset chamber: message list rebuilds cleanly and scrolls to bottom.

If any check fails, fix inline before committing Part B. Commit nothing if all checks pass — this is a validation gate only.

---

## Part B — Sync surfaces

### Task B1: Pid file helpers in `src/gh_sync.rs` (TDD)

**Files:**
- Modify: `src/gh_sync.rs`
- Test: `src/gh_sync.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write failing tests at the bottom of `src/gh_sync.rs`.**

Append to the file (keep any existing `#[cfg(test)]` block intact; add a new one or extend):

```rust
#[cfg(test)]
mod pid_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn pid_path_points_into_dir() {
        let p = sync_pid_path(std::path::Path::new("/tmp/cryo-x"));
        assert_eq!(p, std::path::Path::new("/tmp/cryo-x/cryo-gh-sync.pid"));
    }

    #[test]
    fn read_missing_pid_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_sync_pid(dir.path()).is_none());
    }

    #[test]
    fn read_present_pid_returns_value() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(sync_pid_path(dir.path())).unwrap();
        f.write_all(b"12345\n").unwrap();
        assert_eq!(read_sync_pid(dir.path()), Some(12345));
    }

    #[test]
    fn read_invalid_pid_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(sync_pid_path(dir.path()), "not-a-number").unwrap();
        assert!(read_sync_pid(dir.path()).is_none());
    }

    #[test]
    fn running_is_false_when_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_sync_running(dir.path()));
    }

    #[test]
    fn running_is_false_for_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        // PID 1 is always alive, but we need a *dead* pid: spawn+wait a child.
        let child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        let _ = child.wait_with_output();
        std::fs::write(sync_pid_path(dir.path()), dead_pid.to_string()).unwrap();
        assert!(!is_sync_running(dir.path()));
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail.**

```
cargo test --lib gh_sync::pid_tests
```

Expected: compile errors for `sync_pid_path`, `read_sync_pid`, `is_sync_running` not found.

- [ ] **Step 3: Implement the helpers.**

Append to `src/gh_sync.rs` (before the `#[cfg(test)]` block):

```rust
use std::path::PathBuf;

pub fn sync_pid_path(dir: &Path) -> PathBuf {
    dir.join("cryo-gh-sync.pid")
}

pub fn read_sync_pid(dir: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(sync_pid_path(dir)).ok()?;
    content.trim().parse::<u32>().ok()
}

pub fn is_sync_running(dir: &Path) -> bool {
    match read_sync_pid(dir) {
        Some(pid) => {
            let ret = unsafe { libc::kill(pid as i32, 0) };
            if ret == 0 {
                return true;
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            errno == libc::EPERM
        }
        None => false,
    }
}
```

- [ ] **Step 4: Run the tests again.**

```
cargo test --lib gh_sync::pid_tests
```

Expected: all 6 tests pass.

- [ ] **Step 5: Commit.**

```
git add src/gh_sync.rs
git commit -m "feat(gh_sync): add pid file helpers (sync_pid_path/read_sync_pid/is_sync_running)"
```

---

### Task B2: Pid file helpers in `src/zulip_sync.rs` (TDD)

**Files:**
- Modify: `src/zulip_sync.rs`

- [ ] **Step 1: Write failing tests.**

Append to `src/zulip_sync.rs` — identical structure to Task B1's test block, but with:
- `sync_pid_path(...)` returns `cryo-zulip-sync.pid`
- The module is `pid_tests`

```rust
#[cfg(test)]
mod pid_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn pid_path_points_into_dir() {
        let p = sync_pid_path(std::path::Path::new("/tmp/cryo-x"));
        assert_eq!(p, std::path::Path::new("/tmp/cryo-x/cryo-zulip-sync.pid"));
    }

    #[test]
    fn read_missing_pid_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_sync_pid(dir.path()).is_none());
    }

    #[test]
    fn read_present_pid_returns_value() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(sync_pid_path(dir.path())).unwrap();
        f.write_all(b"12345\n").unwrap();
        assert_eq!(read_sync_pid(dir.path()), Some(12345));
    }

    #[test]
    fn read_invalid_pid_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(sync_pid_path(dir.path()), "not-a-number").unwrap();
        assert!(read_sync_pid(dir.path()).is_none());
    }

    #[test]
    fn running_is_false_when_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_sync_running(dir.path()));
    }

    #[test]
    fn running_is_false_for_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        let _ = child.wait_with_output();
        std::fs::write(sync_pid_path(dir.path()), dead_pid.to_string()).unwrap();
        assert!(!is_sync_running(dir.path()));
    }
}
```

- [ ] **Step 2: Confirm failing tests.**

```
cargo test --lib zulip_sync::pid_tests
```

- [ ] **Step 3: Implement the helpers.**

Append to `src/zulip_sync.rs`:

```rust
use std::path::PathBuf;

pub fn sync_pid_path(dir: &Path) -> PathBuf {
    dir.join("cryo-zulip-sync.pid")
}

pub fn read_sync_pid(dir: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(sync_pid_path(dir)).ok()?;
    content.trim().parse::<u32>().ok()
}

pub fn is_sync_running(dir: &Path) -> bool {
    match read_sync_pid(dir) {
        Some(pid) => {
            let ret = unsafe { libc::kill(pid as i32, 0) };
            if ret == 0 {
                return true;
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            errno == libc::EPERM
        }
        None => false,
    }
}
```

- [ ] **Step 4: Run tests.**

```
cargo test --lib zulip_sync::pid_tests
```

Expected: all 6 pass.

- [ ] **Step 5: Commit.**

```
git add src/zulip_sync.rs
git commit -m "feat(zulip_sync): add pid file helpers matching gh_sync"
```

---

### Task B3: Write pid file from `cmd_gh_sync_daemon` + test

**Files:**
- Modify: `src/bin/cryo_gh.rs:235-325` (the `cmd_gh_sync_daemon` function)

- [ ] **Step 1: Write the pid file at the top of the daemon function.**

In `cmd_gh_sync_daemon`, immediately after the existing `eprintln!("Sync daemon started (PID {})", ...)` (line 240), add:

```rust
    let pid_path = cryochamber::gh_sync::sync_pid_path(&dir);
    std::fs::write(&pid_path, std::process::id().to_string())
        .context("Failed to write cryo-gh-sync.pid")?;
```

- [ ] **Step 2: Remove the pid file after the main loop exits.**

After the `eprintln!("Sync: stopped");` line (around 323) and before `Ok(())`, add:

```rust
    let _ = std::fs::remove_file(&pid_path);
```

This runs on clean loop exit (either shutdown signal or channel disconnect). If the daemon is SIGKILLed the file stays, but `is_sync_running` then returns false because `kill(pid, 0)` returns ESRCH.

- [ ] **Step 3: Verify the existing test suite still passes.**

```
cargo test --lib
```

No new Rust test here — the integration behavior is covered by the smoke test in Step 4 and by the daemon-level tests in Task B11.

- [ ] **Step 4: Manual smoke test.**

In a temp workspace:

```
mkdir -p /tmp/cryo-smoke && cd /tmp/cryo-smoke
cryo init .
# Skip actual gh init; just spawn the daemon directly with CRYO_NO_SERVICE=1:
# Create a minimal gh-sync.json so the daemon doesn't immediately abort.
cat > gh-sync.json <<EOF
{"repo":"fake/fake","discussion_number":1,"discussion_node_id":"fake"}
EOF
CRYO_NO_SERVICE=1 cryo-gh sync-daemon --interval 60 &
sleep 1
ls cryo-gh-sync.pid && cat cryo-gh-sync.pid    # should show running PID
kill %1 && wait %1 2>/dev/null
ls cryo-gh-sync.pid                             # should now be "No such file"
```

You may see a pull error in the daemon's stderr (fake repo) — that's expected and unrelated. The point is to see the pid file appear and disappear.

- [ ] **Step 5: Commit.**

```
git add src/bin/cryo_gh.rs
git commit -m "feat(cryo-gh): write cryo-gh-sync.pid from sync-daemon"
```

---

### Task B4: Write pid file from `cmd_sync_daemon` in `cryo_zulip.rs`

**Files:**
- Modify: `src/bin/cryo_zulip.rs:245-` (the `cmd_sync_daemon` function)

- [ ] **Step 1: Write the pid file at startup.**

After `eprintln!("Zulip sync daemon started (PID {})", std::process::id());` (line 250), add:

```rust
    let pid_path = cryochamber::zulip_sync::sync_pid_path(&dir);
    std::fs::write(&pid_path, std::process::id().to_string())
        .context("Failed to write cryo-zulip-sync.pid")?;
```

- [ ] **Step 2: Remove the pid file after the main loop exits.**

Find the end of the main `loop { ... }` in `cmd_sync_daemon` (follows the same pattern as `cryo_gh.rs`). After the loop exits — immediately before the final `Ok(())` — add:

```rust
    let _ = std::fs::remove_file(&pid_path);
```

- [ ] **Step 3: Run the existing test suite.**

```
cargo test --lib
```

- [ ] **Step 4: Commit.**

```
git add src/bin/cryo_zulip.rs
git commit -m "feat(cryo-zulip): write cryo-zulip-sync.pid from sync-daemon"
```

---

### Task B5: `src/sync_common.rs` — `SyncSummary` + summarizers (TDD)

**Files:**
- Create: `src/sync_common.rs`
- Modify: `src/lib.rs` (add `pub mod sync_common;`)
- Modify: `src/gh_sync.rs` (add `summarize(dir) -> Option<SyncSummary>`)
- Modify: `src/zulip_sync.rs` (add `summarize(dir) -> Option<SyncSummary>`)

- [ ] **Step 1: Create `src/sync_common.rs` with the types (no impl yet).**

```rust
//! Shared sync backend abstraction: summary types and lifecycle wrappers.
//! Two backends (gh, zulip) with near-identical verbs — free functions are
//! enough; no trait needed.

use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncBackend {
    Gh,
    Zulip,
}

impl SyncBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncBackend::Gh => "gh",
            SyncBackend::Zulip => "zulip",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gh" => Some(SyncBackend::Gh),
            "zulip" => Some(SyncBackend::Zulip),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncSummary {
    pub backend: SyncBackend,
    pub configured: bool,
    pub installed: bool,
    pub running: bool,
    pub target: String,
    pub last_pushed_session: Option<u32>,
    pub log_tail_path: PathBuf,
}

pub fn summarize(backend: SyncBackend, dir: &Path) -> Option<SyncSummary> {
    match backend {
        SyncBackend::Gh => crate::gh_sync::summarize(dir),
        SyncBackend::Zulip => crate::zulip_sync::summarize(dir),
    }
}

pub fn summarize_all(dir: &Path) -> Vec<SyncSummary> {
    [SyncBackend::Gh, SyncBackend::Zulip]
        .into_iter()
        .filter_map(|b| summarize(b, dir))
        .collect()
}

// Lifecycle wrappers (implemented in Task B6).
pub fn start(_backend: SyncBackend, _dir: &Path) -> Result<()> {
    anyhow::bail!("not implemented")
}
pub fn stop(_backend: SyncBackend, _dir: &Path) -> Result<()> {
    anyhow::bail!("not implemented")
}
pub fn pull(_backend: SyncBackend, _dir: &Path) -> Result<()> {
    anyhow::bail!("not implemented")
}
pub fn push(_backend: SyncBackend, _dir: &Path) -> Result<()> {
    anyhow::bail!("not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_parse_roundtrip() {
        assert_eq!(SyncBackend::parse("gh"), Some(SyncBackend::Gh));
        assert_eq!(SyncBackend::parse("zulip"), Some(SyncBackend::Zulip));
        assert_eq!(SyncBackend::parse("nope"), None);
        assert_eq!(SyncBackend::Gh.as_str(), "gh");
        assert_eq!(SyncBackend::Zulip.as_str(), "zulip");
    }

    #[test]
    fn summarize_all_empty_for_unconfigured_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(summarize_all(dir.path()).is_empty());
    }

    #[test]
    fn summarize_all_returns_configured_backends() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::gh_sync::GhSyncState {
            repo: "alice/notes".into(),
            discussion_number: 7,
            discussion_node_id: "node".into(),
            last_read_cursor: None,
            self_login: None,
            last_pushed_session: Some(3),
        };
        crate::gh_sync::save_sync_state(
            &dir.path().join("gh-sync.json"),
            &state,
        )
        .unwrap();

        let summaries = summarize_all(dir.path());
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].backend, SyncBackend::Gh);
        assert_eq!(summaries[0].target, "alice/notes#7");
        assert_eq!(summaries[0].last_pushed_session, Some(3));
        assert!(!summaries[0].running);
    }
}
```

- [ ] **Step 2: Add `pub mod sync_common;` to `src/lib.rs`.**

Open `src/lib.rs`, find the other `pub mod X;` lines (they appear in a block near the top), and add:

```rust
pub mod sync_common;
```

- [ ] **Step 3: Add `summarize` to `src/gh_sync.rs`.**

Append (before the `#[cfg(test)]` blocks):

```rust
pub fn summarize(dir: &Path) -> Option<crate::sync_common::SyncSummary> {
    let state = load_sync_state(&dir.join("gh-sync.json")).ok().flatten()?;
    Some(crate::sync_common::SyncSummary {
        backend: crate::sync_common::SyncBackend::Gh,
        configured: true,
        installed: crate::service::is_installed("gh-sync", dir),
        running: is_sync_running(dir),
        target: format!("{}#{}", state.repo, state.discussion_number),
        last_pushed_session: state.last_pushed_session,
        log_tail_path: dir.join("cryo-gh-sync.log"),
    })
}
```

- [ ] **Step 4: Add `summarize` to `src/zulip_sync.rs`.**

```rust
pub fn summarize(dir: &Path) -> Option<crate::sync_common::SyncSummary> {
    let state = load_sync_state(&dir.join("zulip-sync.json")).ok().flatten()?;
    Some(crate::sync_common::SyncSummary {
        backend: crate::sync_common::SyncBackend::Zulip,
        configured: true,
        installed: crate::service::is_installed("zulip-sync", dir),
        running: is_sync_running(dir),
        target: format!("{} · {} / {}", state.site, state.stream, state.topic_name()),
        last_pushed_session: state.last_pushed_session,
        log_tail_path: dir.join("cryo-zulip-sync.log"),
    })
}
```

- [ ] **Step 5: Run tests.**

```
cargo test --lib sync_common::tests
cargo test --lib gh_sync
cargo test --lib zulip_sync
```

Expected: all pass.

- [ ] **Step 6: Commit.**

```
git add src/sync_common.rs src/lib.rs src/gh_sync.rs src/zulip_sync.rs
git commit -m "feat(sync_common): add SyncSummary + per-backend summarize()"
```

---

### Task B6: `src/sync_common.rs` — lifecycle wrappers (shell out to CLIs) (TDD)

**Files:**
- Modify: `src/sync_common.rs`

- [ ] **Step 1: Write failing tests for CLI resolution and start/stop/pull/push.**

Append to the `tests` mod in `src/sync_common.rs`:

```rust
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn make_stub(dir: &Path, name: &str, exit_code: i32, stdout: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "echo {stdout}").unwrap();
        writeln!(f, "exit {exit_code}").unwrap();
        let mut perms = std::fs::metadata(&p).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&p, perms).unwrap();
        p
    }

    #[test]
    fn start_invokes_sync_subcommand_via_env_override() {
        let bin = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let stub = make_stub(bin.path(), "cryo-gh-stub", 0, "ok");
        std::env::set_var("CRYO_GH_CLI", &stub);
        let res = start(SyncBackend::Gh, work.path());
        std::env::remove_var("CRYO_GH_CLI");
        assert!(res.is_ok(), "{res:?}");
    }

    #[test]
    fn stop_propagates_non_zero_exit_as_error() {
        let bin = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let stub = make_stub(bin.path(), "cryo-gh-stub", 7, "boom");
        std::env::set_var("CRYO_GH_CLI", &stub);
        let res = stop(SyncBackend::Gh, work.path());
        std::env::remove_var("CRYO_GH_CLI");
        assert!(res.is_err());
    }

    #[test]
    fn pull_and_push_use_zulip_env_override() {
        let bin = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let stub = make_stub(bin.path(), "cryo-zulip-stub", 0, "ok");
        std::env::set_var("CRYO_ZULIP_CLI", &stub);
        assert!(pull(SyncBackend::Zulip, work.path()).is_ok());
        assert!(push(SyncBackend::Zulip, work.path()).is_ok());
        std::env::remove_var("CRYO_ZULIP_CLI");
    }
```

- [ ] **Step 2: Run to confirm failure.**

```
cargo test --lib sync_common::tests
```

Expected: `start_invokes_sync_subcommand_via_env_override` fails with "not implemented".

- [ ] **Step 3: Replace the four stub functions with real implementations.**

Replace the four `bail!("not implemented")` stubs in `src/sync_common.rs` with:

```rust
fn resolve_cli(backend: SyncBackend) -> Result<std::path::PathBuf> {
    let (env_var, bin_name) = match backend {
        SyncBackend::Gh => ("CRYO_GH_CLI", "cryo-gh"),
        SyncBackend::Zulip => ("CRYO_ZULIP_CLI", "cryo-zulip"),
    };
    if let Ok(p) = std::env::var(env_var) {
        return Ok(std::path::PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let sibling = parent.join(bin_name);
            if sibling.exists() {
                return Ok(sibling);
            }
        }
    }
    // Fall back to PATH lookup
    if let Ok(output) = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin_name}"))
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(std::path::PathBuf::from(path));
            }
        }
    }
    anyhow::bail!("{bin_name} binary not found (tried ${env_var}, sibling of current exe, $PATH)");
}

fn run_subcommand(backend: SyncBackend, dir: &Path, sub: &str) -> Result<()> {
    let cli = resolve_cli(backend)?;
    let output = std::process::Command::new(&cli)
        .current_dir(dir)
        .arg(sub)
        .output()
        .with_context(|| format!("Failed to spawn {}", cli.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let truncated: String = stderr.chars().take(500).collect();
        anyhow::bail!(
            "{} {sub} exited with {}: {}",
            cli.display(),
            output.status,
            truncated.trim()
        );
    }
    Ok(())
}

pub fn start(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "sync")
}
pub fn stop(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "unsync")
}
pub fn pull(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "pull")
}
pub fn push(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "push")
}
```

Add `use anyhow::Context;` to the imports at the top if not already present.

- [ ] **Step 4: Run tests.**

```
cargo test --lib sync_common::tests
```

Expected: all tests pass.

- [ ] **Step 5: Commit.**

```
git add src/sync_common.rs
git commit -m "feat(sync_common): lifecycle wrappers shelling out to cryo-gh/cryo-zulip"
```

---

### Task B7: `GET /api/chambers/:id/sync` endpoint (TDD)

**Files:**
- Create: `src/web/routes/sync.rs`
- Modify: `src/web/routes/mod.rs` (add `pub mod sync;`)
- Modify: `src/web/mod.rs` (register the route)

- [ ] **Step 1: Create `src/web/routes/sync.rs` with the `get_sync` handler and tests.**

```rust
//! Per-chamber sync backend handlers. Delegates to `sync_common` for
//! summaries; `require_workspace` guards the mutating endpoints.

use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Json,
};
use serde_json::Value;

use crate::sync_common::{self, SyncBackend};
use crate::web::discovery::Source;
use crate::web::state::{AppState, SseEvent};

fn require_workspace(entry: &crate::web::discovery::ChamberEntry) -> Result<(), StatusCode> {
    if entry.source == Source::External {
        return Err(StatusCode::CONFLICT);
    }
    Ok(())
}

pub async fn get_sync(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let summaries = sync_common::summarize_all(&path);
    Ok(Json(serde_json::to_value(summaries).unwrap_or(Value::Array(vec![]))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::discovery::{encode_id, ChamberEntry};

    #[tokio::test]
    async fn get_sync_returns_empty_for_unconfigured_chamber() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();

        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();
        let id = encode_id(&alpha.canonicalize().unwrap());
        let res = get_sync(State(app), AxumPath(id)).await.unwrap();
        assert_eq!(res.0, serde_json::json!([]));
    }

    #[tokio::test]
    async fn get_sync_reports_configured_gh_backend() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
        let state = crate::gh_sync::GhSyncState {
            repo: "alice/x".into(),
            discussion_number: 1,
            discussion_node_id: "n".into(),
            last_read_cursor: None,
            self_login: None,
            last_pushed_session: None,
        };
        crate::gh_sync::save_sync_state(&alpha.join("gh-sync.json"), &state).unwrap();

        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();
        let id = encode_id(&alpha.canonicalize().unwrap());
        let res = get_sync(State(app), AxumPath(id)).await.unwrap();
        let arr = res.0.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["backend"], "gh");
        assert_eq!(arr[0]["target"], "alice/x#1");
    }
}
```

- [ ] **Step 2: Add `pub mod sync;` to `src/web/routes/mod.rs`.**

```rust
pub mod sync;
```

Keep the other `pub mod` lines intact.

- [ ] **Step 3: Register the route in `src/web/mod.rs`.**

Inside `build_router_with_state`, add the following in the existing chain (after the `/reset` route, before `/api/events`):

```rust
        .route(
            "/api/chambers/{id}/sync",
            get(crate::web::routes::sync::get_sync),
        )
```

- [ ] **Step 4: Run tests.**

```
cargo test --lib web::routes::sync
```

Expected: both tests pass.

- [ ] **Step 5: Commit.**

```
git add src/web/routes/sync.rs src/web/routes/mod.rs src/web/mod.rs
git commit -m "feat(web): GET /api/chambers/:id/sync endpoint"
```

---

### Task B8: `POST /api/chambers/:id/sync/:backend/{start,stop,pull,push}` endpoints (TDD)

**Files:**
- Modify: `src/web/routes/sync.rs`
- Modify: `src/web/mod.rs`

- [ ] **Step 1: Write failing tests for 409 on external chambers and backend-name validation.**

Append to the `tests` mod in `src/web/routes/sync.rs`:

```rust
    use crate::web::discovery::Source;

    #[tokio::test]
    async fn post_sync_start_returns_409_for_external_chamber() {
        let dir = tempfile::tempdir().unwrap();
        let external = dir.path().join("outside");
        std::fs::create_dir_all(&external).unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        let id = encode_id(&external.canonicalize().unwrap());
        let entry = ChamberEntry {
            id: id.clone(),
            name: "outside".into(),
            path: external.canonicalize().unwrap(),
            source: Source::External,
            config_error: None,
            running: true,
            session: None,
            next_wake: None,
            unread: 0,
            completed: false,
            sync: vec![],
        };
        app.chambers.write().unwrap().insert(id.clone(), entry);

        let err = post_sync_action(State(app), AxumPath((id, "gh".into(), "start".into())))
            .await
            .unwrap_err();
        assert_eq!(err, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn post_sync_action_rejects_unknown_backend() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();
        let id = encode_id(&alpha.canonicalize().unwrap());
        let err = post_sync_action(
            State(app),
            AxumPath((id, "bogus".into(), "start".into())),
        )
        .await
        .unwrap_err();
        assert_eq!(err, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn post_sync_action_rejects_unknown_verb() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();
        let id = encode_id(&alpha.canonicalize().unwrap());
        let err = post_sync_action(State(app), AxumPath((id, "gh".into(), "dance".into())))
            .await
            .unwrap_err();
        assert_eq!(err, StatusCode::BAD_REQUEST);
    }
```

**Note:** these tests reference `ChamberEntry.sync` (Task B9 adds the field). For now, the tests will also fail to compile until B9 ships. That's OK — we keep the plan order because the handler itself does not depend on the `sync` field. After B9, re-run the tests to confirm they pass.

- [ ] **Step 2: Add the unified POST handler.**

Append to `src/web/routes/sync.rs`:

```rust
pub async fn post_sync_action(
    State(app): State<Arc<AppState>>,
    AxumPath((id, backend_str, verb)): AxumPath<(String, String, String)>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    require_workspace(&entry)?;
    let backend = SyncBackend::parse(&backend_str).ok_or(StatusCode::BAD_REQUEST)?;
    let result = match verb.as_str() {
        "start" => sync_common::start(backend, &path),
        "stop" => sync_common::stop(backend, &path),
        "pull" => sync_common::pull(backend, &path),
        "push" => sync_common::push(backend, &path),
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let _ = app.tx.send(SseEvent::StatusChange {
        chamber_id: entry.id.clone(),
    });
    match result {
        Ok(()) => Ok(Json(serde_json::json!({
            "ok": true,
            "message": format!("{} {}", backend.as_str(), verb),
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "ok": false,
            "message": e.to_string(),
        }))),
    }
}
```

- [ ] **Step 3: Register the route in `src/web/mod.rs`.**

Inside `build_router_with_state`, after the `GET .../sync` route, add:

```rust
        .route(
            "/api/chambers/{id}/sync/{backend}/{verb}",
            post(crate::web::routes::sync::post_sync_action),
        )
```

- [ ] **Step 4: Run tests (will still fail on the `sync:` field — that's Task B9).**

```
cargo test --lib web::routes::sync -- --include-ignored
```

Don't commit yet; continue to B9. The compile error on `ChamberEntry { ..., sync: vec![] }` will be fixed there.

- [ ] **Step 5: Commit together with B9 (do not split the compile error across two commits).**

Move on.

---

### Task B9: Extend `ChamberEntry.sync` + `populate_runtime` (TDD)

**Files:**
- Modify: `src/web/discovery.rs`

- [ ] **Step 1: Add a `SyncBadge` struct and extend `ChamberEntry`.**

In `src/web/discovery.rs`, add above the `ChamberEntry` struct:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncBadge {
    pub backend: String,
    pub running: bool,
}
```

Then add `sync: Vec<SyncBadge>` at the end of `ChamberEntry`:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChamberEntry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub source: Source,
    pub config_error: Option<String>,
    pub running: bool,
    pub session: Option<u32>,
    pub next_wake: Option<String>,
    pub unread: usize,
    pub completed: bool,
    pub sync: Vec<SyncBadge>,
}
```

- [ ] **Step 2: Initialize `sync: vec![]` wherever `ChamberEntry` is constructed.**

In the same file, two constructors exist:
- `scan_workspace` (around line 85) — add `sync: vec![]` at the end of the literal.
- `merge_registry` (around line 122) — same.

- [ ] **Step 3: Fill `sync` inside `populate_runtime`.**

Inside `populate_runtime` (around line 141), append inside the `for entry in idx.values_mut()` loop, after the existing fills:

```rust
        // Sync summaries, compact badge form (full detail served by GET /sync)
        entry.sync = crate::sync_common::summarize_all(dir)
            .into_iter()
            .map(|s| SyncBadge {
                backend: s.backend.as_str().into(),
                running: s.running,
            })
            .collect();
```

- [ ] **Step 4: Add a test for the new field.**

Append a new `#[test]` inside the existing test module at the bottom of `src/web/discovery.rs`:

```rust
    #[test]
    fn populate_reports_configured_gh_sync() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
        let state = crate::gh_sync::GhSyncState {
            repo: "a/b".into(),
            discussion_number: 1,
            discussion_node_id: "n".into(),
            last_read_cursor: None,
            self_login: None,
            last_pushed_session: None,
        };
        crate::gh_sync::save_sync_state(&alpha.join("gh-sync.json"), &state).unwrap();

        let mut idx = scan_workspace(dir.path());
        populate_runtime(&mut idx);
        let entry = idx.values().next().unwrap();
        assert_eq!(entry.sync.len(), 1);
        assert_eq!(entry.sync[0].backend, "gh");
        assert!(!entry.sync[0].running);
    }
```

- [ ] **Step 5: Fix any tests in `src/web/routes/chamber.rs` that construct `ChamberEntry` literals.**

Grep for `ChamberEntry {` in tests and add `sync: vec![],` to each literal. The main one is at `src/web/routes/chamber.rs:370-384` (the `start_stop_restart_return_409_for_external` test). Update:

```rust
            let entry = crate::web::discovery::ChamberEntry {
                id: id.clone(),
                name: "outside".into(),
                path: external.canonicalize().unwrap(),
                source: Source::External,
                config_error: None,
                running: true,
                session: None,
                next_wake: None,
                unread: 0,
                completed: false,
                sync: vec![],
            };
```

- [ ] **Step 6: Run the full test suite.**

```
cargo test --lib
```

Expected: all pass, including the new B7/B8 tests that referenced `sync: vec![]`.

- [ ] **Step 7: Commit B8 + B9 together.**

```
git add src/web/routes/sync.rs src/web/mod.rs src/web/discovery.rs src/web/routes/chamber.rs
git commit -m "feat(web): POST sync lifecycle endpoints + ChamberEntry.sync field"
```

- [ ] **Step 8: Remove the temporary `.catch(() => [])` guard from Task A5 Step 3.**

Open `templates/web_shell.html`, find the line inside `buildDetail`'s `Promise.all`:

```js
      fetchJSON(`/api/chambers/${id}/sync`).catch(() => []),
```

Replace it with:

```js
      fetchJSON(`/api/chambers/${id}/sync`),
```

Commit:

```
git add templates/web_shell.html
git commit -m "refactor(web): drop temporary /sync 404 guard (endpoint now live)"
```

---

### Task B10: Daemon pid-file lifecycle integration test

**Files:**
- Create: `tests/sync_pid_file.rs`

- [ ] **Step 1: Write the integration test.**

`tests/sync_pid_file.rs`:

```rust
//! Integration test: cryo-gh sync-daemon writes a pid file on startup and
//! removes it on clean shutdown.
//!
//! We give the daemon a fake gh-sync.json so it starts but can't talk to
//! GitHub. The pull/push errors it logs are expected noise.

use std::time::Duration;

fn target_bin(name: &str) -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap();
    // tests live at target/debug/deps/<test>; binaries at target/debug/<name>
    p.pop(); // deps
    p.pop(); // debug
    p.push(name);
    p
}

#[test]
#[cfg(unix)]
fn cryo_gh_sync_daemon_manages_pid_file() {
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().to_path_buf();

    // Minimal gh-sync.json so the daemon does not abort.
    std::fs::write(
        workdir.join("gh-sync.json"),
        r#"{"repo":"fake/fake","discussion_number":1,"discussion_node_id":"fake"}"#,
    )
    .unwrap();

    let bin = target_bin("cryo-gh");
    assert!(bin.exists(), "build cryo-gh first: cargo build --bin cryo-gh");

    let mut child = std::process::Command::new(&bin)
        .current_dir(&workdir)
        .arg("sync-daemon")
        .arg("--interval")
        .arg("60")
        .env("CRYO_NO_SERVICE", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn cryo-gh sync-daemon");

    // Wait up to 5 seconds for the pid file to appear.
    let pid_path = workdir.join("cryo-gh-sync.pid");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if pid_path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(pid_path.exists(), "pid file should have been created");

    let pid_contents = std::fs::read_to_string(&pid_path).unwrap();
    assert_eq!(pid_contents.trim().parse::<u32>().unwrap(), child.id());

    // SIGTERM the daemon and wait for it.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let _ = child.wait();

    // Allow brief time for cleanup after loop exits.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if !pid_path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!pid_path.exists(), "pid file should be removed after SIGTERM");
}
```

- [ ] **Step 2: Run the test.**

```
cargo build --bin cryo-gh
cargo test --test sync_pid_file
```

Expected: passes.

- [ ] **Step 3: Commit.**

```
git add tests/sync_pid_file.rs
git commit -m "test(sync): pid file is written on startup and removed on SIGTERM"
```

---

### Task B11: UI `buildSyncBox` + `updateSyncBox`

**Files:**
- Modify: `templates/web_shell.html` (inside the IIFE)

- [ ] **Step 1: Add the signature helper + build function.**

Insert alongside the other `build*` / `update*` helpers in `templates/web_shell.html`:

```js
  function syncSignature(sync) {
    if (!sync || !sync.length) return '0:';
    return sync.length + ':' + sync.map(s =>
      `${s.backend}|${s.running}|${s.installed}|${s.target}|${s.last_pushed_session ?? ''}`
    ).join(',');
  }

  function buildSyncBox(sync) {
    view.syncBox.className = 'sync-box';
    view.syncBox.style.display = 'none';
    updateSyncBox(sync);
  }

  function updateSyncBox(sync) {
    const sig = syncSignature(sync);
    if (sig === view.lastSyncSig) return;
    view.lastSyncSig = sig;
    const box = view.syncBox;
    box.innerHTML = '';
    if (!sync || !sync.length) {
      box.style.display = 'none';
      return;
    }
    box.style.display = '';

    const title = document.createElement('div');
    title.className = 'sync-title';
    title.textContent = 'Sync';
    box.appendChild(title);

    for (const s of sync) {
      const row = document.createElement('div');
      row.className = 'sync-row';

      const dot = document.createElement('span');
      dot.className = 'sync-dot';
      let dotChar = '○';
      if (s.running) { dotChar = '●'; dot.classList.add('running'); }
      else if (s.configured && !s.installed) { dotChar = '⚠'; dot.classList.add('warning'); }
      else { dot.classList.add('stopped'); }
      dot.textContent = dotChar;

      const label = document.createElement('span');
      label.className = 'sync-backend';
      label.textContent = s.backend;

      const target = document.createElement('span');
      target.className = 'sync-target';
      target.textContent = s.target;
      target.title = s.target;

      const meta = document.createElement('span');
      meta.className = 'sync-meta';
      meta.textContent = s.last_pushed_session != null ? `last push: session #${s.last_pushed_session}` : '';

      const actions = document.createElement('span');
      actions.className = 'sync-actions';
      const toggle = btn(s.running ? 'stop' : 'start', () => {
        if (s.running) {
          if (!window.confirm('Stop sync and uninstall service?')) return;
        }
        syncAction(view.chamberId, s.backend, s.running ? 'stop' : 'start');
      });
      const pullBtn = btn('pull', () => syncAction(view.chamberId, s.backend, 'pull'));
      const pushBtn = btn('push', () => syncAction(view.chamberId, s.backend, 'push'));
      actions.appendChild(toggle);
      actions.appendChild(pullBtn);
      actions.appendChild(pushBtn);

      row.appendChild(dot);
      row.appendChild(label);
      row.appendChild(target);
      row.appendChild(meta);
      row.appendChild(actions);
      box.appendChild(row);
    }
  }

  async function syncAction(id, backend, verb) {
    try {
      const resp = await fetchJSON(`/api/chambers/${id}/sync/${backend}/${verb}`, { method: 'POST' });
      toast(resp.message || verb, resp.ok ? '' : 'error');
    } catch (e) {
      toast(e.message, 'error');
    }
  }
```

- [ ] **Step 2: Commit.**

```
git add templates/web_shell.html
git commit -m "feat(web): buildSyncBox/updateSyncBox with backend actions"
```

---

### Task B12: CSS for the sync box

**Files:**
- Modify: `templates/web.css`

- [ ] **Step 1: Append at the end of `templates/web.css`.**

```css

/* ----- Sync box ----- */
.sync-box {
  padding: 10px 20px;
  border-bottom: 1px solid var(--border);
  background: var(--surface);
  font-size: 12px;
}
.sync-title {
  font-size: 11px;
  letter-spacing: 1px;
  text-transform: uppercase;
  color: var(--text-dim);
  margin-bottom: 6px;
}
.sync-row {
  display: grid;
  grid-template-columns: auto auto 1fr auto auto;
  align-items: baseline;
  gap: 10px;
  padding: 4px 0;
  line-height: 1.5;
}
.sync-dot {
  width: 1em;
  text-align: center;
  flex-shrink: 0;
}
.sync-dot.running { color: var(--green); }
.sync-dot.stopped { color: var(--text-dim); }
.sync-dot.warning { color: var(--orange); }
.sync-backend {
  color: var(--accent);
  font-weight: 500;
  min-width: 4em;
}
.sync-target {
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sync-meta {
  color: var(--text-dim);
  font-size: 11px;
  white-space: nowrap;
}
.sync-actions { display: flex; gap: 6px; }
.sync-actions button {
  background: var(--surface2);
  border: 1px solid var(--border);
  color: var(--text);
  padding: 3px 8px;
  border-radius: 3px;
  cursor: pointer;
  font-family: inherit;
  font-size: 11px;
}
.sync-actions button:hover { background: var(--accent-dim); }
```

- [ ] **Step 2: Manual visual check.**

```
make example-web
```

Create a stub `gh-sync.json` in an example chamber (e.g. `examples/chambers/mr-lazy/gh-sync.json`) with fake content:

```json
{"repo":"fake/fake","discussion_number":1,"discussion_node_id":"n"}
```

Reload the page. You should see a "Sync" box with one row, stopped dot, `fake/fake#1` as target, `[start] [pull] [push]` buttons. Click start — expect a toast with an error (fake repo); the point is the round trip works.

Remove the stub before committing:

```
rm examples/chambers/mr-lazy/gh-sync.json
```

- [ ] **Step 3: Commit.**

```
git add templates/web.css
git commit -m "style(web): sync box visuals"
```

---

### Task B13: Full manual validation + cleanup

**Files:** none modified.

- [ ] **Step 1: End-to-end smoke test.**

If you have a GitHub repo you can test against, run the real round trip:

```
make check-gh REPO=your-user/your-test-repo
```

Otherwise at minimum:

- [ ] Start `make example-web`.
- [ ] Select a chamber with no sync state file — Sync box is hidden.
- [ ] Create `gh-sync.json` in a chamber directory (fake data is fine) — refresh the page — Sync box appears with `○ stopped`.
- [ ] Click `start`. Expect a toast with either success or the CLI error; running state updates within one SSE tick.
- [ ] Scroll up in the messages area, then send a message via terminal — scroll doesn't jump, the message is appended.
- [ ] Scroll to bottom — send again — new message auto-scrolls into view.

- [ ] **Step 2: Final quality checks.**

```
make fmt
make clippy
make test
```

Expected: all pass. Fix any issues before moving to review.

- [ ] **Step 3: Done — proceed to review-implementation skill.**

Per the user's workflow preference (`MEMORY.md`: "After each execute-plan, run review-implementation skill and fix what can be fixed"), the executing agent should now invoke `review-implementation` and address its findings before opening a PR.

---

## Summary

Part A (Tasks A1–A6) splits `renderDetail` into `buildDetail` + `updateDetail` with diff-aware sub-updaters and append-only message rendering, fixing the scroll reset and flicker caused by the 500 ms `timer.json` watcher.

Part B (Tasks B1–B13) adds pid-file detection to both sync daemons, a unified `sync_common` module that summarises both backends and shells out to the CLIs for lifecycle actions, five new HTTP endpoints, a per-chamber UI row, and CSS. External chambers are read-only for sync just as they are for start/stop/restart/reset.
