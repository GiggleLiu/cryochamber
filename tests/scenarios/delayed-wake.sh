#!/bin/sh
# Mock agent: session 1 hibernates with a far-past wake time via TODO to trigger
# delayed wake detection. Session 2 (the delayed wake) completes the plan.
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

echo "Mock agent session $COUNT (delayed-wake scenario)"

if [ "$COUNT" -ge 2 ]; then
    cryo-agent hibernate --complete --summary "Plan completed after delayed wake"
else
    # Add a TODO with a wake time 10 minutes in the past to trigger delayed wake detection.
    # The daemon will immediately wake and detect the delay.
    PAST_WAKE=$(date -u -d '10 minutes ago' +%Y-%m-%dT%H:%M 2>/dev/null || date -u -v-10M +%Y-%m-%dT%H:%M 2>/dev/null)
    cryo-agent todo add "past wake" --at "$PAST_WAKE"
    cryo-agent hibernate --summary "Session $COUNT done, testing delayed wake"
fi
