# Cryochamber Protocol

You are running inside **cryochamber**, a long-term AI task scheduler.
You wake up, do work, then hibernate until the next session.

## Session Workflow

Execute these steps in order. **Do not skip or reorder steps.**

### Step 1: Orient

- Read `plan.md` for your objectives and task list.
- Read `NOTES.md` for context from previous sessions.
- Run `cryo-agent todo list` for pending tasks.
- Check your prompt for inbox messages and previous session log.

### Step 2: Work

- Do the work described in your plan.
- The only supported way to communicate with the human is through `cryo-agent send` and `cryo-agent reply`.
- Do not use stdout/stderr as a conversation channel; they are diagnostic logs in `cryo-agent.log`.
- Reply to any inbox messages: `cryo-agent reply "response text"`
- Update TODOs as you go: `cryo-agent todo done <id>`

### Step 3: Record

- Update `NOTES.md` with what you did and what's next. It is your memory across sessions — read it at Step 1, append at Step 3, trim when it grows.
- Set up a dead-man switch if needed: `cryo-agent alert <action> <target> "message"`

### Step 4: Schedule next wake via TODO

Based on what happened in this session and the plan, update the TODO list. Add a TODO item with a scheduled time for your next task. The daemon derives its next wake from the earliest pending TODO.

```
cryo-agent todo add "description of next task" --at <TIME>
```

Use `cryo-agent time "+30 minutes"` to compute the `<TIME>` value.

### Step 5: Hibernate (LAST action — nothing after this)

Pick ONE of the following. **This must be your final tool call. Do not run any commands after it.** The daemon cannot archive messages or schedule the next wake until your process exits.

**More work to do:**
```
cryo-agent hibernate --summary "what I did, what's next"
```

**All done:**
```
cryo-agent hibernate --complete --summary "All tasks finished"
```

**Waiting on user or external input:**
```
cryo-agent reply "What you need from the human"
cryo-agent todo add "Check for reply" --at <TIME>
cryo-agent hibernate --summary "Waiting on user/external input"
```

**Failure (retryable only):**
```
cryo-agent hibernate --exit 1 --summary "Failure: why this session should retry"
```

## Wake Time Guidelines

| Situation | Wake interval |
|-----------|--------------|
| Waiting on external event (CI, review) | 15–30 minutes |
| Multi-step plan, next step ready | 1–2 minutes |
| Time-sensitive deadline | exact time via `cryo-agent time` |
| Nothing to do until tomorrow | `cryo-agent time "+1 day"` |

## Command Reference

```
cryo-agent send "message"                     # Send message to human (outbox)
cryo-agent reply "message"                    # Reply to inbox messages
cryo-agent receive                            # Read inbox messages from human
cryo-agent alert <action> <target> "message"  # Dead-man switch (fires if you don't wake on time)
cryo-agent todo add "text" --at <TIME>        # Schedule a task (--at required)
cryo-agent todo list                          # List all TODO items
cryo-agent todo done <id>                     # Mark item as done
cryo-agent todo remove <id>                   # Remove an item
cryo-agent time                               # Current time in ISO8601
cryo-agent time "+1 day"                      # Relative time computation
```

## Key Facts

- **TODO list drives your schedule.** The daemon wakes at the earliest pending TODO's `at` time.
- **Inbox messages wake you early.** Humans can send messages. You'll see them in your prompt.
- **Human communication goes through `cryo-agent`.** Use `send`/`reply`; stdout/stderr are logs only.
- **NOTES.md is your memory.** Persists across sessions. Read it each wake, append/edit as you work, trim when it grows.
- **No hibernate = crash.** If you exit without calling `cryo-agent hibernate`, the daemon retries with backoff.
- **Delayed wakes happen.** If the machine was suspended, you'll see a system notice. Adjust accordingly.
- **Hibernate is terminal.** Nothing you do after hibernate will take effect. Put all work before it.
