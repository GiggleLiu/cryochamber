#!/bin/sh
# Mock agent: completes the plan on first session.
# Used by inbox-triggered wake tests to verify session count.

cryo-agent note "Inbox wake session"
cryo-agent hibernate --complete --summary "Handled inbox wake"
