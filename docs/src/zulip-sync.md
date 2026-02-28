# Zulip Sync

`cryo-zulip` bridges a cryochamber project with a Zulip stream, enabling remote monitoring and two-way messaging. Stream messages become inbox messages for the agent; outbox messages from the agent are posted back to the stream.

## Prerequisites

- A Zulip server with a bot account
- A `zuliprc` file with bot credentials (standard Zulip INI format with `[api]` section containing `email`, `key`, `site`)
- A Zulip stream accessible by the bot
- An initialized cryochamber project (`cryo init`)

## Commands

```bash
cryo-zulip init --config ~/.zuliprc --stream my-stream  # Validate credentials, resolve stream, write zulip-sync.json
cryo-zulip init --config ~/.zuliprc --stream my-stream --topic mychannel  # Custom topic (default: "cryochamber")
cryo-zulip sync [--interval 30]                         # Start background sync daemon
cryo-zulip unsync                                       # Stop the sync daemon
cryo-zulip pull                                         # One-shot: pull new messages → inbox
cryo-zulip push                                         # One-shot: push latest session log → stream
cryo-zulip status                                       # Show sync configuration
```

## How Sync Works

`cryo-zulip sync` spawns a background daemon (just like `cryo start` does). It does two things in a loop:

**Stream → Inbox** (pull direction): Polls the Zulip stream for new messages every `--interval` seconds (default: 30). New messages are written to `messages/inbox/` where the cryo daemon picks them up on the next session. The bot's own messages are filtered out to prevent echo loops.

**Outbox → Stream** (push direction): Watches `messages/outbox/` for new files. When the agent sends a message (via `cryo-agent send`), the sync daemon posts it to the Zulip stream and archives the file to `messages/outbox/archive/`.

```text
Zulip Stream                      Local filesystem
────────────                      ─────────────────
New message        ──(pull)──→    messages/inbox/       → agent reads on wake
                   ←─(push)──    messages/outbox/      ← agent writes via cryo-agent send
```

The sync is managed as a system service (launchd on macOS, systemd on Linux) that **survives reboots**. Logs go to `cryo-zulip-sync.log`.

## Recommended Workflow

### 1. Initialize the project

```bash
cryo init --agent claude
# edit plan.md with your task
```

### 2. Link to Zulip

```bash
cryo-zulip init --config ~/.zuliprc --stream my-stream
```

This validates the bot credentials, resolves the stream ID, and writes `zulip-sync.json`. The zuliprc is copied to `.cryo/zuliprc` for use by the sync daemon.

### 3. Start the daemon and sync

```bash
cryo start
cryo-zulip sync
```

Both run as background daemons. Monitor with `cryo watch`.

### 4. Send messages from Zulip

Post a message in the Zulip stream from the web UI or mobile app. The sync daemon picks it up within 30 seconds and writes it to `messages/inbox/`. The cryo daemon wakes the agent on the next session (or immediately if `watch_inbox = true`).

### 5. Read agent replies on Zulip

When the agent calls `cryo-agent send "message"`, the outbox file is detected immediately by the sync watcher and posted to the Zulip stream.

### 6. Stop

```bash
cryo-zulip unsync   # stop sync daemon
cryo cancel         # stop cryo daemon
```

## One-Shot Usage

For manual or scripted use without the sync daemon:

```bash
cryo-zulip pull    # fetch new stream messages into inbox
cryo-zulip push    # post the latest session log to the stream
```

## Example: Chess by Mail over Zulip

Play correspondence chess against an AI agent, sending moves from the Zulip web UI:

```bash
cd examples/chess-by-mail
cryo-zulip init --config ~/.zuliprc --stream chess-game
cryo init && cryo start
cryo-zulip sync --interval 30
# Send your moves as messages in the Zulip stream!
```

See [Chess by Mail](./examples/chess-by-mail.md) for the full example.

## Files

| File | Purpose |
|------|---------|
| `zulip-sync.json` | Sync state: site, stream, stream ID, bot email, cursor |
| `.cryo/zuliprc` | Bot credentials (copied from user's zuliprc on init) |
| `cryo-zulip-sync.log` | Sync daemon log output |
| `messages/inbox/` | Incoming messages (from Zulip stream) |
| `messages/outbox/` | Outgoing messages (posted to Zulip stream) |
| `messages/outbox/archive/` | Posted outbox messages (archived after sync) |
