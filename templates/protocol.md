# Cryochamber Protocol

You are running inside **cryochamber**, a long-term AI task scheduler.
You wake up, do work, then hibernate until the next session.

## Session Workflow

Every session, follow these steps in order:

1. **Read `plan.md`** — your objectives and task list.
2. **Check your prompt** for new messages and previous session log.
3. **Do the work** described in your task.
4. **Leave notes** for your future self: `cryo-agent note "what I did and what's next"`
5. **Hibernate** — either schedule the next wake or mark the plan complete.

## How to Hibernate

You MUST call one of these before your session ends. If you don't, the daemon treats it as a crash.

### Schedule next wake (more work to do)

```
# 1. Compute a future time
make time OFFSET="+30 minutes"    # prints e.g. 2026-02-24T15:30

# 2. Hibernate with that time
cryo-agent hibernate --wake 2026-02-24T15:30 --summary "Finished task 2, task 3 next"
```

### Mark plan complete (all done)

```
cryo-agent hibernate --complete --summary "All tasks finished"
```

### Report partial progress or failure

```
# Partial progress (exit 1) — daemon will wake you again
cryo-agent hibernate --wake 2026-02-25T09:00 --exit 1 --summary "Blocked on API access"

# Failure (exit 2)
cryo-agent hibernate --wake 2026-02-25T09:00 --exit 2 --summary "Build broken, needs human help"
```

## Deciding When to Wake

- **Waiting on external event** (CI, review, deploy): wake in 15-30 minutes to check.
- **Multi-step plan, next step ready**: wake in 1-2 minutes (just long enough to save context).
- **Time-sensitive deadline**: compute exact time with `make time OFFSET="+2 hours"`.
- **Nothing to do until tomorrow**: `make time OFFSET="+1 day"`.

## Other Commands

```
cryo-agent note "text"                        # Leave a note for next session
cryo-agent reply "message"                    # Send message to human (outbox)
cryo-agent alert <action> <target> "message"  # Dead-man switch (fires if you don't wake on time)
```

## Time Utility

Always use `make time` for timestamps — never guess or hardcode times:

```
make time                          # current time in ISO8601
make time OFFSET="+1 day"         # 1 day from now
make time OFFSET="+2 hours"       # 2 hours from now
make time OFFSET="+30 minutes"    # 30 minutes from now
```

## Key Facts

- **Inbox messages wake you early.** Humans can send messages. You'll see them in your prompt.
- **Notes survive across sessions.** Use `cryo-agent note` liberally — it's your memory.
- **No hibernate = crash.** If you exit without calling `cryo-agent hibernate`, the daemon retries with backoff.
- **Delayed wakes happen.** If the machine was suspended, you'll see a system notice in your prompt. Adjust accordingly.
