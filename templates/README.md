# {{project_name}}

A [cryochamber](https://github.com/GiggleLiu/cryochamber) application.

## Start the Service

```bash
cryo start
```

The daemon installs as an OS service (systemd on Linux, launchd on macOS) so it survives reboots. To run without a service, set `CRYO_NO_SERVICE=1`.

After starting, verify the first session completes successfully:

```bash
cryo status    # should show "Daemon: running" with session number and PID
cryo watch     # follow the live log — look for "agent hibernated" to confirm success
```

## Day-to-Day Usage

**Check on the agent:**

```bash
cryo status       # quick health check: is it running? what session?
cryo log          # read the full session history
cryo web          # open the web UI for a visual overview
```

**Communicate with the agent:**

```bash
cryo send "your message here"   # drop a message in the agent's inbox
cryo receive                    # read messages the agent sent you
```

**Control the daemon:**

```bash
cryo wake         # force the agent to wake up now (don't wait for schedule)
cryo restart      # stop and restart the daemon
cryo cancel       # stop the daemon and clean up state
cryo ps           # list all running cryochamber daemons on this machine
```

## Troubleshooting

If the agent crashes or doesn't hibernate, check the logs:

```bash
cryo log              # look for error messages or missing "agent hibernated"
cat cryo-agent.log    # raw agent output — useful for API errors or crashes
```

To verify the agent can respond at all, run a quick smoke test:

```bash
# For opencode:
echo "Reply OK" | opencode run

# For claude:
claude -p "Reply OK"
```

If this fails, check your API keys and agent installation.

## Files

| File | Purpose |
|------|---------|
| `plan.md` | Task plan — the agent reads this every session |
| `cryo.toml` | Project configuration (agent command, retries, inbox) |
| `cryo.log` | Session event log — append-only history of every session |
| `cryo-agent.log` | Raw agent stdout/stderr output |
| `messages/inbox/` | Incoming messages for the agent |
| `messages/outbox/` | Outgoing messages from the agent |
