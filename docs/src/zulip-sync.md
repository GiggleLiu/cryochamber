# Zulip sync

`cryo-zulip` bridges a chamber with a Zulip stream, giving you remote monitoring and two-way messaging from the Zulip web or mobile app. Stream messages become inbox messages for the agent, and outbox messages from the agent are posted back to the stream.

## Prerequisites

Before you begin, make sure you have:

- A Zulip server with a bot account.
- A `zuliprc` file with bot credentials. This is a standard Zulip INI file with an `[api]` section containing `email`, `key`, and `site`.
- A Zulip stream the bot has permission to read and post to.
- An initialized cryochamber project. See [Getting Started](./getting-started.md) if you don't have one yet.

## Set up sync

1. Link the chamber to your Zulip stream:

   ```bash
   cryo-zulip init --config ~/.zuliprc --stream my-stream
   ```

   This validates the bot credentials, resolves the stream ID, and writes `zulip-sync.json`. The `zuliprc` is copied into `.cryo/zuliprc` for the sync daemon to use.

   Optional flags:

   - `--topic mychannel` — use a custom topic. Defaults to `cryochamber`.
   - `--history` — import existing messages from the stream/topic on the first pull. Without this flag, only messages posted after setup are imported.

2. Start the cryo daemon:

   ```bash
   cryo start
   ```

3. Start the sync daemon:

   ```bash
   cryo-zulip sync
   ```

   By default this polls Zulip every 5 seconds. Override the interval with `--interval 30`.

4. Verify both daemons are running:

   ```bash
   cryo status
   cryo-zulip status
   ```

Both daemons run as system services (launchd on macOS, systemd on Linux) and survive reboots. Sync logs go to `cryo-zulip-sync.log`.

> **Warning**: Don't commit, push, or sync `.cryo/zuliprc` — it holds your bot's API key. The file is gitignored by default; never include it in messages or sync payloads.

## Send a message from Zulip

1. Open the Zulip stream in the web or mobile app.
2. Post a message in the configured topic.
3. Within the poll interval (default 5s), the sync daemon writes the message to `messages/inbox/`.
4. The cryo daemon wakes the agent on the next session, or immediately if `watch_inbox = true` in `cryo.toml`.

The bot's own messages are filtered out so you won't get an echo loop.

## Read agent replies on Zulip

When the agent runs `cryo-agent send "message"`, the sync daemon detects the new outbox file and posts it to the Zulip stream within seconds.

## Stop sync

1. Stop the sync daemon:

   ```bash
   cryo-zulip unsync
   ```

2. (Optional) Stop the cryo daemon as well:

   ```bash
   cryo cancel
   ```

## Example: Chess by mail over Zulip

Play correspondence chess against an AI agent by sending moves from Zulip:

1. Change into the example chamber:

   ```bash
   cd examples/chambers/chess-by-mail
   ```

2. Link it to a Zulip stream:

   ```bash
   cryo-zulip init --config ~/.zuliprc --stream chess-game
   ```

3. Start both daemons:

   ```bash
   cryo start
   cryo-zulip sync --interval 30
   ```

4. Send your moves as messages in the Zulip stream.

See [Chess by Mail](./examples/chess-by-mail.md) for the full walkthrough.

## How sync works

`cryo-zulip sync` runs a background loop that does two things:

| Direction | What happens |
|-----------|--------------|
| **Stream → inbox** (pull) | Polls the Zulip stream for new messages every `--interval` seconds. New messages are written to `messages/inbox/`. The bot's own messages are filtered out. |
| **Outbox → stream** (push) | Watches `messages/outbox/` for new files. New files are posted to the stream and archived to `messages/outbox/archive/`. |

```text
Zulip Stream                      Local filesystem
────────────                      ─────────────────
New message        ──(pull)──→    messages/inbox/       → agent reads on wake
                   ←─(push)──     messages/outbox/      ← agent writes via cryo-agent send
```

## One-shot pull and push

For manual or scripted use without the sync daemon:

```bash
cryo-zulip pull    # fetch new stream messages into messages/inbox/
cryo-zulip push    # post the latest session log to the stream
```

## Command reference

| Command | What it does |
|---------|--------------|
| `cryo-zulip init --config <zuliprc> --stream <name> [--topic <topic>] [--history]` | Validate credentials, resolve the stream, write `zulip-sync.json`. |
| `cryo-zulip sync [--interval N]` | Start the background sync daemon. Default interval comes from `cryo.toml` or falls back to 5 seconds. |
| `cryo-zulip unsync` | Stop the sync daemon. |
| `cryo-zulip pull` | One-shot pull. |
| `cryo-zulip push` | One-shot push. |
| `cryo-zulip status` | Show sync configuration. |

## Files

| File | Purpose |
|------|---------|
| `zulip-sync.json` | Sync state: site, stream, stream ID, bot email, last imported message. |
| `.cryo/zuliprc` | Bot credentials, copied from your `zuliprc` on init. **Never commit or sync.** |
| `cryo-zulip-sync.log` | Sync daemon log output. |
| `messages/inbox/` | Incoming messages (from the Zulip stream). |
| `messages/outbox/` | Outgoing messages (posted to the Zulip stream). |
| `messages/outbox/archive/` | Archived outbox messages after they are posted. |
