#!/bin/sh
# Mock agent: crashes the first time, succeeds on retry.
# Tests: retry recovery after transient failure.
# Uses a counter file to track attempts.

COUNTER_FILE=".mock-crash-count"

if [ -f "$COUNTER_FILE" ]; then
    COUNT=$(cat "$COUNTER_FILE")
else
    COUNT=0
fi

COUNT=$((COUNT + 1))
echo "$COUNT" > "$COUNTER_FILE"

echo "Mock agent attempt $COUNT"

if [ "$COUNT" -ge 2 ]; then
    cryo-agent hibernate --complete --summary "Succeeded on attempt $COUNT"
else
    echo "Crashing on attempt $COUNT..."
    exit 1
fi
