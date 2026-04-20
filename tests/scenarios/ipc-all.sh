#!/bin/sh
# Mock agent: calls send, alert, then hibernates complete.
# Tests: all IPC command handling in the daemon socket server.

cryo-agent send "Status update for operator"
cryo-agent alert notify desktop "Check on mock agent"
cryo-agent hibernate --complete --summary "IPC test passed"
