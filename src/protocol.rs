// src/protocol.rs
use anyhow::Result;
use std::path::Path;

/// Protocol content written to the agent's working directory as CLAUDE.md or AGENTS.md.
/// This file is loaded as system-level context by the agent and survives context compression.
pub const PROTOCOL_CONTENT: &str = r#"# Cryochamber Protocol

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
