#!/bin/sh
nohup sleep 10 >/dev/null 2>&1 &
cryo-agent hibernate --complete
