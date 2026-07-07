# CLI reference

All cryochamber binaries and their commands. For tutorials and how-to guides, follow links from [Tutorial](../tutorial.md) and the [how-to guides](../how-to/create-chamber.md).

Every binary accepts `--version` (print the version and exit) and `--help`.

## Operator CLI (`cryo`)

Run these from inside a chamber directory unless noted otherwise.

| Command | What it does |
|---------|--------------|
| `cryo init [--agent <cmd>]` | Initialize the directory: write `cryo.toml`, `plan.md`, `NOTES.md`, and `README.md`. Existing files are kept. |
| `cryo start [--agent <cmd>]` | Start the daemon. Reads `cryo.toml` and writes overrides to `timer.json`. |
| `cryo start --max-session-duration 3600` | Override the session timeout for this run. |
| `cryo status` | Show whether the daemon is running, the current session number, and the next wake time. |
| `cryo restart` | Restart the running daemon. When it is installed as an OS service, restart the existing service without rewriting or removing it. |
| `cryo cancel` | Stop the daemon and remove the runtime state. |
| `cryo watch [--all] [--viewpoint cryo\|agent]` | Follow a log in real time. `--all` shows the log from the beginning. `--viewpoint cryo` (default) follows the structured event log; `--viewpoint agent` follows raw agent output (`cryo-agent.log`). |
| `cryo log` | Print the full session log. |
| `cryo send "<message>" [--from <name>] [--subject <text>] [--wake]` | Send a message to the agent's inbox. `--from` sets the sender (default `human`), `--subject` sets the subject (default: derived from the body), `--wake` wakes the agent immediately after sending. |
| `cryo receive` | Read messages the agent sent to the outbox. |
| `cryo wake ["message"]` | Wake the daemon immediately, optionally with a message. |
| `cryo clean [--force]` | Remove runtime files such as logs, state, and messages. |
| `cryo ps [--kill-all]` | List, or kill, every running cryo daemon on this machine. Run from anywhere. |

## Hub (`cryohub`)

| Command | What it does |
|---------|--------------|
| `cryohub start [--host <ip>] [--port <n>]` | Install a service that survives reboot. `--host` and `--port` also update the saved hub config. |
| `cryohub start --foreground` | Run the hub in the current terminal instead of installing a service. |
| `cryohub stop` | Uninstall the global hub service. |
| `cryohub restart` | Restart the installed global hub service without reinstalling it. |
| `cryohub status` | Show the global hub URL, chamber root, config path, log path, and service status. Also lists legacy cwd-scoped hub services from older versions. |

## Agent IPC (`cryo-agent`)

These commands are used by the spawned AI agent to communicate with the daemon over a Unix socket. They are not the operator interface.

| Command | What it does |
|---------|--------------|
| `cryo-agent hibernate --summary "..."` | End the session; more work remains. |
| `cryo-agent hibernate --complete` | End the session; the plan is done. |
| `cryo-agent hibernate --exit 1` | Report a failed session. The daemon marks consumed TODOs done and adds a fresh numbered retry TODO. |
| `cryo-agent todo add "text" --at <TIME>` | Schedule the next wake via a TODO. `--at` accepts a relative offset (`+30 minutes`), an ISO 8601 timestamp (`2026-04-25T10:00`; seconds and a space separator are tolerated), or a date only (`2026-04-25`, meaning midnight). |
| `cryo-agent todo list` | List all TODO items. |
| `cryo-agent todo done <id>` | Mark a TODO item as done. |
| `cryo-agent todo remove <id>` | Remove a TODO item. |
| `cryo-agent send "message"` | Write a message to the outbox for the human. |
| `cryo-agent send --stdin` | Read the outbox message body from stdin exactly, including trailing newlines; use for multi-line or shell-sensitive text. |
| `cryo-agent send --question "msg"` | Mark the message as a question awaiting a human reply. |
| `cryo-agent receive` | Claim the current inbox batch from the human. |
| `cryo-agent dialog [--last N \| --all \| --since <iso>]` | Render the conversation transcript (default: last 20 messages). `--last N` shows the last N, `--all` shows every archived message, `--since <iso>` shows messages at or after an ISO 8601 time; the three are mutually exclusive. Also archives any pending inbox batch as a side effect, satisfying the same reply obligation as `receive`. |
| `cryo-agent time` | Print the current local time in ISO 8601 format. |
| `cryo-agent time "+30 minutes"` | Compute a relative offset. Units: `minutes`, `hours`, `days`, `weeks`. |
| `cryo-agent time "2026-04-25T10:00"` | Validate and normalize an ISO 8601 timestamp. |

## Zulip Sync (`cryo-zulip`)

| Command | What it does |
|---------|--------------|
| `cryo-zulip init --config <zuliprc> --stream <name> [--topic <topic>] [--history]` | Validate credentials, resolve the stream, and write `zulip-sync.json`. |
| `cryo-zulip sync [--interval N]` | Start the background sync daemon. Default interval comes from `cryo.toml` or falls back to 5 seconds. |
| `cryo-zulip unsync` | Stop the sync daemon. |
| `cryo-zulip pull` | One-shot pull. |
| `cryo-zulip push` | One-shot push. |
| `cryo-zulip status` | Show sync configuration. |
