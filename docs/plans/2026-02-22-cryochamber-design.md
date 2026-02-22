# Cryochamber Design

A long-term AI agent task scheduler. AI agents hibernate between tasks, waking via OS-level timers to execute scheduled work over days, weeks, months, or years.

## Architecture

Two components:

- **cryochamber** (Rust CLI): Watches log, parses markers, manages OS timers (launchd on macOS, systemd on Linux), spawns the AI agent, handles failures.
- **cryo-skill** (prompt template): Injected into the AI agent (opencode CLI) at invocation time. Teaches the agent the marker protocol so it can communicate scheduling instructions back to the daemon.

### Lifecycle

```
User writes plan.md (natural language)
        |
        v
cryochamber start plan.md
  -> copies plan to working dir
  -> spawns opencode with cryo-skill + plan.md as prompt
  -> AI reads plan, executes first task
  -> AI writes markers to stdout (captured to cryo.log)
  -> daemon parses markers
  -> pre-hibernate validation (the "validation button")
  -> registers OS timer for next wake
  -> registers fallback timer (dead man's switch)
  -> exits (hibernates)
        |
    (OS wakes process at scheduled time)
        |
        v
cryochamber wake
  -> reads cryo.log for context
  -> spawns opencode with cryo-skill + task prompt
  -> AI reads log history + plan, executes task
  -> cycle repeats
```

Key design decision: the AI agent dynamically determines the next wake time. The plan is a starting point; the agent adapts based on what it finds.

## Log-Based Protocol

The AI agent and daemon communicate through a shared log file (`cryo.log`). The AI writes natural language with embedded structured markers. The daemon only parses the markers.

### Session blocks

The daemon wraps each session in delimiters:

```
--- CRYO SESSION 2025-02-22T10:00:00 ---
Task: Review open PRs from last week

Checked 3 PRs. PR #41 has merge conflicts, left a comment
asking the author to rebase. PR #42 and #43 look good, approved.

[CRYO:EXIT 0] Reviewed 3 PRs: approved 2, commented on 1
[CRYO:PLAN PR #41 has conflicts, follow up next session]
[CRYO:WAKE 2025-02-24T09:00]
[CRYO:CMD opencode "Follow up on PR #41 rebase, check for new PRs"]
[CRYO:FALLBACK email user@example.com "Monday PR review did not run"]
--- CRYO END ---
```

### Marker spec

**Required (every session):**

| Marker | Format | Description |
|--------|--------|-------------|
| `EXIT` | `[CRYO:EXIT <code>] <summary>` | Exit status. 0=success, 1=partial, 2=failure |

**Optional:**

| Marker | Format | Description |
|--------|--------|-------------|
| `WAKE` | `[CRYO:WAKE <ISO8601>]` | When to wake up next |
| `CMD` | `[CRYO:CMD <command>]` | What to execute on wake. Default: re-run same command |
| `PLAN` | `[CRYO:PLAN <note>]` | Context note for future self |
| `FALLBACK` | `[CRYO:FALLBACK <action> <target> "<message>"]` | Dead man's switch. Actions: email, webhook |

### Rules

- No WAKE = plan is complete, no more wake-ups
- No CMD = re-use previous command
- Multiple FALLBACKs allowed per session
- Markers can appear anywhere in the session text
- Daemon scans from the latest session block only

## CLI

```
cryochamber start <plan.md>     # Begin: init files, run first task, set timer
cryochamber wake                # Called by OS timer: run next task
cryochamber status              # Show next wake time, last result, log tail
cryochamber cancel              # Cancel all timers, stop schedule
cryochamber validate            # Run pre-hibernate checks manually
cryochamber log                 # Print cryo.log
```

## File Layout

All files live in the same directory as plan.md:

```
~/plans/my-project/
  plan.md          # Natural language plan (user-written)
  cryo.log         # Session log with markers (daemon-managed)
  cryo.toml        # Optional config (fallback email, opencode path)
  timer.json       # Active timer metadata + lock file
```

No central config directory. The plan file's parent directory is the instance.

## Daemon Modules

```
src/
  main.rs              # CLI entry point (clap)
  marker.rs            # Parse [CRYO:*] markers from log text
  log.rs               # Read/write/append session blocks
  timer/
    mod.rs             # CryoTimer trait
    launchd.rs         # macOS LaunchAgent plist management
    systemd.rs         # Linux systemd timer unit management
  agent.rs             # Spawn opencode CLI, capture output
  fallback.rs          # Execute fallback actions (email, webhook)
  validate.rs          # Pre-hibernate health checks
```

### Timer trait

```rust
trait CryoTimer {
    fn schedule_wake(&self, time: DateTime<Utc>, command: &str) -> Result<TimerId>;
    fn schedule_fallback(&self, time: DateTime<Utc>, action: &FallbackAction) -> Result<TimerId>;
    fn cancel(&self, id: &TimerId) -> Result<()>;
    fn verify(&self, id: &TimerId) -> Result<TimerStatus>;
}
```

## Pre-Hibernate Validation

Before exiting, the daemon runs these checks:

| Step | Check | On failure |
|------|-------|------------|
| 1 | Log markers parsed successfully | Abort, print parse errors |
| 2 | WAKE timestamp is in the future | Abort, "wake time is in the past" |
| 3 | CMD is non-empty and executable | Abort, "command not found" |
| 4 | OS timer registered successfully | Abort, "failed to create timer" |
| 5 | OS timer readback matches WAKE | Abort, "timer time mismatch" |
| 6 | Fallback timer registered (if any) | Warn, continue without fallback |
| 7 | Log file still writable | Abort, "log file permission error" |
| 8 | Disk space > threshold | Warn, "low disk space" |

All passed -> hibernate. Any abort -> refuse to hibernate, exit 1.

## Cryo-Skill (AI Agent Prompt)

Template injected into opencode on each invocation:

```
You are running inside cryochamber, a long-term task scheduler.
You will be hibernated after this session and woken up later.

Your plan: see plan.md in the current directory
Your history: see cryo.log (read the latest session)
Current time: {{CURRENT_TIME}}
Session number: {{SESSION_NUMBER}}

After completing your task, you MUST write these markers:

Required:
  [CRYO:EXIT <0|1|2>] <one-line summary>

Optional (if plan has more work):
  [CRYO:WAKE <ISO8601 datetime>]
  [CRYO:CMD <command to run on wake>]
  [CRYO:PLAN <note for future self>]
  [CRYO:FALLBACK <email|webhook> <target> "<message>"]

No WAKE = plan complete. Always read cryo.log and plan.md first.
```

## Error Handling

| Scenario | Detection | Response |
|----------|-----------|----------|
| AI produces no markers | No EXIT in log | EXIT 2 (failure). Fire fallback. No new wake. |
| Invalid WAKE time | Parse fail / time in past | Refuse to hibernate. Print error. |
| opencode crashes | Non-zero exit + no markers | Log stderr. Fire fallback. No new wake. |
| OS timer fails to register | Validation step 4 | Refuse to hibernate. Print error. |
| Machine off at wake time | Late wake, time drift | Execute anyway. AI sees current time and adapts. |
| Log file corrupted | Read failure | Start fresh log. Warn. AI runs without history. |
| Concurrent wake-ups | Lock file (timer.json) | Check PID: alive=exit, dead=steal lock+warn. |

### Graceful degradation

The system degrades toward doing nothing rather than doing something wrong:
- Can't parse -> stop
- Can't set timer -> stop
- Can't send fallback -> log it
- Unsure -> refuse to hibernate, let user intervene
