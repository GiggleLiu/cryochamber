#!/bin/sh
# Session 1 reports a retryable failure via `cryo-agent hibernate --exit 1`.
# Session 2 completes successfully after the daemon retries.

COUNTER_FILE=".mock-hibernate-fail-count"

if [ -f "$COUNTER_FILE" ]; then
    COUNT=$(cat "$COUNTER_FILE")
else
    COUNT=0
fi

COUNT=$((COUNT + 1))
echo "$COUNT" > "$COUNTER_FILE"

if [ "$COUNT" -ge 2 ]; then
    cryo-agent hibernate --complete --summary "Recovered on attempt $COUNT"
else
    cryo-agent hibernate --exit 1 --summary "Retryable failure on attempt $COUNT"
fi
