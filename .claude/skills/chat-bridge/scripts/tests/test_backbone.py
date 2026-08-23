"""Backbone unit tests — mock channel, no network.

Run: python3 -m unittest discover -s tests -v
"""

from __future__ import annotations

import json
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from chat_bridge.backbone import (
    CONFIG_FILE,
    BridgeConfig,
    ChannelSpec,
    _toml_to_json_simple,
    channel_state,
    deliver_to_inbox,
    ensure_runtime_ignored,
    is_directed,
    load_state,
    parse_frontmatter,
    pull_once,
    push_outbox,
    save_state,
    sender_allowed,
    sync_once,
)
from chat_bridge.channel import (
    Attachment,
    BotIdentity,
    Channel,
    ChannelError,
    FetchResult,
    Message,
)
from chat_bridge.cli import cmd_pull


class MockChannel(Channel):
    name = "mock"

    def __init__(self, profile, messages=None, sent=None):
        self._profile = profile
        self._messages = messages or {}
        self._sent = sent if sent is not None else []
        self.cursor = None

    def connect(self):
        pass

    def profile(self):
        return self._profile

    def download(self, attachment, dest):
        dest.mkdir(parents=True, exist_ok=True)
        out = dest / attachment.name
        out.write_bytes(b"fake-content")
        return out

    def fetch_new(self, cursor, limit=1000):
        if cursor is None:
            # initialization anchor
            ids = [int(k) for k in self._messages]
            return FetchResult(messages=[], cursor=str(max(ids)) if ids else "0", done=True)
        newest = int(cursor or 0)
        out = [m for k, m in sorted(self._messages.items()) if int(k) > newest][:limit]
        next_cursor = out[-1].id if out else str(newest)
        return FetchResult(messages=out, cursor=next_cursor,
                           done=len(out) < limit)

    def send(self, target, content, idempotency_key=None):
        self._sent.append((target, content, idempotency_key))
        return f"sent-{len(self._sent)}"


class FailingSendMockChannel(MockChannel):
    def __init__(self, profile, messages=None):
        super().__init__(profile, messages=messages)
        self.fail_send = True

    def send(self, target, content, idempotency_key=None):
        if self.fail_send:
            raise ChannelError("offline")
        return super().send(target, content, idempotency_key)


BOT = BotIdentity(id="bot1", name="flash", email="flash@x",
                  labels=["flash", "flash-bot"], self_ids=["bot1", "flash@x"])


def msg(mid, content, sender="user1", thread="general", mentioned=False,
        mentioned_ids=None, parent=None, sender_name="user"):
    return Message(
        id=mid, sender_id=sender, sender_name=sender_name, content=content,
        thread=thread, thread_name=thread,
        timestamp=datetime.now(timezone.utc),
        mentioned=mentioned, mentioned_ids=mentioned_ids or [], parent_id=parent,
    )


class TriggerGateTests(unittest.TestCase):
    def setUp(self):
        self.cfg = BridgeConfig(require_mention=True,
                                trigger_words=["flash", "flash-bot"])

    def test_explicit_mention(self):
        m = msg("1", "hello", mentioned_ids=["bot1"])
        self.assertTrue(is_directed(m, BOT, self.cfg))

    def test_mention_label_text(self):
        self.assertTrue(is_directed(msg("1", "@**flash** status"), BOT, self.cfg))
        self.assertTrue(is_directed(msg("1", "@flash hi"), BOT, self.cfg))

    def test_trigger_word_start(self):
        self.assertTrue(is_directed(msg("1", "flash, do a sweep"), BOT, self.cfg))
        self.assertTrue(is_directed(msg("1", "flash: status?"), BOT, self.cfg))

    def test_no_false_positive_on_plain_prose(self):
        self.assertFalse(is_directed(msg("1", "flash memory is fast"), BOT, self.cfg))
        self.assertFalse(is_directed(msg("1", "just checking the weather"), BOT, self.cfg))
        self.assertFalse(is_directed(msg("1", "@flashlight status"), BOT, self.cfg))

    def test_platform_mention_flag_is_directed(self):
        self.assertTrue(is_directed(msg("1", "hello", mentioned=True), BOT, self.cfg))

    def test_require_mention_false(self):
        # require_mention=False is enforced at the pull gate, not inside
        # is_directed (which only inspects the message).
        cfg = BridgeConfig(require_mention=False)
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            chan = MockChannel(BOT, messages={"5": msg("5", "plain prose", sender="u1")})
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            state["channels"]["mock:main"] = {"cursor": "4", "last_thread": None,
                                                "delivered": [], "sent_ids": []}
            pull_once(chamber, chan, spec, cfg, state, BOT, chamber / "att")
            inbox = list((chamber / "messages" / "inbox").glob("*.md"))
            self.assertEqual(len(inbox), 1)  # everything is directed when off


class WhitelistTests(unittest.TestCase):
    def test_empty_allows_anyone(self):
        self.assertTrue(sender_allowed(msg("1", "x", sender="user1"), BridgeConfig()))

    def test_whitelist_blocks_others(self):
        cfg = BridgeConfig(allowed_senders=["user1"])
        self.assertTrue(sender_allowed(msg("1", "x", sender="user1"), cfg))
        self.assertFalse(sender_allowed(msg("1", "x", sender="user2"), cfg))


class PullTests(unittest.TestCase):
    def test_init_anchors_at_newest(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            chan = MockChannel(BOT, messages={"10": msg("10", "old"), "20": msg("20", "old2")})
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT, chamber / "att")
            cs = channel_state(state, spec.key)
            self.assertEqual(cs["cursor"], "20")
            self.assertFalse((chamber / "messages" / "inbox").exists() or list((chamber / "messages" / "inbox").glob("*.md")))

    def test_delivers_directed_and_skips_own(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            messages = {
                "21": msg("21", "weather talk", sender="user1"),
                "22": msg("22", "@**flash** status", sender="user1", mentioned_ids=["bot1"]),
                "23": msg("23", "self echo", sender="bot1"),
            }
            chan = MockChannel(BOT, messages=messages)
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            state["channels"]["mock:main"] = {"cursor": "20", "last_thread": None,
                                              "delivered": [], "sent_ids": []}
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT, chamber / "att")
            inbox = list((chamber / "messages" / "inbox").glob("*.md"))
            self.assertEqual(len(inbox), 1)  # only the mention
            cs = channel_state(state, spec.key)
            self.assertEqual(cs["last_thread"], "general")
            self.assertEqual(state["stats"]["ignored"], 1)

    def test_dm_channel_serializes_messages_from_different_senders(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            messages = {
                "31": msg("31", "here is my homework", sender="alice@example.com",
                           thread="alice@example.com"),
                "32": msg("32", "mine too", sender="bob@example.com",
                           thread="bob@example.com"),
                "33": msg("33", "self echo", sender="bot1", thread="bot1"),
            }
            chan = MockChannel(BOT, messages=messages)
            spec = ChannelSpec(name="dms", platform="mock", dm=True)
            state = load_state(chamber)
            state["channels"]["mock:dms"] = {"cursor": "30", "last_thread": None,
                                              "delivered": [], "sent_ids": []}
            pull_once(chamber, chan, spec, BridgeConfig(require_mention=True),
                      state, BOT, chamber / "att")
            inbox = list((chamber / "messages" / "inbox").glob("*.md"))
            self.assertEqual(len(inbox), 1)
            cs = channel_state(state, spec.key)
            self.assertEqual(cs["last_thread"], "alice@example.com")
            self.assertEqual(state["active_route"]["thread"], "alice@example.com")
            self.assertEqual(cs["cursor"], "31")
            self.assertEqual(state["stats"]["ignored"], 0)

    def test_dropbox_mode_collects_and_replies_without_waking_agent(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            messages = {
                "41": msg("41", "here is my test paper", sender="alice@example.com",
                           thread="alice@example.com"),
                "42": msg("42", "mine too", sender="bob@example.com",
                           thread="bob@example.com"),
            }
            chan = MockChannel(BOT, messages=messages)
            spec = ChannelSpec(name="dms", platform="mock", dm=True,
                               auto_reply="received; queued")
            state = load_state(chamber)
            state["channels"]["mock:dms"] = {"cursor": "40", "last_thread": None,
                                              "delivered": [], "sent_ids": []}
            pull_once(chamber, chan, spec, BridgeConfig(require_mention=True),
                      state, BOT, chamber / "att")
            # NO agent wake: no inbox files, no active_route
            inbox = list((chamber / "messages" / "inbox").glob("*.md"))
            self.assertEqual(inbox, [])
            self.assertNotIn("active_route", state)
            # collected per sender with template replies to the right sender
            box = chamber / "messages" / "dropbox"
            senders = [d.name for d in box.iterdir()]
            self.assertTrue(any("alice" in s for s in senders))
            self.assertTrue(any("bob" in s for s in senders))
            sent_threads = [t.thread for (t, _, _) in chan._sent]
            self.assertEqual(sent_threads, ["alice@example.com", "bob@example.com"])
            self.assertTrue(all(c == "received; queued" for (_, c, _) in chan._sent))
            cs = channel_state(state, spec.key)
            self.assertEqual(len(cs["dropbox"]), 2)
            self.assertEqual(cs["cursor"], "42")
            self.assertEqual(state["stats"]["ignored"], 0)

    def test_dropbox_stores_attachments_and_acks_only_after_storage(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            m = msg("51", "submission with file", sender="alice@example.com",
                    thread="alice@example.com")
            m.attachments = [Attachment("photo.jpg", "photo.jpg", "image")]
            chan = MockChannel(BOT, messages={"51": m})
            spec = ChannelSpec(name="dms", platform="mock", dm=True,
                               auto_reply="received; queued")
            state = load_state(chamber)
            state["channels"]["mock:dms"] = {"cursor": "50", "last_thread": None,
                                              "delivered": [], "sent_ids": []}
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT, chamber / "att")
            # the file is durably stored under the sender's dropbox dir
            stored = list((chamber / "messages" / "dropbox").rglob("*photo.jpg"))
            self.assertEqual(len(stored), 1)
            self.assertEqual(stored[0].read_bytes(), b"fake-content")
            # exactly one ack, after storage
            self.assertEqual(len(chan._sent), 1)
            self.assertEqual(chan._sent[0][0].thread, "alice@example.com")
            cs = channel_state(state, spec.key)
            self.assertEqual(cs["dropbox"][-1]["files"], [stored[0].name])

    def test_dropbox_no_files_uses_redirect_template(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            m = msg("61", "question, no file", sender="alice@example.com",
                    thread="alice@example.com")
            chan = MockChannel(BOT, messages={"61": m})
            spec = ChannelSpec(name="dms", platform="mock", dm=True,
                               auto_reply="submission received",
                               auto_reply_no_files="questions go to the QA bot")
            state = load_state(chamber)
            state["channels"]["mock:dms"] = {"cursor": "60", "last_thread": None,
                                              "delivered": [], "sent_ids": []}
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT, chamber / "att")
            self.assertEqual(len(chan._sent), 1)
            self.assertIn("QA bot", chan._sent[0][1])

    def test_dropbox_failed_ack_is_persisted_and_retried(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            m = msg("65", "submission", sender="alice@example.com",
                    thread="alice@example.com")
            chan = FailingSendMockChannel(BOT, messages={"65": m})
            spec = ChannelSpec(name="dms", platform="mock", dm=True,
                               auto_reply="received")
            state = load_state(chamber)
            channel_state(state, spec.key)["cursor"] = "64"

            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT,
                      chamber / "att")
            cs = channel_state(state, spec.key)
            self.assertTrue(cs["dropbox"][-1]["ack_failed"])
            self.assertEqual(len(cs["pending_acks"]), 1)
            saved = load_state(chamber)
            self.assertEqual(saved["channels"][spec.key]["cursor"], "65")

            chan.fail_send = False
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT,
                      chamber / "att")
            self.assertFalse(cs["dropbox"][-1]["ack_failed"])
            self.assertEqual(cs["pending_acks"], [])
            self.assertEqual(len(chan._sent), 1)
            self.assertIsNotNone(chan._sent[0][2])

    def test_dropbox_polled_while_agent_route_active(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            teaching = ChannelSpec(name="teaching", platform="mock", topic="reports")
            dms = ChannelSpec(name="dms", platform="mock", dm=True,
                              auto_reply="received; queued")
            teach_chan = MockChannel(BOT)
            dm_chan = MockChannel(BOT, {
                "71": msg("71", "paper", sender="alice@example.com",
                           thread="alice@example.com")})
            channels = {teaching.key: teach_chan, dms.key: dm_chan}
            bots = {teaching.key: BOT, dms.key: BOT}
            state = load_state(chamber)
            channel_state(state, teaching.key)["cursor"] = "0"
            channel_state(state, dms.key)["cursor"] = "70"
            state["active_route"] = {
                "channel_key": teaching.key, "thread": "reports",
                "parent_id": None,
            }
            cfg = BridgeConfig()
            sync_once(chamber, channels, [teaching, dms], cfg, state, bots)
            # dropbox still collected the submission while the route is active
            stored = list((chamber / "messages" / "dropbox").rglob("*"))
            self.assertTrue(stored)
            self.assertEqual(dm_chan._sent[0][0].thread, "alice@example.com")
            self.assertIn("active_route", state)  # route untouched

    def test_history_import(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            messages = {"1": msg("1", "@**flash** a", sender="u", mentioned_ids=["bot1"]),
                        "2": msg("2", "@**flash** b", sender="u", mentioned_ids=["bot1"])}
            chan = MockChannel(BOT, messages=messages)
            spec = ChannelSpec(name="main", platform="mock", history=True)
            state = load_state(chamber)
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT, chamber / "att")
            inbox = list((chamber / "messages" / "inbox").glob("*.md"))
            self.assertEqual(len(inbox), 2)

    def test_seen_message_is_not_delivered_twice(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            repeated = msg("22", "@**flash** status", mentioned_ids=["bot1"])
            chan = MockChannel(BOT, messages={"22": repeated})
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            state["channels"]["mock:main"] = {
                "cursor": "20", "last_thread": None, "delivered": [],
                "seen_ids": ["22"], "sent_ids": [],
            }
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT,
                      chamber / "att")
            self.assertFalse((chamber / "messages" / "inbox").exists())
            self.assertEqual(state["stats"]["delivered"], 0)

    def test_context_is_batched_only_within_the_trigger_thread(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            messages = {
                "21": msg("21", "context A", thread="topic-a"),
                "22": msg("22", "context B", thread="topic-b"),
                "23": msg("23", "@**flash** act", thread="topic-a",
                          mentioned_ids=["bot1"]),
            }
            chan = MockChannel(BOT, messages=messages)
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            state["channels"]["mock:main"] = {"cursor": "20"}
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT,
                      chamber / "att")

            inbox = list((chamber / "messages" / "inbox").glob("*.md"))
            self.assertEqual(len(inbox), 1)
            _meta, body = parse_frontmatter(inbox[0].read_text())
            self.assertIn("context A", body)
            self.assertIn("@**flash** act", body)
            self.assertNotIn("context B", body)
            pending = channel_state(state, "mock:main")["pending_context"]
            self.assertEqual([entry["content"] for entry in pending], ["context B"])


class ConfigTests(unittest.TestCase):
    def test_saved_config_is_valid_toml_and_roundtrips_strings(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            cfg = BridgeConfig(
                transport="events",
                trigger_words=['flash,bot', 'quote"bot'],
                channels=[ChannelSpec(name="main", platform="zulip",
                                      stream='Research "Lab"', topic="a,b")],
            )
            cfg.save(chamber)
            loaded = BridgeConfig.load(chamber)
            self.assertEqual(loaded.transport, "events")
            self.assertEqual(loaded.trigger_words, cfg.trigger_words)
            self.assertEqual(loaded.channels[0].stream, 'Research "Lab"')
            self.assertEqual(loaded.channels[0].topic, "a,b")

    def test_fallback_toml_parser_preserves_quoted_commas_and_quotes(self):
        # The pre-3.11 fallback (no tomllib) must round-trip the same config.
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            cfg = BridgeConfig(
                trigger_words=['flash,bot', 'quote"bot'],
                channels=[ChannelSpec(name="main", platform="zulip",
                                      stream='Research "Lab"', topic="a,b")],
            )
            cfg.save(chamber)
            data = json.loads(
                _toml_to_json_simple((chamber / CONFIG_FILE).read_text())
            )["bridge"]
            self.assertEqual(data["trigger_words"], ['flash,bot', 'quote"bot'])
            self.assertEqual(data["channels"][0]["stream"], 'Research "Lab"')
            self.assertEqual(data["channels"][0]["topic"], "a,b")

    def test_config_validation_enforces_dropbox_dependencies(self):
        with self.assertRaisesRegex(ChannelError, "auto_reply requires dm"):
            BridgeConfig(channels=[ChannelSpec(
                name="bad", platform="zulip", stream="research",
                auto_reply="received",
            )]).validate()
        with self.assertRaisesRegex(ChannelError, "auto_reply_no_files"):
            BridgeConfig(channels=[ChannelSpec(
                name="bad", platform="zulip", dm=True,
                auto_reply_no_files="attach a file",
            )]).validate()
        with self.assertRaisesRegex(ChannelError, "only for zulip"):
            BridgeConfig(channels=[ChannelSpec(
                name="bad", platform="lark", dm=True,
            )]).validate()

    def test_invalid_state_fails_loudly(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            (chamber / "chat-bridge.json").write_text("{")
            with self.assertRaisesRegex(Exception, "invalid chat-bridge.json"):
                load_state(chamber)

    def test_runtime_and_credentials_are_gitignored(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            (chamber / ".gitignore").write_text("existing\n")
            ensure_runtime_ignored(chamber)
            ensure_runtime_ignored(chamber)
            lines = (chamber / ".gitignore").read_text().splitlines()
            for entry in (".cryo/", "chat-bridge.json", "chat-bridge.log", "messages/"):
                self.assertEqual(lines.count(entry), 1)


class ContextBatchingTests(unittest.TestCase):
    def test_non_directed_buffered_and_attached(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            messages = {
                # task message: NOT directed
                "30": msg("30", "please survey decoding algorithms and post the report",
                          sender="u1", thread="general chat"),
                # bare mention right after: directed
                "31": msg("31", "@**flash**", sender="u1", thread="general chat",
                          mentioned_ids=["bot1"]),
            }
            chan = MockChannel(BOT, messages=messages)
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            state["channels"]["mock:main"] = {"cursor": "29", "last_thread": None,
                                                "delivered": [], "sent_ids": [],
                                                "pending_context": []}
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT, chamber / "att")
            inbox = list((chamber / "messages" / "inbox").glob("*.md"))
            self.assertEqual(len(inbox), 1)  # only the mention is delivered
            body = inbox[0].read_text()
            self.assertIn("survey decoding algorithms", body)  # context attached
            self.assertIn("@-mentioned", body)
            cs = channel_state(state, spec.key)
            self.assertEqual(cs["pending_context"], [])  # consumed after delivery
            self.assertEqual(state["stats"]["ignored"], 1)

    def test_context_does_not_leak_to_next_batch(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            chan = MockChannel(BOT, messages={})
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            state["channels"]["mock:main"] = {"cursor": "0", "last_thread": None,
                                                "delivered": [], "sent_ids": [],
                                                "pending_context": []}
            # directed message with no preceding context -> plain body
            chan._messages = {"40": msg("40", "@**flash** hi", sender="u1",
                                       mentioned_ids=["bot1"])}
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT, chamber / "att")
            body = next((chamber / "messages" / "inbox").glob("*.md")).read_text()
            self.assertNotIn("Context before", body)
            self.assertIn("@**flash** hi", body)


class PushTests(unittest.TestCase):
    def test_routes_via_default_thread_when_no_active_route(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            chan = MockChannel(BOT)
            spec = ChannelSpec(name="main", platform="mock", topic="reports")
            outbox = chamber / "messages" / "outbox"
            outbox.mkdir(parents=True)
            (outbox / "2026-08-14T10-00-00_reply.md").write_text(
                "---\nsubject: Reply\n---\n\nHello agent reply"
            )
            state = load_state(chamber)
            state["last_active"] = "mock:main"
            cs = channel_state(state, "mock:main")
            cs["last_thread"] = "stale-student-dm"
            pushed = push_outbox(chamber, {spec.key: chan}, state, BridgeConfig(),
                                 specs=[spec])
            self.assertEqual(pushed, 1)
            # proactive message: routed to the configured default, NOT a stale
            # last_thread that could be a student's private inbox
            self.assertEqual(chan._sent[0][0].thread, "reports")
            self.assertIn("Hello agent reply", chan._sent[0][1])
            self.assertEqual(len(chan._sent[0][2]), 32)
            self.assertTrue((outbox / "archive" / "2026-08-14T10-00-00_reply.md").exists())

    def test_stale_dm_thread_never_leaks_without_default(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            chan = MockChannel(BOT)
            spec = ChannelSpec(name="dms", platform="mock", dm=True)
            outbox = chamber / "messages" / "outbox"
            outbox.mkdir(parents=True)
            (outbox / "reply.md").write_text("---\n---\n\ninternal status")
            state = load_state(chamber)
            cs = channel_state(state, "mock:dms")
            cs["last_thread"] = "student@example.com"  # stale DM route
            pushed = push_outbox(chamber, {spec.key: chan}, state, BridgeConfig(),
                                 specs=[spec])
            self.assertEqual(pushed, 0)  # quarantined, never sent to the student
            self.assertEqual(chan._sent, [])
            self.assertTrue(list((outbox / "failed").glob("*.md")))

    def test_active_route_wins_over_newer_last_active_state(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            first = MockChannel(BOT)
            second = MockChannel(BOT)
            outbox = chamber / "messages" / "outbox"
            outbox.mkdir(parents=True)
            (outbox / "reply.md").write_text("---\n---\n\nanswer")
            state = load_state(chamber)
            state["active_route"] = {
                "channel_key": "mock:first", "thread": "original", "parent_id": None,
            }
            state["last_active"] = "mock:second"
            channel_state(state, "mock:first")["last_thread"] = "old"
            channel_state(state, "mock:second")["last_thread"] = "newer"
            pushed = push_outbox(
                chamber, {"mock:first": first, "mock:second": second},
                state, BridgeConfig(),
            )
            self.assertEqual(pushed, 1)
            self.assertEqual(first._sent[0][0].thread, "original")
            self.assertEqual(second._sent, [])

    def test_failed_push_reports_error_without_archiving(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            outbox = chamber / "messages" / "outbox"
            outbox.mkdir(parents=True)
            message = outbox / "reply.md"
            message.write_text("---\n---\n\nanswer")
            errors = []
            pushed = push_outbox(chamber, {}, load_state(chamber), BridgeConfig(), errors)
            self.assertEqual(pushed, 0)
            self.assertEqual(len(errors), 1)
            self.assertTrue(message.exists())

    def test_inbox_frontmatter_roundtrip(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            m = msg("99", "@**flash** hi", sender="user1", thread="my-topic",
                    mentioned=True, mentioned_ids=["bot1"])
            path = deliver_to_inbox(chamber, "zulip", m, [])
            meta, body = parse_frontmatter(path.read_text())
            self.assertEqual(meta["platform"], "zulip")
            self.assertEqual(meta["platform_message_id"], "99")
            self.assertEqual(meta["thread"], "my-topic")
            self.assertEqual(meta["mentioned"], "true")
            self.assertEqual(body, "@**flash** hi")


class ReplyToBotTests(unittest.TestCase):
    def test_reply_to_bot_message_is_directed(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            chan = MockChannel(BOT, {"31": msg("31", "thanks, one more thing",
                                               parent="sent-9")})
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            cs = channel_state(state, spec.key)
            cs["cursor"] = "30"
            cs["sent_ids"] = ["sent-9"]
            pull_once(chamber, chan, spec, BridgeConfig(), state, BOT, chamber / "att")
            inbox = list((chamber / "messages" / "inbox").glob("*.md"))
            self.assertEqual(len(inbox), 1)
            self.assertIn("thanks, one more thing", inbox[0].read_text())
            self.assertEqual(state["stats"]["delivered"], 1)

    def test_reply_to_bot_respects_reply_in_thread_off(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            chan = MockChannel(BOT, {"31": msg("31", "thanks, one more thing",
                                               parent="sent-9")})
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            cs = channel_state(state, spec.key)
            cs["cursor"] = "30"
            cs["sent_ids"] = ["sent-9"]
            pull_once(chamber, chan, spec, BridgeConfig(reply_in_thread=False),
                      state, BOT, chamber / "att")
            self.assertFalse((chamber / "messages" / "inbox").exists())
            self.assertEqual(
                len(channel_state(state, spec.key)["pending_context"]), 1
            )


class SyncOnceTests(unittest.TestCase):
    def test_active_route_always_defers_agent_facing_pull(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            messages = {"31": msg("31", "@**flash** ping", sender="u1",
                                 mentioned_ids=["bot1"])}
            chan = MockChannel(BOT, messages=messages)
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            state["channels"]["mock:main"] = {"cursor": "30", "last_thread": None,
                                              "delivered": [], "sent_ids": []}
            state["active_route"] = {
                "channel_key": "mock:main", "thread": "t",
                "parent_id": None,
            }
            sync_once(chamber, {"mock:main": chan}, [spec], BridgeConfig(), state,
                      {"mock:main": BOT})
            inbox = list((chamber / "messages" / "inbox").glob("*.md"))
            self.assertEqual(len(inbox), 0)  # still deferred
            self.assertIn("active_route", state)

    def test_full_cycle(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            messages = {"30": msg("30", "@**flash** ping", sender="u1", mentioned_ids=["bot1"])}
            chan = MockChannel(BOT, messages=messages)
            spec = ChannelSpec(name="main", platform="mock")
            state = load_state(chamber)
            state["channels"]["mock:main"] = {"cursor": "29", "last_thread": None,
                                              "delivered": [], "sent_ids": []}
            sync_once(chamber, {"mock:main": chan}, [spec], BridgeConfig(), state,
                      {"mock:main": BOT})
            inbox = list((chamber / "messages" / "inbox").glob("*.md"))
            self.assertEqual(len(inbox), 1)
            self.assertEqual(state["stats"]["delivered"], 1)
            # second cycle: nothing new, nothing delivered
            sync_once(chamber, {"mock:main": chan}, [spec], BridgeConfig(), state,
                      {"mock:main": BOT})
            self.assertEqual(len(list((chamber / "messages" / "inbox").glob("*.md"))), 1)

    @patch("chat_bridge.cli._make_channels")
    def test_one_shot_pull_refuses_while_a_reply_route_is_active(self, make_channels):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            state = load_state(chamber)
            state["active_route"] = {
                "channel_key": "mock:main", "thread": "topic", "parent_id": None,
            }
            save_state(chamber, state)
            chan = MockChannel(BOT)
            spec = ChannelSpec(name="main", platform="mock")
            make_channels.return_value = ({spec.key: chan}, [spec], {spec.key: BOT})
            with self.assertRaisesRegex(ChannelError, "reply is still pending"):
                cmd_pull(SimpleNamespace(chamber=str(chamber)))

    @patch("chat_bridge.cli._make_channels")
    def test_one_shot_pull_keeps_dropbox_active_while_reply_is_pending(
            self, make_channels):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            state = load_state(chamber)
            state["active_route"] = {
                "channel_key": "mock:teaching", "thread": "reports",
                "parent_id": None,
            }
            spec = ChannelSpec(name="dms", platform="mock", dm=True,
                               auto_reply="received")
            channel_state(state, spec.key)["cursor"] = "70"
            save_state(chamber, state)
            chan = MockChannel(BOT, {
                "71": msg("71", "paper", sender="alice@example.com",
                          thread="alice@example.com"),
            })
            make_channels.return_value = ({spec.key: chan}, [spec], {spec.key: BOT})

            self.assertEqual(cmd_pull(SimpleNamespace(chamber=str(chamber))), 0)
            self.assertEqual(chan._sent[0][0].thread, "alice@example.com")
            saved = load_state(chamber)
            self.assertEqual(saved["active_route"]["thread"], "reports")

    def test_routeless_proactive_outbox_falls_back_and_never_wedges_pull(self):
        # Bootstrap wake: the chamber writes an outbox message before any chat
        # message ever arrived. It must go out on the channel's configured
        # route, and the pull phase must still run in the same cycle.
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            outbox = chamber / "messages" / "outbox"
            outbox.mkdir(parents=True)
            (outbox / "hello.md").write_text("---\n---\n\nproactive status")
            spec = ChannelSpec(name="main", platform="mock", topic="lobby")
            chan = MockChannel(BOT, {"1": msg("1", "@flash hi")})
            state = load_state(chamber)
            channel_state(state, spec.key)["cursor"] = "0"
            sync_once(chamber, {spec.key: chan}, [spec], BridgeConfig(), state,
                      {spec.key: BOT})
            self.assertEqual(chan._sent[0][0].thread, "lobby")
            self.assertTrue((outbox / "archive" / "hello.md").exists())
            self.assertEqual(
                len(list((chamber / "messages" / "inbox").glob("*.md"))), 1
            )

    def test_unroutable_outbox_is_quarantined_and_pull_still_runs(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            outbox = chamber / "messages" / "outbox"
            outbox.mkdir(parents=True)
            (outbox / "hello.md").write_text("---\n---\n\nproactive status")
            spec = ChannelSpec(name="main", platform="mock")  # no default route
            chan = MockChannel(BOT, {"1": msg("1", "@flash hi")})
            state = load_state(chamber)
            channel_state(state, spec.key)["cursor"] = "0"
            sync_once(chamber, {spec.key: chan}, [spec], BridgeConfig(), state,
                      {spec.key: BOT})
            self.assertTrue((outbox / "failed" / "hello.md").exists())
            self.assertEqual(
                len(list((chamber / "messages" / "inbox").glob("*.md"))), 1
            )

    def test_round_robin_polls_next_channel_after_reply(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            first_spec = ChannelSpec(name="first", platform="mock")
            second_spec = ChannelSpec(name="second", platform="mock")
            first = MockChannel(BOT, {"1": msg("1", "@flash hi")})
            second = MockChannel(BOT, {"1": msg("1", "@flash hi")})
            channels = {first_spec.key: first, second_spec.key: second}
            bots = {first_spec.key: BOT, second_spec.key: BOT}
            state = load_state(chamber)
            channel_state(state, first_spec.key)["cursor"] = "0"
            channel_state(state, second_spec.key)["cursor"] = "0"
            cfg = BridgeConfig()

            sync_once(chamber, channels, [first_spec, second_spec], cfg, state, bots)
            self.assertEqual(state["active_route"]["channel_key"], first_spec.key)
            outbox = chamber / "messages" / "outbox"
            outbox.mkdir(parents=True, exist_ok=True)
            (outbox / "reply.md").write_text("---\n---\n\nreply")
            sync_once(chamber, channels, [first_spec, second_spec], cfg, state, bots)
            self.assertEqual(state["active_route"]["channel_key"], second_spec.key)


if __name__ == "__main__":
    unittest.main()
