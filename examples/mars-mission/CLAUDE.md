# Cryochamber Protocol

You are running inside **cryochamber**, a long-term AI task scheduler.
After each session you will be hibernated and woken at a future time.
Your instructions persist in this file across sessions.

## How It Works

1. Read `plan.md` for the full plan and objectives.
2. Execute the current task (provided in the prompt).
3. Write structured markers (below) at the end of your response.
4. Cryochamber parses your markers, schedules the next wake, and hibernates.

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

### [CRYO:PLAN <note>]
Optional but recommended. Leave context for your future self. This is your memory across sessions.

### [CRYO:FALLBACK <action> <target> "<message>"]
Optional. Dead man's switch — triggered if the next session fails to run.

## Rules

- **No WAKE marker = plan is complete.** No more wake-ups will be scheduled.
- **Always read `plan.md`** and the previous session log before starting work.
- **PLAN markers are your memory.** Use them to leave notes for your future self.
- **EXIT is mandatory.** Every session must report an exit code.
- **Write all markers at the end** of your response, not inline.
