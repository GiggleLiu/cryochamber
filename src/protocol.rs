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

Example: `[CRYO:CMD opencode run "check PR #42"]`

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
make time                # current time in ISO8601
make time OFFSET="+1 day"       # 1 day from now
make time OFFSET="+2 hours"     # 2 hours from now
make time OFFSET="+30 minutes"  # 30 minutes from now
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
[CRYO:CMD opencode run "Follow up on PR #41, check for new PRs"]
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

/// Makefile written to the agent's working directory.
/// Provides utility targets the agent can call (e.g., `make time`).
pub const MAKEFILE_CONTENT: &str = r#"# Cryochamber agent utilities
# These targets are available for the AI agent to call during sessions.

.PHONY: time

# Show current time or compute a future time
# Usage: make time                     # current time
#        make time OFFSET="+1 day"     # 1 day from now
#        make time OFFSET="+2 hours"   # 2 hours from now
#        make time OFFSET="+30 minutes"
OFFSET ?=

time:
ifeq ($(OFFSET),)
	@date +%Y-%m-%dT%H:%M
else
	@if date --version >/dev/null 2>&1; then \
		date -d "$(OFFSET)" +%Y-%m-%dT%H:%M; \
	else \
		N=$$(echo "$(OFFSET)" | sed 's/+//;s/[^0-9].*//');\
		U=$$(echo "$(OFFSET)" | sed 's/.*[0-9] *//;s/s$$//'); \
		case "$$U" in \
			minute) date -v+$${N}M +%Y-%m-%dT%H:%M ;; \
			hour)   date -v+$${N}H +%Y-%m-%dT%H:%M ;; \
			day)    date -v+$${N}d +%Y-%m-%dT%H:%M ;; \
			week)   date -v+$${N}w +%Y-%m-%dT%H:%M ;; \
			month)  date -v+$${N}m +%Y-%m-%dT%H:%M ;; \
			year)   date -v+$${N}y +%Y-%m-%dT%H:%M ;; \
			*) echo "Unknown unit: $$U" >&2; exit 1 ;; \
		esac; \
	fi
endif
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

/// Write the agent Makefile if none exists. Returns true if written.
pub fn write_makefile(dir: &Path) -> Result<bool> {
    let path = dir.join("Makefile");
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(path, MAKEFILE_CONTENT)?;
    Ok(true)
}
