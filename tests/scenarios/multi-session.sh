#!/bin/sh
# Mock agent: hibernates with short wake, completes on session 3.
# Tests: multi-session lifecycle through plan completion.
# Uses a counter file to track sessions across invocations.

COUNTER_FILE=".mock-session-count"

# Read or initialize counter
if [ -f "$COUNTER_FILE" ]; then
    COUNT=$(cat "$COUNTER_FILE")
else
    COUNT=0
fi

COUNT=$((COUNT + 1))
echo "$COUNT" > "$COUNTER_FILE"

echo "Mock agent session $COUNT"

if [ "$COUNT" -ge 3 ]; then
    cryo-agent note "Session $COUNT: completing plan"
    cryo-agent hibernate --complete --summary "Plan completed after $COUNT sessions"
else
    WAKE=$(cryo-agent time "+2 seconds")
    cryo-agent note "Session $COUNT: work in progress"
    cryo-agent hibernate --wake "$WAKE" --summary "Session $COUNT done, more to do"
fi
