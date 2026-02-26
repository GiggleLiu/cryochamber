# Commands

## Operator (`cryo`)

```bash
cryo init [--agent <cmd>]           # Initialize working directory (writes cryo.toml)
cryo start [--agent <cmd>]          # Start the daemon (reads cryo.toml for config)
cryo start --max-retries 3          # Override max retries from cryo.toml
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
cryo web [--host <ip>] [--port <n>] # Open browser chat UI
cryo clean [--force]                # Remove runtime files (logs, state, messages)
```

## Agent IPC (`cryo-agent`)

These commands are used by the AI agent to communicate with the daemon. They send JSON messages over a Unix domain socket.

```bash
cryo-agent hibernate --wake <ISO8601>  # Schedule next wake
cryo-agent hibernate --complete        # Mark plan as complete
cryo-agent note "text"                 # Leave a note for next session
cryo-agent send "message"             # Send message to human (writes to outbox)
cryo-agent receive                     # Read inbox messages from human
cryo-agent time "+30 minutes"          # Compute a future timestamp
cryo-agent alert <action> <target> "msg"  # Set dead-man switch
```

## GitHub Sync (`cryo-gh`)

Sync messages with a GitHub Discussion board for remote monitoring and two-way messaging. See the [GitHub Sync](./github-sync.md) page for commands, setup, and workflow.
