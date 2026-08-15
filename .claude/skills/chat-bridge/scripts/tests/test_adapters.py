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
    def test_event_queue_narrow_includes_configured_topic(self):
        with tempfile.TemporaryDirectory() as td:
            chamber = Path(td)
            zuliprc = chamber / "zuliprc"
            zuliprc.write_text("[api]\nsite=https://zulip.example\nemail=bot@example.com\nkey=x\n")
            spec = ChannelSpec(name="main", platform="zulip", stream="research",
                               stream_id=7, topic="decoder")
            chan = ZulipChannel(spec, chamber, zuliprc, "events")
            self.assertEqual(
                chan._register_narrow(),
                '[["stream", "research"], ["topic", "decoder"]]',
            )


if __name__ == "__main__":
    unittest.main()
