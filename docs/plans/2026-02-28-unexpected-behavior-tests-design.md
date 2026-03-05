# Unexpected Behavior Test Suite Design

Testing daemon handling of agent misbehavior, environment surprises, and user misuse.

## Approach

Hybrid — three test layers, each using the right tool:

1. **Mock scenario scripts** (`.sh`) — agent misbehavior and daemon integration
2. **Unit tests** — daemon internals, config/state/socket/log parsing edge cases
3. **CLI integration tests** — user misuse via `assert_cmd`

## Section 1: Agent Misbehavior

New mock `.sh` scenarios + integration tests in `mock_agent_tests.rs`.

| # | Scenario | Behavior | Assert |
|---|----------|----------|--------|
| 1 | `invalid-wake-time.sh` | `hibernate --wake "banana"` | Daemon logs parse error, retries |
| 2 | `slow-exit-no-hibernate.sh` | `sleep 8 && exit 0` | "exit without hibernate", not quick-exit |
| 3 | `double-hibernate.sh` | Two hibernate calls in one session | First wins, second ignored |
| 4 | `note-after-hibernate.sh` | Note sent after hibernate --complete | Session completes, late note harmless |
| 5 | `orphan-child.sh` | Background subprocess + hibernate | Daemon doesn't hang |
| 6 | `hibernate-then-crash.sh` | Hibernate --wake then exit 1 | Daemon respects hibernate despite exit code |

## Section 2: Provider Rotation

Multi-provider configs with specific `rotate_on` policies. Each test writes `cryo.toml` with 2-3 dummy providers, then uses crash/quick-exit scenarios to trigger rotation.

| # | Test | Config | Assert |
|---|------|--------|--------|
| 7 | `rotate_on_quick_exit_rotates` | `quick-exit`, 2 providers | Rotation on quick exit |
| 8 | `rotate_on_quick_exit_no_rotate_on_crash` | `quick-exit`, 2 providers | No rotation on crash |
| 9 | `rotate_on_any_failure_rotates` | `any-failure`, 2 providers | Rotation on crash |
| 10 | `rotate_on_never_no_rotation` | `never`, 2 providers | No rotation ever |
| 11 | `provider_wrap_all_exhausted` | `any-failure`, 2 providers | Wrap detected, 60s backoff |
| 12 | `provider_env_injected` | 1 provider with env var | `check-env.sh` writes env to file |

New scenario: `check-env.sh` — `echo "$MOCK_VAR" > .env-check && cryo-agent hibernate --complete`

## Section 3: Fallback, Delayed Wake & Reports

### Fallback dead-man switch

| # | Test | Setup | Assert |
|---|------|-------|--------|
| 13 | `fallback_fires_on_deadline` | `fallback_alert = "outbox"` | Outbox contains alert after retries exhaust |
| 14 | `fallback_suppressed_when_none` | `fallback_alert = "none"` | Outbox empty |
| 15 | `fallback_cancelled_on_success` | `fallback_alert = "outbox"` | Session completes, no fallback |

New scenarios:
- `alert-then-crash.sh` — `cryo-agent alert email ops@test.com "Session stuck" && exit 1`
- `alert-then-succeed.sh` — `cryo-agent alert email ops@test.com "Watchdog set" && cryo-agent hibernate --complete`

### Delayed wake

| # | Test | Setup | Assert |
|---|------|-------|--------|
| 16 | `delayed_wake_detection` | Pre-seed `timer.json` with stale `next_wake` (10 min ago) | Log contains delay detection message |

### Periodic reports

| # | Test | Setup | Assert |
|---|------|-------|--------|
| 17 | `periodic_report_fires` | `report_interval=1`, old `last_report_time` | Report logged, `last_report_time` updated |
| 18 | `invalid_report_time_warns` | `report_time="not-a-time"` | Warning in daemon output |

## Section 4: Daemon Unit Tests

In `daemon::tests` inside `src/daemon.rs`. Fast, deterministic, no daemon spawn.

### RetryState

| # | Test | Covers |
|---|------|--------|
| 19 | `backoff_exact_sequence` | 5->10->20->40->80->160->320->640->1280->2560->3600->3600 |
| 20 | `backoff_cap_at_3600` | Never exceeds 3600s |
| 21 | `rotate_single_provider` | count=1 -> always wraps |
| 22 | `rotate_advances_index` | count=3 -> 0->1->2->0, wrap on reset |
| 23 | `reset_clears_attempt_preserves_provider` | attempt=0, index unchanged |
| 24 | `exhausted_boundary` | attempt == max -> true |

### Timeout calculation

| # | Test | Covers |
|---|------|--------|
| 25 | `timeout_both_wake_and_report` | `(Some, Some)` -> picks earlier |
| 26 | `timeout_wake_only` | `(Some, None)` -> uses wake |
| 27 | `timeout_report_only` | `(None, Some)` -> uses report |
| 28 | `timeout_neither` | `(None, None)` -> default 3600s |

### Delayed wake detection

| # | Test | Covers |
|---|------|--------|
| 29 | `delayed_wake_under_threshold` | 4 min late -> not delayed |
| 30 | `delayed_wake_over_threshold` | 6 min late -> delayed, formatted |

Implementation note: Tests 25-30 may require extracting inline logic into small helper functions.

## Section 5: Config, State & Socket Unit Tests

### Config (`config::tests`)

| # | Test | Covers |
|---|------|--------|
| 31 | `load_malformed_toml` | Garbage -> descriptive error |
| 32 | `load_partial_toml` | Missing fields -> defaults applied |
| 33 | `apply_overrides_all_fields` | All overrides applied to config |
| 34 | `apply_overrides_none_fields` | Config unchanged |

### State (`state::tests`)

| # | Test | Covers |
|---|------|--------|
| 35 | `load_empty_state_file` | 0 bytes -> Ok(None) |
| 36 | `load_corrupted_state` | `{broken` -> error |
| 37 | `load_minimal_state` | Only session_number -> defaults |
| 38 | `is_locked_stale_pid` | Dead PID -> false |
| 39 | `is_locked_no_pid` | None -> false |
| 40 | `is_locked_own_pid` | Current PID -> true |

### Socket (`socket::tests`)

| # | Test | Covers |
|---|------|--------|
| 41 | `accept_empty_line` | `\n` -> Ok(None) |
| 42 | `accept_malformed_json` | Bad JSON -> parse error |
| 43 | `accept_unknown_fields` | Extra fields -> test actual serde behavior |

## Section 6: Log Parsing Unit Tests

In `log::tests`. All operate on string content, no filesystem needed.

### Session reading

| # | Test | Covers |
|---|------|--------|
| 44 | `read_session_incomplete` | No END marker -> returns to EOF |
| 45 | `read_session_end_before_start` | Malformed -> Ok(None) |
| 46 | `read_latest_multiple_sessions` | Returns only last |

### Note extraction

| # | Test | Covers |
|---|------|--------|
| 47 | `parse_notes_escaped_quotes` | `\"` in note text |
| 48 | `parse_notes_empty_session` | No notes -> empty vec |
| 49 | `parse_notes_truncated_line` | No closing quote -> skipped |

### Wake time extraction

| # | Test | Covers |
|---|------|--------|
| 50 | `parse_wake_valid` | Correct format -> parsed |
| 51 | `parse_wake_missing_comma` | No comma -> None |
| 52 | `parse_wake_no_wake_line` | Complete without wake -> None |

### Task extraction

| # | Test | Covers |
|---|------|--------|
| 53 | `parse_task_present` | Task line -> Some |
| 54 | `parse_task_absent` | No task -> None |

### Session header parsing

| # | Test | Covers |
|---|------|--------|
| 55 | `parse_session_header_valid` | SESSION 5 -> 5 |
| 56 | `parse_session_header_malformed` | SESSION abc -> skipped |
| 57 | `parse_sessions_since_filters` | Date filter works |

## Section 7: User Misuse CLI Tests

New file: `tests/cli_edge_tests.rs`. Uses `assert_cmd`.

### Commands against stopped daemon

| # | Test | Assert |
|---|------|--------|
| 58 | `status_no_daemon` | Graceful "not running" |
| 59 | `cancel_no_daemon` | Graceful exit |
| 60 | `wake_no_daemon` | Error, no daemon |
| 61 | `send_no_daemon` | Message written or clear error |
| 62 | `agent_note_no_daemon` | Connection error |
| 63 | `agent_hibernate_no_daemon` | Connection error |

### Double start / stale lock

| # | Test | Assert |
|---|------|--------|
| 64 | `start_while_running` | "Already running" error |
| 65 | `start_stale_pid_lock` | Stale lock overridden, start succeeds |

### Corrupted project state

| # | Test | Assert |
|---|------|--------|
| 66 | `start_missing_plan` | Actionable error about missing plan |
| 67 | `start_corrupted_config` | Parse error |
| 68 | `start_corrupted_state` | Recovers or clear error |

### Message edge cases

| # | Test | Assert |
|---|------|--------|
| 69 | `send_creates_inbox_dir` | Dir created, message written |
| 70 | `receive_empty_inbox` | Success, no messages |
| 71 | `receive_malformed_message` | Skips bad file, no crash |

### Time subcommand

| # | Test | Assert |
|---|------|--------|
| 72 | `time_no_offset` | Prints YYYY-MM-DDTHH:MM format |
| 73 | `time_invalid_offset` | Parse error |

## New Files

- 9 scenario scripts in `tests/scenarios/`
- `tests/cli_edge_tests.rs` (16 tests)
- Unit tests added to existing `#[cfg(test)]` modules in `daemon.rs`, `config.rs`, `state.rs`, `socket.rs`, `log.rs`
- Integration tests added to `mock_agent_tests.rs`

## Implementation Notes

- Some daemon unit tests (25-30) require extracting inline logic into helper functions
- Provider rotation tests need multi-provider `cryo.toml` configs written in test setup
- Delayed wake test (16) pre-seeds `timer.json` with stale `next_wake` before daemon start
- Stale PID test (38) spawns a short-lived child, waits for exit, uses its PID
