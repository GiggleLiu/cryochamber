#!/bin/sh
# Agent sends hibernate (not complete), then immediately crashes with exit 1.
# Since the hibernate was accepted, the daemon should use the hibernate outcome
# despite the non-zero exit code.
cryo-agent hibernate --summary "about to crash"
exit 1
