# CLI reference

All cryochamber binaries and their commands. For `cryo.toml` and `cryohub.toml` fields, see [Configuration](./configuration.md).

Every binary accepts `--version` (print the version and exit) and `--help`.

## Operator CLI (`cryo`)

Run these from inside a chamber directory unless noted otherwise.

<table>
<thead>
<tr><th>Category</th><th>Command</th><th>What it does</th></tr>
</thead>
<tbody>
<tr class="group"><td rowspan="6">Lifecycle</td><td><code>cryo init [--agent &lt;cmd&gt;]</code></td><td>Initialize the directory: write <code>cryo.toml</code>, <code>plan.md</code>, <code>NOTES.md</code>, and <code>README.md</code>. Existing files are kept.</td></tr>
<tr><td><code>cryo start [--agent &lt;cmd&gt;]</code></td><td>Start the daemon. Reads <code>cryo.toml</code> and writes overrides to <code>timer.json</code>.</td></tr>
<tr><td><code>cryo start --max-session-duration 3600</code></td><td>Override the session timeout for this run.</td></tr>
<tr><td><code>cryo status</code></td><td>Show whether the daemon is running, the current session number, and the next wake time.</td></tr>
<tr><td><code>cryo restart</code></td><td>Restart the running daemon. When it is installed as an OS service, restart the existing service without rewriting or removing it.</td></tr>
<tr><td><code>cryo cancel</code></td><td>Stop the daemon and remove the runtime state.</td></tr>
<tr class="group"><td rowspan="2">Logs</td><td><code>cryo watch [--all] [--viewpoint cryo|agent]</code></td><td>Follow a log in real time. <code>--all</code> shows the log from the beginning. <code>--viewpoint cryo</code> (default) follows the structured event log; <code>--viewpoint agent</code> follows raw agent output (<code>cryo-agent.log</code>).</td></tr>
<tr><td><code>cryo log</code></td><td>Print the full session log.</td></tr>
<tr class="group"><td rowspan="2">Messaging</td><td><code>cryo send "&lt;message&gt;" [--from &lt;name&gt;] [--subject &lt;text&gt;]</code></td><td>Send a message to the agent's inbox; the daemon's inbox watcher wakes the agent. <code>--from</code> sets the sender (default <code>human</code>), <code>--subject</code> sets the subject (default: derived from the body).</td></tr>
<tr><td><code>cryo receive</code></td><td>Read messages the agent sent to the outbox.</td></tr>
<tr class="group"><td rowspan="2">Housekeeping</td><td><code>cryo clean [--force]</code></td><td>Remove runtime files such as logs, state, and messages.</td></tr>
<tr><td><code>cryo ps [--kill-all]</code></td><td>List, or kill, every running cryo daemon on this machine. Run from anywhere.</td></tr>
</tbody>
</table>

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

<table>
<thead>
<tr><th>Category</th><th>Command</th><th>What it does</th></tr>
</thead>
<tbody>
<tr class="group"><td rowspan="4">Hibernating</td><td><code>cryo-agent hibernate --summary "..."</code></td><td>End the session; more work remains. Refused (non-zero exit) while unread inbox mail exists — the agent must <code>receive</code>, reply, and retry, so a session never ends with mail waiting for it. Also refused while no pending TODO declares the next wake. A successful call may block up to the reply window the agent requested with <code>--linger &lt;seconds&gt;</code> (omitted = 300, capped at 86400; <code>0</code> sleeps immediately).</td></tr>
<tr><td><code>cryo-agent hibernate --complete</code></td><td>End the session; the plan is done. Additionally refused while a TODO is due. Never held open by the reply window.</td></tr>
<tr><td><code>cryo-agent hibernate --exit 1</code></td><td>Report a failed session. The daemon marks consumed TODOs done and adds a fresh numbered retry TODO. Failure reports are never refused and never held open.</td></tr>
<tr class="group"><td rowspan="4">TODOs</td><td><code>cryo-agent todo add "text" --at &lt;TIME&gt;</code></td><td>Schedule the next wake via a TODO. <code>--at</code> accepts a relative offset (<code>+30 minutes</code>), an ISO 8601 timestamp (<code>2026-04-25T10:00</code>; seconds and a space separator are tolerated), or a date only (<code>2026-04-25</code>, meaning midnight).</td></tr>
<tr><td><code>cryo-agent todo list</code></td><td>List all TODO items.</td></tr>
<tr><td><code>cryo-agent todo done &lt;id&gt;</code></td><td>Mark a TODO item as done.</td></tr>
<tr><td><code>cryo-agent todo remove &lt;id&gt;</code></td><td>Remove a TODO item.</td></tr>
<tr class="group"><td rowspan="5">Messaging</td><td><code>cryo-agent send "message"</code></td><td>Write a message to the outbox for the human.</td></tr>
<tr><td><code>cryo-agent send --stdin</code></td><td>Read the outbox message body from stdin exactly, including trailing newlines; use for multi-line or shell-sensitive text.</td></tr>
<tr><td><code>cryo-agent send --question "msg"</code></td><td>Mark the message as a question awaiting a human reply.</td></tr>
<tr><td><code>cryo-agent receive</code></td><td>Claim the current inbox batch from the human.</td></tr>
<tr><td><code>cryo-agent dialog [--last N | --all | --since &lt;iso&gt;]</code></td><td>Render the conversation transcript (default: last 20 messages). <code>--last N</code> shows the last N, <code>--all</code> shows every archived message, <code>--since &lt;iso&gt;</code> shows messages at or after an ISO 8601 time; the three are mutually exclusive. Also archives any pending inbox batch as a side effect, satisfying the same reply obligation as <code>receive</code>.</td></tr>
<tr class="group"><td rowspan="3">Time</td><td><code>cryo-agent time</code></td><td>Print the current local time in ISO 8601 format.</td></tr>
<tr><td><code>cryo-agent time "+30 minutes"</code></td><td>Compute a relative offset. Units: <code>minutes</code>, <code>hours</code>, <code>days</code>, <code>weeks</code>.</td></tr>
<tr><td><code>cryo-agent time "2026-04-25T10:00"</code></td><td>Validate and normalize an ISO 8601 timestamp.</td></tr>
</tbody>
</table>

## Zulip Sync (`cryo-zulip`)

| Command | What it does |
|---------|--------------|
| `cryo-zulip init --config <zuliprc> --stream <name> [--topic <topic>] [--history]` | Validate credentials, resolve the stream, and write `zulip-sync.json`. |
| `cryo-zulip sync [--interval N]` | Start the background sync daemon. Default interval comes from `cryo.toml` or falls back to 5 seconds. |
| `cryo-zulip unsync` | Stop the sync daemon. |
| `cryo-zulip pull` | One-shot pull. |
| `cryo-zulip push` | One-shot push. |
| `cryo-zulip status` | Show sync configuration. |
