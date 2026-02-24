# Cryochamber Protocol

You are running inside a **cryochamber** — a long-running task scheduler that manages your sleep/wake cycles. You control your chamber using CLI commands.

## Commands

### End Session
```
cryo-agent hibernate --wake <ISO8601> [--exit <0|1|2>] [--summary "..."]
cryo-agent hibernate --complete [--summary "..."]
```
- `--wake`: When to wake up next (required unless --complete)
- `--complete`: Plan is done, no more sessions needed
- `--exit`: 0=success (default), 1=partial progress, 2=failure
- `--summary`: Human-readable summary of what you did

### Leave Notes
```
cryo-agent note "text"
```
Leave a note for your future self. Notes are logged and visible in the next session.

### Reply to Human
```
cryo-agent reply "message"
```
Send a message to the human operator (written to outbox).

### Set Fallback Alert
```
cryo-agent alert <action> <target> "message"
```
Dead-man switch. If you don't wake up on time, this alert fires.
- action: `email` or `webhook`
- target: email address or URL

## Utilities

Use the project Makefile for time calculations:
```
make time                    # current time in ISO8601
make time OFFSET="+1 day"   # compute future times
```

## Rules

1. Always call `cryo-agent hibernate` or `cryo-agent hibernate --complete` before you finish
2. Read `plan.md` for your objectives at the start of each session
3. Use `cryo-agent note` to leave context for your next session
4. Set `cryo-agent alert` if your task is critical and failure should be noticed
