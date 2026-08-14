"""backbone — platform-agnostic sync engine.

Owns everything the channel adapters do not:
  - config (bridge.toml) and state (chat-bridge.json)
  - the trigger gate (mention / trigger word / reply-to-bot)
  - anti-echo, dedupe, sender whitelist
  - mailbox IO (messages/inbox + messages/outbox)
  - the sync loop (pull → deliver → push) and systemd service management

A single backbone instance can run several channels (multi-platform,
multi-stream) against one chamber.
"""

from __future__ import annotations

import hashlib
import json
import os
import shlex
import tempfile
from contextlib import contextmanager
from dataclasses import dataclass, field, replace
from datetime import datetime, timezone
from pathlib import Path

from .channel import (
    BotIdentity,
    Channel,
    ChannelError,
    Message,
    ReplyTarget,
    parse_frontmatter,
    render_mailbox_message,
    slugify,
)

CONFIG_FILE = "bridge.toml"
STATE_FILE = "chat-bridge.json"
LOG_FILE = "chat-bridge.log"
DEFAULT_POLL_INTERVAL = 15
DEFAULT_TRIGGER_WORDS = ["flash", "flash-bot"]
SEEN_IDS_LIMIT = 500
_log_file: Path | None = None


# ------------------------------------------------------------------ logging

def configure_logging(chamber: Path) -> None:
    """Write bridge logs inside the chamber, independent of the caller's cwd."""
    global _log_file
    _log_file = chamber / LOG_FILE


def log(msg: str) -> None:
    line = f"[{datetime.now().astimezone().strftime('%H:%M:%S')}] {msg}"
    print(line, flush=True)
    if _log_file is not None:
        try:
            with _log_file.open("a") as f:
                f.write(line + "\n")
        except OSError:
            pass


# ------------------------------------------------------------------ config

@dataclass
class ChannelSpec:
    name: str
    platform: str            # zulip | lark
    # zulip
    stream: str = ""
    stream_id: int = 0
    topic: str | None = None  # required by validate(): the reply route
    # lark
    chat_id: str | None = None
    chat_type: str | None = None  # p2p | group | None=any
    history: bool = False     # import existing history on first run

    @property
    def key(self) -> str:
        return f"{self.platform}:{self.name}"


@dataclass
class BridgeConfig:
    poll_interval: int = DEFAULT_POLL_INTERVAL
    require_mention: bool = True
    trigger_words: list[str] = field(default_factory=lambda: list(DEFAULT_TRIGGER_WORDS))
    allowed_senders: list[str] = field(default_factory=list)  # empty = anyone
    reply_in_thread: bool = True
    transport: str = "auto"   # zulip: auto|poll|events ; lark: event-stream|poll
    channels: list[ChannelSpec] = field(default_factory=list)

    @staticmethod
    def load(chamber: Path) -> BridgeConfig:
        cfg = BridgeConfig()
        path = chamber / CONFIG_FILE
        if not path.exists():
            return cfg
        data = json.loads(toml_to_json(path.read_text()))
        poll = data.get("bridge", {})
        cfg.poll_interval = int(poll.get("poll_interval", cfg.poll_interval))
        cfg.require_mention = bool(poll.get("require_mention", cfg.require_mention))
        cfg.trigger_words = [str(w) for w in poll.get("trigger_words", cfg.trigger_words)]
        cfg.allowed_senders = [str(s) for s in poll.get("allowed_senders", [])]
        cfg.reply_in_thread = bool(poll.get("reply_in_thread", cfg.reply_in_thread))
        cfg.transport = str(poll.get("transport", cfg.transport))
        for c in poll.get("channels", []):
            cfg.channels.append(ChannelSpec(
                name=str(c.get("name", "main")),
                platform=str(c.get("platform", "zulip")),
                stream=str(c.get("stream", "")),
                stream_id=int(c.get("stream_id", 0) or 0),
                topic=c.get("topic"),
                chat_id=c.get("chat_id"),
                chat_type=c.get("chat_type"),
                history=bool(c.get("history", False)),
            ))
        return cfg

    def save(self, chamber: Path) -> None:
        channels = []
        for c in self.channels:
            d = {"name": c.name, "platform": c.platform}
            if c.stream:
                d["stream"] = c.stream
            if c.stream_id:
                d["stream_id"] = c.stream_id
            if c.topic is not None:
                d["topic"] = c.topic
            if c.chat_id:
                d["chat_id"] = c.chat_id
            if c.chat_type:
                d["chat_type"] = c.chat_type
            if c.history:
                d["history"] = True
            channels.append(d)
        body = {
            "bridge": {
                "poll_interval": self.poll_interval,
                "require_mention": self.require_mention,
                "trigger_words": self.trigger_words,
                "allowed_senders": self.allowed_senders,
                "reply_in_thread": self.reply_in_thread,
                "transport": self.transport,
                "channels": channels,
            }
        }
        atomic_write(chamber / CONFIG_FILE, toml_from_json(json.dumps(body)))

    def validate(self) -> None:
        if self.poll_interval <= 0:
            raise ChannelError("bridge.poll_interval must be greater than zero")
        if not self.channels:
            raise ChannelError("no bridge channels configured; run 'chat-bridge init' first")
        keys = [channel.key for channel in self.channels]
        if len(keys) != len(set(keys)):
            raise ChannelError("bridge channel names must be unique per platform")
        for channel in self.channels:
            if channel.platform == "zulip" and not channel.topic:
                raise ChannelError(
                    f"Zulip channel {channel.name!r} requires a topic for reply-safe routing"
                )
            if channel.platform == "lark" and not channel.chat_id:
                raise ChannelError(
                    f"Lark channel {channel.name!r} requires --chat-id for reply-safe routing"
                )


def toml_to_json(text: str) -> str:
    """Minimal TOML -> JSON for the flat config shape we generate."""
    try:
        import tomllib
        return json.dumps(tomllib.loads(text))
    except (ModuleNotFoundError, ValueError):
        # Accept bridge.toml files written by early versions, whose unquoted
        # transport value was not valid TOML.
        pass
    return _toml_to_json_simple(text)


def _toml_to_json_simple(text: str) -> str:
    """Parse the small TOML subset used by bridge.toml into JSON text."""
    lines = text.splitlines()
    section = None
    out: dict = {}
    cur: dict | None = None
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[[") and line.endswith("]]"):
            section = line[2:-2].strip()
            cur = None
        elif line.startswith("[") and line.endswith("]"):
            section = line[1:-1].strip()
            cur = None
        else:
            section = None
        if section == "bridge":
            cur = out.setdefault("bridge", {})
            continue
        if section and section.startswith("bridge.channels"):
            out.setdefault("bridge", {}).setdefault("channels", []).append({})
            cur = out["bridge"]["channels"][-1]
            continue
        if cur is None or "=" not in line:
            continue
        k, _, v = line.partition("=")
        k = k.strip()
        v = v.strip()
        cur[k] = _toml_scalar(v)
    return json.dumps(out)


def _toml_scalar(v: str):
    if v.startswith("[") and v.endswith("]"):
        return [_toml_scalar(item) for item in _split_toml_list(v[1:-1])]
    if len(v) >= 2 and v.startswith('"') and v.endswith('"'):
        try:
            return json.loads(v)
        except json.JSONDecodeError:
            return v[1:-1]
    if len(v) >= 2 and v.startswith("'") and v.endswith("'"):
        return v[1:-1]
    if v.lower() in ("true", "false"):
        return v.lower() == "true"
    try:
        return int(v)
    except ValueError:
        return v


def _split_toml_list(inner: str) -> list[str]:
    """Split a TOML inline-list body on commas outside quoted strings."""
    items: list[str] = []
    buf: list[str] = []
    quote: str | None = None
    escaped = False
    for ch in inner:
        if quote:
            buf.append(ch)
            if escaped:
                escaped = False
            elif quote == '"' and ch == "\\":
                escaped = True
            elif ch == quote:
                quote = None
        elif ch in "\"'":
            quote = ch
            buf.append(ch)
        elif ch == ",":
            items.append("".join(buf).strip())
            buf = []
        else:
            buf.append(ch)
    tail = "".join(buf).strip()
    if tail:
        items.append(tail)
    return [item for item in items if item]


def toml_from_json(text: str) -> str:
    d = json.loads(text)
    b = d.get("bridge", {})
    lines = ["# chat-bridge configuration (unified backbone)", "[bridge]"]
    for k in ("poll_interval", "require_mention", "trigger_words",
              "allowed_senders", "reply_in_thread", "transport"):
        if k in b:
            v = b[k]
            lines.append(f"{k} = {_toml_value(v)}")
    for c in b.get("channels", []):
        lines.append("")
        lines.append("[[bridge.channels]]")
        for k, v in c.items():
            if v is None:
                continue
            lines.append(f"{k} = {_toml_value(v)}")
    return "\n".join(lines) + "\n"


def _toml_value(value) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, list):
        return "[" + ", ".join(_toml_value(item) for item in value) + "]"
    return str(value)


# ------------------------------------------------------------------ state

def load_state(chamber: Path) -> dict:
    path = chamber / STATE_FILE
    if path.exists():
        try:
            return json.loads(path.read_text())
        except json.JSONDecodeError as exc:
            raise ChannelError(f"invalid {STATE_FILE}: {exc}") from exc
    return {"channels": {}, "stats": {"pulled": 0, "delivered": 0,
                                      "pushed": 0, "ignored": 0}}


def save_state(chamber: Path, state: dict) -> None:
    atomic_write(chamber / STATE_FILE, json.dumps(state, indent=2))


def channel_state(state: dict, key: str) -> dict:
    cs = state.setdefault("channels", {}).setdefault(key, {})
    cs.setdefault("cursor", None)
    cs.setdefault("last_thread", None)
    cs.setdefault("last_parent", None)
    cs.setdefault("delivered", [])
    cs.setdefault("seen_ids", [])
    cs.setdefault("sent_ids", [])
    cs.setdefault("pending_context", [])
    return cs


PENDING_CONTEXT_LIMIT = 20
PENDING_CONTEXT_MAX_AGE = 2 * 3600  # seconds; older context is dropped


def _trim_pending(cs: dict, now=None) -> None:
    import time as _time
    if now is None:
        now = _time.time()
    kept = []
    for entry in cs.get("pending_context", []):
        if now - entry.get("ts", 0) <= PENDING_CONTEXT_MAX_AGE:
            kept.append(entry)
    cs["pending_context"] = kept[-PENDING_CONTEXT_LIMIT:]


def _buffer_context(cs: dict, msg: Message) -> None:
    """Keep a non-directed message as reference context (feishu-collaborate style)."""
    _trim_pending(cs)
    cs["pending_context"].append({
        "id": msg.id,
        "sender": msg.sender_name or msg.sender_id,
        "content": msg.content,
        "ts": msg.timestamp.timestamp(),
        "thread": msg.thread,
    })


def _format_with_context(msg: Message, kind: str, pending: list[dict]) -> str:
    """Render the inbox body: batched context + the directive, like
    feishu-collaborate's `_format_inbox_body`."""
    labels = {
        "mention": ("Context before you were @-mentioned (reference only):",
                    "Someone now explicitly @-mentions you:"),
        "trigger": ("Context before you were summoned (reference only):",
                    "Someone now calls you explicitly:"),
        "reply": ("Context before your reply (reference only):",
                  "Someone now replied to your message:"),
    }
    ctx_label, dir_label = labels.get(kind, labels["mention"])
    lines: list[str] = []
    if pending:
        lines.append(f"{ctx_label}")
        lines.append("")
        for i, entry in enumerate(pending):
            if i:
                lines.append("")
            ts = datetime.fromtimestamp(
                entry["ts"], tz=timezone.utc
            ).astimezone().strftime("%H:%M")
            lines.append(f"{ts} {entry['sender']}:")
            lines.append("")
            for l in (entry["content"] or "").splitlines() or [""]:
                lines.append(f"> {l}")
    if lines:
        lines.append("")
    lines.append(dir_label)
    lines.append("")
    ts = msg.ts_local.strftime("%H:%M")
    lines.append(f"{ts} {msg.sender_name or msg.sender_id}:")
    lines.append("")
    for l in (msg.content or "").splitlines() or [""]:
        lines.append(f"> {l}")
    return "\n".join(lines)


def atomic_write(path: Path, data: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    tmp = Path(tmp_name)
    try:
        with os.fdopen(fd, "w") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(tmp, path)
    finally:
        tmp.unlink(missing_ok=True)


@contextmanager
def bridge_lock(chamber: Path):
    """Serialize state mutations across the service and one-shot commands."""
    import fcntl

    lock_dir = chamber / ".cryo"
    lock_dir.mkdir(parents=True, exist_ok=True)
    with (lock_dir / "chat-bridge.lock").open("a+") as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise ChannelError(
                "another chat-bridge process is already running for this chamber"
            ) from exc
        yield


def ensure_runtime_ignored(chamber: Path) -> None:
    """Keep credentials, chat content, and generated state out of Git."""
    path = chamber / ".gitignore"
    existing = path.read_text() if path.exists() else ""
    lines = set(existing.splitlines())
    required = [".cryo/", "chat-bridge.json", "chat-bridge.log", "messages/"]
    missing = [entry for entry in required if entry not in lines]
    if not missing:
        return
    prefix = existing
    if prefix and not prefix.endswith("\n"):
        prefix += "\n"
    atomic_write(path, prefix + "\n# chat-bridge runtime\n" + "\n".join(missing) + "\n")


# ------------------------------------------------------------------ trigger gate

def is_directed(msg: Message, bot: BotIdentity, cfg: BridgeConfig) -> str | None:
    """feishu-collaborate `require_mention` gate, platform-agnostic.

    A message is directed at the bot when it:
      (a) explicitly mentions the bot (mentioned_ids or mention label text),
      (b) starts with a trigger word followed by punctuation (`flash, ...`),
      (c) is a reply to one of the bot's own messages (reply-in-thread).

    Returns the trigger kind ("mention" | "trigger" | "reply") or None.
    """
    content = (msg.content or "").strip()
    lower = content.lower()

    # (a) explicit mention
    if msg.mentioned or (bot.id and bot.id in msg.mentioned_ids):
        return "mention"
    for label in bot.labels:
        if not label:
            continue
        label = label.lower()
        if f"@**{label}**" in lower or _contains_plain_mention(lower, label):
            return "mention"

    # (b) trigger word at the start, followed by punctuation or @-sign
    start = lower.lstrip()
    for w in cfg.trigger_words:
        wl = w.lower()
        mention = f"@{wl}"
        if start.startswith(mention) and (
            len(start) == len(mention)
            or not (start[len(mention)].isalnum() or start[len(mention)] in "_-")
        ):
            return "trigger"
        if start.startswith(wl) and start[len(wl):len(wl) + 1] in ",:，：!。":
            return "trigger"

    # (c) reply to one of the bot's own messages (in-thread follow-up)
    if cfg.reply_in_thread and msg.parent_id:
        # resolved per-channel via sent_ids by the caller; placeholder:
        return None

    return None


def _contains_plain_mention(content: str, label: str) -> bool:
    token = f"@{label}"
    start = 0
    while (index := content.find(token, start)) >= 0:
        end = index + len(token)
        if end == len(content) or not (content[end].isalnum() or content[end] in "_-"):
            return True
        start = end
    return False


def sender_allowed(msg: Message, cfg: BridgeConfig) -> bool:
    if not cfg.allowed_senders:
        return True
    return msg.sender_id in cfg.allowed_senders


# ------------------------------------------------------------------ mailbox

def deliver_to_inbox(chamber: Path, platform: str, msg: Message,
                     attachments: list[Path]) -> Path | None:
    store = chamber / "messages" / "inbox"
    store.mkdir(parents=True, exist_ok=True)
    meta = {
        "from": f"{platform}:{msg.sender_id}",
        "subject": msg.thread_name,
        "timestamp": msg.ts_local.strftime("%Y-%m-%dT%H-%M-%S"),
        "platform": platform,
        "platform_message_id": msg.id,
        "thread": msg.thread,
        "mentioned": "true" if msg.mentioned else "false",
    }
    body = msg.content
    if attachments:
        links = []
        for p in attachments:
            rel = p.relative_to(chamber)
            links.append(f"![{p.name}]({rel})" if p.suffix.lower() in (".png", ".jpg", ".jpeg", ".gif", ".webp") else f"[{p.name}]({rel})")
        body = body + "\n\n" + "\n".join(links)
    name = f"{msg.ts_local.strftime('%Y-%m-%dT%H-%M-%S')}_{slugify(msg.thread_name)}_{msg.id}.md"
    path = store / name
    atomic_write(path, render_mailbox_message(meta, body))
    return path


def push_outbox(chamber: Path, channels: dict[str, Channel], state: dict,
                cfg: BridgeConfig, errors: list[str] | None = None,
                specs: list[ChannelSpec] | None = None) -> int:
    outbox = chamber / "messages" / "outbox"
    archive = outbox / "archive"
    archive.mkdir(parents=True, exist_ok=True)
    default_threads = {s.key: (s.topic or s.chat_id) for s in (specs or [])
                       if (s.topic or s.chat_id)}
    pushed = 0
    for f in sorted(outbox.iterdir()):
        if not f.is_file() or not f.name.endswith(".md"):
            continue
        meta, body = parse_frontmatter(f.read_text())
        # Outbox files carry no platform metadata (cryo-agent send writes
        # none), so route via state: the channel that last delivered a
        # message is where the reply belongs (in its last thread). Proactive
        # chamber messages with no conversation state yet (e.g. the bootstrap
        # wake) fall back to the channel's configured route; a file with no
        # resolvable route is quarantined so it can never wedge the loop.
        platform = meta.get("platform") or ""
        active_route = state.get("active_route") or {}
        ckey = meta.get("channel_key") or active_route.get("channel_key") \
            or _guess_channel_key(platform, channels) or state.get("last_active")
        if ckey is None and len(channels) == 1:
            ckey = next(iter(channels))
        chan = channels.get(ckey)
        if chan is None:
            if not channels:
                error = f"push skipped {f.name}: no channel {ckey}"
                log(error)
                if errors is not None:
                    errors.append(error)
                continue
            if active_route.get("channel_key") == ckey:
                state.pop("active_route", None)
            error = _quarantine_outbox(f, outbox, f"no channel {ckey}")
            if errors is not None:
                errors.append(error)
            continue
        cs = channel_state(state, ckey)
        thread = meta.get("thread") or (
            active_route.get("thread") if active_route.get("channel_key") == ckey else None
        ) or cs.get("last_thread") or default_threads.get(ckey)
        if not thread:
            error = _quarantine_outbox(f, outbox, "no reply thread known")
            if errors is not None:
                errors.append(error)
            continue
        parent_id = active_route.get("parent_id") \
            if active_route.get("channel_key") == ckey else cs.get("last_parent")
        target = ReplyTarget(
            thread=thread,
            parent_id=parent_id if cfg.reply_in_thread else None,
        )
        content = body
        if meta.get("subject"):
            content = f"{meta['subject']}\n\n{content}"
        try:
            idempotency_key = hashlib.sha256(
                f"{ckey}:{f.name}:{thread}".encode()
            ).hexdigest()[:32]
            mid = chan.send(target, content.strip(), idempotency_key=idempotency_key)
        except ChannelError as e:
            error = f"push failed {f.name}: {e}"
            log(error)
            if errors is not None:
                errors.append(error)
            continue
        cs["sent_ids"] = (cs["sent_ids"] + [mid])[-SEEN_IDS_LIMIT:]
        os.replace(f, archive / f.name)
        pushed += 1
    return pushed


def _guess_channel_key(platform: str, channels: dict[str, Channel]) -> str | None:
    if platform:
        for key, chan in channels.items():
            if chan.name == platform:
                return key
    return None


def _quarantine_outbox(f: Path, outbox: Path, reason: str) -> str:
    failed = outbox / "failed"
    failed.mkdir(parents=True, exist_ok=True)
    os.replace(f, failed / f.name)
    error = f"push quarantined {f.name} -> failed/: {reason}"
    log(error)
    return error


# ------------------------------------------------------------------ sync loop

def pull_once(chamber: Path, chan: Channel, spec: ChannelSpec, cfg: BridgeConfig,
              state: dict, bot: BotIdentity, attachments_dir: Path) -> int:
    key = spec.key
    cs = channel_state(state, key)
    if cs["cursor"] is None:
        if not spec.history:
            # First run without --history: anchor at newest, don't replay history.
            res = chan.fetch_new(None, limit=1)
            cs["cursor"] = res.cursor
            save_state(chamber, state)
            log(f"[{key}] initialized at cursor {cs['cursor']}")
            return 0
        anchor: str | None = "0"  # history import: from the beginning
    else:
        anchor = cs["cursor"]

    res = chan.fetch_new(anchor, limit=1000)
    all_new = res.messages
    if res.cursor:
        cs["cursor"] = res.cursor
    delivered = 0

    for msg in all_new:
        if msg.id in cs["seen_ids"]:
            continue
        cs["seen_ids"] = (cs["seen_ids"] + [msg.id])[-SEEN_IDS_LIMIT:]
        state["stats"]["pulled"] += 1
        if bot.self_ids and msg.sender_id in bot.self_ids:
            continue  # anti-echo: never react to our own messages
        if not sender_allowed(msg, cfg):
            state["stats"]["ignored"] += 1
            log(f"[{key}] ignored (sender not whitelisted): {msg.id} from {msg.sender_id}")
            continue
        if cfg.require_mention:
            directed = is_directed(msg, bot, cfg)
            if not directed and cfg.reply_in_thread and _is_reply_to_bot(msg, cs):
                directed = "reply"
            if not directed:
                # not directed: keep it as reference context (feishu-collaborate
                # batching), so the next directed message carries the thread.
                _buffer_context(cs, msg)
                state["stats"]["ignored"] += 1
                log(f"[{key}] buffered as context: {msg.id} from {msg.sender_id} thread={msg.thread_name}")
                continue
        if cfg.require_mention:
            _trim_pending(cs)
            pending = [entry for entry in cs["pending_context"]
                       if entry.get("thread") == msg.thread]
            if pending:
                body = _format_with_context(msg, directed or "mention", pending)
                msg = replace(msg, content=body)
        attachments: list[Path] = []
        if msg.attachments:
            for att in msg.attachments:
                try:
                    p = chan.download(att, attachments_dir)
                    attachments.append(p)
                except ChannelError as e:
                    log(f"[{key}] attachment download failed {att.name}: {e}")
        deliver_to_inbox(chamber, spec.platform, msg, attachments)
        cs["delivered"] = (cs["delivered"] + [msg.id])[-SEEN_IDS_LIMIT:]
        # Consume context only for the delivered thread; another thread's
        # chatter must never leak into a later prompt.
        cs["pending_context"] = [entry for entry in cs["pending_context"]
                                 if entry.get("thread") != msg.thread]
        cs["last_thread"] = msg.thread
        if msg.parent_id:
            cs["last_parent"] = msg.parent_id
        state["last_active"] = key
        state.setdefault("active_route", {
            "channel_key": key,
            "thread": msg.thread,
            "parent_id": msg.parent_id,
        })
        state["stats"]["delivered"] += 1
        delivered += 1
        log(f"[{key}] delivered to inbox: {msg.id} from {msg.sender_id} thread={msg.thread_name}")

    save_state(chamber, state)
    return delivered


def _is_reply_to_bot(msg: Message, cs: dict) -> bool:
    """Reply-to-bot: message's parent is one of the bot's recent sent ids."""
    if not msg.parent_id:
        return False
    return msg.parent_id in (cs.get("sent_ids") or [])


def sync_once(chamber: Path, channels: dict[str, Channel], specs: list[ChannelSpec],
              cfg: BridgeConfig, state: dict, bots: dict[str, BotIdentity]) -> None:
    errors: list[str] = []
    pushed = push_outbox(chamber, channels, state, cfg, errors, specs)
    if pushed:
        state["stats"]["pushed"] += pushed
        if not errors:
            state.pop("active_route", None)
        log(f"pushed {pushed} outbox message(s)")
    # Only an unanswered active route defers pulling (serialized replies).
    # Push errors alone must not: a stuck proactive outbox file would
    # otherwise wedge the pull phase forever.
    if state.get("active_route"):
        save_state(chamber, state)
        return

    attachments_dir = chamber / "messages" / "attachments"
    attachments_dir.mkdir(parents=True, exist_ok=True)
    if not specs:
        save_state(chamber, state)
        return
    start = int(state.get("next_channel_index", 0)) % len(specs)
    ordered = [(start + offset) % len(specs) for offset in range(len(specs))]
    delivered_any = False
    for index in ordered:
        spec = specs[index]
        chan = channels[spec.key]
        try:
            delivered = pull_once(
                chamber, chan, spec, cfg, state, bots[spec.key], attachments_dir
            )
            if delivered:
                state["next_channel_index"] = (index + 1) % len(specs)
                delivered_any = True
                break
        except ChannelError as e:
            log(f"[{spec.key}] pull error: {e}")
    if not delivered_any:
        state["next_channel_index"] = (start + 1) % len(specs)
    save_state(chamber, state)


# ------------------------------------------------------------------ services

def service_name(chamber: Path, kind: str = "chat-bridge") -> str:
    h = hashlib.md5(str(chamber.resolve()).encode()).hexdigest()[:16]
    return f"com.cryo.{kind}.{h}"


def install_service(chamber: Path, command: list[str]) -> str:
    import shutil

    if shutil.which("systemctl") is None:
        raise ChannelError(
            "systemd is not available; use 'chat-bridge run --no-service' under your OS supervisor"
        )
    if any("\n" in part or "\r" in part for part in [str(chamber), *command]):
        raise ChannelError("service paths and arguments must not contain newlines")
    name = service_name(chamber)
    unit_dir = Path.home() / ".config" / "systemd" / "user"
    unit_dir.mkdir(parents=True, exist_ok=True)
    cmd = shlex.join(command).replace("%", "%%")
    working_directory = str(chamber).replace("%", "%%")
    unit = unit_dir / f"{name}.service"
    unit.write_text(f"""[Unit]
Description=Cryochamber chat bridge ({chamber.name})
After=default.target

[Service]
Type=simple
WorkingDirectory={working_directory}
ExecStart={cmd}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
""")
    _systemctl("daemon-reload")
    _systemctl("enable", "--now", name)
    return name


def uninstall_service(chamber: Path) -> bool:
    name = service_name(chamber)
    _systemctl("disable", "--now", name, check=False)
    unit = Path.home() / ".config" / "systemd" / "user" / f"{name}.service"
    if unit.exists():
        unit.unlink()
    _systemctl("daemon-reload", check=False)
    return True


def _systemctl(*args, check: bool = True) -> None:
    import subprocess
    try:
        proc = subprocess.run(
            ["systemctl", "--user", *args], check=False,
            capture_output=True, text=True,
        )
    except FileNotFoundError as exc:
        if check:
            raise ChannelError("systemctl not found; systemd user services are unavailable") from exc
        return
    if check and proc.returncode != 0:
        detail = proc.stderr.strip() or proc.stdout.strip() or "unknown error"
        raise ChannelError(f"systemctl --user {' '.join(args)} failed: {detail}")
