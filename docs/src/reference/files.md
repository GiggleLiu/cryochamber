# Runtime files reference

Every file a running chamber creates, where it lives, and whether it is safe to commit or sync.

## Chamber-local files

| File | Created by | Persists across runs | Sync-safe? | Purpose |
|------|------------|----------------------|------------|---------|
| `cryo.toml` | `cryo init` | yes | yes | Project configuration. |
| `plan.md` | `cryo init` or you | yes | yes | The plan the agent reads each session. |
| `NOTES.md` | `cryo init` | yes | yes | Agent's persistent cross-session memory. |
| `timer.json` | `cryo start` | no | no | Runtime state: session number, PID lock, CLI overrides. |
| `todo.json` | first `cryo-agent todo add` | no | no | Per-project TODO list and scheduler source of truth. |
| `cryo.log` | daemon | no | no | Append-only structured event log. |
| `cryo-agent.log` | daemon | no | no | Agent stdout and stderr, including raw tool-call output. |
| `messages/inbox/` | local writers and sync daemons | no | no | Incoming messages waiting for the agent. |
| `messages/inbox/archive/` | daemon on receive | no | no | Processed inbox messages. |
| `messages/outbox/` | agent and daemon | no | no | Outgoing messages waiting for delivery. |
| `messages/outbox/archive/` | sync daemons | no | no | Outbox messages already delivered remotely. |
| `.cryo/cryo.sock` | daemon | no | no | Unix domain socket for agent-daemon IPC. |

## Sync-channel state files

| File | Created by | Persists across runs | Sync-safe? | Purpose |
|------|------------|----------------------|------------|---------|
| `gh-sync.json` | `cryo-gh init` | yes | yes | GitHub Discussion sync state: repo, Discussion ID, last-read marker. |
| `cryo-gh-sync.log` | `cryo-gh sync` | no | no | GitHub sync daemon log. |
| `zulip-sync.json` | `cryo-zulip init` | yes | yes | Zulip sync state: site, stream, stream ID, last-imported message. |
| `.cryo/zuliprc` | `cryo-zulip init` | yes | no - contains API key | Zulip credentials. Never commit, push, or sync this file. Already gitignored. |
| `cryo-zulip-sync.log` | `cryo-zulip sync` | no | no | Zulip sync daemon log. |

## OS service files

| File | Created by | Purpose |
|------|------------|---------|
| `~/Library/LaunchAgents/com.cryo.*.plist` | `cryo start` on macOS | launchd service definition. |
| `~/.config/systemd/user/com.cryo.*.service` | `cryo start` on Linux | systemd user service definition. |

Set `CRYO_NO_SERVICE=1` before `cryo start` to skip OS service install and run the daemon as a plain background process instead.
