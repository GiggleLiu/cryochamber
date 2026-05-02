# Command reference

Cryochamber ships five binaries. This page lists the commands grouped by binary.

- [`cryo`](#operator-cli-cryo) — operator CLI for daily use.
- [`cryohub`](#hub-cryohub) — multi-chamber web dashboard.
- [`cryo-agent`](#agent-ipc-cryo-agent) — agent-to-daemon IPC. Used by the spawned agent, not by you.
- [`cryo-gh`](#github-sync-cryo-gh) — GitHub Discussion sync.
- [`cryo-zulip`](#zulip-sync-cryo-zulip) — Zulip stream sync.

## Operator CLI (`cryo`)

Run these from inside a chamber directory unless noted otherwise.

| Command                                  | What it does                                                       |
|------------------------------------------|--------------------------------------------------------------------|
| `cryo init [--agent <cmd>]`              | Initialize the directory: write `cryo.toml`, `plan.md`, `NOTES.md`, and `README.md`. Existing files are kept. |
| `cryo start [--agent <cmd>]`             | Start the daemon. Reads `cryo.toml` and writes overrides to `timer.json`. |
| `cryo start --max-session-duration 3600` | Override the session timeout for this run.                         |
| `cryo status`                            | Show whether the daemon is running, the current session number, and the next wake time. |
| `cryo restart`                           | Stop the running daemon and start a fresh one.                     |
| `cryo cancel`                            | Stop the daemon and remove the runtime state.                      |
| `cryo watch [--all]`                     | Follow the session log in real time.                               |
| `cryo log`                               | Print the full session log.                                        |
| `cryo send "<message>"`                  | Send a message to the agent's inbox.                               |
| `cryo receive`                           | Read messages the agent sent to the outbox.                        |
| `cryo wake ["message"]`                  | Wake the daemon immediately (optionally with a message).           |
| `cryo clean [--force]`                   | Remove runtime files (logs, state, messages).                      |
| `cryo ps [--kill-all]`                   | List (or kill) every running cryo daemon on this machine. Run from anywhere. |

## Hub (`cryohub`)

Cryohub is a browser dashboard for managing chambers. It always operates on the current directory and refuses to start in a directory that itself contains a `cryo.toml`. See [Hub](./hub.md) for the full guide.

| Command                                    | What it does                                              |
|--------------------------------------------|-----------------------------------------------------------|
| `cryohub start [--host <ip>] [--port <n>]` | Install a service that survives reboot.                   |
| `cryohub start --foreground`               | Run the hub in the current terminal instead.              |
| `cryohub stop`                             | Uninstall the service for this directory.                 |
| `cryohub status`                           | Show this directory's service plus any others on the host. |

## Agent IPC (`cryo-agent`)

These commands are used by the spawned AI agent to communicate with the daemon over a Unix socket. They are **not** the operator interface — you generally don't run them by hand.

> **Note**: Human-visible communication should go through `cryo-agent send`. Stdout and stderr are only written to `cryo-agent.log`. If a session ends without an agent-authored outbox message, the daemon writes a stand-in `from: cryochamber` message so the run is still visible.

| Command                                | What it does                                                                                            |
|----------------------------------------|---------------------------------------------------------------------------------------------------------|
| `cryo-agent hibernate --summary "..."` | End the session; more work to do.                                                                       |
| `cryo-agent hibernate --complete`      | End the session; plan is done.                                                                          |
| `cryo-agent hibernate --exit 1`        | Report a failed session. The daemon re-injects the consumed TODO with an `(attempt k)` suffix.          |
| `cryo-agent todo add "text" --at <TIME>` | Schedule the next wake via a TODO.                                                                    |
| `cryo-agent send "message"`            | Write a message to the outbox (visible to the human).                                                   |
| `cryo-agent receive`                   | Claim the current inbox batch from the human.                                                           |
| `cryo-agent time`                      | Print current local time (ISO 8601).                                                                    |
| `cryo-agent time "+30 minutes"`        | Relative offset. Units: `minutes`, `hours`, `days`, `weeks`.                                            |
| `cryo-agent time "2026-04-25T10:00"`   | ISO 8601 pass-through (validates and normalizes).                                                       |

> **Note**: Agents keep cross-session memory in `NOTES.md` in the chamber root. Read and append that file directly — there is no IPC command for it.

## GitHub Sync (`cryo-gh`)

See [GitHub Sync](./github-sync.md) for setup, workflow, and the full command list.

## Zulip Sync (`cryo-zulip`)

See [Zulip Sync](./zulip-sync.md) for setup, workflow, and the full command list.
