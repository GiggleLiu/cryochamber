#!/bin/sh
# Mock agent: sleeps forever, never calls hibernate.
# Tests: session timeout enforcement, agent kill.
echo "Agent will sleep forever..."
sleep 99999
