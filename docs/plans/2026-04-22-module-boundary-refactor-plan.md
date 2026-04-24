# Module Boundary Refactor Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development if explicitly authorized; otherwise use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce unwanted coupling between core runtime, IPC, sync, channel, and hub modules without changing user-visible behavior.

**Architecture:** Preserve the existing CLI, daemon protocol, file formats, and web endpoints while introducing narrower boundary modules. Start with pure extraction and call-site rewiring, then move backend dispatch and local persistence out of transport modules, and only then split the daemon internals.

**Tech Stack:** Rust 2021, existing `anyhow`, `serde`, `chrono`, `notify`, `signal-hook`, `axum`, `ureq`, `clap`; no new dependencies expected.

**Context:** This plan follows the module-separation review from April 22, 2026. Current hot spots are `daemon`, `sync_common`, `channel::{github,zulip}`, `hub::{discovery,routes,chamber,lifecycle}`, and the small `state` / `socket` / `process` cycle.

---

## File Structure

**Create:**
- `src/daemon_client.rs` - daemon-aware IPC client helpers: state-backed request sending, daemon liveness check, and wake signaling.
- `src/sync_control.rs` - concrete sync backend dispatch used by the hub and future CLIs: summarize, start, stop, pull, push, running-state wait.
- `src/chamber_status.rs` - chamber read-model helpers for status, messages, todos, and overview data currently duplicated in hub discovery/routes.
- Optional later split under `src/daemon/` once the small boundary work lands:
  - `src/daemon/request.rs`
  - `src/daemon/session.rs`
  - `src/daemon/schedule.rs`
  - `src/daemon/effects.rs`

**Modify:**
- `src/lib.rs` - export new modules.
- `src/socket.rs` - keep only wire types and Unix socket transport; remove direct `state` reads.
- `src/process.rs` - keep only process primitives; remove daemon-specific wake helper.
- `src/lifecycle.rs` - call `daemon_client` for daemon liveness and daemon request helpers.
- `src/bin/cryo_agent.rs` - send daemon requests through `daemon_client`.
- `src/bin/cryo.rs` - use shared lifecycle/client helpers where possible.
- `src/sync_common.rs` - keep backend-neutral loop, formatting, pid-file guard, error classification, watcher utilities.
- `src/gh_sync.rs`, `src/zulip_sync.rs` - keep backend state persistence and backend-specific summary construction.
- `src/hub/discovery.rs`, `src/hub/routes/chamber.rs`, `src/hub/routes/sync.rs`, `src/hub/lifecycle.rs` - use `chamber_status`, `sync_control`, and core lifecycle helpers instead of assembling everything directly.
- `src/channel/github.rs`, `src/channel/zulip.rs` - return remote messages/cursors without writing local inbox files or updating sync state.
- `src/bin/cryo_gh.rs`, `src/bin/cryo_zulip.rs` - own local message persistence and sync-state updates for their backend.
- Tests under `src/unit_tests/**` and `tests/**` as called out per task.

**Delete:** none in the first pass. Remove compatibility wrappers only after all call sites have moved and tests cover the new boundaries.

---

## Non-Goals

- Do not change CLI command names, flags, output text, or daemon socket request/response JSON.
- Do not change `cryo.toml`, `timer.json`, `todo.json`, `gh-sync.json`, `zulip-sync.json`, or message markdown formats.
- Do not replace `gh` CLI usage or the current Zulip `ureq` client.
- Do not redesign `cryohub` UI in this refactor.
- Do not split the whole daemon file first; the daemon split is last because it has the largest blast radius.

---

## Chunk 1: Break the State / Socket / Process Cycle

### Task 1: Introduce `daemon_client`

**Files:**
- Create: `src/daemon_client.rs`
- Modify: `src/lib.rs`
- Modify: `src/socket.rs`
- Modify: `src/lifecycle.rs`
- Modify: `src/process.rs`
- Modify: `src/bin/cryo_agent.rs`
- Modify: `src/bin/cryo.rs`
- Test: `src/unit_tests/socket.rs`
- Test: add `src/unit_tests/daemon_client.rs`

- [ ] **Step 1: Add tests for transport-only socket behavior.**

Add or update socket unit tests so `socket::send_request_with_instance_id(socket_dir, request, instance_id)` serializes the optional `instance_id` but does not read `timer.json`.

Run: `cargo test socket:: -- --nocapture`

Expected: tests compile-fail or fail until the new transport helper exists.

- [ ] **Step 2: Create `src/daemon_client.rs`.**

Move daemon-aware behavior into this module:

```rust
pub fn send_request(dir: &Path, request: &crate::socket::Request) -> anyhow::Result<crate::socket::Response>;
pub fn daemon_responding(dir: &Path) -> bool;
pub fn signal_daemon_wake(dir: &Path) -> bool;
```

Implementation rules:
- `daemon_client::send_request` loads `state::state_path(dir)` and extracts `instance_id`.
- It calls a socket transport function that takes the instance ID explicitly.
- `signal_daemon_wake` loads state, pings the daemon through `daemon_client::send_request`, then calls `process::send_signal(pid, SIGUSR1)`.

- [ ] **Step 3: Simplify `socket.rs`.**

Keep `Request`, `Response`, `socket_path`, `SocketServer`, and `Responder` in `socket.rs`.

Replace the current `send_request(dir, request)` state lookup with one of these transport-only APIs:

```rust
pub fn send_request(dir: &Path, request: &Request) -> anyhow::Result<Response>;
pub fn send_request_with_instance_id(
    dir: &Path,
    request: &Request,
    instance_id: Option<&str>,
) -> anyhow::Result<Response>;
```

`socket::send_request` should pass `None` and remain transport-only. It must not call `crate::state`.

- [ ] **Step 4: Simplify `process.rs`.**

Remove `process::signal_daemon_wake`. Keep process primitives:

```rust
pub fn send_signal(pid: u32, signal: i32) -> bool;
pub(crate) fn pid_probe_indicates_alive(ret: i32, errno: i32) -> bool;
pub fn terminate_pid(pid: u32) -> Result<()>;
pub fn spawn_daemon(dir: &Path, exe: &Path) -> Result<()>;
```

- [ ] **Step 5: Rewire callers.**

Update:
- `src/lifecycle.rs` uses `daemon_client::send_request` for `daemon_responding`.
- `src/bin/cryo_agent.rs` uses `daemon_client::send_request`.
- `src/bin/cryo.rs` uses `daemon_client` where it pings or wakes the daemon.
- `src/hub/routes/chamber.rs` uses `daemon_client::signal_daemon_wake`.

- [ ] **Step 6: Run focused tests.**

Run:

```bash
cargo test socket:: -- --nocapture
cargo test daemon_client:: -- --nocapture
cargo test lifecycle:: -- --nocapture
cargo test process:: -- --nocapture
```

Expected: all targeted unit tests pass.

- [ ] **Step 7: Run full verification.**

Run:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: all tests pass and clippy reports no warnings.

- [ ] **Step 8: Commit.**

```bash
git add src/lib.rs src/daemon_client.rs src/socket.rs src/process.rs src/lifecycle.rs src/bin/cryo_agent.rs src/bin/cryo.rs src/hub/routes/chamber.rs src/unit_tests/socket.rs src/unit_tests/daemon_client.rs
git commit -m "refactor: isolate daemon client from socket and process primitives"
```

---

## Chunk 2: Make Sync Common Truly Common

### Task 2: Move Concrete Backend Dispatch to `sync_control`

**Files:**
- Create: `src/sync_control.rs`
- Modify: `src/lib.rs`
- Modify: `src/sync_common.rs`
- Modify: `src/hub/discovery.rs`
- Modify: `src/hub/routes/sync.rs`
- Modify: `src/unit_tests/sync_common.rs`
- Test: add `src/unit_tests/sync_control.rs`

- [ ] **Step 1: Add a regression test for `sync_common` dependencies.**

Add a small unit test or source-level test that fails if `sync_common.rs` contains direct references to `crate::gh_sync` or `crate::zulip_sync`.

Run: `cargo test sync_common:: -- --nocapture`

Expected: the new test fails before extraction.

- [ ] **Step 2: Create `sync_control.rs`.**

Move concrete dispatch functions from `sync_common` into `sync_control`:

```rust
pub fn summarize(backend: SyncBackend, dir: &Path) -> Option<SyncSummary>;
pub fn summarize_all(dir: &Path) -> Vec<SyncSummary>;
pub fn start(backend: SyncBackend, dir: &Path) -> Result<()>;
pub fn stop(backend: SyncBackend, dir: &Path) -> Result<()>;
pub fn pull(backend: SyncBackend, dir: &Path) -> Result<()>;
pub fn push(backend: SyncBackend, dir: &Path) -> Result<()>;
pub fn is_running(backend: SyncBackend, dir: &Path) -> bool;
pub fn wait_for_state(backend: SyncBackend, dir: &Path, expected: bool, timeout: Duration) -> bool;
```

`SyncBackend`, `SyncSummary`, `PidFile`, `SyncLoopBackend`, `SyncLoopCommand`, `SyncCycleStatus`, `classify_sync_error`, `format_outbox_post`, `watch_outbox`, and `run_sync_loop` stay in `sync_common`.

- [ ] **Step 3: Rewire hub sync callers.**

Update:
- `hub/discovery.rs` calls `sync_control::summarize_all`.
- `hub/routes/sync.rs` calls `sync_control::{summarize, start, stop, pull, push, wait_for_state}`.

- [ ] **Step 4: Keep backend state modules one-way.**

Allow `gh_sync.rs` and `zulip_sync.rs` to construct `sync_common::SyncSummary`, but do not let `sync_common` call them. This gives a one-way direction:

```text
hub/routes -> sync_control -> gh_sync / zulip_sync
backend CLIs -> sync_common loop helpers
gh_sync / zulip_sync -> sync_common data types
```

- [ ] **Step 5: Run focused tests.**

Run:

```bash
cargo test sync_common:: -- --nocapture
cargo test sync_control:: -- --nocapture
cargo test hub::routes::sync:: -- --nocapture
cargo test hub::discovery:: -- --nocapture
```

Expected: all focused tests pass.

- [ ] **Step 6: Run full verification.**

Run:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: all tests pass and clippy reports no warnings.

- [ ] **Step 7: Commit.**

```bash
git add src/lib.rs src/sync_common.rs src/sync_control.rs src/hub/discovery.rs src/hub/routes/sync.rs src/unit_tests/sync_common.rs src/unit_tests/sync_control.rs
git commit -m "refactor: move sync backend dispatch out of sync_common"
```

---

## Chunk 3: Move Local Persistence Out of Channel Transports

### Task 3: Extract GitHub Pull Transport from Inbox Writes

**Files:**
- Modify: `src/channel/github.rs`
- Modify: `src/bin/cryo_gh.rs`
- Test: `tests/github_channel_tests.rs`
- Test: `tests/gh_sync_tests.rs`

- [ ] **Step 1: Add a transport-level test.**

Test that a new `fetch_comments` helper returns parsed `Message` values and a cursor without requiring a work directory.

Target API:

```rust
pub struct GithubPullResult {
    pub messages: Vec<Message>,
    pub cursor: Option<String>,
}
```

Run: `cargo test github -- --nocapture`

Expected: fails until `GithubPullResult` and the new helper exist.

- [ ] **Step 2: Split GitHub pull behavior.**

Add a pure transport function to `channel/github.rs` that accepts owner/repo/discussion/cursor/skip-author and returns `GithubPullResult`. Keep GraphQL query and response parsing in `channel/github.rs`.

Do not call:
- `crate::message::ensure_dirs`
- `crate::message::write_message`

from the new transport function.

- [ ] **Step 3: Move inbox writes into `cryo_gh.rs`.**

Update `cmd_gh_pull` and `GhSyncLoopBackend::receive` to:
- call the new GitHub transport function,
- write returned messages with `message::write_message(&dir, "inbox", msg)`,
- persist `last_read_cursor` in `gh-sync.json`.

Keep `pull_comments` as a temporary compatibility wrapper only if needed by tests; mark it with a comment saying new code should use the split API.

- [ ] **Step 4: Run focused tests.**

Run:

```bash
cargo test --test github_channel_tests -- --nocapture
cargo test --test gh_sync_tests -- --nocapture
cargo test github -- --nocapture
```

Expected: GitHub parser/sync tests pass.

- [ ] **Step 5: Commit.**

```bash
git add src/channel/github.rs src/bin/cryo_gh.rs tests/github_channel_tests.rs tests/gh_sync_tests.rs
git commit -m "refactor: separate GitHub transport from inbox persistence"
```

### Task 4: Extract Zulip Pull Transport from Inbox Writes

**Files:**
- Modify: `src/channel/zulip.rs`
- Modify: `src/bin/cryo_zulip.rs`
- Test: `src/unit_tests/channel/zulip.rs`
- Test: `tests/zulip_channel_tests.rs`
- Test: `tests/zulip_sync_tests.rs`

- [ ] **Step 1: Add a transport-level test.**

Test that a new `fetch_messages_since` or `pull_messages_from_remote` helper returns a result without requiring a work directory and without calling `zulip_sync`.

Target API:

```rust
pub struct ZulipPullResult {
    pub messages: Vec<Message>,
    pub newest_seen_id: Option<u64>,
}
```

Run: `cargo test channel::zulip -- --nocapture`

Expected: fails until the result type and helper exist.

- [ ] **Step 2: Split Zulip pull behavior.**

Move pagination and filtering into a transport function that returns `ZulipPullResult`.

Do not call:
- `crate::message::ensure_dirs`
- `crate::message::write_message`
- `crate::zulip_sync::remember_seen_message_id`

from `channel/zulip.rs`.

- [ ] **Step 3: Move state and inbox writes into `cryo_zulip.rs`.**

Update `cmd_pull` and `ZulipSyncLoopBackend::receive` to:
- call the new Zulip transport function,
- use `zulip_sync::remember_seen_message_id` while updating `last_message_id`,
- write returned messages with `message::write_message(&dir, "inbox", msg)`,
- persist `zulip-sync.json`.

- [ ] **Step 4: Run focused tests.**

Run:

```bash
cargo test --test zulip_channel_tests -- --nocapture
cargo test --test zulip_sync_tests -- --nocapture
cargo test channel::zulip -- --nocapture
```

Expected: Zulip parser/sync tests pass.

- [ ] **Step 5: Commit.**

```bash
git add src/channel/zulip.rs src/bin/cryo_zulip.rs src/unit_tests/channel/zulip.rs tests/zulip_channel_tests.rs tests/zulip_sync_tests.rs
git commit -m "refactor: separate Zulip transport from inbox persistence"
```

### Task 5: Share Outbox Archiving

**Files:**
- Modify: `src/message.rs`
- Modify: `src/bin/cryo_gh.rs`
- Modify: `src/bin/cryo_zulip.rs`
- Test: `src/unit_tests/message.rs`
- Test: `tests/gh_sync_tests.rs`
- Test: `tests/zulip_sync_tests.rs`

- [ ] **Step 1: Add tests for outbox archive helper.**

Add `message::archive_outbox_messages(dir, filenames)` tests matching `archive_messages` behavior for inbox.

Run: `cargo test message:: -- --nocapture`

Expected: fails until helper exists.

- [ ] **Step 2: Implement `archive_outbox_messages`.**

Use the same semantics as `archive_messages`, but move files from `messages/outbox/` to `messages/outbox/archive/`.

- [ ] **Step 3: Rewire sync CLIs.**

Replace duplicated archive-path construction in `push_outbox` for GitHub and Zulip with `message::archive_outbox_messages`.

- [ ] **Step 4: Verify.**

Run:

```bash
cargo test message:: -- --nocapture
cargo test --test gh_sync_tests -- --nocapture
cargo test --test zulip_sync_tests -- --nocapture
cargo clippy --all-targets -- -D warnings
```

Expected: all targeted tests pass and clippy reports no warnings.

- [ ] **Step 5: Commit.**

```bash
git add src/message.rs src/bin/cryo_gh.rs src/bin/cryo_zulip.rs src/unit_tests/message.rs tests/gh_sync_tests.rs tests/zulip_sync_tests.rs
git commit -m "refactor: share outbox archiving across sync backends"
```

---

## Chunk 4: Introduce Chamber Read Models for Hub and CLI

### Task 6: Extract Chamber Status Read Model

**Files:**
- Create: `src/chamber_status.rs`
- Modify: `src/lib.rs`
- Modify: `src/hub/routes/chamber.rs`
- Modify: `src/hub/discovery.rs`
- Modify: `src/bin/cryo.rs` only if status output can reuse helpers without changing text
- Test: add `src/unit_tests/chamber_status.rs`
- Test: `src/unit_tests/hub/routes/chamber.rs`
- Test: `src/unit_tests/hub/discovery.rs`
- Test: `tests/cli_tests.rs`

- [ ] **Step 1: Add read-model tests.**

Create tests for:
- missing `timer.json` returns stopped/default status,
- state agent override wins over `cryo.toml` agent,
- next wake comes from the earliest open todo,
- completion summary comes from latest session log,
- messages are sorted chronologically and tagged with session numbers.

Run: `cargo test chamber_status:: -- --nocapture`

Expected: fails until `chamber_status` exists.

- [ ] **Step 2: Create core read-model structs.**

Add serializable structs where useful, but keep hub-specific JSON formatting in hub routes if that avoids churn:

```rust
pub struct ChamberStatus {
    pub running: bool,
    pub session: u32,
    pub agent: String,
    pub log_tail: String,
    pub next_wake: Option<String>,
    pub notes_content: String,
    pub task: Option<String>,
    pub completed: bool,
    pub completion_summary: Option<String>,
}

pub struct ChamberMessage {
    pub id: String,
    pub direction: String,
    pub from: String,
    pub subject: String,
    pub body: String,
    pub timestamp: String,
    pub session: Option<u32>,
}

pub struct ChamberOverview {
    pub running: bool,
    pub session: Option<u32>,
    pub next_wake: Option<String>,
    pub next_wake_display: Option<String>,
    pub wake_imminent: bool,
    pub unread: usize,
    pub task: Option<String>,
    pub last_message_preview: Option<String>,
    pub completed: bool,
    pub sync: Vec<crate::hub::discovery::SyncBadge>,
}
```

Helper functions:

```rust
pub fn status(dir: &Path) -> ChamberStatus;
pub fn messages(dir: &Path) -> Vec<ChamberMessage>;
pub fn todos(dir: &Path) -> Vec<TodoItem>;
pub fn overview(dir: &Path) -> ChamberOverview;
```

- [ ] **Step 3: Rewire hub route JSON builders.**

Update `hub/routes/chamber.rs`:
- `status_json` calls `chamber_status::status`.
- `messages_json` calls `chamber_status::messages`.
- `todos_json` calls `chamber_status::todos`.

Do not change the JSON keys returned by existing HTTP endpoints.

- [ ] **Step 4: Rewire discovery runtime population.**

Update `hub/discovery.rs` to use `chamber_status::overview` for runtime fields instead of directly reading state, todos, messages, logs, and sync summaries.

- [ ] **Step 5: Consider CLI reuse without output changes.**

If `src/bin/cryo.rs::cmd_status` can call `chamber_status::status` without changing output text, do that. Otherwise leave CLI output untouched and note why in the commit message.

- [ ] **Step 6: Verify focused behavior.**

Run:

```bash
cargo test chamber_status:: -- --nocapture
cargo test hub::routes::chamber:: -- --nocapture
cargo test hub::discovery:: -- --nocapture
cargo test --test cli_tests -- --nocapture
```

Expected: all focused tests pass and existing endpoint/CLI behavior is unchanged.

- [ ] **Step 7: Run full verification.**

Run:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: all tests pass and clippy reports no warnings.

- [ ] **Step 8: Commit.**

```bash
git add src/lib.rs src/chamber_status.rs src/hub/routes/chamber.rs src/hub/discovery.rs src/bin/cryo.rs src/unit_tests/chamber_status.rs src/unit_tests/hub/routes/chamber.rs src/unit_tests/hub/discovery.rs tests/cli_tests.rs
git commit -m "refactor: extract chamber status read model"
```

---

## Chunk 5: Reduce Hub / CLI Lifecycle Duplication

### Task 7: Move Per-Chamber Lifecycle Operations into Core Lifecycle

**Files:**
- Modify: `src/lifecycle.rs`
- Modify: `src/hub/lifecycle.rs`
- Modify: `src/bin/cryo.rs`
- Test: `src/unit_tests/hub/lifecycle.rs`
- Test: `src/unit_tests/process.rs`
- Test: `tests/lifecycle_tests.rs`
- Test: `tests/cli_hub.rs`

- [ ] **Step 1: Add tests around shared stop/restart behavior.**

Cover:
- stop clears PID but preserves `timer.json` overrides,
- cancel removes `timer.json`,
- restart preserves session number and overrides,
- hub reset archives runtime files and recreates message directories.

Run: `cargo test lifecycle -- --nocapture`

Expected: tests fail or require updates until operations are shared.

- [ ] **Step 2: Move shared operations to `src/lifecycle.rs`.**

Add explicit-dir APIs:

```rust
pub fn stop_chamber(dir: &Path) -> Result<()>;
pub fn restart_chamber(dir: &Path, exe: &Path) -> Result<DaemonLaunchMode>;
pub fn archive_logs(dir: &Path) -> Result<PathBuf>;
pub fn archive_runtime(dir: &Path) -> Result<PathBuf>;
pub fn reset_chamber(dir: &Path) -> Result<PathBuf>;
```

Keep binary-resolution logic in the caller:
- `cryo` can use `current_exe()`.
- `cryohub` still needs sibling/PATH resolution for `cryo`.

- [ ] **Step 3: Thin out `hub/lifecycle.rs`.**

Keep only:
- `resolve_cryo_exe`,
- hub-specific wrappers that pass the resolved `cryo` executable to core lifecycle,
- any hub-specific watcher/reset coordination remains in routes/state.

- [ ] **Step 4: Rewire CLI lifecycle paths where safe.**

Update `cmd_restart`, `cmd_cancel`, and `cmd_clean` only where shared helpers preserve exact behavior and output. Do not change user-facing messages unless a test is updated intentionally.

- [ ] **Step 5: Verify lifecycle behavior.**

Run:

```bash
cargo test --test lifecycle_tests -- --nocapture
cargo test --test cli_hub -- --nocapture
cargo test hub::lifecycle -- --nocapture
cargo clippy --all-targets -- -D warnings
```

Expected: all lifecycle and hub tests pass.

- [ ] **Step 6: Commit.**

```bash
git add src/lifecycle.rs src/hub/lifecycle.rs src/bin/cryo.rs src/unit_tests/hub/lifecycle.rs tests/lifecycle_tests.rs tests/cli_hub.rs
git commit -m "refactor: share chamber lifecycle operations"
```

---

## Chunk 6: Split Daemon Internals After Boundaries Are Stable

### Task 8: Extract Daemon Request Handling

**Files:**
- Create: `src/daemon/request.rs`
- Modify: `src/daemon.rs` or convert to `src/daemon/mod.rs`
- Test: existing `src/unit_tests/daemon.rs`
- Test: existing `src/unit_tests/daemon_properties.rs`

- [ ] **Step 1: Move internal request enums and pure handlers.**

Move these from `daemon.rs`:
- `TodoRequest`
- `DaemonRequest`
- `impl From<socket::Request> for DaemonRequest`
- `TodoRequestOutcome`
- `TodoOperationError`
- `TodoEffects`
- `handle_todo_request`
- `resolve_hibernate_request` if it does not need daemon instance state

- [ ] **Step 2: Keep public behavior unchanged.**

`Daemon::handle_active_request` should become orchestration over helpers, not the owner of all request decision logic.

- [ ] **Step 3: Verify daemon tests.**

Run:

```bash
cargo test daemon:: -- --nocapture
cargo test daemon_properties:: -- --nocapture
```

Expected: daemon unit and property tests pass.

- [ ] **Step 4: Commit.**

```bash
git add src/daemon.rs src/daemon/request.rs src/unit_tests/daemon.rs src/unit_tests/daemon_properties.rs
git commit -m "refactor: extract daemon request handling"
```

### Task 9: Extract Daemon Session Effects and Runtime

**Files:**
- Create: `src/daemon/effects.rs`
- Create: `src/daemon/session.rs`
- Modify: `src/daemon.rs` or `src/daemon/mod.rs`
- Test: existing daemon tests

- [ ] **Step 1: Move effect traits and filesystem implementations.**

Move:
- `SessionEffects`
- `FsSessionEffects`
- `FileTodoEffects`
- `SessionRuntime`
- `ProcessSessionRuntime`
- `SessionLauncher`
- `ProcessSessionLauncher`

Keep `Daemon::drive_active_session` either in `session.rs` or as a thin method that delegates to `session::drive_active_session`.

- [ ] **Step 2: Verify no public API changed.**

`Daemon::new(dir)` and `Daemon::run()` remain the public entry points.

- [ ] **Step 3: Verify daemon and integration tests.**

Run:

```bash
cargo test daemon:: -- --nocapture
cargo test --test daemon_tests -- --nocapture
cargo test --test mock_agent_tests -- --nocapture
cargo test --test integration_test -- --nocapture
```

Expected: all targeted tests pass.

- [ ] **Step 4: Commit.**

```bash
git add src/daemon.rs src/daemon/effects.rs src/daemon/session.rs src/unit_tests/daemon.rs tests/daemon_tests.rs tests/mock_agent_tests.rs tests/integration_test.rs
git commit -m "refactor: extract daemon session runtime and effects"
```

### Task 10: Extract Daemon Scheduling and Bootstrap

**Files:**
- Create: `src/daemon/schedule.rs`
- Modify: `src/daemon.rs` or `src/daemon/mod.rs`
- Test: existing daemon tests and report tests

- [ ] **Step 1: Move scheduling helpers.**

Move:
- `RetryState`
- `RetryPlan`
- `scheduled_fallback_for`
- `should_rotate_provider`
- `compute_sleep_timeout`
- `next_wake_from_todos`
- `detect_delayed_wake`
- `delayed_wake_notice`
- `pending_fallback_to_state`
- `pending_fallback_from_state`
- bootstrap structs that are schedule/bootstrap-only

- [ ] **Step 2: Preserve event-loop behavior.**

`Daemon::run_with_platform` should remain readable: load config/state, compute bootstrap, start resources, run loop. Scheduling decisions should call helpers from `schedule.rs`.

- [ ] **Step 3: Verify broad runtime behavior.**

Run:

```bash
cargo test daemon:: -- --nocapture
cargo test report:: -- --nocapture
cargo test state:: -- --nocapture
cargo test todo:: -- --nocapture
cargo test --test daemon_tests -- --nocapture
cargo test --test integration_test -- --nocapture
cargo test --test mock_agent_tests -- --nocapture
cargo clippy --all-targets -- -D warnings
```

Expected: all targeted tests pass and clippy reports no warnings.

- [ ] **Step 4: Commit.**

```bash
git add src/daemon.rs src/daemon/schedule.rs src/unit_tests/daemon.rs src/unit_tests/report.rs src/unit_tests/state.rs src/unit_tests/todo.rs tests/daemon_tests.rs tests/integration_test.rs tests/mock_agent_tests.rs
git commit -m "refactor: extract daemon scheduling logic"
```

---

## Final Verification

- [ ] **Step 1: Run the standard repo check.**

```bash
make check
```

Expected: format check, clippy, and tests all pass.

- [ ] **Step 2: Run mock integration checks.**

```bash
make check-mock
```

Expected: mock-agent integration tests pass.

- [ ] **Step 3: Run hub checks if the PR touched hub behavior.**

```bash
cargo test --test cli_hub -- --nocapture
cargo test --test hub_multi_chamber -- --nocapture
```

Expected: hub CLI and multi-chamber integration tests pass.

- [ ] **Step 4: Review module dependency direction.**

Run a source search and confirm these are true:

```bash
rg -n "crate::state" src/socket.rs src/process.rs
rg -n "crate::gh_sync|crate::zulip_sync" src/sync_common.rs
rg -n "crate::message::write_message|crate::message::ensure_dirs|crate::zulip_sync" src/channel
```

Expected:
- no `state` references in `socket.rs` or `process.rs`,
- no concrete backend references in `sync_common.rs`,
- no local inbox persistence or sync-state references in channel transport modules.

- [ ] **Step 5: Commit final documentation updates if needed.**

Only update docs if behavior or public architecture changed. Do not commit this implementation plan unless the PR policy explicitly wants plan documents included.

---

## Suggested PR Boundaries

If this grows beyond a single reviewable PR, split it in this order:

1. `daemon_client` extraction and low-level dependency cleanup.
2. `sync_control` extraction and sync-common cleanup.
3. Channel transport/local persistence split.
4. Chamber status read model and lifecycle deduplication.
5. Daemon internal file split.

Each PR should pass `make check` independently and should avoid changing user-visible behavior unless explicitly called out in its description.
