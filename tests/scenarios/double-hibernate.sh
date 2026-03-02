#!/bin/sh
# First hibernate (not complete) — daemon accepts it
cryo-agent hibernate --summary "first hibernate"
# Second hibernate (complete) — overrides the first
cryo-agent hibernate --complete
