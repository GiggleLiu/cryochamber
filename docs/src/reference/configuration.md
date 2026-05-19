# Configuration

Each chamber is configured through a `cryo.toml` file in its directory. `cryo init` creates one with sensible defaults.

## `cryo.toml`

```toml
# cryo.toml — cryochamber project configuration
agent = "opencode"               # Agent command (opencode, claude, codex, ...)
max_session_duration = 0         # Session timeout in seconds (0 = no timeout)
watch_dirs = ["messages/inbox"]  # Directories to watch for reactive wake ([] disables)
```

| Field | Default | Description |
|-------|---------|-------------|
| `agent` | `"opencode"` | Agent command to run. Use `"claude"` for Claude Code, `"codex"` for Codex, or any executable on `PATH`. |
| `max_session_duration` | `0` | Session timeout in seconds. `0` disables the timeout. |
| `watch_dirs` | `["messages/inbox"]` | List of directories the daemon watches for new files to wake the agent reactively. Paths are interpreted relative to the chamber directory unless absolute. Set to `[]` to disable reactive wake entirely. |

See [`cryohub.toml`](#cryohubtoml) below.

## `cryohub.toml`

Cryohub settings live in `$XDG_CONFIG_HOME/cryo/cryohub.toml`, or `~/.config/cryo/cryohub.toml` if `XDG_CONFIG_HOME` is unset. The default local dashboard URL is `http://127.0.0.1:8765`. The dashboard's New Chamber button creates chambers under the configured `chamber_root`, which defaults to `~/.cryo/chambers`.

```toml
host = "127.0.0.1"
port = 8765
chamber_root = "/Users/alice/.cryo/chambers"
```

For project-owned chamber collections, set `chamber_root` to a project path such as `/path/to/project/.cryo/chambers`.

| Field | Default | Description |
|-------|---------|-------------|
| `host` | `"127.0.0.1"` | Bind address for the global dashboard service. |
| `port` | `8765` | TCP port for the global dashboard service. |
| `chamber_root` | `~/.cryo/chambers` | Default location for chambers created from the dashboard UI. |

## Override config from the command line

Flags passed to `cryo start` override `cryo.toml` for that session. The overrides are stored in `timer.json` (runtime state) and do not modify `cryo.toml`.

```bash
cryo start --agent claude
cryo start --max-session-duration 3600
```

## Config vs. state

| File | Purpose | Persists across runs |
|------|---------|----------------------|
| `cryo.toml` | Project configuration. Check into git. | Yes |
| `timer.json` | Runtime state: session number, PID lock, CLI overrides. | No |
