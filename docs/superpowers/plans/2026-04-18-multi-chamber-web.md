# Multi-chamber `cryo web` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace single-chamber `cryo web` with a workspace-scoped web UI that discovers chambers under `./chambers/`, merges in externally registered daemons, serves a sidebar-driven SPA, and supports per-chamber lifecycle (`start`/`stop`/`restart`) alongside the existing monitor/message actions.

**Architecture:** Split `src/web.rs` (~680 LoC) into `src/web/` with focused submodules: `discovery` (scan `chambers/*/cryo.toml` + merge with `registry::list()`), `state` (shared `AppState` holding a canonical `ChamberIndex`), `routes/{chambers,chamber,events,pages}` (axum handlers — all per-chamber handlers take `&Path`), `watchers` (lazy per-chamber file watchers feeding a single broadcast channel), `lifecycle` (thin wrappers over the existing `service::install` / `process::terminate_pid` paths). The frontend is one HTML shell that drives both sidebar and detail pane from a single SSE stream carrying a `chamber_id` on every event.

**Tech Stack:** Rust (axum 0.8, tokio, tokio-stream, notify 8), serde/serde_json, urlencoding (new dep), tempfile + tower (dev-deps, for route testing).

---

## File Structure

**Create:**
- `src/web/mod.rs` — module root: `pub fn serve`, `pub fn build_router`, re-exports.
- `src/web/state.rs` — `AppState { workspace_dir, chambers, tx }` + lookup helpers.
- `src/web/discovery.rs` — `ChamberEntry`, `ChamberIndex`, `Source`, `discover`, id helpers.
- `src/web/routes/mod.rs` — `pub mod chambers; pub mod chamber; pub mod events; pub mod pages;`
- `src/web/routes/chambers.rs` — `GET /api/chambers`, `POST /api/chambers/refresh`.
- `src/web/routes/chamber.rs` — per-chamber handlers (status, messages, send, wake, start, stop, restart) + parameterized helpers moved out of old `web.rs`.
- `src/web/routes/events.rs` — `GET /api/events` (SSE) with `chamber_id`-tagged events.
- `src/web/routes/pages.rs` — `GET /`, `GET /c/:id`, `GET /assets/web.css`.
- `src/web/watchers.rs` — per-chamber watcher handles, spawn/drop lifecycle.
- `src/web/lifecycle.rs` — `start_chamber`, `stop_chamber`, `restart_chamber`.
- `templates/web_shell.html` — the single-page app shell (replaces `templates/web.html`).
- `templates/web.css` — shared stylesheet, served at `/assets/web.css`.

**Modify:**
- `Cargo.toml` — add `urlencoding = "2"` dep; add `tower = { version = "0.5", features = ["util"] }` to `[dev-dependencies]`.
- `src/lib.rs` — no change needed (`pub mod web;` already re-exports the new module because `src/web/mod.rs` replaces `src/web.rs`).
- `src/bin/cryo.rs` — `cmd_web` and `cmd_web_daemon` become workspace-mode; reject cwd-is-a-chamber with migration error.
- `README.md` — short section pointing at the new workspace layout.
- `docs/src/SUMMARY.md` and a new `docs/src/web.md` (or extend existing web docs) — workspace layout + migration recipe.

**Delete:**
- `src/web.rs` — content moved into `src/web/mod.rs` (task 2 preserves behavior, later tasks replace).
- `templates/web.html` — replaced by `web_shell.html` + `web.css`.

---

## Task 1: Add dependencies and scaffold empty module tree (no behavior change)

**Files:**
- Modify: `Cargo.toml`
- Create: `src/web/mod.rs` (by moving `src/web.rs`)
- Create: `src/web/state.rs`, `src/web/discovery.rs`, `src/web/watchers.rs`, `src/web/lifecycle.rs`, `src/web/routes/mod.rs`, `src/web/routes/chambers.rs`, `src/web/routes/chamber.rs`, `src/web/routes/events.rs`, `src/web/routes/pages.rs`
- Delete: `src/web.rs`

- [ ] **Step 1: Add deps to `Cargo.toml`**

Under `[dependencies]` (after the `ureq = "3"` line):

```toml
urlencoding = "2"
```

Under `[dev-dependencies]` (after `tempfile = "3"`):

```toml
tower = { version = "0.5", features = ["util"] }
```

- [ ] **Step 2: Move `src/web.rs` to `src/web/mod.rs`**

```bash
mkdir -p src/web/routes
git mv src/web.rs src/web/mod.rs
```

- [ ] **Step 3: Create empty stub files for the submodules**

Create each file with a single-line module doc comment so `cargo fmt` / `clippy` is happy. Example for `src/web/discovery.rs`:

```rust
//! Chamber discovery: scan `./chambers/*/cryo.toml` and merge with the daemon registry.
```

Do the same (one-line `//!` doc) for `state.rs`, `watchers.rs`, `lifecycle.rs`, `routes/mod.rs`, `routes/chambers.rs`, `routes/chamber.rs`, `routes/events.rs`, `routes/pages.rs`.

- [ ] **Step 4: Run `cargo build` to verify behavior unchanged**

```bash
cargo build
```

Expected: build succeeds. Nothing wired up to the new modules yet, so the old code in `src/web/mod.rs` still works identically.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/web/
git commit -m "chore: scaffold web/ submodule tree and add urlencoding/tower deps"
```

---

## Task 2: Chamber id helpers and `Source` enum

**Files:**
- Modify: `src/web/discovery.rs`
- Test: `src/web/discovery.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing test for id round-trip**

In `src/web/discovery.rs`:

```rust
//! Chamber discovery: scan `./chambers/*/cryo.toml` and merge with the daemon registry.

use std::path::{Path, PathBuf};

/// Where a chamber was sourced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Under `./chambers/` in the workspace.
    Workspace,
    /// Running daemon registered elsewhere on the machine.
    External,
}

/// Encode a canonicalized absolute path as a URL-safe chamber id.
pub fn encode_id(path: &Path) -> String {
    urlencoding::encode(&path.to_string_lossy()).into_owned()
}

/// Decode a chamber id back to an absolute path.
pub fn decode_id(id: &str) -> Option<PathBuf> {
    urlencoding::decode(id).ok().map(|s| PathBuf::from(s.into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        let path = PathBuf::from("/Users/alice/work space/chambers/my chamber");
        let id = encode_id(&path);
        assert!(!id.contains(' '), "id must be URL-safe");
        assert!(!id.contains('/'), "id must not contain raw slashes");
        let back = decode_id(&id).unwrap();
        assert_eq!(back, path);
    }

    #[test]
    fn decode_rejects_invalid() {
        // %ZZ is not valid percent-encoding
        assert!(decode_id("%ZZ").is_none());
    }

    #[test]
    fn source_serialises_lowercase() {
        let json = serde_json::to_string(&Source::Workspace).unwrap();
        assert_eq!(json, "\"workspace\"");
        let json = serde_json::to_string(&Source::External).unwrap();
        assert_eq!(json, "\"external\"");
    }
}
```

- [ ] **Step 2: Wire module into `src/web/mod.rs`**

Add at the top of `src/web/mod.rs` (after existing `use` lines):

```rust
pub mod discovery;
pub mod state;
pub mod watchers;
pub mod lifecycle;
pub mod routes;
```

And in `src/web/routes/mod.rs`:

```rust
pub mod chambers;
pub mod chamber;
pub mod events;
pub mod pages;
```

- [ ] **Step 3: Run the tests**

```bash
cargo test --lib web::discovery
```

Expected: three tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/web/
git commit -m "feat(web): Source enum + chamber id encode/decode"
```

---

## Task 3: `ChamberEntry` struct and workspace scan

**Files:**
- Modify: `src/web/discovery.rs`
- Test: `src/web/discovery.rs`

- [ ] **Step 1: Write failing test for `scan_workspace`**

Append to `src/web/discovery.rs`:

```rust
use std::collections::BTreeMap;

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
}

/// A map from chamber id → entry.
pub type ChamberIndex = BTreeMap<String, ChamberEntry>;

/// Scan `<workspace>/chambers/*` for chambers. Returns entries for every
/// subdirectory (even ones with broken or missing `cryo.toml` — those get a
/// `config_error`). Runtime fields (`running`, `session`, `next_wake`,
/// `unread`) are filled in by `populate_runtime`, not here.
pub fn scan_workspace(workspace: &Path) -> ChamberIndex {
    let chambers_dir = workspace.join("chambers");
    let mut out = ChamberIndex::new();
    let Ok(rd) = std::fs::read_dir(&chambers_dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or(path.clone());
        let name = canonical
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "(unknown)".into());
        let cryo_toml = canonical.join("cryo.toml");
        let config_error = if !cryo_toml.exists() {
            Some("missing cryo.toml".into())
        } else {
            crate::config::load_config(&cryo_toml).err().map(|e| e.to_string())
        };
        let id = encode_id(&canonical);
        out.insert(
            id.clone(),
            ChamberEntry {
                id,
                name,
                path: canonical,
                source: Source::Workspace,
                config_error,
                running: false,
                session: None,
                next_wake: None,
                unread: 0,
            },
        );
    }
    out
}

#[cfg(test)]
mod scan_tests {
    use super::*;

    #[test]
    fn scan_empty_workspace_returns_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let idx = scan_workspace(dir.path());
        assert!(idx.is_empty());
    }

    #[test]
    fn scan_finds_chambers_with_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        std::fs::create_dir_all(chambers.join("alpha")).unwrap();
        std::fs::create_dir_all(chambers.join("beta")).unwrap();
        // Write minimal valid cryo.toml
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&chambers.join("alpha").join("cryo.toml"), &cfg).unwrap();
        crate::config::save_config(&chambers.join("beta").join("cryo.toml"), &cfg).unwrap();

        let idx = scan_workspace(dir.path());
        assert_eq!(idx.len(), 2);
        let names: Vec<_> = idx.values().map(|e| e.name.clone()).collect();
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));
        for entry in idx.values() {
            assert_eq!(entry.source, Source::Workspace);
            assert!(entry.config_error.is_none(), "valid toml should have no error");
        }
    }

    #[test]
    fn scan_flags_missing_cryo_toml_as_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("chambers").join("broken")).unwrap();
        let idx = scan_workspace(dir.path());
        assert_eq!(idx.len(), 1);
        let entry = idx.values().next().unwrap();
        assert!(entry.config_error.is_some());
    }
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test --lib web::discovery
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/web/discovery.rs
git commit -m "feat(web): scan_workspace walks ./chambers/ into a ChamberIndex"
```

---

## Task 4: Merge with daemon registry (external + canonicalization dedup)

**Files:**
- Modify: `src/web/discovery.rs`
- Test: `src/web/discovery.rs`

- [ ] **Step 1: Write failing test**

Append to `src/web/discovery.rs`:

```rust
/// Merge running daemons from `entries` into `idx`. Entries whose path is
/// already present in the index (keyed by canonicalized path) simply flip
/// `running = true`; entries whose path is new get added with
/// `source = External`.
pub fn merge_registry(idx: &mut ChamberIndex, entries: &[crate::registry::DaemonEntry]) {
    for entry in entries {
        let raw = PathBuf::from(&entry.dir);
        let canonical = raw.canonicalize().unwrap_or(raw);
        let id = encode_id(&canonical);
        if let Some(existing) = idx.get_mut(&id) {
            existing.running = true;
            continue;
        }
        let name = canonical
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "(unknown)".into());
        idx.insert(
            id.clone(),
            ChamberEntry {
                id,
                name,
                path: canonical,
                source: Source::External,
                config_error: None,
                running: true,
                session: None,
                next_wake: None,
                unread: 0,
            },
        );
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use crate::registry::DaemonEntry;

    #[test]
    fn external_daemon_appears_with_external_source() {
        let dir = tempfile::tempdir().unwrap();
        let external = dir.path().join("somewhere-else");
        std::fs::create_dir_all(&external).unwrap();
        let mut idx = ChamberIndex::new();
        merge_registry(
            &mut idx,
            &[DaemonEntry {
                pid: 1,
                dir: external.to_string_lossy().into_owned(),
                socket_path: None,
            }],
        );
        assert_eq!(idx.len(), 1);
        let entry = idx.values().next().unwrap();
        assert_eq!(entry.source, Source::External);
        assert!(entry.running);
    }

    #[test]
    fn running_workspace_chamber_flips_running_not_source() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        std::fs::create_dir_all(chambers.join("alpha")).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&chambers.join("alpha").join("cryo.toml"), &cfg).unwrap();

        let mut idx = scan_workspace(dir.path());
        let alpha_path = chambers.join("alpha").canonicalize().unwrap();
        merge_registry(
            &mut idx,
            &[DaemonEntry {
                pid: 42,
                dir: alpha_path.to_string_lossy().into_owned(),
                socket_path: None,
            }],
        );
        let entry = idx.values().next().unwrap();
        assert_eq!(entry.source, Source::Workspace);
        assert!(entry.running);
    }

    #[test]
    #[cfg(unix)]
    fn symlinked_chamber_is_deduped() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real-chamber");
        std::fs::create_dir_all(&real).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&real.join("cryo.toml"), &cfg).unwrap();

        let chambers = dir.path().join("chambers");
        std::fs::create_dir_all(&chambers).unwrap();
        std::os::unix::fs::symlink(&real, chambers.join("alpha")).unwrap();

        let mut idx = scan_workspace(dir.path());
        let real_canonical = real.canonicalize().unwrap();
        merge_registry(
            &mut idx,
            &[DaemonEntry {
                pid: 1,
                dir: real_canonical.to_string_lossy().into_owned(),
                socket_path: None,
            }],
        );
        // Must be 1 entry, not 2 — symlink target canonicalizes to the same path
        assert_eq!(idx.len(), 1);
        let entry = idx.values().next().unwrap();
        assert_eq!(entry.source, Source::Workspace);
        assert!(entry.running);
    }
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test --lib web::discovery
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/web/discovery.rs
git commit -m "feat(web): merge_registry adds external chambers and dedups via canonicalization"
```

---

## Task 5: Runtime population (`running`, `session`, `next_wake`, `unread`)

**Files:**
- Modify: `src/web/discovery.rs`
- Test: `src/web/discovery.rs`

- [ ] **Step 1: Write failing test**

Append to `src/web/discovery.rs`:

```rust
/// Fill in runtime fields on each entry from its on-disk state.
/// `running` is left as-is if already true (set by `merge_registry`).
pub fn populate_runtime(idx: &mut ChamberIndex) {
    for entry in idx.values_mut() {
        let dir = &entry.path;

        // Session # and pid from timer.json
        if let Ok(Some(st)) = crate::state::load_state(&crate::state::state_path(dir)) {
            entry.session = Some(st.session_number);
            if !entry.running {
                entry.running = crate::state::is_locked(&st);
            }
        }

        // Next wake from todo.json
        let todo_path = dir.join("todo.json");
        entry.next_wake = crate::todo::TodoList::load(&todo_path)
            .ok()
            .and_then(|list| list.next_wake_time().map(String::from));

        // Unread = pending inbox messages (not archived)
        entry.unread = crate::message::read_inbox(dir)
            .map(|v| v.len())
            .unwrap_or(0);
    }
}

/// One-shot discovery: scan workspace, merge registry, populate runtime.
pub fn discover(workspace: &Path) -> ChamberIndex {
    let mut idx = scan_workspace(workspace);
    if let Ok(entries) = crate::registry::list() {
        merge_registry(&mut idx, &entries);
    }
    populate_runtime(&mut idx);
    idx
}

#[cfg(test)]
mod populate_tests {
    use super::*;

    #[test]
    fn populate_reads_session_and_unread() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();

        // Fake runtime state: session 7, not locked (no live PID)
        let st = crate::state::CryoState {
            session_number: 7,
            pid: None,
            retry_count: 0,
            agent_override: None,
            max_retries_override: None,
            max_session_duration_override: None,
            last_report_time: None,
            provider_index: None,
            instance_id: None,
            pending_fallback: None,
        };
        crate::state::save_state(&crate::state::state_path(&alpha), &st).unwrap();

        // Fake inbox with one message
        crate::message::ensure_dirs(&alpha).unwrap();
        let msg = crate::message::Message {
            from: "tester".into(),
            subject: "hi".into(),
            body: "yo".into(),
            timestamp: chrono::Local::now().naive_local(),
            metadata: Default::default(),
        };
        crate::message::write_message(&alpha, "inbox", &msg).unwrap();

        let mut idx = scan_workspace(dir.path());
        populate_runtime(&mut idx);
        let entry = idx.values().next().unwrap();
        assert_eq!(entry.session, Some(7));
        assert_eq!(entry.unread, 1);
        assert!(!entry.running, "no live pid -> not running");
    }
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test --lib web::discovery
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/web/discovery.rs
git commit -m "feat(web): populate runtime fields (session, next_wake, unread, running)"
```

---

## Task 6: `AppState`, chamber resolution, SSE event type

**Files:**
- Modify: `src/web/state.rs`, `src/web/mod.rs`
- Test: `src/web/state.rs`

- [ ] **Step 1: Write the file**

Replace the stub content of `src/web/state.rs` with:

```rust
//! Shared application state for the web server.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::web::discovery::{ChamberEntry, ChamberIndex};

/// SSE event broadcast to all connected clients. Every event carries
/// `chamber_id` so the sidebar (which listens to all events) and the detail
/// pane (which filters to one id) can route them.
#[derive(Clone, Debug)]
pub enum SseEvent {
    NewMessage {
        chamber_id: String,
        direction: String,
        from: String,
        subject: String,
        body: String,
        timestamp: String,
    },
    StatusChange {
        chamber_id: String,
    },
    LogLine {
        chamber_id: String,
        line: String,
    },
    /// Workspace-level refresh — chambers list changed (added/removed).
    IndexChanged,
}

pub struct AppState {
    pub workspace_dir: PathBuf,
    pub chambers: Arc<RwLock<ChamberIndex>>,
    pub tx: tokio::sync::broadcast::Sender<SseEvent>,
}

impl AppState {
    pub fn new(workspace_dir: PathBuf) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(256);
        Self {
            workspace_dir,
            chambers: Arc::new(RwLock::new(ChamberIndex::new())),
            tx,
        }
    }

    /// Resolve an id to `(path, ChamberEntry)` if the id refers to a known
    /// chamber in the current index.
    pub fn resolve(&self, id: &str) -> Option<(PathBuf, ChamberEntry)> {
        let idx = self.chambers.read().ok()?;
        idx.get(id).map(|e| (e.path.clone(), e.clone()))
    }

    /// Overwrite the chamber index with a fresh discovery pass.
    pub fn refresh(&self) {
        let fresh = crate::web::discovery::discover(&self.workspace_dir);
        if let Ok(mut idx) = self.chambers.write() {
            *idx = fresh;
        }
        let _ = self.tx.send(SseEvent::IndexChanged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_finds_known_chamber() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();

        let state = AppState::new(dir.path().to_path_buf());
        state.refresh();
        let id = crate::web::discovery::encode_id(&alpha.canonicalize().unwrap());
        let resolved = state.resolve(&id);
        assert!(resolved.is_some());
        let (path, entry) = resolved.unwrap();
        assert_eq!(path, alpha.canonicalize().unwrap());
        assert_eq!(entry.name, "alpha");
    }

    #[test]
    fn resolve_returns_none_for_unknown_id() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(dir.path().to_path_buf());
        assert!(state.resolve("nonexistent").is_none());
    }
}
```

- [ ] **Step 2: Remove old `AppState` + `SseEvent` from `src/web/mod.rs`**

In `src/web/mod.rs`, delete the old `SseEvent` enum (lines currently ~23–34) and the old `AppState` struct (currently ~36–39). At the top of the file, add:

```rust
pub use state::{AppState, SseEvent};
```

This will break everything that depended on the old shape until tasks 7+ land. To keep the crate building for this commit, also gate the rest of `mod.rs` behind a comment marker we'll remove in Task 7:

Add right after `pub use state::{AppState, SseEvent};`:

```rust
// Legacy single-chamber router — will be fully replaced in subsequent tasks.
// For now, it references `AppState` fields that no longer match; we exclude
// it from compilation while the migration is in progress.
#[cfg(feature = "__legacy_web")]
mod legacy { /* everything from the old web.rs lives here temporarily */ }
```

Rather than fighting the old code, **simplify**: replace the entire body of `src/web/mod.rs` *below* `pub use state::{AppState, SseEvent};` and the other `pub mod ...;` declarations with a minimal placeholder:

```rust
use std::path::PathBuf;

/// Placeholder: real router is built in Task 10+.
pub fn build_router(workspace_dir: PathBuf) -> axum::Router {
    let _ = workspace_dir;
    axum::Router::new()
}

pub async fn serve(workspace_dir: PathBuf, host: &str, port: u16) -> anyhow::Result<()> {
    crate::message::ensure_dirs(&workspace_dir)?;
    let app = build_router(workspace_dir);
    let addr = format!("{host}:{port}");
    println!("Cryochamber web UI: http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Format a duration in milliseconds as a human-readable relative string.
pub fn format_relative_time(diff_ms: i64) -> String {
    if diff_ms <= 0 {
        return "now".to_string();
    }
    let mins = diff_ms / 60_000;
    let hours = diff_ms / 3_600_000;
    let days = diff_ms / 86_400_000;
    if mins < 1 {
        "<1m".to_string()
    } else if hours < 1 {
        format!("{mins}m")
    } else if days < 1 {
        let rem_m = (diff_ms % 3_600_000) / 60_000;
        format!("{hours}h {rem_m}m")
    } else {
        let rem_h = (diff_ms % 86_400_000) / 3_600_000;
        format!("{days}d {rem_h}h")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_relative_time_now() {
        assert_eq!(format_relative_time(0), "now");
        assert_eq!(format_relative_time(-5000), "now");
    }

    #[test]
    fn test_format_relative_time_minutes_hours_days() {
        assert_eq!(format_relative_time(30_000), "<1m");
        assert_eq!(format_relative_time(60_000), "1m");
        assert_eq!(format_relative_time(3_600_000), "1h 0m");
        assert_eq!(format_relative_time(86_400_000), "1d 0h");
    }
}
```

This deletes the old single-chamber handlers and tests — subsequent tasks restore the equivalent functionality in the new shape.

- [ ] **Step 3: Run `cargo build` and the discovery/state tests**

```bash
cargo build
cargo test --lib web::state
cargo test --lib web::discovery
```

Expected: build succeeds (with warnings about unused `cmd_web*` references being fine), `web::state` tests pass, `web::discovery` tests still pass.

- [ ] **Step 4: Commit**

```bash
git add src/web/
git commit -m "feat(web): AppState with ChamberIndex + SseEvent chamber_id tagging"
```

---

## Task 7: Parameterized status/messages helpers (per-chamber, take `&Path`)

**Files:**
- Modify: `src/web/routes/chamber.rs`
- Test: `src/web/routes/chamber.rs`

- [ ] **Step 1: Write the file**

Replace the stub content of `src/web/routes/chamber.rs` with:

```rust
//! Per-chamber HTTP handlers. All functions take `dir: &Path` so they can be
//! reused across chambers — nothing here is tied to a single global project.

use std::path::Path;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::web::state::{AppState, SseEvent};

/// Build the JSON status payload for a single chamber.
pub fn status_json(dir: &Path) -> Value {
    let cfg = crate::config::load_config(&crate::config::config_path(dir))
        .ok()
        .flatten()
        .unwrap_or_default();

    let (running, session, agent) =
        match crate::state::load_state(&crate::state::state_path(dir)).ok().flatten() {
            Some(st) => {
                let is_running = crate::state::is_locked(&st);
                let effective_agent = st
                    .agent_override
                    .as_deref()
                    .unwrap_or(&cfg.agent)
                    .to_string();
                (is_running, st.session_number, effective_agent)
            }
            None => (false, 0, cfg.agent.clone()),
        };

    let next_wake: Option<String> = {
        let todo_path = dir.join("todo.json");
        crate::todo::TodoList::load(&todo_path)
            .ok()
            .and_then(|list| list.next_wake_time().map(String::from))
    };

    let log_file = crate::log::log_path(dir);
    let log_tail = crate::log::read_current_session(&log_file)
        .ok()
        .flatten()
        .unwrap_or_default();
    let notes = crate::log::parse_latest_session_notes(&log_file).unwrap_or_default();
    let task = crate::log::parse_latest_session_task(&log_file).ok().flatten();

    let next_wake_rel = next_wake.as_deref().and_then(|w| {
        let wake = chrono::NaiveDateTime::parse_from_str(w, "%Y-%m-%dT%H:%M").ok()?;
        let now = chrono::Local::now().naive_local();
        let diff_ms = (wake - now).num_milliseconds();
        Some(format!("{w} ({})", crate::web::format_relative_time(diff_ms)))
    });

    json!({
        "running": running,
        "session": session,
        "agent": agent,
        "log_tail": log_tail,
        "next_wake": next_wake_rel,
        "notes": notes,
        "task": task,
    })
}

/// Build the list of all messages (archive + inbox + outbox) for a chamber.
pub fn messages_json(dir: &Path) -> Value {
    let mut all: Vec<Value> = Vec::new();
    let to_json = |msg: &crate::message::Message, direction: &str| -> Value {
        json!({
            "direction": direction,
            "from": msg.from,
            "subject": msg.subject,
            "body": msg.body,
            "timestamp": msg.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string(),
        })
    };
    if let Ok(archived) = crate::message::read_inbox_archive(dir) {
        for (_f, m) in archived {
            all.push(to_json(&m, "inbox"));
        }
    }
    if let Ok(inbox) = crate::message::read_inbox(dir) {
        for (_f, m) in inbox {
            all.push(to_json(&m, "inbox"));
        }
    }
    if let Ok(outbox) = crate::message::read_outbox(dir) {
        for (_f, m) in outbox {
            all.push(to_json(&m, "outbox"));
        }
    }
    all.sort_by(|a, b| {
        a["timestamp"]
            .as_str()
            .unwrap_or("")
            .cmp(b["timestamp"].as_str().unwrap_or(""))
    });
    Value::Array(all)
}

pub async fn get_status(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(status_json(&path)))
}

pub async fn get_messages(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(messages_json(&path)))
}

#[derive(Deserialize)]
pub struct SendRequest {
    body: String,
    from: Option<String>,
    subject: Option<String>,
}

pub async fn post_send(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<SendRequest>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let msg = crate::message::Message {
        from: req.from.unwrap_or_else(|| "human".into()),
        subject: req.subject.unwrap_or_default(),
        body: req.body,
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
    };
    match crate::message::write_message(&path, "inbox", &msg) {
        Ok(_) => {
            let _ = app.tx.send(SseEvent::NewMessage {
                chamber_id: entry.id,
                direction: "inbox".into(),
                from: msg.from.clone(),
                subject: msg.subject.clone(),
                body: msg.body.clone(),
                timestamp: msg.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string(),
            });
            Ok(Json(json!({"ok": true, "message": "Message sent"})))
        }
        Err(e) => Ok(Json(json!({"ok": false, "message": format!("Failed: {e}")}))),
    }
}

#[derive(Deserialize, Default)]
pub struct WakeRequest {
    message: Option<String>,
}

pub async fn post_wake(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<WakeRequest>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let body = req
        .message
        .unwrap_or_else(|| "Wake requested from web UI.".into());
    let msg = crate::message::Message {
        from: "operator".into(),
        subject: "Wake".into(),
        body,
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
    };
    if let Err(e) = crate::message::write_message(&path, "inbox", &msg) {
        return Ok(Json(json!({"ok": false, "message": format!("Failed: {e}")})));
    }
    let signaled = crate::process::signal_daemon_wake(&path);
    Ok(Json(json!({
        "ok": true,
        "message": if signaled { "Wake signal sent" } else { "Message queued (no daemon running)" }
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_json_for_missing_state_has_zero_session() {
        let dir = tempfile::tempdir().unwrap();
        let v = status_json(dir.path());
        assert_eq!(v["running"], false);
        assert_eq!(v["session"], 0);
    }

    #[test]
    fn messages_json_sorted_by_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let early = crate::message::Message {
            from: "a".into(),
            subject: "".into(),
            body: "first".into(),
            timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            metadata: Default::default(),
        };
        let late = crate::message::Message {
            from: "b".into(),
            subject: "".into(),
            body: "second".into(),
            timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 2)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            metadata: Default::default(),
        };
        crate::message::write_message(dir.path(), "inbox", &late).unwrap();
        crate::message::write_message(dir.path(), "outbox", &early).unwrap();
        let arr = messages_json(dir.path());
        let arr = arr.as_array().unwrap();
        assert_eq!(arr[0]["body"], "first");
        assert_eq!(arr[1]["body"], "second");
    }
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test --lib web::routes::chamber
```

Expected: both tests pass. Full `cargo build` succeeds.

- [ ] **Step 3: Commit**

```bash
git add src/web/routes/chamber.rs
git commit -m "feat(web): parameterized per-chamber handlers (status/messages/send/wake)"
```

---

## Task 8: Chambers list + refresh routes

**Files:**
- Modify: `src/web/routes/chambers.rs`
- Test: `src/web/routes/chambers.rs`

- [ ] **Step 1: Write the file**

Replace the stub content of `src/web/routes/chambers.rs` with:

```rust
//! `/api/chambers` routes: list + refresh.

use std::sync::Arc;

use axum::{extract::State, response::Json};
use serde_json::Value;

use crate::web::state::AppState;

pub async fn get_chambers(State(app): State<Arc<AppState>>) -> Json<Value> {
    let idx = app.chambers.read().unwrap();
    let list: Vec<&crate::web::discovery::ChamberEntry> = idx.values().collect();
    Json(serde_json::to_value(&list).unwrap_or(Value::Array(vec![])))
}

pub async fn post_refresh(State(app): State<Arc<AppState>>) -> Json<Value> {
    app.refresh();
    let idx = app.chambers.read().unwrap();
    let list: Vec<&crate::web::discovery::ChamberEntry> = idx.values().collect();
    Json(serde_json::to_value(&list).unwrap_or(Value::Array(vec![])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;

    #[tokio::test]
    async fn get_chambers_lists_workspace_scans() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        std::fs::create_dir_all(chambers.join("alpha")).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&chambers.join("alpha").join("cryo.toml"), &cfg).unwrap();

        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();

        let Json(v) = get_chambers(State(app)).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "alpha");
        assert_eq!(arr[0]["source"], "workspace");
    }

    #[tokio::test]
    async fn refresh_picks_up_new_chamber() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("chambers")).unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();

        let Json(initial) = get_chambers(State(app.clone())).await;
        assert_eq!(initial.as_array().unwrap().len(), 0);

        // Add a chamber, then refresh
        let new_dir = dir.path().join("chambers").join("beta");
        std::fs::create_dir_all(&new_dir).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&new_dir.join("cryo.toml"), &cfg).unwrap();

        let Json(after) = post_refresh(State(app)).await;
        assert_eq!(after.as_array().unwrap().len(), 1);
    }
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test --lib web::routes::chambers
```

Expected: both tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/web/routes/chambers.rs
git commit -m "feat(web): GET /api/chambers and POST /api/chambers/refresh"
```

---

## Task 9: Lifecycle functions (start/stop/restart chamber)

**Files:**
- Modify: `src/web/lifecycle.rs`
- Test: `src/web/lifecycle.rs`

- [ ] **Step 1: Write the file**

Replace the stub content of `src/web/lifecycle.rs` with:

```rust
//! Per-chamber lifecycle wrappers: start, stop, restart. These reproduce the
//! paths in `cryo start` / `cryo cancel` / `cryo restart` (see `src/bin/cryo.rs`)
//! but take an explicit `dir: &Path` and do not read the process-wide `work_dir()`.

use std::path::Path;

use anyhow::{Context, Result};

use crate::state::{self, CryoState};

/// Start a daemon for the chamber at `dir`. Mirrors `cmd_start` in the CLI.
pub fn start_chamber(dir: &Path) -> Result<()> {
    if !crate::config::config_path(dir).exists() {
        anyhow::bail!("Not a chamber: no cryo.toml in {}", dir.display());
    }
    if !dir.join("plan.md").exists() {
        anyhow::bail!("Missing plan.md in {}", dir.display());
    }

    if let Some(existing) = state::load_state(&state::state_path(dir))? {
        if state::is_locked(&existing) {
            anyhow::bail!("A daemon is already running in {}", dir.display());
        }
    }

    let cfg = crate::config::load_config(&crate::config::config_path(dir))?.unwrap_or_default();
    validate_agent_command(&cfg.agent)?;

    crate::message::ensure_dirs(dir)?;

    let cryo_state = CryoState {
        session_number: 0,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        pending_fallback: None,
    };
    state::save_state(&state::state_path(dir), &cryo_state)?;

    launch_daemon(dir)?;
    Ok(())
}

/// Stop the daemon for the chamber at `dir`. Mirrors `cmd_cancel`, but leaves
/// timer.json intact (stop is not the same as cancel — restart needs overrides).
pub fn stop_chamber(dir: &Path) -> Result<()> {
    let _ = crate::service::uninstall("daemon", dir);
    if let Some(st) = state::load_state(&state::state_path(dir))? {
        if state::is_locked(&st) {
            if let Some(pid) = st.pid {
                crate::process::terminate_pid(pid)?;
            }
        }
        let updated = CryoState { pid: None, ..st };
        state::save_state(&state::state_path(dir), &updated)?;
    }
    Ok(())
}

/// Restart = stop + start. Preserves overrides and session number.
pub fn restart_chamber(dir: &Path) -> Result<()> {
    stop_chamber(dir)?;
    // `cmd_start` guards against an existing locked state — we already cleared
    // the lock above, so starting is safe even if the state file still exists.
    launch_daemon(dir)
}

fn launch_daemon(dir: &Path) -> Result<()> {
    if std::env::var("CRYO_NO_SERVICE").is_ok() {
        crate::process::spawn_daemon(dir)?;
    } else {
        let exe =
            std::env::current_exe().context("Failed to resolve cryo executable path")?;
        let log_path = crate::log::log_path(dir);
        crate::service::install("daemon", dir, &exe, &["daemon"], &log_path, false)?;
    }
    Ok(())
}

fn validate_agent_command(agent_cmd: &str) -> Result<()> {
    let program = crate::agent::agent_program(agent_cmd)?;
    let status = std::process::Command::new("which")
        .arg(&program)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => anyhow::bail!("Agent command '{}' not found on PATH", program),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_chamber_rejects_missing_cryo_toml() {
        let dir = tempfile::tempdir().unwrap();
        let err = start_chamber(dir.path()).unwrap_err();
        assert!(err.to_string().contains("no cryo.toml"));
    }

    #[test]
    fn start_chamber_rejects_missing_plan_md() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&crate::config::config_path(dir.path()), &cfg).unwrap();
        let err = start_chamber(dir.path()).unwrap_err();
        assert!(err.to_string().contains("plan.md"));
    }

    #[test]
    fn stop_chamber_is_idempotent_on_nothing_running() {
        let dir = tempfile::tempdir().unwrap();
        // No config, no state file — should succeed as a no-op.
        stop_chamber(dir.path()).unwrap();
    }
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test --lib web::lifecycle
```

Expected: all three tests pass. (We test the *error* paths and the no-op path here; the happy-path is integration-tested in Task 10 under `CRYO_NO_SERVICE=1`.)

- [ ] **Step 3: Commit**

```bash
git add src/web/lifecycle.rs
git commit -m "feat(web): start_chamber / stop_chamber / restart_chamber wrappers"
```

---

## Task 10: Lifecycle HTTP endpoints + 409 for external chambers

**Files:**
- Modify: `src/web/routes/chamber.rs` (append handlers)
- Test: `src/web/routes/chamber.rs`

- [ ] **Step 1: Append lifecycle handlers to `src/web/routes/chamber.rs`**

Append this code after `post_wake` (and its `WakeRequest` struct):

```rust
use crate::web::discovery::Source;

fn require_workspace(entry: &crate::web::discovery::ChamberEntry) -> Result<(), StatusCode> {
    if entry.source == Source::External {
        return Err(StatusCode::CONFLICT);
    }
    Ok(())
}

pub async fn post_start(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    require_workspace(&entry)?;
    let result = crate::web::lifecycle::start_chamber(&path);
    app.refresh();
    match result {
        Ok(()) => Ok(Json(json!({"ok": true, "message": "Started"}))),
        Err(e) => Ok(Json(json!({"ok": false, "message": e.to_string()}))),
    }
}

pub async fn post_stop(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    require_workspace(&entry)?;
    let result = crate::web::lifecycle::stop_chamber(&path);
    app.refresh();
    match result {
        Ok(()) => Ok(Json(json!({"ok": true, "message": "Stopped"}))),
        Err(e) => Ok(Json(json!({"ok": false, "message": e.to_string()}))),
    }
}

pub async fn post_restart(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    require_workspace(&entry)?;
    let result = crate::web::lifecycle::restart_chamber(&path);
    app.refresh();
    match result {
        Ok(()) => Ok(Json(json!({"ok": true, "message": "Restarted"}))),
        Err(e) => Ok(Json(json!({"ok": false, "message": e.to_string()}))),
    }
}
```

- [ ] **Step 2: Add a test for the external-409 rule**

Append to the existing `#[cfg(test)] mod tests` block:

```rust
#[tokio::test]
async fn start_stop_restart_return_409_for_external() {
    let dir = tempfile::tempdir().unwrap();
    let external = dir.path().join("outside");
    std::fs::create_dir_all(&external).unwrap();

    let app = Arc::new(AppState::new(dir.path().to_path_buf()));
    // Inject a synthetic external entry directly into the index
    {
        let id = crate::web::discovery::encode_id(&external.canonicalize().unwrap());
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
        };
        app.chambers.write().unwrap().insert(id, entry);
    }
    let id = crate::web::discovery::encode_id(&external.canonicalize().unwrap());

    let err = post_start(State(app.clone()), AxumPath(id.clone()))
        .await
        .unwrap_err();
    assert_eq!(err, StatusCode::CONFLICT);

    let err = post_stop(State(app.clone()), AxumPath(id.clone()))
        .await
        .unwrap_err();
    assert_eq!(err, StatusCode::CONFLICT);

    let err = post_restart(State(app), AxumPath(id)).await.unwrap_err();
    assert_eq!(err, StatusCode::CONFLICT);
}
```

- [ ] **Step 3: Run the tests**

```bash
cargo test --lib web::routes::chamber
```

Expected: all tests (including the new 409 test) pass.

- [ ] **Step 4: Commit**

```bash
git add src/web/routes/chamber.rs
git commit -m "feat(web): POST /api/chambers/:id/{start,stop,restart} (409 for external)"
```

---

## Task 11: Lazy watcher manager

**Files:**
- Modify: `src/web/watchers.rs`
- Test: `src/web/watchers.rs`

- [ ] **Step 1: Write the file**

Replace the stub content of `src/web/watchers.rs` with:

```rust
//! Lazy per-chamber file watchers. A `WatcherRegistry` keeps one watcher
//! thread per chamber path; `ensure_watching` is idempotent so the discovery
//! pass can just call it for every known chamber on every refresh.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use notify::{recommended_watcher, Event as NotifyEvent, EventKind, RecursiveMode, Watcher};

use crate::web::state::SseEvent;

/// Stored handle per chamber: the watcher (kept alive by the thread) and the
/// join handle on the background log/state poll thread.
struct Handle {
    _watcher: notify::RecommendedWatcher,
    _stop: Arc<Mutex<bool>>,
}

#[derive(Default, Clone)]
pub struct WatcherRegistry {
    inner: Arc<Mutex<HashMap<PathBuf, Handle>>>,
}

impl WatcherRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a watcher for `dir` if we don't already have one.
    pub fn ensure_watching(
        &self,
        chamber_id: String,
        dir: &Path,
        tx: tokio::sync::broadcast::Sender<SseEvent>,
    ) {
        let mut map = self.inner.lock().unwrap();
        if map.contains_key(dir) {
            return;
        }
        if let Some(handle) = spawn_watcher(chamber_id, dir, tx) {
            map.insert(dir.to_path_buf(), handle);
        }
    }

    /// Drop watchers for any chamber whose path is not in `keep`.
    pub fn retain(&self, keep: &std::collections::BTreeSet<PathBuf>) {
        let mut map = self.inner.lock().unwrap();
        map.retain(|p, _| keep.contains(p));
    }
}

fn spawn_watcher(
    chamber_id: String,
    dir: &Path,
    tx: tokio::sync::broadcast::Sender<SseEvent>,
) -> Option<Handle> {
    let inbox = dir.join("messages").join("inbox");
    let outbox = dir.join("messages").join("outbox");

    // File watcher: messages
    let tx_msg = tx.clone();
    let inbox_for_cb = inbox.clone();
    let outbox_for_cb = outbox.clone();
    let id_for_cb = chamber_id.clone();
    let mut watcher = recommended_watcher(move |res: Result<NotifyEvent, _>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Create(_)) {
                for path in &event.paths {
                    if path.extension().is_some_and(|e| e == "md") {
                        let direction = if path.starts_with(&inbox_for_cb) {
                            "inbox"
                        } else if path.starts_with(&outbox_for_cb) {
                            "outbox"
                        } else {
                            continue;
                        };
                        if let Ok(content) = std::fs::read_to_string(path) {
                            if let Ok(msg) = crate::message::parse_message(&content) {
                                let _ = tx_msg.send(SseEvent::NewMessage {
                                    chamber_id: id_for_cb.clone(),
                                    direction: direction.to_string(),
                                    from: msg.from,
                                    subject: msg.subject,
                                    body: msg.body,
                                    timestamp: msg
                                        .timestamp
                                        .format("%Y-%m-%dT%H:%M:%S")
                                        .to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
    })
    .ok()?;

    let _ = std::fs::create_dir_all(&inbox);
    let _ = std::fs::create_dir_all(&outbox);
    watcher.watch(&inbox, RecursiveMode::NonRecursive).ok()?;
    watcher.watch(&outbox, RecursiveMode::NonRecursive).ok()?;

    // Background poll: log tail + timer.json change.
    let stop = Arc::new(Mutex::new(false));
    let stop_clone = stop.clone();
    let tx_log = tx.clone();
    let tx_state = tx;
    let log_path = crate::log::log_path(dir);
    let state_path = crate::state::state_path(dir);
    let id_log = chamber_id.clone();
    let id_state = chamber_id;
    std::thread::spawn(move || {
        let mut last_size = log_path.metadata().map(|m| m.len()).unwrap_or(0);
        let mut last_state = std::fs::read_to_string(&state_path).unwrap_or_default();
        loop {
            if *stop_clone.lock().unwrap() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            if let Ok(meta) = log_path.metadata() {
                let cur = meta.len();
                if cur > last_size {
                    if let Ok(content) = std::fs::read_to_string(&log_path) {
                        let new_bytes = &content[last_size as usize..];
                        for line in new_bytes.lines() {
                            if !line.trim().is_empty() {
                                let _ = tx_log.send(SseEvent::LogLine {
                                    chamber_id: id_log.clone(),
                                    line: line.to_string(),
                                });
                            }
                        }
                    }
                    last_size = cur;
                }
            }
            if let Ok(content) = std::fs::read_to_string(&state_path) {
                if content != last_state {
                    let _ = tx_state.send(SseEvent::StatusChange {
                        chamber_id: id_state.clone(),
                    });
                    last_state = content;
                }
            }
        }
    });

    Some(Handle {
        _watcher: watcher,
        _stop: stop,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn watcher_emits_new_message_event_with_chamber_id() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();

        let (tx, mut rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let reg = WatcherRegistry::new();
        reg.ensure_watching("cham-1".into(), dir.path(), tx.clone());

        let msg = crate::message::Message {
            from: "tester".into(),
            subject: "hi".into(),
            body: "yo".into(),
            timestamp: chrono::Local::now().naive_local(),
            metadata: Default::default(),
        };
        crate::message::write_message(dir.path(), "inbox", &msg).unwrap();

        // Wait up to 3 seconds for the event (notify + fs flush is racy)
        let event = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("timed out waiting for watcher event")
            .expect("channel closed");

        match event {
            SseEvent::NewMessage { chamber_id, direction, .. } => {
                assert_eq!(chamber_id, "cham-1");
                assert_eq!(direction, "inbox");
            }
            other => panic!("expected NewMessage, got {:?}", other),
        }
    }

    #[test]
    fn ensure_watching_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let reg = WatcherRegistry::new();
        reg.ensure_watching("x".into(), dir.path(), tx.clone());
        reg.ensure_watching("x".into(), dir.path(), tx);
        // Implementation detail: one entry per dir in the map.
        assert_eq!(reg.inner.lock().unwrap().len(), 1);
    }
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test --lib web::watchers
```

Expected: both tests pass. The async test may take 1–2 seconds.

- [ ] **Step 3: Commit**

```bash
git add src/web/watchers.rs
git commit -m "feat(web): lazy per-chamber WatcherRegistry with chamber_id-tagged events"
```

---

## Task 12: SSE route with chamber_id-tagged events

**Files:**
- Modify: `src/web/routes/events.rs`
- Test: `src/web/routes/events.rs`

- [ ] **Step 1: Write the file**

Replace the stub content of `src/web/routes/events.rs` with:

```rust
//! GET /api/events — one SSE stream for the entire UI. Every event carries
//! `chamber_id` (except `IndexChanged`, which is workspace-level).

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::web::state::{AppState, SseEvent};

pub async fn get_events(
    State(app): State<Arc<AppState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = app.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result: Result<SseEvent, _>| {
        result.ok().map(|event| {
            let ev = match event {
                SseEvent::NewMessage {
                    chamber_id,
                    direction,
                    from,
                    subject,
                    body,
                    timestamp,
                } => Event::default()
                    .event("message")
                    .json_data(json!({
                        "chamber_id": chamber_id,
                        "direction": direction,
                        "from": from,
                        "subject": subject,
                        "body": body,
                        "timestamp": timestamp,
                    }))
                    .unwrap(),
                SseEvent::StatusChange { chamber_id } => Event::default()
                    .event("status")
                    .json_data(json!({"chamber_id": chamber_id}))
                    .unwrap(),
                SseEvent::LogLine { chamber_id, line } => Event::default()
                    .event("log")
                    .json_data(json!({"chamber_id": chamber_id, "line": line}))
                    .unwrap(),
                SseEvent::IndexChanged => Event::default().event("index").data("changed"),
            };
            Ok(ev)
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn broadcast_multiplexes_by_chamber_id() {
        // Confirms the broadcast channel correctly carries chamber_id — the
        // actual SSE serialization is exercised by integration tests in Task 14.
        let (tx, mut rx_a) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let mut rx_b = tx.subscribe();
        tx.send(SseEvent::StatusChange {
            chamber_id: "alpha".into(),
        })
        .unwrap();
        let a = rx_a.recv().await.unwrap();
        let b = rx_b.recv().await.unwrap();
        match (a, b) {
            (
                SseEvent::StatusChange { chamber_id: ca },
                SseEvent::StatusChange { chamber_id: cb },
            ) => {
                assert_eq!(ca, "alpha");
                assert_eq!(cb, "alpha");
            }
            _ => panic!("expected StatusChange"),
        }
    }
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test --lib web::routes::events
```

Expected: passes.

- [ ] **Step 3: Commit**

```bash
git add src/web/routes/events.rs
git commit -m "feat(web): /api/events SSE with chamber_id-tagged events"
```

---

## Task 13: HTML shell, CSS, and page routes

**Files:**
- Create: `templates/web_shell.html`, `templates/web.css`
- Delete: `templates/web.html`
- Modify: `src/web/routes/pages.rs`

- [ ] **Step 1: Create `templates/web.css`**

Extract the `<style>` block from `templates/web.html` (lines 7–~250 of the old file) into `templates/web.css`. Keep the existing look-and-feel (dark theme, monospace). Add the new sidebar rules below (so the whole file is one self-contained stylesheet):

```css
/* Paste the contents of the old <style> block here (without the <style>/</style> tags). */
/* Then append the sidebar + two-pane rules: */

.app { display: flex; height: 100vh; }
.sidebar {
  width: 280px;
  flex-shrink: 0;
  background: var(--surface);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.sidebar h2 {
  font-size: 11px;
  letter-spacing: 2px;
  text-transform: uppercase;
  color: var(--accent);
  padding: 14px 16px;
  border-bottom: 1px solid var(--border);
}
.sidebar ul { list-style: none; flex: 1; overflow-y: auto; }
.sidebar li {
  padding: 8px 16px;
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  border-left: 2px solid transparent;
  font-size: 13px;
}
.sidebar li:hover { background: var(--surface2); }
.sidebar li.active { background: var(--surface2); border-left-color: var(--accent); }
.sidebar .unread {
  margin-left: auto;
  background: var(--accent-dim);
  color: var(--accent);
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 8px;
}
.sidebar footer {
  padding: 10px 16px;
  border-top: 1px solid var(--border);
  font-size: 11px;
  color: var(--text-dim);
}
.sidebar footer button {
  background: none;
  border: 1px solid var(--border);
  color: var(--text-dim);
  padding: 3px 8px;
  border-radius: 3px;
  cursor: pointer;
  margin-right: 6px;
  font-family: inherit;
  font-size: 11px;
}
.pane { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
.pane-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-dim);
  flex-direction: column;
  gap: 12px;
}
.lifecycle-buttons { display: flex; gap: 6px; }
.lifecycle-buttons button {
  background: var(--surface2);
  border: 1px solid var(--border);
  color: var(--text);
  padding: 4px 10px;
  border-radius: 3px;
  cursor: pointer;
  font-family: inherit;
  font-size: 12px;
}
.lifecycle-buttons button:hover { background: var(--accent-dim); }
.toast {
  position: fixed;
  bottom: 20px;
  right: 20px;
  background: var(--surface2);
  border: 1px solid var(--border);
  padding: 10px 16px;
  border-radius: 4px;
  font-size: 13px;
  z-index: 100;
}
.toast.error { border-color: var(--red); color: var(--red); }
```

- [ ] **Step 2: Create `templates/web_shell.html`**

```html
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Cryochamber</title>
<link rel="stylesheet" href="/assets/web.css">
</head>
<body>
<div class="app">
  <aside class="sidebar">
    <h2>Cryochamber</h2>
    <ul id="chamber-list"></ul>
    <footer>
      <button id="refresh">⟳ refresh</button>
      <span id="workspace-path"></span>
    </footer>
  </aside>
  <main class="pane" id="pane">
    <div class="pane-empty">
      <div>Pick a chamber from the sidebar</div>
      <div id="workspace-summary"></div>
    </div>
  </main>
</div>
<div id="toast"></div>

<script>
(function() {
  const state = {
    chambers: [],
    currentId: null,
  };

  // Parse /c/:id from the URL, if present
  function urlChamberId() {
    const m = window.location.pathname.match(/^\/c\/(.+)$/);
    return m ? m[1] : null;
  }

  async function fetchJSON(url, opts) {
    const res = await fetch(url, opts);
    if (!res.ok) throw new Error(`${url}: ${res.status}`);
    return res.json();
  }

  function toast(msg, kind) {
    const el = document.getElementById('toast');
    el.textContent = msg;
    el.className = 'toast' + (kind === 'error' ? ' error' : '');
    el.style.display = 'block';
    setTimeout(() => { el.style.display = 'none'; }, 4000);
  }

  function statusDot(entry) {
    if (entry.config_error) return '✗';
    if (entry.source === 'external') return '⚠';
    return entry.running ? '●' : '○';
  }

  function renderSidebar() {
    const ul = document.getElementById('chamber-list');
    ul.innerHTML = '';
    const sorted = [...state.chambers].sort((a, b) => {
      const rank = e => (e.running ? 0 : (e.source === 'external' ? 2 : 1));
      const ra = rank(a), rb = rank(b);
      if (ra !== rb) return ra - rb;
      return a.name.localeCompare(b.name);
    });
    for (const c of sorted) {
      const li = document.createElement('li');
      li.dataset.id = c.id;
      if (c.id === state.currentId) li.classList.add('active');
      const dot = document.createElement('span');
      dot.textContent = statusDot(c);
      const name = document.createElement('span');
      name.textContent = c.name;
      li.appendChild(dot);
      li.appendChild(name);
      if (c.unread > 0) {
        const badge = document.createElement('span');
        badge.className = 'unread';
        badge.textContent = c.unread;
        li.appendChild(badge);
      }
      li.addEventListener('click', () => selectChamber(c.id));
      ul.appendChild(li);
    }
  }

  function renderSummary() {
    const running = state.chambers.filter(c => c.running).length;
    const stopped = state.chambers.filter(c => !c.running && c.source === 'workspace').length;
    const external = state.chambers.filter(c => c.source === 'external').length;
    const el = document.getElementById('workspace-summary');
    if (el) el.textContent = `${running} running · ${stopped} stopped · ${external} external`;
  }

  async function loadChambers() {
    state.chambers = await fetchJSON('/api/chambers');
    renderSidebar();
    renderSummary();
  }

  async function selectChamber(id) {
    state.currentId = id;
    window.history.pushState({}, '', `/c/${id}`);
    renderSidebar();
    await renderDetail(id);
  }

  async function renderDetail(id) {
    const entry = state.chambers.find(c => c.id === id);
    if (!entry) return;
    const pane = document.getElementById('pane');
    const [status, messages] = await Promise.all([
      fetchJSON(`/api/chambers/${id}/status`),
      fetchJSON(`/api/chambers/${id}/messages`),
    ]);
    pane.innerHTML = '';
    const header = document.createElement('div');
    header.style.padding = '12px 20px';
    header.style.borderBottom = '1px solid var(--border)';
    header.innerHTML = `
      <div style="display:flex; align-items:center; justify-content:space-between;">
        <div>
          <strong style="color: var(--accent)">${entry.name}</strong>
          <span style="color: var(--text-dim); margin-left: 8px;">${statusDot(entry)} session #${status.session}</span>
          ${status.next_wake ? `<span style="color: var(--text-dim); margin-left: 12px;">Next wake: ${status.next_wake}</span>` : ''}
        </div>
        <div class="lifecycle-buttons"></div>
      </div>
      ${status.task ? `<div style="margin-top:6px; color: var(--text-dim); font-size:12px;">Task: ${status.task}</div>` : ''}
    `;
    const btns = header.querySelector('.lifecycle-buttons');
    if (entry.source === 'workspace') {
      if (entry.running) {
        btns.appendChild(btn('wake', () => lifecycle(id, 'wake')));
        btns.appendChild(btn('stop', () => lifecycle(id, 'stop')));
        btns.appendChild(btn('restart', () => lifecycle(id, 'restart')));
      } else if (!entry.config_error) {
        btns.appendChild(btn('start', () => lifecycle(id, 'start')));
      }
    } else {
      btns.appendChild(btn('wake', () => lifecycle(id, 'wake')));
    }
    pane.appendChild(header);

    const msgBox = document.createElement('div');
    msgBox.style.flex = '1';
    msgBox.style.overflowY = 'auto';
    msgBox.style.padding = '12px 20px';
    for (const m of messages) {
      const row = document.createElement('div');
      row.style.marginBottom = '10px';
      row.style.padding = '8px';
      row.style.background = m.direction === 'inbox' ? 'var(--inbox-bg)' : 'var(--outbox-bg)';
      row.style.borderRadius = '3px';
      row.innerHTML = `<div style="font-size:11px; color: var(--text-dim)">${m.direction} · ${m.from} · ${m.timestamp}</div>
                       <div style="margin-top:4px; white-space: pre-wrap;">${escapeHtml(m.body)}</div>`;
      msgBox.appendChild(row);
    }
    pane.appendChild(msgBox);

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
  }

  function btn(label, onClick) {
    const b = document.createElement('button');
    b.textContent = label;
    b.addEventListener('click', onClick);
    return b;
  }

  function escapeHtml(s) {
    return s.replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  }

  async function lifecycle(id, action) {
    try {
      const resp = await fetchJSON(`/api/chambers/${id}/${action}`, {method: 'POST'});
      toast(resp.message || action, resp.ok ? '' : 'error');
      await loadChambers();
      if (state.currentId === id) await renderDetail(id);
    } catch (e) {
      toast(e.message, 'error');
    }
  }

  document.getElementById('refresh').addEventListener('click', async () => {
    try {
      state.chambers = await fetchJSON('/api/chambers/refresh', {method: 'POST'});
      renderSidebar();
      renderSummary();
      if (state.currentId) await renderDetail(state.currentId);
    } catch (e) { toast(e.message, 'error'); }
  });

  // Wire the SSE stream
  const evt = new EventSource('/api/events');
  evt.addEventListener('message', async e => {
    const d = JSON.parse(e.data);
    if (state.currentId === d.chamber_id) {
      // Simplest: just re-render the detail pane when a new message appears.
      await renderDetail(state.currentId);
    }
    // Always refresh sidebar unread counts
    await loadChambers();
  });
  evt.addEventListener('status', async e => {
    await loadChambers();
    if (state.currentId) await renderDetail(state.currentId);
  });
  evt.addEventListener('log', e => {
    const d = JSON.parse(e.data);
    if (state.currentId === d.chamber_id) {
      const box = document.getElementById('log-box');
      if (box) { box.textContent += (box.textContent ? '\n' : '') + d.line; box.scrollTop = box.scrollHeight; }
    }
  });
  evt.addEventListener('index', async () => { await loadChambers(); });

  // Bootstrap
  (async () => {
    await loadChambers();
    const urlId = urlChamberId();
    if (urlId && state.chambers.some(c => c.id === urlId)) {
      await selectChamber(urlId);
    } else {
      renderSummary();
    }
  })();
})();
</script>
</body>
</html>
```

- [ ] **Step 3: Delete the old template**

```bash
git rm templates/web.html
```

- [ ] **Step 4: Write page route handlers**

Replace the stub content of `src/web/routes/pages.rs` with:

```rust
//! HTML shell + static assets.

use axum::response::Html;

const SHELL_HTML: &str = include_str!("../../../templates/web_shell.html");
const WEB_CSS: &str = include_str!("../../../templates/web.css");

pub async fn get_index() -> Html<&'static str> {
    Html(SHELL_HTML)
}

pub async fn get_css() -> ([(&'static str, &'static str); 1], &'static str) {
    ([("content-type", "text/css")], WEB_CSS)
}
```

- [ ] **Step 5: Build to verify embeds resolve**

```bash
cargo build
```

Expected: build succeeds.

- [ ] **Step 6: Commit**

```bash
git add templates/ src/web/routes/pages.rs
git commit -m "feat(web): SPA shell (sidebar + pane) and split CSS into web.css"
```

---

## Task 14: Wire the final router in `build_router` + integration test

**Files:**
- Modify: `src/web/mod.rs`
- Test: new integration test in `tests/web_multi_chamber.rs`

- [ ] **Step 1: Rewrite `build_router` and `serve` in `src/web/mod.rs`**

Replace the body of `src/web/mod.rs` (everything after the `pub mod ...;` lines and `pub use state::{AppState, SseEvent};`) with:

```rust
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};

use crate::web::state::AppState;

pub fn build_router(workspace_dir: PathBuf) -> Router {
    let app = Arc::new(AppState::new(workspace_dir));
    app.refresh();
    build_router_with_state(app)
}

/// Separate entry point so integration tests can inject their own `AppState`.
pub fn build_router_with_state(app: Arc<AppState>) -> Router {
    // Kick off watchers for every currently known chamber
    let watchers = crate::web::watchers::WatcherRegistry::new();
    {
        let idx = app.chambers.read().unwrap();
        for entry in idx.values() {
            watchers.ensure_watching(entry.id.clone(), &entry.path, app.tx.clone());
        }
    }

    Router::new()
        .route("/", get(crate::web::routes::pages::get_index))
        .route("/c/{id}", get(crate::web::routes::pages::get_index))
        .route("/assets/web.css", get(crate::web::routes::pages::get_css))
        .route("/api/chambers", get(crate::web::routes::chambers::get_chambers))
        .route(
            "/api/chambers/refresh",
            post(crate::web::routes::chambers::post_refresh),
        )
        .route(
            "/api/chambers/{id}/status",
            get(crate::web::routes::chamber::get_status),
        )
        .route(
            "/api/chambers/{id}/messages",
            get(crate::web::routes::chamber::get_messages),
        )
        .route(
            "/api/chambers/{id}/send",
            post(crate::web::routes::chamber::post_send),
        )
        .route(
            "/api/chambers/{id}/wake",
            post(crate::web::routes::chamber::post_wake),
        )
        .route(
            "/api/chambers/{id}/start",
            post(crate::web::routes::chamber::post_start),
        )
        .route(
            "/api/chambers/{id}/stop",
            post(crate::web::routes::chamber::post_stop),
        )
        .route(
            "/api/chambers/{id}/restart",
            post(crate::web::routes::chamber::post_restart),
        )
        .route("/api/events", get(crate::web::routes::events::get_events))
        .with_state(app)
}

pub async fn serve(workspace_dir: PathBuf, host: &str, port: u16) -> anyhow::Result<()> {
    let app = Arc::new(AppState::new(workspace_dir));
    app.refresh();
    let router = build_router_with_state(app);
    let addr = format!("{host}:{port}");
    if !host.starts_with("127.") && host != "localhost" {
        eprintln!(
            "Warning: cryo web is binding on {host} — lifecycle actions (start/stop/restart) are exposed without auth. Use 127.0.0.1 unless you know what you're doing."
        );
    }
    println!("Cryochamber web UI: http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

/// Format a duration in milliseconds as a human-readable relative string.
pub fn format_relative_time(diff_ms: i64) -> String {
    if diff_ms <= 0 {
        return "now".to_string();
    }
    let mins = diff_ms / 60_000;
    let hours = diff_ms / 3_600_000;
    let days = diff_ms / 86_400_000;
    if mins < 1 {
        "<1m".to_string()
    } else if hours < 1 {
        format!("{mins}m")
    } else if days < 1 {
        let rem_m = (diff_ms % 3_600_000) / 60_000;
        format!("{hours}h {rem_m}m")
    } else {
        let rem_h = (diff_ms % 86_400_000) / 3_600_000;
        format!("{days}d {rem_h}h")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_relative_time() {
        assert_eq!(format_relative_time(0), "now");
        assert_eq!(format_relative_time(60_000), "1m");
        assert_eq!(format_relative_time(3_600_000), "1h 0m");
        assert_eq!(format_relative_time(86_400_000), "1d 0h");
    }
}
```

- [ ] **Step 2: Write integration test**

Create `tests/web_multi_chamber.rs`:

```rust
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cryochamber::config;
use cryochamber::web::{build_router_with_state, discovery, state::AppState};
use tower::ServiceExt;

/// Build a workspace with two chambers. Populate the AppState index
/// *without* calling `registry::list()` so the test is isolated from whatever
/// daemons happen to be running on the developer's or CI machine.
fn setup_app(tmp: &tempfile::TempDir) -> Arc<AppState> {
    let chambers = tmp.path().join("chambers");
    for name in ["alpha", "beta"] {
        let d = chambers.join(name);
        std::fs::create_dir_all(&d).unwrap();
        let cfg = config::CryoConfig::default();
        config::save_config(&d.join("cryo.toml"), &cfg).unwrap();
    }
    let app = Arc::new(AppState::new(tmp.path().to_path_buf()));
    let mut idx = discovery::scan_workspace(tmp.path());
    discovery::populate_runtime(&mut idx);
    *app.chambers.write().unwrap() = idx;
    app
}

#[tokio::test]
async fn list_chambers_returns_both() {
    let tmp = tempfile::tempdir().unwrap();
    let app = setup_app(&tmp);
    let router = build_router_with_state(app);

    let resp = router
        .oneshot(Request::builder().uri("/api/chambers").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn send_message_writes_to_correct_chamber() {
    let tmp = tempfile::tempdir().unwrap();
    let app = setup_app(&tmp);

    // Grab alpha's id from the index
    let id = {
        let idx = app.chambers.read().unwrap();
        idx.values().find(|e| e.name == "alpha").unwrap().id.clone()
    };

    let router = build_router_with_state(app);
    let body = serde_json::json!({"body": "hello alpha"}).to_string();
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/chambers/{id}/send"))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Read alpha's inbox, assert the message landed
    let alpha_dir = tmp.path().join("chambers").join("alpha").canonicalize().unwrap();
    let msgs = cryochamber::message::read_inbox(&alpha_dir).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].1.body, "hello alpha");

    // Confirm beta's inbox is empty
    let beta_dir = tmp.path().join("chambers").join("beta").canonicalize().unwrap();
    let beta_msgs = cryochamber::message::read_inbox(&beta_dir).unwrap();
    assert_eq!(beta_msgs.len(), 0);
}

#[tokio::test]
async fn unknown_chamber_id_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = setup_app(&tmp);
    let router = build_router_with_state(app);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/chambers/nonexistent/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 3: Run the integration test**

```bash
cargo test --test web_multi_chamber
```

Expected: all three tests pass.

- [ ] **Step 4: Run the whole suite to catch regressions**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: everything green.

- [ ] **Step 5: Commit**

```bash
git add src/web/mod.rs tests/web_multi_chamber.rs
git commit -m "feat(web): wire chamber routes into build_router + integration tests"
```

---

## Task 15: CLI — `cmd_web` and `cmd_web_daemon` go workspace-mode

**Files:**
- Modify: `src/bin/cryo.rs`
- Test: `tests/cli_web.rs` (new)

- [ ] **Step 1: Replace `cmd_web` in `src/bin/cryo.rs`**

Find `cmd_web` (currently around line 365). Replace the whole function with:

```rust
fn cmd_web(host: Option<String>, port: Option<u16>, foreground: bool, stop: bool) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    if stop {
        if cryochamber::service::uninstall("web", &dir)? {
            println!("Web service stopped and removed.");
        } else {
            println!("No web service installed for this directory.");
        }
        return Ok(());
    }

    // Enforce workspace mode: must not be inside a chamber (unless chambers/ also exists).
    let has_chambers_dir = dir.join("chambers").is_dir();
    let is_chamber = cryochamber::config::config_path(&dir).exists();
    if is_chamber && !has_chambers_dir {
        anyhow::bail!(
            "cryo web now runs in workspace mode.\n\n\
             This directory contains a cryo.toml (it's a chamber), not a chambers/ directory.\n\
             Create a workspace:\n  \
               mkdir -p ~/cryo-workspace/chambers\n  \
               ln -s {} ~/cryo-workspace/chambers/{}\n  \
               cd ~/cryo-workspace && cryo web\n",
            dir.display(),
            dir.file_name().and_then(|s| s.to_str()).unwrap_or("this-chamber"),
        );
    }

    // Defaults (no cryo.toml in workspace dir, so no config-sourced defaults)
    let host = host.unwrap_or_else(|| "127.0.0.1".to_string());
    let port = port.unwrap_or(8765);

    if foreground {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(cryochamber::web::serve(dir, &host, port))
    } else {
        let exe = std::env::current_exe().context("Failed to resolve cryo executable path")?;
        let port_str = port.to_string();
        let log_path = dir.join("cryo-web.log");
        cryochamber::service::install(
            "web",
            &dir,
            &exe,
            &["web-daemon", "--host", &host, "--port", &port_str],
            &log_path,
            true,
        )?;
        println!("Web UI service installed: http://{host}:{port}");
        println!("Log: cryo-web.log");
        println!("Survives reboot. Stop with: cryo web --stop");
        Ok(())
    }
}
```

- [ ] **Step 2: Update `cmd_web_daemon`**

Find `cmd_web_daemon` (right after `cmd_web`). Replace with:

```rust
fn cmd_web_daemon(host: String, port: u16) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(cryochamber::web::serve(dir, &host, port))
}
```

(Behaviorally unchanged — `work_dir()` is now the workspace dir, not a chamber.)

- [ ] **Step 3: Write a CLI test for the refusal path**

Create `tests/cli_web.rs`:

```rust
use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn cryo_web_rejects_chamber_cwd_with_migration_message() {
    let tmp = tempfile::tempdir().unwrap();
    // Simulate a chamber: a cryo.toml but no chambers/ subdir.
    let cfg = cryochamber::config::CryoConfig::default();
    cryochamber::config::save_config(&tmp.path().join("cryo.toml"), &cfg).unwrap();

    Command::cargo_bin("cryo")
        .unwrap()
        .current_dir(tmp.path())
        .env("CRYO_NO_SERVICE", "1")
        .arg("web")
        .arg("--foreground")
        .timeout(std::time::Duration::from_secs(3))
        .assert()
        .failure()
        .stderr(contains("workspace mode"));
}
```

- [ ] **Step 4: Run the test**

```bash
cargo test --test cli_web
```

Expected: passes. The binary exits with an error and the message contains "workspace mode".

- [ ] **Step 5: Run the full suite and clippy**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/bin/cryo.rs tests/cli_web.rs
git commit -m "feat(cli): cryo web enforces workspace mode, rejects chamber-cwd"
```

---

## Task 16: End-to-end happy-path test (start a chamber via HTTP with CRYO_NO_SERVICE=1)

**Files:**
- Modify: `tests/web_multi_chamber.rs`

- [ ] **Step 1: Append lifecycle integration test**

Append to `tests/web_multi_chamber.rs`:

```rust
#[tokio::test]
async fn start_chamber_via_api_creates_background_daemon() {
    // Force the background-process launch path so no service install happens.
    std::env::set_var("CRYO_NO_SERVICE", "1");

    let tmp = tempfile::tempdir().unwrap();
    let chambers = tmp.path().join("chambers");
    let alpha = chambers.join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = config::CryoConfig {
        // A "true" agent that exists on every POSIX system so preflight passes.
        agent: "true".into(),
        ..Default::default()
    };
    config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
    // cmd_start requires plan.md
    std::fs::write(alpha.join("plan.md"), "test plan").unwrap();

    let app = Arc::new(AppState::new(tmp.path().to_path_buf()));
    let mut idx = discovery::scan_workspace(tmp.path());
    discovery::populate_runtime(&mut idx);
    *app.chambers.write().unwrap() = idx;
    let id = {
        let idx = app.chambers.read().unwrap();
        idx.values().find(|e| e.name == "alpha").unwrap().id.clone()
    };

    let router = build_router_with_state(app.clone());
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/chambers/{id}/start"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["ok"], true, "start should succeed: {v:?}");

    // Daemon writes timer.json fairly quickly. Poll briefly, then assert.
    let state_path = cryochamber::state::state_path(&alpha.canonicalize().unwrap());
    for _ in 0..30 {
        if state_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(state_path.exists(), "daemon should have written timer.json");

    // Clean up: stop the daemon we spawned
    let _ = cryochamber::web::lifecycle::stop_chamber(&alpha.canonicalize().unwrap());
}
```

- [ ] **Step 2: Export `lifecycle` from `src/web/mod.rs`**

Ensure the line `pub mod lifecycle;` is present near the top of `src/web/mod.rs` (it was added in Task 2; verify it's `pub`, not private).

- [ ] **Step 3: Run the test**

```bash
cargo test --test web_multi_chamber start_chamber_via_api
```

Expected: passes. The test sets `CRYO_NO_SERVICE=1` so the daemon is launched as a plain background process that exits quickly on its own (the daemon tries to spawn the `true` binary, which exits immediately — the daemon will crash-loop briefly before the stop call tears it down; that's fine for this test).

- [ ] **Step 4: Commit**

```bash
git add tests/web_multi_chamber.rs src/web/mod.rs
git commit -m "test(web): end-to-end start_chamber via HTTP with CRYO_NO_SERVICE=1"
```

---

## Task 17: Documentation

**Files:**
- Modify: `README.md`, `docs/src/SUMMARY.md`
- Create or modify: `docs/src/web.md` (create if missing)

- [ ] **Step 1: Update `README.md`**

Find the existing `cryo web` section (or the command-reference section). Replace it with (or add near it):

```markdown
### Web UI (multi-chamber)

`cryo web` runs a workspace-wide dashboard. A **workspace** is a directory that contains a `chambers/` subdirectory; each `chambers/<name>/` is a cryo project (a **chamber**).

```
~/my-cryo-workspace/
  chambers/
    chess-by-mail/
    mr-lazy/
    reports/
```

Run `cryo web` from the workspace dir. The UI lists every chamber with a status dot, lets you send messages, wake the agent, and start/stop/restart daemons. Running daemons registered elsewhere on the machine (outside `./chambers/`) appear as **external** chambers for monitoring only.

**Migrating from a single-chamber project:**

```bash
mkdir -p ~/cryo-workspace/chambers
ln -s $(pwd) ~/cryo-workspace/chambers/my-chamber
cd ~/cryo-workspace && cryo web
```
```

- [ ] **Step 2: Add a `docs/src/web.md` page**

If `docs/src/web.md` doesn't exist, create it. If it does, rewrite the page:

```markdown
# Web UI

`cryo web` runs a workspace-scoped web dashboard on `http://127.0.0.1:8765` by default.

## Workspace layout

A workspace is a directory containing a `chambers/` subdirectory. Each chamber is a regular cryochamber project (a dir with `cryo.toml`):

```
~/my-cryo-workspace/
  chambers/
    chess-by-mail/     # cryo.toml + plan.md here
    mr-lazy/
    reports/
```

Start the UI from the workspace dir:

```bash
cd ~/my-cryo-workspace
cryo web           # installs a service that survives reboot
cryo web --foreground   # run in foreground (no service)
cryo web --stop    # stop and remove the service
```

## What the UI does

- **Sidebar** — every chamber, sorted by running → stopped → external. Shows status dot, name, unread-message badge.
- **Main pane** — full detail for the selected chamber: status, task, next wake, notes, message history, log tail, send widget.
- **Lifecycle buttons** — `start` / `stop` / `restart` for workspace chambers. External chambers show no lifecycle buttons.

## External chambers

Running daemons anywhere on the machine (registered via `cryo start` from any working directory) appear as **external** chambers if they aren't under the current workspace's `./chambers/`. They're monitor-only from the UI.

## Migrating from single-chamber mode

Earlier versions of `cryo web` ran inside a chamber and served that one chamber. To migrate:

```bash
mkdir -p ~/cryo-workspace/chambers
ln -s $(pwd) ~/cryo-workspace/chambers/my-chamber
cd ~/cryo-workspace && cryo web
```

Running `cryo web` from a chamber dir now prints a migration error.

## Security

The default bind is `127.0.0.1`. If you pass `--host 0.0.0.0`, cryo prints a warning because lifecycle actions are exposed over the network without authentication. Don't do that on a shared network. Token auth is tracked as future work.
```

- [ ] **Step 3: Link the page in `docs/src/SUMMARY.md`**

Ensure `web.md` is listed in the SUMMARY. If there is already a line for an old web page, update the title:

```markdown
- [Web UI](web.md)
```

- [ ] **Step 4: Build the book to verify the layout**

```bash
make book
```

Expected: book builds without errors. (If `make book` fails because `mdbook` isn't installed, the Makefile auto-installs it.)

- [ ] **Step 5: Commit**

```bash
git add README.md docs/src/
git commit -m "docs: workspace layout, chambers/ directory, migration recipe"
```

---

## Final gate: full CI sweep

- [ ] **Step 1: Run the full check target**

```bash
make check
```

Expected: fmt clean, clippy with `-D warnings` clean, all tests pass.

- [ ] **Step 2: Manual smoke test**

```bash
# In a scratch dir:
WS=$(mktemp -d)/cryo-ws
mkdir -p "$WS/chambers/demo"
cd "$WS/chambers/demo"
cargo run --bin cryo -- init --agent opencode
echo "- [ ] test task" > plan.md
cd "$WS"
CRYO_NO_SERVICE=1 cargo run --bin cryo -- web --foreground &
sleep 2
curl -s http://127.0.0.1:8765/api/chambers | python3 -m json.tool
kill %1
```

Expected: `/api/chambers` lists the `demo` chamber with `source: "workspace"`.

- [ ] **Step 3: Commit any final touch-ups if the smoke test surfaced issues**

Otherwise this task has no commit.

---

## Self-Review Notes

- **Spec → plan coverage:** goals (Task 14, 15), discovery (Tasks 3–5), process model (Task 11), HTTP surface (Tasks 7–10, 12, 14), Rust code layout (Task 1 structure + Tasks 6–13), UI (Task 13), CLI migration (Task 15), testing (Tasks 3–12, 14, 16), risks 1–4 mitigated in code (1 in Task 14 warning, 2 in Task 3 config_error, 3 in Task 11 retain API, 4 in Task 4 canonicalization), risk 5 in Task 15 error message, docs in Task 17.
- **No placeholders:** every code-changing step contains full code. No "TBD", "implement later", or "similar to Task N".
- **Type consistency:** `ChamberEntry`, `ChamberIndex`, `Source::{Workspace, External}`, `SseEvent::{NewMessage, StatusChange, LogLine, IndexChanged}` with `chamber_id` field names are used identically across Tasks 2→3→6→7→10→11→12→14.
- **Route paths:** `/api/chambers`, `/api/chambers/refresh`, `/api/chambers/:id/...`, `/assets/web.css`, `/`, `/c/:id` — spec-aligned.
