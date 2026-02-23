[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/GiggleLiu/cryochamber/graph/badge.svg?token=ucZRgZz154)](https://codecov.io/gh/GiggleLiu/cryochamber)
[![Docs](https://img.shields.io/badge/docs-gh--pages-blue)](https://giggleliu.github.io/cryochamber/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

**Cryochamber** is a long-term AI agent task scheduler. It hibernates an AI agent between sessions and wakes it at the right time — not on a fixed schedule. AI agent checks the log and decide the next move, just like an intersteller travelers.

Our goal is full automation of human activities. Many real world tasks span hours, days, or months and are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, then uses OS-level timers (launchd on macOS, systemd on Linux) to make it happen.

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

# Start the plan (defaults to plan.md in current directory)
cryo start                           # uses ./plan.md
```

To run a chess playing examples:
```bash
cd examples/chess-by-mail && cryo start
```

See [`examples/`](examples/) for complete, runnable examples.

**What happens next:**

1. Cryochamber spawns a persistent daemon in the background
2. The daemon runs your agent with the plan and a task prompt
3. The agent does its work and writes `[CRYO:*]` markers at the end
4. The daemon parses markers, sleeps until the next wake time, and repeats
5. New messages in `messages/inbox/` wake the daemon immediately

Monitor progress with `cryo watch`. Check state with `cryo status`.

## How It Works

```
cryo start plan.md → spawn daemon → run agent → parse markers → sleep
                                                                   ↓
                    inbox message → (immediate wake) ← ─ ─ ─ ─ ─ ─┤
                                                                   ↓
                                    (wake time reached) ← ─ ─ ─ ─ ┘
                                         ↓
                                    run agent → parse markers → ...
```

**The daemon** (cryochamber) handles lifecycle: sleeping until wake time, watching the inbox for reactive wake, enforcing session timeout, retrying on failure, and executing fallback alerts if something goes wrong.

**The agent** (any AI coding agent — opencode, Claude Code, etc.) handles reasoning: reading the plan, doing the work, and deciding when to wake up next. It communicates back to the daemon through structured markers in its output.

**Sessions** are the unit of work. Each session gets the plan, any new inbox messages, and the previous session's output as context. The agent's `[CRYO:PLAN]` markers serve as memory across sessions.

## Markers (for AI agents)

The agent writes these markers at the end of its output:

| Marker | Purpose | Example |
|--------|---------|---------|
| `[CRYO:EXIT <code>]` | Session result (0=success, 1=partial, 2=failure) | `[CRYO:EXIT 0] Reviewed 3 PRs` |
| `[CRYO:WAKE <time>]` | Next wake time (omit = plan complete) | `[CRYO:WAKE 2026-03-08T09:00]` |
| `[CRYO:CMD <cmd>]` | Task for next session | `[CRYO:CMD opencode "check PRs"]` |
| `[CRYO:PLAN <note>]` | Memory for future sessions | `[CRYO:PLAN PR #41 needs fixes]` |
| `[CRYO:FALLBACK <action> <target> "<msg>"]` | Dead man's switch | `[CRYO:FALLBACK email user@ex.com "task failed"]` |
| `[CRYO:REPLY "<msg>"]` | Reply to human (synced to Discussion) | `[CRYO:REPLY "Done, API updated."]` |

## Commands (for Admin)

```bash
cryo init [--agent <cmd>]           # Initialize working directory
cryo start [<plan|dir>] [--agent <cmd>] # Start a plan (default: ./plan.md)
cryo start --foreground             # Run in foreground (block until session completes)
cryo start --max-retries 3          # Retry agent spawn failures (default: 1)
cryo start --max-session-duration 3600  # Session timeout in seconds (default: 1800)
cryo start --no-watch               # Disable inbox file watching
cryo status                         # Show current state
cryo restart                        # Kill running session and restart daemon
cryo cancel                         # Cancel all timers and stop the daemon
cryo watch [--all]                  # Watch session log in real-time
cryo validate                       # Check if ready to hibernate
cryo log                            # Print session log
cryo send "<message>"               # Send a message to the agent's inbox
cryo receive                        # Read messages from the agent's outbox
cryo wake                           # Called by OS timer (foreground mode)
cryo gh init --repo owner/repo      # Create a GitHub Discussion for sync
cryo gh pull                        # Pull Discussion comments into inbox
cryo gh push                        # Push session summary to Discussion
cryo gh sync                        # Pull then push (full sync)
cryo gh status                      # Show GitHub sync status
```

## License

[MIT](LICENSE)
