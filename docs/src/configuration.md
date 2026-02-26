# Configuration

`cryo init` creates a `cryo.toml` file with project settings:

```toml
# cryo.toml — Cryochamber project configuration
agent = "opencode"        # Agent command (opencode, claude, codex, etc.)
max_retries = 1           # Max retry attempts on agent failure (1 = no retry)
max_session_duration = 0  # Session timeout in seconds (0 = no timeout)
watch_inbox = true        # Watch inbox for reactive wake

# Web UI host and port (for `cryo web`)
# web_host = "127.0.0.1"
# web_port = 3945
```

## Fields

| Field | Default | Description |
|-------|---------|-------------|
| `agent` | `"opencode"` | Agent command to run. Use `"claude"` for Claude Code, `"codex"` for Codex. |
| `max_retries` | `1` | Max retry attempts on agent failure. `1` means no retry. |
| `max_session_duration` | `0` | Session timeout in seconds. `0` disables timeout. |
| `watch_inbox` | `true` | Watch `messages/inbox/` for new files and wake immediately. |
| `web_host` | `"127.0.0.1"` | Host for `cryo web` to listen on. Set to `"0.0.0.0"` for remote access. |
| `web_port` | `3945` | Port for `cryo web` to listen on. |

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
