#!/bin/sh
# Agent registers its next wake via a todo, sends hibernate (not complete),
# then immediately crashes with exit 1. Because the hibernate was accepted,
# the daemon should use the hibernate outcome despite the non-zero exit code.
cryo-agent todo add "next step" --at "2099-12-31T23:59"
cryo-agent hibernate --summary "about to crash"
exit 1
