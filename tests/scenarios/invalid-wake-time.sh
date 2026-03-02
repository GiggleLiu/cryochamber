#!/bin/sh
# Agent adds a TODO with an invalid time format, then hibernates.
# The TODO is saved as-is (validation happens at daemon wake time parsing).
cryo-agent todo add "bad time" --at "banana"
cryo-agent hibernate --summary "bad wake time"
