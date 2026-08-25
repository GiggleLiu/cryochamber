# Configuration

Each chamber is configured through a `cryo.toml` file in its directory. `cryo init` creates one with sensible defaults.

## `cryo.toml`

```toml
# cryo.toml — cryochamber project configuration
agent = "pi"                     # Agent command (pi, opencode, claude, codex, kimi, ...)
max_session_duration = 3600      # Session timeout in seconds (0 = no timeout)
watch_dirs = ["messages/inbox"]  # Directories to watch for reactive wake ([] disables)
zulip_poll_interval = 5          # Zulip sync poll interval in seconds

# Provider environment injected into every agent session (optional).
[provider]
name = "anthropic"               # Display name, shown in `cryo status`
env = { ANTHROPIC_API_KEY = "sk-ant-..." }  # Env vars set when spawning the agent
```

| Field | Default | Description |
|-------|---------|-------------|
| `agent` | `"pi"` | Agent command to run. Use `"opencode"` for OpenCode, `"claude"` for Claude Code, `"codex"` for Codex, `"kimi"` for Kimi Code, or any executable on `PATH`. New chambers use the host-level `default_agent` unless `cryo init --agent` supplies an explicit command. |
| `max_session_duration` | `3600` | Session timeout in seconds. `0` disables the timeout. |
| `watch_dirs` | `["messages/inbox"]` | List of directories the daemon watches for new files to wake the agent reactively. Paths are interpreted relative to the chamber directory unless absolute. Set to `[]` to disable reactive wake entirely. |
| `zulip_poll_interval` | `5` | How often `cryo-zulip sync` polls Zulip, in seconds. `cryo-zulip sync --interval N` overrides it for one run. |

> **The reply window is not configured here.** How long a successful
> `hibernate` stays open for a follow-up message is chosen by the agent per
> hibernate via `cryo-agent hibernate --linger <seconds>` (omitted = 300 s,
> capped at 86400; `0` sleeps immediately). The session clock is suspended
> while a hibernate is parked and each follow-up round gets a fresh budget,
> so a generous linger can hold one session open well past
> `max_session_duration`.

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
default_agent = "pi"
public = false
owner_name = "human"
public_hosts = []
# console_dir = "/absolute/path/to/console/dist"   # optional override, see below
```

For project-owned chamber collections, set `chamber_root` to a project path such as `/path/to/project/.cryo/chambers`.

Unknown keys are rejected: a typo such as `console-dir` fails `cryohub start` with an error naming the key rather than being silently ignored.

| Field | Default | Description |
|-------|---------|-------------|
| `host` | `"127.0.0.1"` | Bind address for the global dashboard service. |
| `port` | `8765` | TCP port for the global dashboard service. |
| `chamber_root` | `~/.cryo/chambers` | Default location for chambers created from the dashboard UI. |
| `default_agent` | `"pi"` | Host-level agent command for new chambers created by either the Console or plain `cryo init`. Change it in the Console's Settings sheet, edit this file, or run `cryohub start --default-agent <cmd>`. An explicit `cryo init --agent <cmd>` overrides it. Existing chambers keep their own `cryo.toml`. |
| `public` | `true` | Whether bearer-token auth is enforced on every `/api` route. On by default; a config file written before this default that omits the key also loads as `true`, while an explicit `public = false` stays open. Cleared only by `cryohub start --no-public` — a plain `cryohub start` keeps whatever is saved here. |
| `owner_name` | `"human"` | Sender name stamped on messages the owner sends in public mode. A client-supplied `from` is ignored. |
| `public_hosts` | `[]` | Extra `Host` header values to accept, on top of loopback and `host`. Needed when a reverse proxy forwards the public hostname. |
| `console_dir` | *(unset — embedded)* | Serve the [Agent Console](../agent-console.md) from this directory instead of the build embedded in the `cryohub` binary. Must be an absolute path to a vite `dist/`. Development and custom builds only. |

The Console updates `default_agent` without a hub restart. The next chamber it
creates uses the new command; no existing chamber is rewritten.

### Serving the Agent Console

The [Agent Console](../agent-console.md) is the hub's web surface — there is no other dashboard — and it is **embedded in the binary**: `cryohub start` serves it with no configuration.

The hub answers `/` and any client-side route with the console's `index.html`, serves hashed assets from `/assets/` with immutable caching, and keeps `/api` untouched. Nothing outside the console source is reachable — a `../` path or a symlink pointing out of an override directory is a 404. The console's own pages stay unauthenticated even under `--public`, because they are the login screen; every `/api` route stays behind the bearer token.

Set `console_dir` only to serve a different build (`make console-build` writes `console/dist/`). Make it **absolute**: the hub canonicalizes it from the service process's working directory, which launchd/systemd choose. `cryohub status` reports which source is live. A hub whose override directory has no `index.html` — or a binary built without the console and no override — answers pages with a short setup page (HTTP 503) rather than a bare 404; the API keeps working throughout.

### Behind a reverse proxy

The hub rejects any request whose `Host` header is neither loopback nor a configured host — that is what stops a malicious page from scripting the loopback service via DNS rebinding. A proxy that preserves the public hostname (Caddy's default) therefore needs that name allowed:

```toml
public_hosts = ["agents.example.com"]
```

The alternative is to make the proxy rewrite it — in Caddy, `header_up Host 127.0.0.1` inside the `reverse_proxy` block.

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
