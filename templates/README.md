# {{project_name}}

A [cryochamber](https://github.com/GiggleLiu/cryochamber) application.

## Start the Service

```bash
cryo start                                                    # start the daemon
```

Depending on the way you interact with your agent, start the corresponding service wtih:
```bash
cryo-zulip init --config ./zuliprc --stream "my-stream"       # if using Zulip
cryo-zulip sync
cryo-gh init --repo owner/repo                                # if using GitHub Discussions
cryo-gh sync
cryo web                                                      # if using the web UI
```

## Manage the running service

Go to the project folder and type:
```bash
cryo status          # check if the daemon is running
cryo watch           # follow the live log
cryo send "message"  # send a message to the agent
cryo cancel          # stop the daemon
```

**Control the daemon:**

```bash
cryo wake         # force the agent to wake up now (don't wait for schedule)
cryo restart      # stop and restart the daemon
cryo cancel       # stop the daemon and clean up state
cryo ps           # list all running cryochamber daemons on this machine
```

## Messaging Channels

Cryochamber supports external messaging channels that sync between a remote service and the local inbox/outbox directories. The cryo daemon and agent remain unaware of the channel — all sync is handled by a dedicated binary. These are configured automatically when using `/make-plan`.

| Channel | Binary | Backend | Docs |
|---------|--------|---------|------|
| Web UI | `cryo web` | Built-in HTTP server | [Web UI](https://giggleliu.github.io/cryochamber/web-ui.html) |
| GitHub Discussions | `cryo-gh` | GitHub GraphQL API | [GitHub Sync](https://giggleliu.github.io/cryochamber/github-sync.html) |
| Zulip | `cryo-zulip` | Zulip REST API | [Zulip Sync](https://giggleliu.github.io/cryochamber/zulip-sync.html) |

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
