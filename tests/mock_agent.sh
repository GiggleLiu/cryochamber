#!/bin/sh
# Mock agent for cryochamber integration tests.
# Uses cryo CLI commands to communicate with the daemon.
# CRYO_BIN must be set to the path of the cryo binary.

# Optionally echo something to stdout (still the agent's output, just not parsed by daemon)
echo "${MOCK_AGENT_OUTPUT:-Agent running}"

# Leave a note if requested
if [ -n "$MOCK_AGENT_NOTE" ]; then
    "$CRYO_BIN" note "$MOCK_AGENT_NOTE" 2>/dev/null || true
fi

# Default: hibernate --complete
if [ "$MOCK_AGENT_COMPLETE" = "false" ] && [ -n "$MOCK_AGENT_WAKE" ]; then
    "$CRYO_BIN" hibernate --wake "$MOCK_AGENT_WAKE" --summary "${MOCK_AGENT_SUMMARY:-continuing}" 2>/dev/null || true
else
    "$CRYO_BIN" hibernate --complete --summary "${MOCK_AGENT_SUMMARY:-mock done}" 2>/dev/null || true
fi
