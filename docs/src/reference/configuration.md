# Configuration

Each chamber is configured through a `cryo.toml` file in its directory. `cryo init` creates one with sensible defaults.

## `cryo.toml`

```toml
# cryo.toml — cryochamber project configuration
agent = "opencode"               # Agent command (opencode, claude, codex, pi, kimi, ...)
max_session_duration = 3600      # Session timeout in seconds (0 = no timeout)
watch_dirs = ["messages/inbox"]  # Directories to watch for reactive wake ([] disables)
zulip_poll_interval = 5          # Zulip sync poll interval in seconds
wait_timeout = 14400             # Default `cryo-agent receive --wait` timeout in seconds (clamped to 1-86400)

# Provider environment injected into every agent session (optional).
[provider]
name = "anthropic"               # Display name, shown in `cryo status`
env = { ANTHROPIC_API_KEY = "sk-ant-..." }  # Env vars set when spawning the agent
```

| Field | Default | Description |
|-------|---------|-------------|
| `agent` | `"opencode"` | Agent command to run. Use `"claude"` for Claude Code, `"codex"` for Codex, `"pi"` for Pi, `"kimi"` for Kimi Code, or any executable on `PATH`. |
| `max_session_duration` | `3600` | Session timeout in seconds. `0` disables the timeout. |
| `watch_dirs` | `["messages/inbox"]` | List of directories the daemon watches for new files to wake the agent reactively. Paths are interpreted relative to the chamber directory unless absolute. Set to `[]` to disable reactive wake entirely. |
| `zulip_poll_interval` | `5` | How often `cryo-zulip sync` polls Zulip, in seconds. `cryo-zulip sync --interval N` overrides it for one run. |
| `wait_timeout` | `14400` | Optional. Default timeout in seconds for `cryo-agent receive --wait` when the agent doesn't pass `--timeout`. The daemon clamps any value to `1`-`86400` (so `0` waits 1 s, not 0; the upper bound is 24 h). |

## `[provider]`

Cryochamber supports a single active provider profile. The `[provider]` table
carries a display `name` and an `env` map of environment variables that are
injected into every spawned agent session — this is where API keys for the
agent's model belong.

```toml
[provider]
name = "anthropic"
env = { ANTHROPIC_API_KEY = "sk-ant-...", OPENCODE_MODEL = "claude-sonnet-4-20250514" }
```

`cryo status` shows the provider name once one is configured.

> **Security**: values under `[provider].env` are secrets. `cryo init` writes a
> chamber `.gitignore` that ignores `.cryo/`, but `cryo.toml` itself is *not*
> gitignored — if you commit or push the chamber, keep API keys out of version
> control. Either add `cryo.toml` to your own `.gitignore`, or leave the keys
> out of `cryo.toml` and export them in the environment before `cryo start`
> instead.

### Legacy `[[providers]]` (deprecated)

Older configs used a `[[providers]]` array. It is still accepted for backward
compatibility, but only the first entry is used — provider rotation was removed.
Loading a config that uses `[[providers]]` prints a deprecation warning, and the
next `save` rewrites it to the canonical single `[provider]` form. Migrate to
`[provider]`.

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
