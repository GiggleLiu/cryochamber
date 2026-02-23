// src/protocol.rs
use anyhow::Result;
use std::path::Path;

/// Protocol content written to the agent's working directory as CLAUDE.md or AGENTS.md.
/// This file is loaded as system-level context by the agent and survives context compression.
pub const PROTOCOL_CONTENT: &str = r#"# Cryochamber Protocol

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

## Utilities

You can call `cryo time` to get the current time, or compute a future time:

```
cryo time                # current time
cryo time "+1 day"       # 1 day from now
cryo time "+2 hours"     # 2 hours from now
cryo time "+30 minutes"  # 30 minutes from now
cryo time "+1 week"      # 1 week from now
cryo time "+3 months"    # ~3 months from now
```

Use this to calculate accurate WAKE times.

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
"#;

/// Template plan written by `cryo init` if no plan.md exists.
pub const TEMPLATE_PLAN: &str = r#"# My Plan

## Goal

Describe the high-level objective here.

## Tasks

1. First task description
2. Second task description
3. ...

## Notes

- Add any constraints, configuration, or context here.
"#;

/// Determine the protocol filename based on the agent command.
/// Returns `"CLAUDE.md"` if the executable name contains "claude", otherwise `"AGENTS.md"`.
/// Only inspects the first token (executable), so flags like `--model claude-3.7` are ignored.
pub fn protocol_filename(agent_cmd: &str) -> &'static str {
    let executable = agent_cmd
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    if executable.to_lowercase().contains("claude") {
        "CLAUDE.md"
    } else {
        "AGENTS.md"
    }
}

/// Write the protocol file to the given directory.
/// Skips writing if the file already exists (no-clobber). Returns true if written.
pub fn write_protocol_file(dir: &Path, filename: &str) -> Result<bool> {
    let path = dir.join(filename);
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(path, PROTOCOL_CONTENT)?;
    Ok(true)
}

/// Check if a protocol file (CLAUDE.md or AGENTS.md) exists in the directory.
/// Returns the filename if found.
pub fn find_protocol_file(dir: &Path) -> Option<&'static str> {
    if dir.join("CLAUDE.md").exists() {
        Some("CLAUDE.md")
    } else if dir.join("AGENTS.md").exists() {
        Some("AGENTS.md")
    } else {
        None
    }
}

/// Write a template plan.md if none exists. Returns true if written.
pub fn write_template_plan(dir: &Path) -> Result<bool> {
    let path = dir.join("plan.md");
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(path, TEMPLATE_PLAN)?;
    Ok(true)
}
