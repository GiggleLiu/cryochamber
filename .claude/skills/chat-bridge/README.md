# chat-bridge

Skill for bridging a cryochamber to a chat platform — **Zulip** or
**Feishu/Lark** — with one unified backbone and platform adapters.

- **Install**: `claude skill install --path .claude/skills/chat-bridge`
  (or copy the directory into `~/.agents/skills/` for pi)
- **Invoke**: `/chat-bridge`

## Architecture

```
scripts/chat-bridge (CLI: init / pull / push / run / status / unsync)
        │
        └── chat_bridge/
            ├── backbone.py   shared engine (platform-agnostic)
            │                 trigger gate · anti-echo · dedupe · whitelist
            │                 mailbox IO · reply routing · systemd · stats
            ├── channel.py    Channel protocol (the abstraction)
            ├── zulip.py      Zulip adapter (REST + realtime events queue)
            └── lark.py       Lark adapter (lark-cli event stream)
```

The `Channel` protocol is the same abstraction for both platforms:
`profile() / fetch_new(cursor) / send(target, content) / upload() / download()`.
All policy (mention-gate, quiet mode, reply routing, dedupe, services) lives
in `backbone.py` and is platform-identical.

## Features (A + full B set)

- A — mention/trigger-word/thread-follow-up gate, quiet by default,
  in-thread replies, anti-echo, atomic state + dedupe + stats,
  outbox push, systemd service + linger.
- B1 — sender whitelist (`allowed_senders`).
- B2 — attachments: Zulip `/user_uploads` both ways; Lark
  `+messages-resources-download` (in) and `--image/--file` send (out).
- B3 — multiple channels per chamber (`[[bridge.channels]]`).
- B4 — Feishu group `@bot` mode (content-based mention gate, same as Zulip).
- B5 — Zulip history import (`--history`).
- B6 — unified mailbox frontmatter / formatting across platforms.
- B7 — Zulip real-time events queue (bounded long-poll; instant pickup).

## Quick start

```bash
# From this skill directory, keep the launcher beside its Python package.
mkdir -p ~/.local/bin
ln -sf "$PWD/scripts/chat-bridge" ~/.local/bin/chat-bridge

# Zulip
chat-bridge init --chamber <chamber> --platform zulip \
    --stream "QEC-automated search" --config path/to/zuliprc [--topic T]
# Lark (install with: npx @larksuite/cli@latest install)
chat-bridge init --chamber <chamber> --platform lark [--chat-id oc_xxx]

chat-bridge run --chamber <chamber>   # installs the systemd user service
chat-bridge pull / status / unsync --chamber <chamber>
```

Per-chamber config: `bridge.toml` (`trigger_words`, `allowed_senders`,
`require_mention`, `reply_in_thread`, `transport`, `[[bridge.channels]]`).
State: `chat-bridge.json`.

## Tests

```bash
cd .claude/skills/chat-bridge/scripts
python3 -m unittest discover -s tests -v    # mock channel, no network
ruff check chat_bridge tests
```

## Notes

- `loginctl enable-linger <user>` on headless hosts — otherwise services die
  when no SSH session is open.
- The bot must be subscribed to the Zulip stream before `init`.
- Lark events carry no mentions array — group `@bot` detection is
  content-based (mention text / trigger word), the same gate as Zulip.
- `chat-bridge run` installs a systemd user service. On macOS, use
  `chat-bridge run --no-service` under a supervisor such as launchd.
