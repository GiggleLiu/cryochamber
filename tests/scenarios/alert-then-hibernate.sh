#!/bin/sh
cryo-agent alert email ops@test.com "Watchdog set"
cryo-agent todo add "scheduled wake" --at "$TEST_WAKE_AT"
cryo-agent hibernate --summary "Waiting for next wake"
