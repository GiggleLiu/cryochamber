#!/bin/sh
cryo-agent alert email ops@test.com "Watchdog set"
cryo-agent hibernate --complete
