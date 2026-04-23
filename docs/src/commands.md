# Commands

## Operator (`cryo`)

```bash
cryo init [--agent <cmd>]           # Initialize working directory (writes cryo.toml)
cryo start [--agent <cmd>]          # Start the daemon (reads cryo.toml for config)
cryo start --max-session-duration 3600  # Override session timeout from cryo.toml
cryo status                         # Show current state
cryo ps [--kill-all]                # List (or kill) all running daemons
cryo restart                        # Kill running daemon and restart
cryo cancel                         # Stop the daemon and remove state
cryo watch [--all]                  # Watch session log in real-time
cryo log                            # Print session log
cryo send "<message>"               # Send a message to the agent's inbox
cryo receive                        # Read messages from the agent's outbox
cryo wake ["message"]               # Send a wake message to the daemon's inbox
cryo clean [--force]                # Remove runtime files (logs, state, messages)
```

## Hub (`cryohub`)

Browser dashboard for managing chambers. Always operates on the current directory — `cd` into a parent of chamber subdirectories first. See [Hub](./hub.md).

```bash
cryohub start [--host <ip>] [--port <n>]   # install service (survives reboot)
cryohub start --foreground                  # run in the current process instead
cryohub stop                                # uninstall the service for this dir
cryohub status                              # show this dir's service + any others
```

## Agent IPC (`cryo-agent`)

These commands are used by the AI agent to communicate with the daemon. They send JSON messages over a Unix domain socket.
Human-visible communication should go through `cryo-agent send` / `cryo-agent reply`; stdout/stderr are only written to `cryo-agent.log`.
If a session ends without an agent outbox message, the daemon writes a stand-in status message so the run is still visible.

```bash
cryo-agent hibernate --summary "..."   # End session (more work to do)
cryo-agent hibernate --complete        # End session (plan done)
cryo-agent hibernate --exit 1          # Report a failed session (daemon marks the wake crashed; the consumed TODO is re-injected with an attempt bump)
cryo-agent todo add "text" --at <TIME> # Schedule next wake via TODO
cryo-agent send "message"             # Send message to human (writes to outbox)
cryo-agent receive                     # Read and archive inbox messages from human
cryo-agent time                        # Current time (ISO8601 local)
cryo-agent time "+30 minutes"          # Relative offset (minutes|hours|days|weeks)
cryo-agent time "2026-04-25T10:00"     # ISO8601 pass-through (validates + normalizes)
```

Agents keep free-form cross-session memory in `NOTES.md` in the chamber root. Read and append that file directly instead of using an IPC command.

## GitHub Sync (`cryo-gh`)

Sync messages with a GitHub Discussion board for remote monitoring and two-way messaging. See the [GitHub Sync](./github-sync.md) page for commands, setup, and workflow.
