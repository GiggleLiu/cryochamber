# Configuration

Each chamber is configured through a `cryo.toml` file in its directory. `cryo init` creates one with sensible defaults.

## Sample `cryo.toml`

```toml
# cryo.toml — cryochamber project configuration
agent = "opencode"        # Agent command (opencode, claude, codex, ...)
max_session_duration = 0  # Session timeout in seconds (0 = no timeout)
watch_inbox = true        # Wake immediately when a new inbox file appears

# Periodic status report written to messages/outbox/
# report_time = "09:00"     # local wall-clock time (HH:MM)
# report_interval = 24      # hours between reports (0 disables)
```

## Fields

| Field                  | Default      | Description                                                                                                                                                |
|------------------------|--------------|------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `agent`                | `"opencode"` | Agent command to run. Use `"claude"` for Claude Code, `"codex"` for Codex, or any executable on `PATH`.                                                    |
| `max_session_duration` | `0`          | Session timeout in seconds. `0` disables the timeout.                                                                                                      |
| `watch_inbox`          | `true`       | Watch `messages/inbox/` for new files and wake the agent immediately.                                                                                      |
| `report_time`          | `"09:00"`    | Local wall-clock time for periodic reports, formatted `HH:MM`.                                                                                             |
| `report_interval`      | `0`          | Hours between periodic reports. `0` disables reports; common values are `24` (daily) and `168` (weekly). Reports are written to `messages/outbox/`.        |

> **Note**: Cryohub settings are not in `cryo.toml`. Pass `--host` and `--port` on the command line (defaults: `127.0.0.1:8765`). Cryohub always operates on the current directory and refuses to start in a directory that itself contains a `cryo.toml`.

## Override config from the command line

Flags passed to `cryo start` override `cryo.toml` for that session. The overrides are stored in `timer.json` (runtime state) and do not modify `cryo.toml`.

```bash
cryo start --agent claude               # override the agent
cryo start --max-session-duration 3600  # override the timeout
```

## Config vs. state

| File         | Purpose                                                       | Persists across runs |
|--------------|---------------------------------------------------------------|----------------------|
| `cryo.toml`  | Project configuration. Check into git.                        | Yes                  |
| `timer.json` | Runtime state (session number, PID lock, CLI overrides).      | No                   |
