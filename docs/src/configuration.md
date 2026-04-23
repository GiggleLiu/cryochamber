# Configuration

`cryo init` creates a `cryo.toml` file with project settings:

```toml
# cryo.toml — Cryochamber project configuration
agent = "opencode"        # Agent command (opencode, claude, codex, etc.)
max_session_duration = 0  # Session timeout in seconds (0 = no timeout)
watch_inbox = true        # Watch inbox for reactive wake

# Periodic status report written to messages/outbox/
# report_time = "09:00"     # HH:MM local time
# report_interval = 24      # hours between reports; 0 disables reports
```

## Fields

| Field | Default | Description |
|-------|---------|-------------|
| `agent` | `"opencode"` | Agent command to run. Use `"claude"` for Claude Code, `"codex"` for Codex. |
| `max_session_duration` | `0` | Session timeout in seconds. `0` disables timeout. |
| `watch_inbox` | `true` | Watch `messages/inbox/` for new files and wake immediately. |
| `report_time` | `"09:00"` | Local wall-clock time for periodic status reports, formatted as `HH:MM`. |
| `report_interval` | `0` | Hours between periodic reports. `0` disables reports; common values are `24` for daily and `168` for weekly. Reports are written to `messages/outbox/`. |

`cryohub` settings are not `cryo.toml` fields — they are CLI flags
(`cryohub start [--host 0.0.0.0] [--port 8765]`, host/port default to
`127.0.0.1:8765`). `cryohub` always operates on the current directory; `cd`
into a directory whose immediate subdirectories are chambers (not into a
chamber itself) before running it.

## CLI Overrides

CLI flags to `cryo start` override config values for that session:

```bash
cryo start --agent claude             # override agent
cryo start --max-session-duration 3600  # override timeout
```

These overrides are stored in `timer.json` (runtime state) and do not modify `cryo.toml`.

## Config vs State

| File | Purpose | Persists |
|------|---------|----------|
| `cryo.toml` | Project configuration (checked into git) | Yes |
| `timer.json` | Runtime state (session number, PID lock, CLI overrides) | No (ephemeral) |
