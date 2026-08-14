---
name: chat-bridge
description: Use when the user wants to bridge a cryochamber to a chat platform — Zulip or Feishu (Lark) — so the chamber agent can be driven from chat and reply into it. One unified backbone with per-platform adapters: trigger models (mention-gated vs pull-all), credentials, services, and end-to-end validation.
---

# Chat Bridge (Zulip / Feishu — unified)

Bridge a cryochamber to a chat platform so the chamber agent can be
remote-controlled from chat and post replies back. One backbone handles both
platforms with the same abstraction: `scripts/chat-bridge` (CLI) +
`chat_bridge/` package (`backbone.py` + `zulip.py` + `lark.py`).

## How it works (the file contract)

```
chat platform  ⇄  bridge (backbone + channel adapter)  ⇄  chamber mailbox
   (bot API)       fetch_new / send                        messages/inbox  → wake agent
                                                           messages/outbox ← agent replies
```

- A new inbox file wakes the agent (cryochamber `watch_dirs`).
- The agent replies with `cryo-agent send`, which writes to `messages/outbox/`.
- The bridge posts outbox messages back to chat (same thread/topic), then archives.
- The bridge never talks to the agent and the agent never talks to chat — the
  mailbox is the entire interface. Swapping the agent is a one-line change in
  `cryo.toml`.

The `Channel` protocol is the same abstraction for both platforms:
`profile()`, `fetch_new(cursor)`, `send(target, content)`, `upload()`,
`download()`. All policy — trigger gate, quiet mode, anti-echo, dedupe,
reply routing, whitelist, stats, systemd services — lives in `backbone.py`
and is platform-identical.

## Step 0 — Decide platform, transport, and trigger model

Ask the user **one question at a time**. Start with the platform:

- **Zulip** — REST API (`transport = events` realtime queue, or `poll`).
  Trigger models:
  - **Mention-gated** (default, recommended for shared/busy streams):
    only messages directed at the bot reach the agent — `@**bot**` mentions
    or trigger words (`flash, ...`). (Zulip has no message-level reply ids;
    reply-to-bot follow-ups are a Lark-only trigger, see below.)
    Everything else stays out of the chamber inbox. Recent same-thread chatter
    is buffered as reference context and included only when a directed message
    arrives, so unrelated topics/chats never leak into the prompt.
  - **Pull-all** (`require_mention = false`): every message in the stream
    wakes the agent. Simple; fine for low-traffic channels.
- **Feishu / Lark** — `lark-cli` event stream (WebSocket long connection, no
  ports opened). `require_mention` gates both p2p and group `@bot` messages;
  mention detection is content-based (lark events carry no mentions array).
  Lark messages carry reply ids, so with `reply_in_thread` a reply to one of
  the bot's own messages also counts as directed and is answered in-thread.

If unsure, recommend **Zulip + mention-gated** — quietest, matches
group-chat etiquette (one consolidated reply per batch, never reply unless
directed).

## Prerequisites

1. `cryo` installed and a chamber exists (`cryo init` or the `make-plan` skill).
2. Credentials:
   - Zulip: a `zuliprc` file; the bot must be **subscribed** to the stream.
   - Lark: install the tested CLI with `npx @larksuite/cli@1.0.53 install`, then complete
     `lark-cli config init` and `lark-cli auth login --recommend`; app scopes
     `im:message.p2p_msg:readonly` (+ send) and `im.message.receive_v1`
     event subscription enabled.
3. The agent command (`pi`, `opencode`, ...) on PATH with credentials.

## Setup

```bash
cd <chamber>
# Zulip
chat-bridge init --platform zulip --stream "STREAM" --topic T \
    --config path/to/zuliprc [--history] [--trigger flash] [--allow-sender id...]
# Lark
chat-bridge init --platform lark --chat-id oc_xxx [--chat-type p2p|group]
# multi-channel: repeat init with --name <other>
chat-bridge run            # installs the systemd user service (or --no-service)
```

`init` requires one concrete reply route per channel: a Zulip topic or a Lark
chat id. For Zulip, use `--topic T` (whole-stream routing is rejected). It
validates credentials before replacing `.cryo/zuliprc` (0600), adds runtime
credentials/state to the chamber's `.gitignore`, resolves the channel, and
anchors the cursor at the newest message. Zulip's
`--history` option imports the past instead; Lark's event stream does not offer
history import. With no Lark `--chat-type`, the bridge defaults to `p2p`.
Per-chamber config: `bridge.toml`. State: `chat-bridge.json`; logs:
`chat-bridge.log` in the chamber. Multiple configured channels are serialized:
the bridge does not pull another route until the chamber's reply to the active
route has been posted.

Common flags in `bridge.toml`:

| Key | Default | Meaning |
|---|---|---|
| `poll_interval` | 15 | seconds between sync cycles |
| `require_mention` | true | only directed messages wake the agent |
| `trigger_words` | `["flash", "flash-bot"]` | trigger words (start + punctuation) |
| `allowed_senders` | `[]` | whitelist of platform ids; empty = anyone |
| `reply_in_thread` | true | treat replies to the bot's messages as directed and answer in-thread (message-level replies are Lark-only; Zulip routes by topic) |
| `transport` | auto | shared by all channels; zulip: `auto`/`poll`/`events` (other values behave as `poll`); lark always uses its event stream |

## Step 3 — Survive reboots and logout

```bash
sudo loginctl enable-linger <user>        # critical on headless hosts
systemctl --user list-units "com.cryo.*" --no-legend
# expect com.cryo.daemon.* and com.cryo.chat-bridge.* running
```

Without linger the user systemd manager (and every bridge service) shuts down
when the last SSH session closes — the chamber silently stops reacting.

## Step 4 — End-to-end validation

1. **Init sanity**: `chat-bridge init` logs the bot identity and anchor cursor.
2. **Quiet test**: post a non-mention message; confirm `chat-bridge pull`
   logs `buffered as context` and the inbox stays empty.
3. **Trigger test**: post `@**bot** status` **from a real user account, not
   the bot** — anti-echo skips the bot's own messages.
4. **Reply round-trip**: wait for the agent session (`cryo.log`), then
   confirm the reply appears in the same topic/thread.
5. **Idle persistence**: close all SSH sessions, wait ~1 min, reconnect,
   confirm both services still active.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| init fails resolving stream | Bot not subscribed to the stream | Add the bot in Zulip stream settings |
| Mention posted but nothing happens | Different stream, or posted **as the bot** | Check the actual stream name; test from a real user |
| HTTP 400 on `/register` narrow | Numeric stream id in the events-queue narrow | `/register` wants `[["stream", "NAME"]]` (name, not id); `/messages` wants dict narrows |
| Events long-poll never returns | Server ignores `timeout` param | Client bounds the poll (10 s) and recycles the queue — by design |
| Services die when no SSH session | `loginctl linger` disabled | `sudo loginctl enable-linger <user>` |
| Agent wakes for every message | `require_mention = false` on a busy stream | Set `require_mention = true` |
| Reply posted to the wrong topic | Outbox files carry no platform metadata | Routing uses `last_active` channel + `last_thread` from state |

## Common Mistakes

| Mistake | Fix |
|---|---|
| Leaving `.cryo/zuliprc` world-readable or in git | `chmod 600`, `.gitignore` covers `.cryo/` |
| Testing the mention path as the bot | Anti-echo hides it — always test from a real user |
| Skipping `loginctl enable-linger` on a server | Services die on logout; chamber goes silent |
| Forgetting bot subscription before init | Init fails or misses messages |
| Running `chat-bridge run` on macOS | Service installation is systemd-only; use `run --no-service` under your own supervisor |
