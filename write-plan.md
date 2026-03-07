# Coverage Improvement Plan

**Goal:** Raise patch coverage from 70.35% → ≥90% by writing targeted unit and integration tests
for the 763 missing lines spread across 10+ files.

**Current state:** Codecov reports the following gaps:

| File | Patch % | Missing |
|------|---------|---------|
| `src/bin/cryo_zulip.rs` | 0.00% | 256 lines |
| `src/channel/zulip.rs` | 33.86% | 166 lines |
| `src/daemon.rs` | 76.65% | 141 lines |
| `src/bin/cryo.rs` | 25.28% | 65 lines |
| `src/platform/unix/service.rs` | 13.43% | 58 lines |
| `src/report.rs` | 86.82% | 17 lines |
| `src/bin/cryo_gh.rs` | 0.00% | 11 lines |
| `src/platform/unix/process.rs` | 80.00% | 11 lines |
| `src/message.rs` | 87.50% | 6 lines |
| `src/log.rs` | 98.60% | 5 lines |

**Strategy:** Work from the cheapest wins (pure functions, helpers) toward the most expensive
(network I/O, OS services). Tests that don't require daemon spawn go first; integration tests go last.

---

## Phase 1 — Pure function and helper tests (≈200 lines)

### Task 1.1 — `src/channel/zulip.rs`: cover `base64_encode` and `check_result`

Both are pure, side-effect-free helpers. Add `#[cfg(test)]` tests directly in
`src/channel/zulip.rs`.

- `base64_encode(b"")` → `""`
- `base64_encode(b"M")` → `"TQ=="`
- `base64_encode(b"Ma")` → `"TWE="`
- `base64_encode(b"Man")` → `"TWFu"` (no padding)
- Round-trip: `base64_encode(b"hello world")` == standard result
- `check_result`: `{"result":"error","msg":"Bad API key"}` → `Err` with message
- `check_result`: `{"result":"success"}` → `Ok(())`

### Task 1.2 — `src/channel/zulip.rs`: cover `basic_auth` format

Add a test that constructs a `ZulipClient` from a temp zuliprc and calls
`client.credentials()` to derive the expected Basic auth header manually, then
verifies `basic_auth()` matches `"Basic <base64(email:key)>"`.

### Task 1.3 — `src/message.rs`: cover `read_inbox_archive`

Add tests in `tests/message_tests.rs`:

- `read_inbox_archive` on missing directory → `Ok([])`
- `read_inbox_archive` with one valid `.md` file → returns the message
- `read_inbox_archive` skips non-`.md` files and subdirectories

### Task 1.4 — `src/log.rs`: cover remaining 5 lines

Identify the uncovered branches via `make coverage` and add targeted tests in
`src/log.rs::tests`:

- `session_count` on a non-existent file → `0`
- `session_count` on a log with N sessions → correct count
- `read_latest_session` on a non-existent file → `Ok(None)`
- `parse_latest_session_notes` scan-backward: session 1 has note, session 2 has no
  note → return session 2's (empty) notes or fall back to session 1's notes — test
  actual backward-scan behavior.

### Task 1.5 — `src/report.rs`: cover `send_report_notification` period label logic

The `period_label` match arm is three branches (0–23h, 24–167h, ≥168h).
Add unit tests for `generate_report` with synthetic logs covering each period bucket, and
a test for `send_report_notification` that catches the notification result (ignore the
actual desktop dispatch; just assert it does not panic and returns `Ok` or a graceful
error on headless CI by using `DISPLAY=` env var stub).

Alternatively, extract the period-label computation into a small `#[inline]` private
function `period_label(hours: u64) -> String` and add three direct tests:

- `period_label(0)` → `"0h"`
- `period_label(23)` → `"23h"`
- `period_label(24)` → `"1d"`
- `period_label(167)` → `"6d"`
- `period_label(168)` → `"1w"`
- `period_label(336)` → `"2w"`

---

## Phase 2 — Platform layer tests (≈70 lines)

### Task 2.1 — `src/platform/unix/process.rs`: cover `force_kill` error path

Add a test:

- `force_kill(4_000_000)` → `Err(...)` containing "Failed to send SIGKILL"

Cover `terminate_child` by spawning a real child (`sleep 10`) via `std::process::Command`,
then calling `terminate_child(&mut child, child.id())` and asserting the child is no
longer alive afterwards.

### Task 2.2 — `src/platform/unix/process.rs`: cover `terminate`

Add a test that spawns a real child (`sleep 60`) and calls `terminate(pid)`, asserting
the process is gone within the timeout. Mark with `#[cfg(unix)]`.

### Task 2.3 — `src/platform/unix/service.rs`: cover `is_installed` false path

Add `#[cfg(test)]` tests:

- `is_installed("nonexistent-label", some_tmpdir)` → `false` (file never created)
- Stub out the xml_escape function test (macOS-only): test that `&`, `<`, `>`, `"`, `'`
  are all escaped correctly. Expose `xml_escape` as `pub(crate)` for the test.

For Linux: test that `is_installed` returns `false` when the `.service` file does not
exist. These tests do not invoke `systemctl` or `launchctl` and are safe in CI.

The `install` / `uninstall` paths that shell out to `launchctl`/`systemctl` are covered
by `make check-service` (live test); mark as `#[ignore]` in unit tests or skip in CI.

---

## Phase 3 — Daemon helper unit tests (≈100 lines)

These live inside `src/daemon.rs #[cfg(test)]`.

### Task 3.1 — Cover `sleep_or_shutdown` interrupt path

- Set `shutdown = true` before calling → returns `true` immediately (< 1 ms)
- Call with `duration = 0` → returns `false` immediately
- Call with `duration = 300ms`, set flag after 50 ms in a thread → returns `true` in < 400 ms

### Task 3.2 — Cover `get_task`

Build a real `Daemon` pointing at a temp dir and:

- Log a session with a `task: ` line → `get_task()` returns `Some("...")`
- No log file → `get_task()` returns `None`
- Empty log → `get_task()` returns `None`

### Task 3.3 — Cover `check_fallback`

- `pending = None` → no panic, nothing happens
- `pending = Some((deadline_in_past, FallbackAction { action: "outbox", ... }))` →
  fallback executes, `pending` becomes `None`, outbox file created
- `pending = Some((deadline_in_future, ...))` → nothing happens

### Task 3.4 — Cover `handle_failure_retry` alert path

Create a temp dir with `cryo.toml` containing `fallback_alert = "outbox"`.
Construct a `Daemon` pointing at it. Set `retry.attempt = retry.max_retries - 1`,
call `handle_failure_retry(&mut retry, "outbox")`.
Assert:
- outbox contains a retry-exhausted alert message
- Returns `false` (does not shut down — daemon keeps retrying)

---

## Phase 4 — CLI binary tests via `assert_cmd` (≈100 lines)

These belong in `tests/cli_edge_tests.rs` (extend the existing file) or a new
`tests/zulip_cli_tests.rs` / `tests/gh_cli_tests.rs`.

### Task 4.1 — `cryo-zulip status` without zulip-sync.json

```
cryo-zulip status  (no zulip-sync.json present)
→ stdout contains "not configured"
→ exit success
```

### Task 4.2 — `cryo-zulip status` with valid zulip-sync.json

Write a synthetic `zulip-sync.json` to a temp dir and run:
```
cryo-zulip status
→ stdout contains the site URL, stream name, stream ID
→ exit success
```

### Task 4.3 — `cryo-zulip pull` without zulip-sync.json

```
cryo-zulip pull
→ exit failure
→ stderr/stdout contains "zulip-sync.json not found"
```

### Task 4.4 — `cryo-zulip push` without session log

Write `zulip-sync.json` + `.cryo/zuliprc` but no `cryo.log`:
```
cryo-zulip push
→ "No session log found"
→ exit success
```

### Task 4.5 — `cryo-zulip push` with already-pushed session

Write `zulip-sync.json` with `last_pushed_session = 1` and `timer.json` with
`session_number = 1`. Add a minimal `cryo.log` with one completed session.
```
cryo-zulip push
→ "already pushed"
→ exit success
```

### Task 4.6 — `cryo-gh status` without gh-sync.json

```
cryo-gh status
→ stdout contains "not configured"
→ exit success
```

### Task 4.7 — `cryo-gh status` with valid gh-sync.json

Write a synthetic `gh-sync.json` and run:
```
cryo-gh status
→ stdout contains repo name and discussion number
```

### Task 4.8 — `cryo-gh push` without session log

```
cryo-gh push  (no cryo.log)
→ "No session log found"
→ exit success
```

### Task 4.9 — `cryo status` with provider config

Write `cryo.toml` with two providers. Write `timer.json` with `provider_index = 1`.
```
cryo status
→ stdout contains "Provider: <name> (2/2)"
```

### Task 4.10 — `cryo clean --force` removes runtime files

```
cryo init && cryo clean --force
→ runtime files listed (timer.json, cryo.log, messages/) are absent
→ exit success
```

### Task 4.11 — `cryo receive` empty outbox

```
cryo receive  (no outbox)
→ "No messages in outbox"
→ exit success
```

### Task 4.12 — `cryo watch --all` on missing log exits cleanly

```
cryo watch --all  (no cryo.log, no timer.json)
→ exits within 3s
→ stdout contains "No cryochamber instance found"
```

---

## Phase 5 — Zulip sync daemon integration tests (≈120 lines)

These test `push_outbox` and `cmd_sync_daemon` internals using a mock HTTP server
(e.g. [`httpmock`](https://docs.rs/httpmock) or [`wiremock`](https://docs.rs/wiremock)).

### Task 5.1 — `push_outbox` with empty outbox

Construct a temp dir with no messages/outbox directory:
```rust
push_outbox(&dir, &client, &sync_state) → Ok(())
// No HTTP calls made
```

### Task 5.2 — `push_outbox` with one outbox message

Spin up a mock HTTP server that accepts `POST /api/v1/messages` and returns
`{"result":"success","id":999}`. Put one message in `messages/outbox/`.
Assert:
- HTTP POST was made
- Message file moved to `messages/outbox/archive/`

### Task 5.3 — `push_outbox` send failure leaves file in place

Mock server returns an error. Assert:
- File is NOT archived
- Function returns `Ok(())` (errors are logged, not propagated per message)

### Task 5.4 — `pull_messages` empty stream

Mock server returns `{"result":"success","messages":[],"found_newest":true}`.
```rust
client.pull_messages(stream_id, None, None, &dir) → Ok(None)
```
Inbox directory is empty.

### Task 5.5 — `pull_messages` self-filtering

Mock returns messages including one from `self_email`. Assert:
- Inbox contains only non-self messages
- `newest_id` reflects the highest raw message ID (even the filtered one)

---

## Phase 6 — `send_report_notification` smoke test (≈17 lines)

### Task 6.1 — Wrap notification in a feature flag or env-var guard

Add an env-var check: if `CRYO_SKIP_DESKTOP_NOTIFY=1`, skip the `notify_rust`
call and return `Ok(())`. This allows CI to exercise the function without needing
a display server. Then add a test:

```rust
std::env::set_var("CRYO_SKIP_DESKTOP_NOTIFY", "1");
let summary = ReportSummary { total_sessions: 3, failed_sessions: 1, period_hours: 25 };
assert!(send_report_notification(&summary, "my-project").is_ok());
```

---

## Sequencing

```
Phase 1  →  Phase 2  →  Phase 3  →  Phase 4  →  Phase 5  →  Phase 6
(fast)       (unix)       (daemon)     (CLI)       (HTTP mock)  (notify)
```

Each phase is independently mergeable. Phases 1–4 require no external
dependencies. Phase 5 adds `httpmock` or `wiremock` as a dev dependency.

## Acceptance criteria

- `make coverage` shows patch coverage ≥ 90%
- `make check` (fmt + clippy + test) passes with zero warnings
- No new `#[allow(dead_code)]` or `#[allow(unused)]` suppressions introduced
- All new tests are deterministic (no `std::thread::sleep` longer than 2s in unit tests)
- Integration tests that shell out (`launchctl`, `systemctl`) are tagged `#[ignore]`

