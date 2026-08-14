"""chat-bridge CLI — unified across platforms.

Subcommands mirror cryo-zulip's UX so switching platforms feels identical:
  init        configure a channel and validate credentials
  pull        one inbound fetch (deliver to inbox)
  push        one outbound push (post outbox to chat)
  run         sync loop (pull + push) as a systemd user service
  status      show channel/state summary
  unsync      stop and remove the sync service
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from pathlib import Path

from . import __version__
from .backbone import (
    CONFIG_FILE,
    BridgeConfig,
    ChannelSpec,
    bridge_lock,
    channel_state,
    configure_logging,
    ensure_runtime_ignored,
    install_service,
    load_state,
    log,
    pull_once,
    push_outbox,
    save_state,
    sync_once,
    uninstall_service,
)
from .channel import ChannelError


def _make_channels(cfg: BridgeConfig, chamber: Path,
                   zuliprc: Path | None = None) -> tuple[dict, list, dict]:
    from .lark import LarkChannel
    from .zulip import ZulipChannel

    cfg.validate()
    channels: dict[str, object] = {}
    specs: list[ChannelSpec] = []
    bots: dict[str, object] = {}
    try:
        for spec in cfg.channels:
            if spec.platform == "zulip":
                chan = ZulipChannel(spec, chamber,
                                    zuliprc or chamber / ".cryo" / "zuliprc",
                                    cfg.transport)
            elif spec.platform == "lark":
                chan = LarkChannel(spec, chamber, cfg.transport)
            else:
                raise ChannelError(f"unknown platform {spec.platform}")
            chan.connect()
            channels[spec.key] = chan
            specs.append(spec)
            bots[spec.key] = chan.profile()
    except Exception:
        for chan in channels.values():
            chan.close()
        raise
    return channels, specs, bots


def cmd_init(args) -> int:
    chamber = Path(args.chamber).resolve()
    chamber.mkdir(parents=True, exist_ok=True)
    configure_logging(chamber)
    ensure_runtime_ignored(chamber)
    if args.platform == "zulip" and not args.stream:
        log("error: --stream is required for zulip")
        return 1
    if args.platform == "lark" and args.history:
        log("error: --history is currently supported only for Zulip")
        return 1
    if args.interval is not None and args.interval <= 0:
        log("error: --interval must be greater than zero")
        return 1
    cfg = BridgeConfig.load(chamber)
    pending_zuliprc: Path | None = None
    dest: Path | None = None
    if args.platform == "zulip":
        if args.config:
            rc_dir = chamber / ".cryo"
            rc_dir.mkdir(parents=True, exist_ok=True)
            dest = rc_dir / "zuliprc"
            pending_zuliprc = rc_dir / ".zuliprc.pending"
            flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
            if hasattr(os, "O_NOFOLLOW"):
                flags |= os.O_NOFOLLOW
            try:
                fd = os.open(pending_zuliprc, flags, 0o600)
                os.fchmod(fd, 0o600)
                with os.fdopen(fd, "wb") as target, \
                        Path(args.config).resolve().open("rb") as source:
                    target.write(source.read())
                    target.flush()
                    os.fsync(target.fileno())
            except OSError:
                pending_zuliprc.unlink(missing_ok=True)
                raise
        elif not (chamber / ".cryo" / "zuliprc").exists():
            log("error: no .cryo/zuliprc; pass --config <zuliprc path>")
            return 1
    if args.trigger:
        cfg.trigger_words = args.trigger
    if args.interval:
        cfg.poll_interval = args.interval
    if args.transport:
        cfg.transport = args.transport
    if args.allow_sender:
        cfg.allowed_senders = args.allow_sender
    spec = ChannelSpec(
        name=args.name,
        platform=args.platform,
        stream=args.stream or "",
        topic=args.topic,
        chat_id=args.chat_id,
        chat_type=args.chat_type,
        history=args.history,
    )
    if args.platform == "lark" and not spec.chat_type:
        spec.chat_type = "p2p"
    cfg.channels = [s for s in cfg.channels if s.key != spec.key] + [spec]

    # Validate credentials and resolve channels before persisting configuration.
    try:
        channels, specs, bots = _make_channels(cfg, chamber, pending_zuliprc)
    except Exception:
        if pending_zuliprc is not None:
            pending_zuliprc.unlink(missing_ok=True)
        raise
    try:
        state = load_state(chamber)
        for s in specs:
            cs = channel_state(state, s.key)
            if cs["cursor"] is None and not s.history:
                res = channels[s.key].fetch_new(None, limit=1)
                cs["cursor"] = res.cursor
                log(f"[{s.key}] anchored at cursor {res.cursor}")
            elif s.history:
                log(f"[{s.key}] history import armed (first pull reads from the beginning)")
        if pending_zuliprc is not None and dest is not None:
            os.replace(pending_zuliprc, dest)
            log(f"copied zuliprc -> {dest} (0600)")
        cfg.save(chamber)
        save_state(chamber, state)
        for key, chan in channels.items():
            b = bots[key]
            log(f"channel {key}: {chan.name} bot={b.name} ({b.id})")
    finally:
        for chan in channels.values():
            chan.close()
        if pending_zuliprc is not None:
            pending_zuliprc.unlink(missing_ok=True)
    log(f"config written to {chamber / CONFIG_FILE}")
    log("next: chat-bridge run  (or pull/push for one-off cycles)")
    return 0


def cmd_pull(args) -> int:
    chamber = Path(args.chamber).resolve()
    configure_logging(chamber)
    cfg = BridgeConfig.load(chamber)
    channels, specs, bots = _make_channels(cfg, chamber)
    try:
        state = load_state(chamber)
        if state.get("active_route"):
            raise ChannelError(
                "a chamber reply is still pending; push it before pulling another route"
            )
        attachments_dir = chamber / "messages" / "attachments"
        attachments_dir.mkdir(parents=True, exist_ok=True)
        start = int(state.get("next_channel_index", 0)) % len(specs)
        for offset in range(len(specs)):
            index = (start + offset) % len(specs)
            spec = specs[index]
            delivered = pull_once(chamber, channels[spec.key], spec, cfg, state,
                                  bots[spec.key], attachments_dir)
            if delivered:
                state["next_channel_index"] = (index + 1) % len(specs)
                save_state(chamber, state)
                break
    finally:
        for chan in channels.values():
            chan.close()
    return 0


def cmd_push(args) -> int:
    chamber = Path(args.chamber).resolve()
    configure_logging(chamber)
    cfg = BridgeConfig.load(chamber)
    channels, _specs, _bots = _make_channels(cfg, chamber)
    try:
        state = load_state(chamber)
        errors: list[str] = []
        pushed = push_outbox(chamber, channels, state, cfg, errors)
        state["stats"]["pushed"] += pushed
        if pushed and not errors:
            state.pop("active_route", None)
        save_state(chamber, state)
    finally:
        for chan in channels.values():
            chan.close()
    return 1 if errors else 0


def cmd_run(args) -> int:
    chamber = Path(args.chamber).resolve()
    configure_logging(chamber)
    cfg = BridgeConfig.load(chamber)
    cfg.validate()
    if args.interval is not None:
        if args.interval <= 0:
            raise ChannelError("--interval must be greater than zero")
        cfg.poll_interval = args.interval
    if args.no_service:
        channels, specs, bots = _make_channels(cfg, chamber)
        state = load_state(chamber)
        log(f"sync loop starting (direct, interval={cfg.poll_interval}s)")
        _loop(chamber, channels, specs, cfg, state, bots)
        return 0
    launcher = Path(__file__).resolve().parent.parent / "chat-bridge"
    if not launcher.exists():
        raise ChannelError(f"chat-bridge launcher not found at {launcher}")
    # The service runs the foreground loop (--no-service) so it never
    # re-installs itself; the interval flag is only informational.
    command = [str(launcher), "run", "--no-service", "--chamber", str(chamber),
               "--interval", str(cfg.poll_interval)]
    name = install_service(chamber, command)
    log(f"sync service installed: {name}")
    log(f"log: {chamber.name}/chat-bridge.log   stop: chat-bridge unsync")
    return 0


def _loop(chamber: Path, channels: dict, specs: list, cfg: BridgeConfig,
          state: dict, bots: dict) -> None:
    import signal as _signal

    stop = {"flag": False}

    def _on_signal(signum, _frame):
        stop["flag"] = True

    _signal.signal(_signal.SIGTERM, _on_signal)
    _signal.signal(_signal.SIGINT, _on_signal)
    while not stop["flag"]:
        sync_once(chamber, channels, specs, cfg, state, bots)
        time.sleep(cfg.poll_interval)
    for chan in channels.values():
        chan.close()


def cmd_status(args) -> int:
    chamber = Path(args.chamber).resolve()
    configure_logging(chamber)
    cfg = BridgeConfig.load(chamber)
    state = load_state(chamber)
    print(f"config: {chamber / CONFIG_FILE}")
    for spec in cfg.channels:
        cs = channel_state(state, spec.key)
        print(f"channel {spec.key}: platform={spec.platform} "
              f"cursor={cs['cursor']} last_thread={cs['last_thread']} "
              f"delivered={len(cs['delivered'])}")
    print("stats:", state.get("stats"))
    return 0


def cmd_unsync(args) -> int:
    chamber = Path(args.chamber).resolve()
    configure_logging(chamber)
    uninstall_service(chamber)
    log(f"sync service removed for {chamber}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(prog="chat-bridge", description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--version", action="version", version=__version__)
    sub = ap.add_subparsers(dest="command", required=True)

    def add_common(p, extra=()):
        p.add_argument("--chamber", default=".", help="chamber directory")
        for name, kwargs in extra:
            p.add_argument(name, **kwargs)

    p = sub.add_parser("init", help="configure a channel and validate")
    add_common(p, [
        ("--platform", {"choices": ["zulip", "lark"], "required": True}),
        ("--config", {"default": None, "help": "zuliprc path to copy into .cryo/ (zulip)"}),
        ("--name", {"default": "main", "help": "channel name (multi-channel key)"}),
        ("--stream", {"default": "", "help": "zulip stream name"}),
        ("--topic", {"default": None, "help": "zulip topic (None = whole stream)"}),
        ("--chat-id", {"default": None, "help": "lark chat_id (None = any whitelisted p2p)"}),
        ("--chat-type", {"choices": ["p2p", "group"], "default": None,
                         "help": "lark chat type (default: p2p)"}),
        ("--history", {"action": "store_true", "help": "import existing history on first pull"}),
        ("--trigger", {"action": "append", "help": "trigger words (repeatable)"}),
        ("--allow-sender", {"action": "append", "help": "sender whitelist ids (repeatable; empty = anyone)"}),
        ("--interval", {"type": int, "default": None}),
        ("--transport", {"choices": ["auto", "poll", "events", "event-stream"], "default": None}),
    ])

    for name, help_ in (("pull", "one inbound fetch"), ("push", "one outbound push"),
                        ("status", "show state"), ("unsync", "remove sync service")):
        p = sub.add_parser(name, help=help_)
        add_common(p)
    p = sub.add_parser("run", help="sync loop as a systemd user service")
    add_common(p, [("--interval", {"type": int, "default": None}),
                   ("--no-service", {"action": "store_true", "help": "run in foreground"})])
    return ap


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    handlers = {
        "init": cmd_init,
        "pull": cmd_pull,
        "push": cmd_push,
        "run": cmd_run,
        "status": cmd_status,
        "unsync": cmd_unsync,
    }
    try:
        needs_lock = args.command in {"init", "pull", "push"} or (
            args.command == "run" and args.no_service
        )
        if needs_lock:
            chamber = Path(args.chamber).resolve()
            with bridge_lock(chamber):
                return handlers[args.command](args)
        return handlers[args.command](args)
    except (ChannelError, OSError, ValueError) as exc:
        print(f"chat-bridge: error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
