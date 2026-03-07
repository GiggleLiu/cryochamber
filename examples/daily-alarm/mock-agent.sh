#!/bin/bash
# Mock agent that uses --daily flag

echo "Current time: $(cryo-agent time)"
NEXT_WAKE=$(cryo-agent time --daily 13:00)
echo "Next wake: $NEXT_WAKE"

cryo-agent note "Session completed at $(cryo-agent time)"
cryo-agent todo add "Daily greeting" --at "$NEXT_WAKE"
cryo-agent hibernate --summary "Scheduled next wake at $NEXT_WAKE"
