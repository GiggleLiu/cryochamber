[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/GiggleLiu/cryochamber/graph/badge.svg?token=ucZRgZz154)](https://codecov.io/gh/GiggleLiu/cryochamber)
[![Docs](https://img.shields.io/badge/docs-gh--pages-blue)](https://giggleliu.github.io/cryochamber/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

**Cryochamber** is a long-term AI agent task scheduler. It hibernates an AI agent between sessions and wakes it at the right time — not on a fixed schedule. AI agent checks the log and decide the next move, just like an intersteller travelers.

Our goal is full automation of human activities. Many real world tasks span hours, days, or months and are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, with a persistent daemon that manages the lifecycle.

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

# Start the daemon and watch output
cryo start && cryo watch

# While the agent runs, you can interact from another terminal:
cryo send "Please also check issue #12"   # message appears in next session
cryo status                                # check current state
cryo cancel                                # stop the daemon
```

To run a chess playing example:
```bash
cd examples/chess-by-mail && cryo init && cryo start && cryo watch
```

See [`examples/`](examples/) for complete, runnable examples.

**What happens next:**

1. Cryochamber spawns a persistent daemon in the background
2. The daemon runs your agent with the plan and a task prompt
3. The agent does its work and calls `cryo-agent hibernate` to schedule the next wake
4. The daemon sleeps until the next wake time and repeats
5. New messages in `messages/inbox/` wake the daemon immediately

Session events are logged to `cryo.log`. Monitor progress with `cryo watch`. Check state with `cryo status`.

## How It Works

```
cryo start plan.md → spawn daemon → run agent → agent calls cryo-agent hibernate → sleep
                                                                                      ↓
                    inbox message → (immediate wake) ← ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┤
                                                                                      ↓
                                    (wake time reached) ← ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘
                                         ↓
                                    run agent → agent calls cryo-agent hibernate → ...
```

**The daemon** (cryochamber) handles lifecycle: sleeping until wake time, watching the inbox for reactive wake, enforcing session timeout, retrying on failure, and executing fallback alerts if something goes wrong.

**The agent** (any AI coding agent — opencode, Claude Code, etc.) handles reasoning: reading the plan, doing the work, and deciding when to wake up next. It communicates with the daemon via `cryo-agent` CLI commands over a Unix domain socket.

**Sessions** are the unit of work. Each session gets the plan, any new inbox messages, and the previous session's event log as context. The agent uses `cryo-agent note` to leave memory for future sessions.

## Example: Mr. Lazy

The [`examples/mr-lazy`](examples/mr-lazy/) example demonstrates the full daemon lifecycle. The agent has a 25% chance of waking up each session — otherwise it complains and goes back to sleep.

```bash
make check-round-trip   # run it yourself (Ctrl-C to stop)
```

Here's a real run (from `cryo.log`):

```
--- CRYO SESSION 1 | 2026-02-24T00:03:10Z ---
task: Continue the plan
agent: opencode
inbox: 0 messages
[00:03:12] agent started (pid 54321)
[00:03:18] note: "Rolled 2/4. Complaint #1 delivered."
[00:03:19] hibernate: wake=2026-02-24T00:05, exit=0, summary="Refused to wake up"
[00:03:19] agent exited (code 0)
--- CRYO END ---

--- CRYO SESSION 2 | 2026-02-24T00:05:00Z ---
task: Continue the plan
agent: opencode
inbox: 0 messages
[00:05:02] agent started (pid 54400)
[00:05:08] note: "Rolled 4/4 - grudgingly woke up after 2 sessions"
[00:05:09] hibernate: complete, exit=0, summary="Finally got out of bed"
[00:05:09] agent exited (code 0)
--- CRYO END ---
```

The daemon ran two sessions, sleeping 2 minutes between them, then stopped when the agent called `cryo-agent hibernate --complete`. All output is in `cryo.log`.

## Commands

### Operator (`cryo`)

```bash
cryo init [--agent <cmd>]           # Initialize working directory
cryo start [<plan|dir>] [--agent <cmd>] # Start a plan (default: ./plan.md)
cryo start --max-retries 3          # Retry agent spawn failures (default: 1)
cryo start --max-session-duration 3600  # Session timeout in seconds (default: no timeout)
cryo start --no-watch               # Disable inbox file watching
cryo status                         # Show current state
cryo ps [--kill-all]                # List (or kill) all running daemons
cryo restart                        # Kill running daemon and restart
cryo cancel                         # Stop the daemon and remove state
cryo watch [--all]                  # Watch session log in real-time
cryo log                            # Print session log
cryo send "<message>"               # Send a message to the agent's inbox
cryo receive                        # Read messages from the agent's outbox
cryo wake ["message"]               # Wake the daemon immediately with an optional message
```

### Agent IPC (`cryo-agent`)

```bash
cryo-agent hibernate --wake <ISO8601>  # Schedule next wake
cryo-agent hibernate --complete        # Mark plan as complete
cryo-agent note "text"                 # Leave a note for next session
cryo-agent reply "message"             # Reply to human (writes to outbox)
cryo-agent alert <action> <target> "msg"  # Set dead-man switch
```

### GitHub Sync (`cryo-gh`)

```bash
cryo-gh init --repo owner/repo     # Create a GitHub Discussion for sync
cryo-gh pull                        # Pull Discussion comments into inbox
cryo-gh push                        # Push session summary to Discussion
cryo-gh sync                        # Pull then push (full sync)
cryo-gh status                      # Show GitHub sync status
```

## FAQ

**What happens if my computer sleeps or shuts down during a scheduled wake?**

The daemon process is suspended along with everything else. When your machine wakes up, the daemon resumes and detects that the scheduled wake time has passed. It runs the session immediately and includes a "DELAYED WAKE" notice in the agent's prompt with the original scheduled time and how late the session is. The agent can then decide whether time-sensitive tasks need adjustment. Fallback alerts are not triggered prematurely — only if the session itself fails after running.

**How do I manually wake a sleeping daemon?**

Use `cryo wake` to send an immediate wake signal. You can include a message: `cryo wake "Please check the latest PR"`. This writes to the inbox, which triggers the daemon's file watcher. You can also use `cryo send` for the same effect.

## License

[MIT](LICENSE)
