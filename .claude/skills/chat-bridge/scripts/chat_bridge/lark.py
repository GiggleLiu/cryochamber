"""Lark (Feishu) channel adapter.

Implements the Channel protocol over the official `lark-cli` binary:
  - credentials: `lark-cli auth login` (managed by lark-cli itself)
  - inbound:  `lark-cli event consume im.message.receive_v1` — a WebSocket
              long connection (pure outbound, no open ports). Events arrive
              as NDJSON on stdout.
  - outbound: `lark-cli im +messages-send` / `+messages-reply`
  - attachments: `lark-cli im +messages-resources-download` (in),
              `+messages-send --image/--file` (out)
  - group @bot: group events are delivered when directed (mention text /
              trigger word) — mention detection is content-based, the same
              gate the backbone applies to every platform.
"""

from __future__ import annotations

import json
import queue
import subprocess
import threading
from contextlib import suppress
from datetime import datetime, timezone
from pathlib import Path

from .channel import (
    LOCAL_LINK_RE,
    Attachment,
    BotIdentity,
    Channel,
    ChannelError,
    FetchResult,
    Message,
    ReplyTarget,
    resolve_local_link,
)

LARK_CLI = "lark-cli"
EVENT_KEY = "im.message.receive_v1"
READY_TIMEOUT = 90
ONE_SHOT_WAIT_SECONDS = 5.0


def _run_cli(args: list[str], timeout: int = 60, cwd: Path | None = None) -> dict:
    """Run a lark-cli command; return parsed JSON (best effort)."""
    try:
        proc = subprocess.run(
            [LARK_CLI, *args, "--format", "json"],
            capture_output=True, text=True, timeout=timeout, cwd=cwd,
            check=False,
        )
    except FileNotFoundError as e:
        raise ChannelError(f"lark-cli not found on PATH ({LARK_CLI})") from e
    except subprocess.TimeoutExpired as e:
        raise ChannelError(f"lark-cli {' '.join(args[:3])} timed out") from e
    if proc.returncode != 0:
        raise ChannelError(
            f"lark-cli {' '.join(args[:3])} failed ({proc.returncode}): "
            f"{proc.stderr.strip()[:200] or proc.stdout.strip()[:200]}"
        )
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError:
        return {"ok": True, "raw": proc.stdout}


class LarkChannel(Channel):
    name = "lark"

    def __init__(self, spec, chamber: Path, transport: str = "event-stream"):
        self.spec = spec
        self.chamber = chamber.resolve()
        self.transport = transport if transport != "auto" else "event-stream"
        self._bot: BotIdentity | None = None
        self._proc: subprocess.Popen | None = None
        self._queue: queue.Queue = queue.Queue()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    # ---------------------------------------------------------- identity

    def connect(self) -> None:
        info = _run_cli(["api", "GET", "/open-apis/bot/v3/info"])
        bot = info.get("bot") or {}
        open_id = bot.get("open_id") or ""
        app_name = bot.get("app_name") or ""
        self._bot = BotIdentity(
            id=open_id,
            name=app_name,
            email="",
            labels=[app_name],
            self_ids=[open_id],
        )
        if not open_id:
            raise ChannelError("lark: could not resolve bot open_id")

    def profile(self) -> BotIdentity:
        if self._bot is None:
            self.connect()
        return self._bot

    # ------------------------------------------------------------ inbound

    def _spawn_stream(self) -> None:
        self._stop.clear()
        try:
            self._proc = subprocess.Popen(
                [LARK_CLI, "event", "consume", EVENT_KEY, "--as", "bot", "--quiet"],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
        except FileNotFoundError as e:
            raise ChannelError(f"lark-cli not found on PATH ({LARK_CLI})") from e
        self._thread = threading.Thread(target=self._read_stream, daemon=True)
        self._thread.start()

    def _read_stream(self) -> None:
        assert self._proc and self._proc.stdout
        for line in self._proc.stdout:
            if self._stop.is_set():
                break
            line = line.strip()
            if not line:
                continue
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            if ev.get("type") == EVENT_KEY or "event_id" in ev:
                self._queue.put(ev)

    def fetch_new(self, cursor: str | None, limit: int = 1000) -> FetchResult:
        started = False
        if self._proc is None or self._proc.poll() is not None:
            self._spawn_stream()
            started = True
            # give the stream a moment to start delivering
            if cursor is None:
                return FetchResult(messages=[], cursor="0", done=True)
        events: list[dict] = []
        if started:
            try:
                events.append(self._queue.get(timeout=ONE_SHOT_WAIT_SECONDS))
            except queue.Empty:
                pass
        try:
            while len(events) < limit:
                events.append(self._queue.get_nowait())
        except queue.Empty:
            pass
        messages: list[Message] = []
        newest = int(cursor or 0)
        for ev in events:
            create_ms = int(ev.get("create_time") or ev.get("timestamp") or 0)
            # Several events can share the same millisecond. The backbone's
            # persisted seen_ids list handles replay deduplication.
            if create_ms < newest:
                continue
            newest = max(newest, create_ms)
            msg = self._event_to_message(ev)
            if msg:
                messages.append(msg)
        return FetchResult(messages=messages, cursor=str(newest), done=True)

    def _event_to_message(self, ev: dict) -> Message | None:
        sender = ev.get("sender_id") or ""
        if not sender:
            return None
        chat_id = ev.get("chat_id") or ""
        if self.spec.chat_id and chat_id != self.spec.chat_id:
            return None
        if self.spec.chat_type and ev.get("chat_type") != self.spec.chat_type:
            return None
        msg_id = ev.get("message_id") or ev.get("id") or ""
        content_raw = str(ev.get("content") or "").strip()
        attachments: list[Attachment] = []
        content = content_raw
        try:
            parsed = json.loads(content_raw)
            if isinstance(parsed, dict):
                content = parsed.get("text") or content_raw
                for key_field, kind in (("image_key", "image"), ("file_key", "file")):
                    if parsed.get(key_field):
                        attachments.append(Attachment(
                            key=parsed[key_field], name=parsed[key_field],
                            kind=kind, meta={"message_id": msg_id},
                        ))
        except json.JSONDecodeError:
            pass
        create_ms = int(ev.get("create_time") or 0)
        ts = datetime.fromtimestamp(create_ms / 1000, tz=timezone.utc) if create_ms else \
            datetime.now(timezone.utc)
        return Message(
            id=msg_id or ev.get("event_id") or "",
            sender_id=sender,
            sender_name=sender,
            content=content,
            thread=chat_id,
            thread_name=chat_id,
            timestamp=ts,
            mentioned=False,  # lark events expose no mentions array; gate is content-based
            mentioned_ids=[],
            parent_id=None,
            attachments=attachments,
            raw=ev,
        )

    # ------------------------------------------------------------ outbound

    def send(self, target: ReplyTarget, content: str,
             idempotency_key: str | None = None) -> str:
        chat_id = target.thread
        if not chat_id:
            raise ChannelError("lark: no chat_id for reply target")
        attachments: list[Path] = []

        def collect(match) -> str:
            path = resolve_local_link(self.chamber, match.group(3))
            if path is None:
                return match.group(0)
            attachments.append(path)
            return ""

        body = LOCAL_LINK_RE.sub(collect, content).strip()
        ids: list[str] = []
        if body:
            ids.append(self._send_payload(
                target, "--markdown", body, idempotency_key, len(ids)
            ))
        for path in attachments:
            flag = "--image" if path.suffix.lower() in (".png", ".jpg", ".jpeg", ".gif", ".webp") else "--file"
            rel = path.relative_to(self.chamber)
            ids.append(self._send_payload(
                target, flag, str(rel), idempotency_key, len(ids)
            ))
        if not ids:
            raise ChannelError("lark: reply content is empty")
        return ids[-1]

    def _send_payload(self, target: ReplyTarget, flag: str, value: str,
                      idempotency_key: str | None, segment: int) -> str:
        if target.parent_id:
            args = ["im", "+messages-reply", "--message-id", target.parent_id,
                    flag, value, "--as", "bot"]
        else:
            args = ["im", "+messages-send", "--chat-id", target.thread,
                    flag, value, "--as", "bot"]
        if idempotency_key:
            args.extend(["--idempotency-key", f"{idempotency_key}-{segment}"])
        r = _run_cli(args, cwd=self.chamber)
        data = r.get("data") or r
        return str(data.get("message_id") or data.get("id") or "")

    def upload(self, path: Path) -> str:
        raise ChannelError(
            "lark uploads require a reply target; attach files through send()"
        )

    def download(self, attachment: Attachment, dest: Path) -> Path:
        dest.mkdir(parents=True, exist_ok=True)
        msg_id = (attachment.meta or {}).get("message_id")
        if not msg_id:
            raise ChannelError(f"lark download: no message_id for {attachment.key}")
        try:
            rel_dest = dest.resolve().relative_to(self.chamber)
        except ValueError as e:
            raise ChannelError("lark download destination must be inside the chamber") from e
        output = rel_dest / attachment.key
        _run_cli(["im", "+messages-resources-download",
                  "--message-id", msg_id, "--file-key", attachment.key,
                  "--type", attachment.kind, "--output", str(output)],
                 cwd=self.chamber)
        # lark-cli may infer and append an extension from the response headers.
        candidates = sorted(dest.glob(f"{attachment.key}*"),
                            key=lambda p: p.stat().st_mtime, reverse=True)
        if candidates:
            return candidates[0]
        raise ChannelError(f"lark download: no file written for {attachment.key}")

    def close(self) -> None:
        self._stop.set()
        if self._proc:
            try:
                self._proc.terminate()
                self._proc.wait(timeout=5)
            except (OSError, subprocess.TimeoutExpired):
                with suppress(OSError):
                    self._proc.kill()
            self._proc = None
