[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/GiggleLiu/cryochamber/graph/badge.svg?token=ucZRgZz154)](https://codecov.io/gh/GiggleLiu/cryochamber)
[![Docs](https://img.shields.io/badge/docs-gh--pages-blue)](https://giggleliu.github.io/cryochamber/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

**Cryochamber** is a long-term AI agent task scheduler. It hibernates an AI agent between sessions and wakes it at the right time — not on a fixed schedule, but when the agent itself decides it should wake up.

Some tasks span hours, days, or months and are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, then uses OS-level timers (launchd on macOS, systemd on Linux) to make it happen.

## Quick Start

**Prerequisites:** Rust toolchain ([rustup.rs](https://rustup.rs)), macOS or Linux.

```bash
# Install
cargo install --path .

# Initialize a working directory
cryo init                      # for opencode (writes AGENTS.md)
cryo init --agent claude       # for Claude Code (writes CLAUDE.md)

# Edit the generated plan with your tasks
vim plan.md

# Start the plan
cryo start plan.md
```

What happens next:
1. Cryochamber runs your agent with the plan and a task prompt
2. The agent does its work and writes `[CRYO:*]` markers at the end
3. Cryochamber parses the markers, schedules an OS timer for the next wake time, and exits
4. When the timer fires, cryochamber wakes, gives the agent its plan + session history, and repeats

Check the current state any time with `cryo status`.

## How It Works

```
cryo start plan.md → run agent → parse markers → schedule timer → hibernate
                                                                      ↓
cryo wake  ← ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ (OS timer fires) ← ─ ─ ─ ─ ─ ─ ┘
     ↓
run agent → parse markers → ...
```

**The daemon** (cryochamber) handles lifecycle: scheduling timers, managing state, passing context between sessions, and executing fallback alerts if something goes wrong.

**The agent** (any AI coding agent — opencode, Claude Code, etc.) handles reasoning: reading the plan, doing the work, and deciding when to wake up next. It communicates back to the daemon through structured markers in its output.

**Sessions** are the unit of work. Each session gets the plan, any new inbox messages, and the previous session's output as context. The agent's `[CRYO:PLAN]` markers serve as memory across sessions.

## Markers

The agent writes these markers at the end of its output:

| Marker | Purpose | Example |
|--------|---------|---------|
| `[CRYO:EXIT <code>]` | Session result (0=success, 1=partial, 2=failure) | `[CRYO:EXIT 0] Reviewed 3 PRs` |
| `[CRYO:WAKE <time>]` | Next wake time (omit = plan complete) | `[CRYO:WAKE 2026-03-08T09:00]` |
| `[CRYO:CMD <cmd>]` | Task for next session | `[CRYO:CMD opencode "check PRs"]` |
| `[CRYO:PLAN <note>]` | Memory for future sessions | `[CRYO:PLAN PR #41 needs fixes]` |
| `[CRYO:FALLBACK <action> <target> "<msg>"]` | Dead man's switch | `[CRYO:FALLBACK email user@ex.com "task failed"]` |

## Commands

```bash
cryo init [--agent <cmd>]           # Initialize working directory
cryo start <plan> [--agent <cmd>]   # Start a new plan
cryo wake                           # Called by OS timer
cryo status                         # Show current state
cryo cancel                         # Cancel all timers
cryo validate                       # Check if ready to hibernate
cryo log                            # Print session log
cryo time ["+N unit"]               # Show current time or compute offset
cryo send "<message>"               # Send a message to the agent's inbox
cryo receive                        # Read messages from the agent's outbox
```

The `time` command helps agents calculate accurate wake times:

```bash
cryo time                  # 2026-02-23T09:00
cryo time "+1 day"         # 2026-02-24T09:00
cryo time "+2 hours"       # 2026-02-23T11:00
cryo time "+3 months"      # 2026-05-24T09:00
```

## Examples

See [`examples/`](examples/) for complete, runnable examples.

### Chess by Mail

Play correspondence chess against an AI agent with adaptive scheduling.

**Why not cron?** The agent adapts its polling interval — checking frequently when you're active, going to sleep when you're away. A new move wakes the agent on demand, not on a schedule. Board state persists across sessions that may be hours or days apart.

[See the example](examples/chess-by-mail/)

### Conference Program Chair

Manage a CS conference from CFP through author notification — a ~3 month workflow where every deadline depends on human behavior.

**Why not cron?** If submissions are low, the agent extends the deadline — shifting all downstream dates. Reviewer reminders escalate from gentle to urgent based on how many reviews are missing. Wake intervals shrink as pressure increases. The entire timeline is elastic.

**Session trace:**

*Session 1 (Day 1).* Sends call for papers. Estimates soft deadline in 2 weeks.
```
[CRYO:EXIT 0] CFP sent to 4 mailing lists, 47 submissions expected
[CRYO:WAKE 2026-03-14T09:00]
[CRYO:CMD "Check submission count against target of 40"]
[CRYO:PLAN Soft deadline Mar 14. Hard deadline Mar 24. Target: 40+ submissions.]
```

*Session 2 (Day 14).* Only 23 submissions — below target. Extends the deadline by 10 days. All downstream dates shift.
```
[CRYO:EXIT 1] Only 23/40 submissions — extending deadline
[CRYO:WAKE 2026-03-24T09:00]
[CRYO:CMD "Close submissions, begin reviewer assignment"]
[CRYO:PLAN Extended to Mar 24. Sent reminder. If still <30, consider second extension.]
```

*Session 5 (Day 43).* Two reviewers still missing. Sends urgent reminders and assigns backups. Tighter wake interval now.
```
[CRYO:EXIT 1] 2 reviewers unresponsive — escalating
[CRYO:WAKE 2026-04-12T09:00]
[CRYO:PLAN Assigned backup reviewers for papers #12, #31. All others complete.]
```

### Deep-Space Mission Planner

Manage a simulated Mars probe mission spanning ~100 days. Burn windows, comm passes, and observation opportunities are all aperiodic.

**Why not cron?** Burn windows are determined by orbital mechanics. A sensor failure cascades into science plan changes, pointing schedule changes, and shifted wake times. The agent adapts the entire mission plan based on events.

**Session trace:**

*Session 1 (T+0).* Reads mission parameters. Computes trajectory. Schedules first correction burn at T+3 days.
```
[CRYO:EXIT 0] Mission initialized. Trajectory computed. TCM-1 at T+3d.
[CRYO:WAKE 2026-03-03T14:30]
[CRYO:CMD "Execute TCM-1 simulation, verify delta-v expenditure"]
[CRYO:PLAN Delta-v budget: 2.1 km/s. TCM-1 planned: 12 m/s. Next window: T+12d.]
```

*Session 5 (T+45d).* Thermal sensor lost to solar storm. Agent drops thermal mapping, elevates atmospheric spectroscopy. All future wake times shift.
```
[CRYO:EXIT 1] Spectrometer OK. Thermal sensor lost — science plan revised.
[CRYO:WAKE 2026-05-24T06:00]
[CRYO:CMD "Pre-orbit-insertion systems check, compute burn parameters"]
[CRYO:PLAN REVISED SCIENCE PLAN: Primary=atmospheric spectroscopy, Secondary=gravity field.
  Thermal mapping DROPPED (sensor failure T+45d). Delta-v remaining: 1.84 km/s.]
```

## Platform Support

| Platform | Timer Backend | Status |
|----------|--------------|--------|
| macOS | launchd (plist + launchctl) | Supported |
| Linux | systemd (user units + systemctl) | Supported |

## Development

```bash
cargo build                          # build
cargo test                           # run all tests
cargo clippy --all-targets -- -D warnings  # lint
make check                           # fmt + clippy + test
```

## License

[MIT](LICENSE)
