# Configuration

`cryo init` creates a `cryo.toml` file with project settings:

```toml
# cryo.toml — Cryochamber project configuration
agent = "opencode"        # Agent command (opencode, claude, codex, etc.)
max_retries = 1           # Failed attempts before retry alerting (daemon keeps retrying)
max_session_duration = 0  # Session timeout in seconds (0 = no timeout)
watch_inbox = true        # Watch inbox for reactive wake

# Fallback alert method when the dead-man switch fires
# fallback_alert = "outbox"  # "outbox" writes to messages/outbox/; "none" disables alerts

# Periodic status report written to messages/outbox/
# report_time = "09:00"     # HH:MM local time
# report_interval = 24      # hours between reports; 0 disables reports
```

## Fields

| Field | Default | Description |
|-------|---------|-------------|
| `agent` | `"opencode"` | Agent command to run. Use `"claude"` for Claude Code, `"codex"` for Codex. |
| `max_retries` | `5` | Failed attempts before sending a retry alert. The daemon continues retrying with backoff. Template uses `1` for fail-fast one-shot tasks; bump to `5+` for long-running assistants. |
| `max_session_duration` | `0` | Session timeout in seconds. `0` disables timeout. |
| `watch_inbox` | `true` | Watch `messages/inbox/` for new files and wake immediately. |
| `fallback_alert` | `"outbox"` | Fallback alert behavior. `"outbox"` writes alerts to `messages/outbox/`; `"none"` suppresses fallback alert files. Legacy `"notify"` is accepted and treated as `"outbox"`. |
| `report_time` | `"09:00"` | Local wall-clock time for periodic status reports, formatted as `HH:MM`. |
| `report_interval` | `0` | Hours between periodic reports. `0` disables reports; common values are `24` for daily and `168` for weekly. Reports are written to `messages/outbox/`. |

`cryo web` host and port are not `cryo.toml` fields — they are CLI flags
(`cryo web --host 0.0.0.0 --port 8765`, defaults `127.0.0.1:8765`). `cryo web`
runs at the workspace level (expects a `chambers/` directory), not per-chamber.

## CLI Overrides

CLI flags to `cryo start` override config values for that session:

```bash
cryo start --agent claude             # override agent
cryo start --max-retries 3            # override retries
cryo start --max-session-duration 3600  # override timeout
```

These overrides are stored in `timer.json` (runtime state) and do not modify `cryo.toml`.

## Config vs State

| File | Purpose | Persists |
|------|---------|----------|
| `cryo.toml` | Project configuration (checked into git) | Yes |
| `timer.json` | Runtime state (session number, PID lock, CLI overrides) | No (ephemeral) |
