#!/bin/sh
echo "$MOCK_VAR" > .env-check
cryo-agent hibernate --complete
