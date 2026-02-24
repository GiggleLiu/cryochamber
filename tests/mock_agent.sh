#!/bin/sh
# Mock agent for cryochamber integration tests.
# Outputs the content of MOCK_AGENT_OUTPUT env var.
# If CRYO_BIN is set, uses it to call `cryo hibernate --complete` via socket.
# Otherwise just echoes markers to stdout (legacy mode).
echo "${MOCK_AGENT_OUTPUT:-[CRYO:EXIT 0] mock done}"

if [ -n "$CRYO_BIN" ]; then
    # Socket-based flow: tell the daemon we're done
    "$CRYO_BIN" hibernate --complete --summary "mock done" 2>/dev/null || true
fi
