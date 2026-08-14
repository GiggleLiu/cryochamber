"""chat_bridge — unified cryochamber ↔ chat platform bridge.

One backbone, pluggable channel adapters. Supported platforms:
  - Zulip   (REST + real-time events queue)
  - Feishu / Lark (lark-cli event stream)

Shared contract with cryochamber:
  - inbound  chat messages  -> messages/inbox/   (wakes the agent)
  - outbound agent replies  -> messages/outbox/  (posted back to chat)

Design: every platform is a `Channel` implementing the same protocol
(fetch_new / send / profile / upload / download). All policy — trigger
gate, anti-echo, dedupe, reply routing, whitelist, stats, services — lives
in `backbone.py` and is platform-agnostic.
"""

__version__ = "0.1.0"
