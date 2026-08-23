"""Zulip channel adapter.

Implements the Channel protocol over the Zulip REST API:
  - credentials: a zuliprc INI file (.cryo/zuliprc) with [api] site/email/key
  - inbound:  GET /api/v1/messages (narrow = stream, optional topic)
  - realtime: POST /register + GET /api/v1/events long-poll (transport=events)
  - outbound: POST /api/v1/messages (stream + topic)
  - attachments: POST /user_uploads (out), GET /user_uploads/{url} (in)
"""

from __future__ import annotations

import base64
import configparser
import hashlib
import json
import os
import socket
import tempfile
import time
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

from .channel import (
    LOCAL_LINK_RE,
    Attachment,
    BotIdentity,
    Channel,
    ChannelError,
    ChannelTimeout,
    FetchResult,
    Message,
    ReplyTarget,
    resolve_local_link,
)

MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024


class ZulipChannel(Channel):
    name = "zulip"

    def __init__(self, spec, chamber: Path, zuliprc: Path, transport: str = "auto"):
        self.spec = spec
        self.chamber = chamber.resolve()
        self.zuliprc = zuliprc
        self.transport = transport
        site, email, key = _load_zuliprc(zuliprc)
        self.site = site
        self.email = email
        self.key = key
        self._queue_id: str | None = None
        self._last_event_id: int = 0
        self._bot: BotIdentity | None = None
        self.stream_id: int = spec.stream_id

    # ------------------------------------------------------------- HTTP

    def _basic(self) -> str:
        raw = base64.b64encode(f"{self.email}:{self.key}".encode()).decode()
        return "Basic " + raw

    def request(self, method: str, path: str, params: dict | None = None,
                data: dict | None = None, raw: bool = False,
                timeout: int = 40):
        url = f"{self.site}/api/v1/{path}"
        if params:
            url += "?" + urllib.parse.urlencode(params)
        req = urllib.request.Request(url, method=method)
        req.add_header("Authorization", self._basic())
        body = None
        if data is not None:
            req.add_header("Content-Type", "application/x-www-form-urlencoded")
            body = urllib.parse.urlencode(data).encode()
        try:
            with urllib.request.urlopen(req, data=body, timeout=timeout) as resp:
                payload = resp.read()
                return payload if raw else json.loads(payload.decode())
        except urllib.error.HTTPError as e:
            detail = e.read().decode()[:300]
            raise ChannelError(f"zulip HTTP {e.code}: {detail}") from e
        except TimeoutError as e:
            raise ChannelTimeout(f"zulip timeout: {e}") from e
        except urllib.error.URLError as e:
            if isinstance(getattr(e, "reason", None), (TimeoutError, socket.timeout)):
                raise ChannelTimeout(f"zulip timeout: {e}") from e
            raise ChannelError(f"zulip network error: {e}") from e

    # ---------------------------------------------------------- identity

    def connect(self) -> None:
        prof = self.request("GET", "users/me")
        self._bot = BotIdentity(
            id=str(prof.get("user_id", "")),
            name=prof.get("full_name") or "",
            email=prof.get("email") or "",
            labels=[prof.get("full_name") or "", (prof.get("email") or "").split("@")[0]],
            self_ids=[str(prof.get("user_id", "")), prof.get("email") or ""],
        )
        if not self.spec.dm and not self.stream_id and self.spec.stream:
            streams = self.request("GET", "streams")
            for s in streams.get("streams", []):
                if s.get("name") == self.spec.stream:
                    self.stream_id = s["stream_id"]
                    self.spec.stream_id = self.stream_id
                    break
            if not self.stream_id:
                raise ChannelError(
                    f"zulip stream '{self.spec.stream}' not found — is the bot subscribed?"
                )
        if self.transport == "auto":
            self.transport = "events" if self._queue_supported() else "poll"

    def _register_narrow(self) -> str:
        if self.spec.dm:
            return json.dumps([["is", "private"]])
        operand = self.spec.stream or str(self.stream_id)
        narrow = [["stream", operand]]
        if self.spec.topic:
            narrow.append(["topic", self.spec.topic])
        return json.dumps(narrow)

    def _queue_supported(self) -> bool:
        try:
            r = self.request("POST", "register", data={
                "narrow": self._register_narrow(),
                "fetch_event_types": json.dumps(["message"]),
            })
            self._queue_id = r.get("queue_id")
            self._last_event_id = -1
            return bool(self._queue_id)
        except ChannelError:
            return False

    def profile(self) -> BotIdentity:
        if self._bot is None:
            self.connect()
        return self._bot

    # ------------------------------------------------------------ inbound

    def fetch_new(self, cursor: str | None, limit: int = 1000) -> FetchResult:
        if cursor is None:
            # initialization: return the newest message to anchor the cursor
            r = self._fetch_window("newest", 1, 0)
            msgs = r.get("messages") or []
            newest = max((m["id"] for m in msgs), default=0)
            return FetchResult(messages=[], cursor=str(newest), done=True)
        if cursor == "0":
            # history import: the events queue has no history, poll from the start
            return self._fetch_poll(cursor, limit)
        if self.transport == "events":
            # realtime long-poll (instant pickup) + poll catch-up (messages
            # posted while the bridge was down never enter the queue).
            ev = self._fetch_events(cursor, limit)
            if ev is None:
                return self._fetch_poll(cursor, limit)
            poll = self._fetch_poll(cursor, limit)
            by_id = {m.id: m for m in poll.messages}
            for m in ev.messages:
                by_id.setdefault(m.id, m)
            merged = [by_id[k] for k in sorted(by_id, key=int)]
            merged = merged[:limit]
            fallback_cursor = max(int(ev.cursor or 0), int(poll.cursor or 0))
            merged_cursor = merged[-1].id if merged else str(fallback_cursor)
            return FetchResult(
                messages=merged,
                cursor=merged_cursor,
                done=True,
            )
        return self._fetch_poll(cursor, limit)

    def _is_group_dm(self, m: dict) -> bool:
        """True for a group direct message (bot + >=2 humans)."""
        return bool(
            self.spec.dm
            and m.get("type") == "private"
            and len(m.get("display_recipient") or []) > 2
        )

    def _fetch_poll(self, cursor: str, limit: int) -> FetchResult:
        anchor = int(cursor) if cursor and cursor.isdigit() else 0
        if cursor == "0":
            anchor = 1  # from the very beginning
        if self.spec.dm:
            narrow = [{"operator": "is", "operand": "private"}]
        else:
            narrow = [{"operator": "stream", "operand": self.stream_id}]
            if self.spec.topic:
                narrow.append({"operator": "topic", "operand": self.spec.topic})
        messages: list[Message] = []
        newest = anchor
        page = anchor
        while True:
            r = self.request("GET", "messages", params={
                "narrow": json.dumps(narrow),
                "anchor": page,
                "num_after": 1000,
                "apply_markdown": "false",
            })
            batch = r.get("messages") or []
            if not batch:
                break
            for m in batch:
                if m["id"] <= anchor:
                    continue  # anchor may be inclusive
                newest = max(newest, m["id"])
                if self._is_group_dm(m):
                    log_warn(f"zulip: skipping group DM {m['id']} in DM-mode channel")
                    continue
                messages.append(self._to_message(m))
                if len(messages) >= limit:
                    return FetchResult(messages=messages, cursor=str(newest), done=False)
            if r.get("found_newest") or len(batch) < 1000:
                break
            page = newest
        return FetchResult(messages=messages, cursor=str(newest), done=True)

    def _fetch_window(self, anchor, num_before: int, num_after: int) -> dict:
        if self.spec.dm:
            narrow = [{"operator": "is", "operand": "private"}]
        else:
            narrow = [{"operator": "stream", "operand": self.stream_id}]
            if self.spec.topic:
                narrow.append({"operator": "topic", "operand": self.spec.topic})
        return self.request("GET", "messages", params={
            "narrow": json.dumps(narrow),
            "anchor": anchor,
            "num_before": num_before,
            "num_after": num_after,
            "apply_markdown": "false",
        })

    def _fetch_events(self, cursor: str, limit: int) -> FetchResult | None:
        """Real-time transport: long-poll the Zulip events queue for messages."""
        if not self._queue_id:
            try:
                r = self.request("POST", "register", data={
                    "narrow": self._register_narrow(),
                    "fetch_event_types": json.dumps(["message"]),
                })
                self._queue_id = r.get("queue_id")
                self._last_event_id = -1
            except ChannelError as e:
                log_warn(f"zulip register failed, falling back to poll: {e}")
                self.transport = "poll"
                return None
        try:
            r = self.request("GET", "events", params={
                "queue_id": self._queue_id,
                "last_event_id": self._last_event_id,
                "dont_block": "false",
            }, timeout=10)  # server may hold the connection; bound it client-side
        except ChannelTimeout:
            return FetchResult(messages=[], cursor=cursor, done=True)  # keep polling
        except ChannelError as e:
            log_warn(f"zulip events error ({e}); re-registering")
            self._queue_id = None
            return self._fetch_poll(cursor, limit)
        events = r.get("events") or []
        messages: list[Message] = []
        newest = int(cursor) if cursor.isdigit() else 0
        for ev in events:
            self._last_event_id = max(self._last_event_id, int(ev.get("id", 0)))
            if ev.get("type") != "message":
                continue
            m = ev.get("message") or {}
            if m.get("id", 0) <= newest:
                continue
            newest = max(newest, m["id"])
            if self._is_group_dm(m):
                log_warn(f"zulip: skipping group DM {m.get('id')} in DM-mode channel")
                continue
            messages.append(self._to_message(m))
            if len(messages) >= limit:
                break
        return FetchResult(messages=messages, cursor=str(newest), done=True)

    def _to_message(self, m: dict) -> Message:
        ts = datetime.fromtimestamp(m.get("timestamp", 0), tz=timezone.utc)
        mentioned_ids = [str(u.get("id")) for u in (m.get("mentioned_users") or [])]
        attachments = _extract_upload_links(m.get("content") or "")
        for attachment in attachments:
            attachment.meta["message_id"] = str(m.get("id", 0))
        sender_id = m.get("sender_email") or m.get("sender_id", "")
        # DM channels: the reply route is the sender, so the thread carries
        # their identifier (email preferred — private send accepts it directly).
        if self.spec.dm:
            thread = sender_id or "dm"
        else:
            thread = m.get("subject") or "general"
        return Message(
            id=str(m.get("id", 0)),
            sender_id=sender_id,
            sender_name=m.get("sender_full_name") or "unknown",
            content=m.get("content") or "",
            thread=thread,
            thread_name=thread,
            timestamp=ts,
            mentioned=bool(m.get("mentioned", False)),
            mentioned_ids=mentioned_ids,
            parent_id=None,  # Zulip threads are topics; no parent ids in API
            attachments=attachments,
            raw=m,
        )

    # ------------------------------------------------------------ outbound

    def send(self, target: ReplyTarget, content: str,
             idempotency_key: str | None = None) -> str:
        def externalize(match) -> str:
            path = resolve_local_link(self.chamber, match.group(3))
            return self.upload(path) if path is not None else match.group(0)

        content = LOCAL_LINK_RE.sub(externalize, content)
        if self.spec.dm:
            data = {
                "type": "private",
                "to": json.dumps([target.thread]),  # Zulip wants a JSON-encoded array
                "content": content,
            }
        else:
            data = {
                "type": "stream",
                "to": self.stream_id,
                "topic": target.thread,
                "content": content,
            }
        r = self.request("POST", "messages", data=data)
        return str(r.get("id", ""))

    def upload(self, path: Path) -> str:
        """Upload a local file; return a markdown link (image if it looks like one)."""
        import mimetypes
        if path.stat().st_size > MAX_ATTACHMENT_BYTES:
            raise ChannelError(
                f"zulip upload exceeds {MAX_ATTACHMENT_BYTES // (1024 * 1024)} MB: {path.name}"
            )
        boundary = "----chatbridge" + str(time.time()).replace(".", "")
        with open(path, "rb") as f:
            raw = f.read()
        ctype = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
        body = (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="file"; filename="{path.name}"\r\n'
            f"Content-Type: {ctype}\r\n\r\n"
        ).encode() + raw + f"\r\n--{boundary}--\r\n".encode()
        url = f"{self.site}/api/v1/user_uploads"
        req = urllib.request.Request(url, data=body, method="POST")
        req.add_header("Authorization", self._basic())
        req.add_header("Content-Type", f"multipart/form-data; boundary={boundary}")
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                r = json.loads(resp.read().decode())
        except urllib.error.HTTPError as e:
            raise ChannelError(f"zulip upload failed: {e.read().decode()[:200]}") from e
        url_path = r.get("uri") or r.get("url") or ""
        if url_path.startswith("/"):
            url_path = self.site + url_path
        if path.suffix.lower() in (".png", ".jpg", ".jpeg", ".gif", ".webp"):
            return f"![{path.name}]({url_path})"
        return f"[{path.name}]({url_path})"

    def download(self, attachment: Attachment, dest: Path) -> Path:
        dest.mkdir(parents=True, exist_ok=True)
        safe = _safe_name(attachment.name)
        identity = f"{attachment.meta.get('message_id', '')}:{attachment.key}"
        prefix = hashlib.sha256(identity.encode()).hexdigest()[:12]
        out = dest / f"{prefix}_{safe}"
        url = attachment.key
        if url.startswith(self.site):
            path = url[len(self.site):]
        elif url.startswith("/"):
            path = url
        else:
            raise ChannelError(f"zulip download: bad url {url}")
        req = urllib.request.Request(self.site + path)
        req.add_header("Authorization", self._basic())
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                length = resp.headers.get("Content-Length")
                if length and int(length) > MAX_ATTACHMENT_BYTES:
                    raise ChannelError(
                        f"zulip download exceeds {MAX_ATTACHMENT_BYTES // (1024 * 1024)} MB: {attachment.name}"
                    )
                payload = resp.read(MAX_ATTACHMENT_BYTES + 1)
                if len(payload) > MAX_ATTACHMENT_BYTES:
                    raise ChannelError(
                        f"zulip download exceeds {MAX_ATTACHMENT_BYTES // (1024 * 1024)} MB: {attachment.name}"
                    )
                fd, tmp_name = tempfile.mkstemp(prefix=f".{out.name}.", dir=dest)
                tmp = Path(tmp_name)
                try:
                    with os.fdopen(fd, "wb") as handle:
                        handle.write(payload)
                        handle.flush()
                        os.fsync(handle.fileno())
                    os.replace(tmp, out)
                finally:
                    tmp.unlink(missing_ok=True)
        except urllib.error.HTTPError as e:
            raise ChannelError(f"zulip download failed: {e.read().decode()[:200]}") from e
        return out


# ---------------------------------------------------------------- helpers

def _load_zuliprc(path: Path) -> tuple[str, str, str]:
    cp = configparser.ConfigParser()
    cp.read(path)
    try:
        site = cp.get("api", "site").rstrip("/")
        email = cp.get("api", "email")
        key = cp.get("api", "key")
    except (configparser.Error, KeyError) as e:
        raise ChannelError(f"invalid zuliprc {path}: {e}") from e
    return site, email, key


def _extract_upload_links(content: str) -> list[Attachment]:
    """Find /user_uploads/ markdown links in raw Zulip content."""
    import re
    out: list[Attachment] = []
    for m in re.finditer(r"(\!?)\[([^\]]*)\]\((/user_uploads/[^\)]+)\)", content):
        kind = "image" if m.group(1) else "file"
        name = m.group(2) or os.path.basename(m.group(3))
        out.append(Attachment(key=m.group(3), name=name, kind=kind))
    return out


def _safe_name(name: str) -> str:
    import re
    name = re.sub(r"[^A-Za-z0-9._-]+", "_", name)
    return name or "attachment"


def log_warn(msg: str) -> None:
    from .backbone import log
    log(msg)
