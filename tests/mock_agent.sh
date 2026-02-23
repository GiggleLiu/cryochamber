#!/bin/sh
# Mock agent for cryochamber integration tests.
# Outputs the content of MOCK_AGENT_OUTPUT env var.
# Ignores --prompt and other arguments.
echo "${MOCK_AGENT_OUTPUT:-[CRYO:EXIT 0] mock done}"
