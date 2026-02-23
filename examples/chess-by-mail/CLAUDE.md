# Cryochamber Protocol

You are running inside **cryochamber**, a long-term AI task scheduler.
After each session you will be hibernated and woken at a future time.
Your instructions persist in this file across sessions.

## How It Works

1. Read `plan.md` for the full plan and objectives.
2. Check your prompt for new messages (from humans or external systems).
3. Execute the current task (provided in the prompt).
4. Write structured markers (below) at the end of your response.
5. Cryochamber parses your markers, schedules the next wake, and hibernates.

## Message System

Cryochamber uses a file-based message inbox/outbox:

- **Inbox** (`messages/inbox/`): Messages from humans or external systems appear in your prompt automatically.
- **Outbox** (`messages/outbox/`): Fallback alerts are written here. External runners deliver them via email, webhook, etc.
- Processed inbox messages are archived to `messages/inbox/archive/`.

You do not need to read the inbox directory yourself — new messages are included in your prompt.
- You can reply to messages using `[CRYO:REPLY "your reply here"]` markers.

## Required Markers

You MUST write these markers at the end of every response:

### [CRYO:EXIT <code>] <summary>
Report your session result. Codes:
- `0` = success
- `1` = partial success
- `2` = failure

Example: `[CRYO:EXIT 0] Reviewed 3 PRs, approved 2, commented on 1`

### [CRYO:WAKE <ISO8601 datetime>]
When to wake up next. Omit this marker ONLY if the plan is complete.

Example: `[CRYO:WAKE 2026-03-08T09:00]`

### [CRYO:CMD <command>]
Optional. What agent command to run on next wake. If omitted, re-uses the previous command.

Example: `[CRYO:CMD opencode "check PR #42"]`

### [CRYO:PLAN <note>]
Optional but recommended. Leave context for your future self. This is your memory across sessions.

Example: `[CRYO:PLAN PR #41 needs author to fix lint issues before re-review]`

### [CRYO:FALLBACK <action> <target> "<message>"]
Optional. Dead man's switch — triggered if the next session fails to run.
- `action`: `email` or `webhook`

Example: `[CRYO:FALLBACK email user@example.com "weekly review did not run"]`

### [CRYO:REPLY "<message>"]
Optional. Post a reply to the human (synced to Discussion if gh sync is configured).

Example: `[CRYO:REPLY "Updated the API endpoint as requested."]`

## Utilities

Use `make` targets to compute accurate WAKE times:

```
make time                          # current time in ISO8601
make time OFFSET="+1 day"         # 1 day from now
make time OFFSET="+2 hours"       # 2 hours from now
make time OFFSET="+30 minutes"    # 30 minutes from now
```

Or use `date` directly: `date -u +%Y-%m-%dT%H:%M`

## Rules

- **No WAKE marker = plan is complete.** No more wake-ups will be scheduled.
- **Always read `plan.md`** and the previous session log before starting work.
- **PLAN markers are your memory.** Use them to leave notes for your future self.
- **EXIT is mandatory.** Every session must report an exit code.
- **Write all markers at the end** of your response, not inline.

## Example Session Output

```
Checked all open PRs. Found 3 ready for review.
Approved PR #42 and #43. Left comments on PR #41.

[CRYO:EXIT 0] Reviewed 3 PRs: approved 2, commented on 1
[CRYO:PLAN PR #41 needs author to fix lint issues]
[CRYO:WAKE 2026-03-08T09:00]
[CRYO:CMD opencode "Follow up on PR #41, check for new PRs"]
[CRYO:FALLBACK email user@example.com "Monday PR review did not run"]
```
