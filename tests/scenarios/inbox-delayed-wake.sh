#!/bin/sh
# Mock agent: session 1 hibernates with a far-past wake time, sleeping briefly
# to allow the test to write an inbox file during the session. Session 2 verifies
# that inbox-triggered wake suppresses delayed wake detection.

COUNTER_FILE=".mock-session-count"

if [ -f "$COUNTER_FILE" ]; then
    COUNT=$(cat "$COUNTER_FILE")
else
    COUNT=0
fi

COUNT=$((COUNT + 1))
echo "$COUNT" > "$COUNTER_FILE"

if [ "$COUNT" -ge 2 ]; then
    cryo-agent note "Session $COUNT: completing after inbox wake"
    cryo-agent hibernate --complete --summary "Plan completed after inbox wake"
else
    # Hibernate with a wake time 10 minutes in the past.
    PAST_WAKE=$(date -u -d '10 minutes ago' +%Y-%m-%dT%H:%M 2>/dev/null || date -u -v-10M +%Y-%m-%dT%H:%M 2>/dev/null)
    cryo-agent hibernate --wake "$PAST_WAKE" --summary "Session $COUNT done"
    # Sleep after hibernate so the test can write an inbox file while this session
    # is still running. The InboxChanged event queues before the daemon's event loop
    # resumes, ensuring it takes priority over the timeout.
    sleep 2
fi
