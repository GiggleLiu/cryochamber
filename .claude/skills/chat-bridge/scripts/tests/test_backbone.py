"""Backbone unit tests — mock channel, no network.

Run: python3 -m unittest discover -s tests -v
"""

from __future__ import annotations

import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

from chat_bridge.backbone import (
    BridgeConfig,
    ChannelSpec,
    channel_state,
    deliver_to_inbox,
    is_directed,
    load_state,
    parse_frontmatter,
    pull_once,
    push_outbox,
    sender_allowed,
    sync_once,
)
from chat_bridge.channel import (
    BotIdentity,
    Channel,
    FetchResult,
    Message,
)


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

    def fetch_new(self, cursor, limit=1000):
        if cursor is None:
            # initialization anchor
            ids = [int(k) for k in self._messages]
            return FetchResult(messages=[], cursor=str(max(ids)) if ids else "0", done=True)
        newest = int(cursor or 0)
        out = [m for k, m in sorted(self._messages.items()) if int(k) > newest]
        self._messages = {k: m for k, m in self._messages.items() if int(k) > newest}
        return FetchResult(messages=out, cursor=str(max(int(k) for k in self._messages)) if self._messages else str(newest), done=True)

    def send(self, target, content):
        self._sent.append((target, content))
        return f"sent-{len(self._sent)}"


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
    def test_routes_via_last_active_and_archives(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            chan = MockChannel(BOT)
            outbox = chamber / "messages" / "outbox"
            outbox.mkdir(parents=True)
            (outbox / "2026-08-14T10-00-00_reply.md").write_text(
                "---\nsubject: Reply\n---\n\nHello agent reply"
            )
            state = load_state(chamber)
            state["last_active"] = "mock:main"
            cs = channel_state(state, "mock:main")
            cs["last_thread"] = "general"
            pushed = push_outbox(chamber, {"mock:main": chan}, state, BridgeConfig())
            self.assertEqual(pushed, 1)
            self.assertEqual(chan._sent[0][0].thread, "general")
            self.assertIn("Hello agent reply", chan._sent[0][1])
            self.assertTrue((outbox / "archive" / "2026-08-14T10-00-00_reply.md").exists())

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


class SyncOnceTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
