"""Channel protocol — the abstraction every platform adapter implements.

The backbone only ever talks to `Channel`. A platform is a black box that:
  - reports the bot's identity (for anti-echo and mention detection),
  - yields new messages since a cursor (inbound),
  - sends replies to a thread (outbound),
  - uploads/downloads attachments.

The `thread` string is the platform's opaque reply-routing handle:
  - Zulip : the stream topic (reply goes to the same topic)
  - Lark  : the chat_id (reply goes to the same chat/thread)
"""

from __future__ import annotations

import abc
import re
import urllib.parse
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path


def iso_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


@dataclass
class Attachment:
    """A file attached to a message. `path` is set when downloaded locally."""

    key: str                 # platform resource key / url
    name: str                # display filename
    kind: str = "file"       # image | file | audio | video
    path: Path | None = None
    meta: dict = field(default_factory=dict)


@dataclass
class Message:
    """Normalized inbound message, platform-agnostic."""

    id: str
    sender_id: str
    sender_name: str
    content: str                 # markdown text
    thread: str                  # reply handle: zulip topic / lark chat_id
    thread_name: str             # human label: topic / chat name
    timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    mentioned: bool = False      # explicitly mentions the bot
    mentioned_ids: list[str] = field(default_factory=list)
    parent_id: str | None = None # replied-to message id (for in-thread routing)
    attachments: list[Attachment] = field(default_factory=list)
    raw: dict = field(default_factory=dict)  # platform payload, for adapters

    @property
    def ts_local(self) -> datetime:
        return self.timestamp.astimezone()


@dataclass
class FetchResult:
    messages: list[Message]
    cursor: str                  # newest cursor to persist
    done: bool = True            # reached the newest message


@dataclass
class ReplyTarget:
    thread: str                  # zulip topic / lark chat_id
    parent_id: str | None = None  # reply to a specific message (lark threads)


@dataclass
class BotIdentity:
    id: str
    name: str
    email: str = ""
    labels: list[str] = field(default_factory=list)  # mention labels, e.g. ["flash"]
    self_ids: list[str] = field(default_factory=list)  # ids that count as self


class ChannelError(Exception):
    """Raised by adapters on recoverable platform errors."""


class ChannelTimeout(ChannelError):
    """A read timed out — not an error for long-poll transports."""


class Channel(abc.ABC):
    """Platform adapter interface. Implementations live in zulip.py / lark.py."""

    name: str = "base"

    def connect(self) -> None:
        """One-time setup (validate credentials, resolve channels)."""

    @abc.abstractmethod
    def profile(self) -> BotIdentity:
        """Return the bot identity on this platform."""

    @abc.abstractmethod
    def fetch_new(self, cursor: str | None, limit: int = 1000) -> FetchResult:
        """Return messages strictly newer than `cursor` (None = from beginning
        if history import, else from newest)."""

    @abc.abstractmethod
    def send(self, target: ReplyTarget, content: str) -> str:
        """Post `content` to the target thread. Returns the platform message id."""

    def upload(self, path: Path) -> str:
        """Upload a local file; return a markdown link usable in `send`."""
        raise ChannelError(f"{self.name}: upload not supported")

    def download(self, attachment: Attachment, dest: Path) -> Path:
        """Download an attachment into `dest`; return the saved path."""
        raise ChannelError(f"{self.name}: download not supported")

    def close(self) -> None:
        """Release resources (event queues, subprocesses)."""


def dump_message(msg: Message) -> dict:
    """Serialize a Message (for logs/state)."""
    return {
        "id": msg.id,
        "sender_id": msg.sender_id,
        "sender_name": msg.sender_name,
        "thread": msg.thread,
        "thread_name": msg.thread_name,
        "timestamp": msg.timestamp.isoformat(),
        "mentioned": msg.mentioned,
        "parent_id": msg.parent_id,
    }


def parse_frontmatter(text: str) -> tuple[dict, str]:
    """Split a mailbox file into (frontmatter dict, body)."""
    meta: dict[str, str] = {}
    if text.startswith("---"):
        parts = text.split("---", 2)
        if len(parts) == 3:
            for line in parts[1].strip().splitlines():
                if ":" in line:
                    k, v = line.split(":", 1)
                    meta[k.strip()] = v.strip()
            return meta, parts[2].strip()
    return meta, text.strip()


def render_mailbox_message(meta: dict, body: str) -> str:
    """Render frontmatter + body the way cryochamber expects inbox files."""
    lines = ["---"]
    for k, v in meta.items():
        v = str(v).replace("\n", " ").replace("\r", " ")
        lines.append(f"{k}: {v}")
    lines.append("---")
    lines.append("")
    lines.append(body)
    return "\n".join(lines) + "\n"


def slugify(text: str) -> str:
    text = re.sub(r"[^a-zA-Z0-9\u4e00-\u9fff]+", "-", text).strip("-")
    return (text[:40] or "msg").lower()


LOCAL_LINK_RE = re.compile(r"(!?)\[([^\]]*)\]\(([^)]+)\)")


def resolve_local_link(chamber: Path, target: str) -> Path | None:
    """Resolve a Markdown link to a safe chamber-local file, if it is one."""
    parsed = urllib.parse.urlparse(target)
    if parsed.scheme or parsed.netloc or target.startswith("#"):
        return None
    raw = urllib.parse.unquote(parsed.path)
    path = Path(raw)
    candidate = (path if path.is_absolute() else chamber / path).resolve()
    chamber = chamber.resolve()
    try:
        relative = candidate.relative_to(chamber)
    except ValueError:
        return None
    if relative.parts and relative.parts[0] == ".cryo":
        return None
    return candidate if candidate.is_file() else None
