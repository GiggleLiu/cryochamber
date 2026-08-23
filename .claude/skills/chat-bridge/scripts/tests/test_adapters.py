"""Adapter contract tests that do not require network credentials."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from chat_bridge.backbone import ChannelSpec
from chat_bridge.channel import Attachment, ReplyTarget, resolve_local_link
from chat_bridge.lark import LarkChannel
from chat_bridge.zulip import ZulipChannel


class LocalLinkTests(unittest.TestCase):
    def test_resolves_only_safe_chamber_files(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            public = chamber / "report.txt"
            public.write_text("ok")
            secret = chamber / ".cryo" / "zuliprc"
            secret.parent.mkdir()
            secret.write_text("secret")
            self.assertEqual(resolve_local_link(chamber, "report.txt"), public.resolve())
            self.assertIsNone(resolve_local_link(chamber, ".cryo/zuliprc"))
            self.assertIsNone(resolve_local_link(chamber, "../outside.txt"))
            self.assertIsNone(resolve_local_link(chamber, "https://example.com/a"))


class LarkChannelTests(unittest.TestCase):
    def test_filters_configured_chat_and_type(self):
        with tempfile.TemporaryDirectory() as td:
            spec = ChannelSpec(name="main", platform="lark", chat_id="oc_keep",
                               chat_type="group")
            chan = LarkChannel(spec, Path(td))
            base = {
                "sender_id": "ou_user", "message_id": "om_1", "content": "hello",
                "create_time": "1000", "chat_type": "group",
            }
            self.assertIsNone(chan._event_to_message({**base, "chat_id": "oc_other"}))
            self.assertIsNone(chan._event_to_message({**base, "chat_id": "oc_keep",
                                                       "chat_type": "p2p"}))
            self.assertIsNotNone(chan._event_to_message({**base, "chat_id": "oc_keep"}))

    def test_reply_events_carry_parent_id(self):
        with tempfile.TemporaryDirectory() as td:
            spec = ChannelSpec(name="main", platform="lark", chat_id="oc_keep",
                               chat_type="p2p")
            chan = LarkChannel(spec, Path(td))
            base = {
                "sender_id": "ou_user", "message_id": "om_2", "content": "follow-up",
                "create_time": "1000", "chat_type": "p2p", "chat_id": "oc_keep",
            }
            self.assertEqual(
                chan._event_to_message({**base, "parent_id": "om_bot"}).parent_id,
                "om_bot",
            )
            self.assertEqual(
                chan._event_to_message(
                    {**base, "message": {"root_id": "om_root"}}
                ).parent_id,
                "om_root",
            )
            self.assertIsNone(chan._event_to_message(base).parent_id)

    def test_keeps_distinct_events_with_same_millisecond_timestamp(self):
        class RunningProcess:
            @staticmethod
            def poll():
                return None

        with tempfile.TemporaryDirectory() as td:
            chan = LarkChannel(ChannelSpec(name="main", platform="lark"), Path(td))
            chan._proc = RunningProcess()
            for index in (1, 2):
                chan._queue.put({
                    "sender_id": "ou_user", "message_id": f"om_{index}",
                    "content": "hello", "create_time": "1000",
                    "timestamp": "1000", "chat_id": "oc_chat", "chat_type": "p2p",
                })
            result = chan.fetch_new("1000")
            self.assertEqual([message.id for message in result.messages], ["om_1", "om_2"])

    @patch("chat_bridge.lark._run_cli")
    def test_download_uses_chamber_relative_output(self, run_cli):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            dest = chamber / "messages" / "attachments"
            spec = ChannelSpec(name="main", platform="lark")
            chan = LarkChannel(spec, chamber)

            def create_download(args, **kwargs):
                output = args[args.index("--output") + 1]
                path = kwargs["cwd"] / output
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"image")
                return {"ok": True}

            run_cli.side_effect = create_download
            out = chan.download(Attachment("img_1", "image", "image",
                                           meta={"message_id": "om_1"}), dest)
            self.assertEqual(out.resolve(), dest.resolve() / "img_1")
            args, kwargs = run_cli.call_args
            self.assertEqual(kwargs["cwd"], chamber.resolve())
            self.assertEqual(args[0][args[0].index("--output") + 1],
                             "messages/attachments/img_1")

    @patch("chat_bridge.lark._run_cli")
    def test_send_uploads_local_attachment_from_chamber(self, run_cli):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            attachment = chamber / "result.txt"
            attachment.write_text("result")
            run_cli.side_effect = [
                {"data": {"message_id": "om_text"}},
                {"data": {"message_id": "om_file"}},
            ]
            chan = LarkChannel(ChannelSpec(name="main", platform="lark"), chamber)
            sent = chan.send(ReplyTarget("oc_chat"),
                             "Done\n\n[result](result.txt)",
                             idempotency_key="batch-key")
            self.assertEqual(sent, "om_file")
            text_args = run_cli.call_args_list[0].args[0]
            file_args = run_cli.call_args_list[1].args[0]
            self.assertIn("--markdown", text_args)
            self.assertNotIn("result.txt", text_args[text_args.index("--markdown") + 1])
            self.assertEqual(file_args[file_args.index("--file") + 1], "result.txt")
            self.assertEqual(
                text_args[text_args.index("--idempotency-key") + 1], "batch-key-0"
            )
            self.assertEqual(
                file_args[file_args.index("--idempotency-key") + 1], "batch-key-1"
            )


class ZulipChannelTests(unittest.TestCase):
    def _channel(self, td, **kw):
        zuliprc = Path(td) / "zuliprc"
        zuliprc.write_text("[api]\nsite=https://zulip.example\nemail=bot@example.com\nkey=x\n")
        spec = ChannelSpec(name="main", platform="zulip", **kw)
        return ZulipChannel(spec, Path(td), zuliprc, "poll")

    def test_event_queue_narrow_includes_configured_topic(self):
        with tempfile.TemporaryDirectory() as td:
            chan = self._channel(td, stream="research", stream_id=7, topic="decoder")
            self.assertEqual(
                chan._register_narrow(),
                '[["stream", "research"], ["topic", "decoder"]]',
            )

    def test_dm_narrow_is_private(self):
        with tempfile.TemporaryDirectory() as td:
            chan = self._channel(td, dm=True)
            self.assertEqual(chan._register_narrow(), '[["is", "private"]]')

    @patch("chat_bridge.zulip.ZulipChannel.request")
    def test_dm_poll_narrow_is_private(self, request):
        request.return_value = {"messages": [], "found_newest": True}
        with tempfile.TemporaryDirectory() as td:
            chan = self._channel(td, dm=True)
            chan._fetch_poll("5", 1000)
            params = request.call_args.kwargs["params"]
            self.assertEqual(params["narrow"], '[{"operator": "is", "operand": "private"}]')

    @patch("chat_bridge.zulip.ZulipChannel.request")
    def test_dm_poll_limit_preserves_the_next_senders_message(self, request):
        request.return_value = {
            "messages": [
                {"id": 6, "timestamp": 0, "type": "private",
                 "sender_email": "alice@example.com", "content": "first",
                 "display_recipient": [{"id": 1}, {"id": 2}]},
                {"id": 7, "timestamp": 0, "type": "private",
                 "sender_email": "bob@example.com", "content": "second",
                 "display_recipient": [{"id": 1}, {"id": 3}]},
            ],
            "found_newest": True,
        }
        with tempfile.TemporaryDirectory() as td:
            chan = self._channel(td, dm=True)
            result = chan._fetch_poll("5", 1)
            self.assertEqual([m.id for m in result.messages], ["6"])
            self.assertEqual(result.cursor, "6")
            self.assertFalse(result.done)

    def test_dm_message_thread_is_the_sender(self):
        with tempfile.TemporaryDirectory() as td:
            chan = self._channel(td, dm=True)
            m = chan._to_message({
                "id": 42, "timestamp": 0, "sender_email": "alice@example.com",
                "sender_id": 9, "sender_full_name": "Alice", "content": "hi",
            })
            self.assertEqual(m.thread, "alice@example.com")
            self.assertEqual(m.thread_name, "alice@example.com")

    def test_stream_message_thread_is_the_subject(self):
        with tempfile.TemporaryDirectory() as td:
            chan = self._channel(td, stream="research", stream_id=7)
            m = chan._to_message({
                "id": 1, "timestamp": 0, "sender_email": "bob@example.com",
                "subject": "decoder", "content": "hi",
            })
            self.assertEqual(m.thread, "decoder")

    def test_group_dm_is_filtered_in_dm_mode(self):
        with tempfile.TemporaryDirectory() as td:
            chan = self._channel(td, dm=True)
            group = {"id": 2, "timestamp": 0, "type": "private",
                     "sender_email": "alice@example.com", "content": "hi all",
                     "display_recipient": [{"id": 1}, {"id": 2}, {"id": 3}]}
            self.assertTrue(chan._is_group_dm(group))
            one2one = {"id": 3, "timestamp": 0, "type": "private",
                       "sender_email": "alice@example.com", "content": "hi",
                       "display_recipient": [{"id": 1}, {"id": 2}]}
            self.assertFalse(chan._is_group_dm(one2one))
            stream = {"id": 4, "timestamp": 0, "type": "stream",
                      "sender_email": "alice@example.com", "content": "hi"}
            self.assertFalse(chan._is_group_dm(stream))

    @patch("chat_bridge.zulip.ZulipChannel.request")
    def test_dm_send_uses_private_type(self, request):
        request.return_value = {"id": "99"}
        with tempfile.TemporaryDirectory() as td:
            chan = self._channel(td, dm=True)
            chan.send(ReplyTarget("alice@example.com"), "ack")
            data = request.call_args.kwargs["data"]
            self.assertEqual(data["type"], "private")
            # Zulip requires a JSON-encoded array on the wire; urlencode of a
            # Python list would emit a Python repr (['alice@...']) that the
            # server rejects.
            self.assertEqual(data["to"], '["alice@example.com"]')
            import urllib.parse
            wire = urllib.parse.urlencode(data)
            # urlencoded JSON array of the email
            self.assertIn("to=%5B%22alice%40example.com%22%5D", wire)
            import json as _json
            self.assertEqual(_json.loads(data["to"]), ["alice@example.com"])

    @patch("chat_bridge.zulip.ZulipChannel.request")
    def test_stream_send_uses_stream_type(self, request):
        request.return_value = {"id": "99"}
        with tempfile.TemporaryDirectory() as td:
            chan = self._channel(td, stream="research", stream_id=7, topic="decoder")
            chan.send(ReplyTarget("decoder"), "report")
            data = request.call_args.kwargs["data"]
            self.assertEqual(data["type"], "stream")
            self.assertEqual(data["topic"], "decoder")


if __name__ == "__main__":
    unittest.main()
