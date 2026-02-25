[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-rust--API-blue)](https://giggleliu.github.io/cryochamber/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

**Cryochamber** for AI agents (claude, opencode and codex). It hibernates an AI agent between sessions and wakes it at the right time — not on a fixed schedule. AI agent checks the plan and log, complete some task and decide the next wake time. Cryochamber empower AI agents the ability to run tasks that may span days, weeks or even years, just like an interstellar travelers.

Our goal is to automate long running activities that are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, with a persistent daemon that manages the lifecycle.

## Quick Start

**Prerequisites:** Rust toolchain ([rustup.rs](https://rustup.rs)), macOS or Linux.

```bash
# Install
cargo install --path .

# Initialize a working directory
cryo init                      # for opencode (writes AGENTS.md + cryo.toml)
cryo init --agent claude       # for Claude Code (writes CLAUDE.md + cryo.toml)

# Edit the generated plan and config
vim plan.md                    # your task plan
vim cryo.toml                  # agent, retries, timeout, inbox settings

# Start the daemon and watch output
cryo start && cryo watch

# While the agent runs, you can interact from another terminal:
cryo send "Please also check issue #12"   # message appears in next session
cryo status                                # check current state
cryo cancel                                # stop the daemon
```

The [`examples/mr-lazy/plan.md`](examples/mr-lazy/plan.md) example demonstrates the full daemon lifecycle. The agent has a 25% chance of waking up each session — otherwise it complains and goes back to sleep.
```bash
cd examples/mr-lazy && cryo init && cryo start && cryo watch
```

See [`examples/`](examples/) for complete, runnable examples.

**What happens next:**

1. Cryochamber spawns a persistent daemon in the background
2. The daemon runs your agent with the plan and a task prompt
3. The agent does its work and calls `cryo-agent hibernate` to schedule the next wake
4. The daemon sleeps until the next wake time and repeats
5. New messages in `messages/inbox/` wake the daemon immediately

Session events are logged to `cryo.log`. Monitor progress with `cryo watch`. Check state with `cryo status`.

```text
cryo start → spawn daemon → run agent → agent calls cryo-agent hibernate → sleep
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

Here's a real run (from `cryo.log`):

```
(base) ➜  mr-lazy git:(main) ✗ cat cryo.log
Daemon: socket listening at /Users/liujinguo/rcode/cryochamber/examples/mr-lazy/.cryo/cryo.sock
Daemon: watching messages/inbox/ for new messages
Daemon: Session #1: Running agent...
--- CRYO SESSION 1 | 2026-02-25T01:13:12Z ---
task: Continue the plan
agent: opencode
inbox: 0 messages
[01:13:12] agent started (pid 75159)
[01:13:50] hibernate: wake=2026-02-25T01:16, exit=0, summary="Woke at 1:13 AM, rolled 2, complained about the indecency of early hours"
[01:13:55] note: "Session 1: Woke at 1:13 AM, rolled 2 (not 4), complained about the indecency of early hours, hibernated for 3 minutes until 1:16 AM"
[01:14:00] agent exited (code 0)
[01:14:00] session complete
--- CRYO END ---
Daemon: next wake at 2026-02-25 01:16
Daemon: scheduled wake time reached
Daemon: Session #2: Running agent...
--- CRYO SESSION 2 | 2026-02-25T01:16:00Z ---
task: Continue the plan
agent: opencode
inbox: 0 messages
[01:16:00] agent started (pid 79470)
[01:16:50] note: "Session 2: Rolled 3 (not 4), complained about the moon having better things to do at 1:16 AM, hibernating until 1:17 AM"
[01:16:53] hibernate: wake=2026-02-25T01:17, exit=0, summary="Rolled 3, complained about the moon having better things to do, going back to sleep"
[01:16:57] agent exited (code 0)
[01:16:57] session complete
--- CRYO END ---
Daemon: next wake at 2026-02-25 01:17
Daemon: scheduled wake time reached
Daemon: Session #3: Running agent...
--- CRYO SESSION 3 | 2026-02-25T01:17:00Z ---
task: Continue the plan
agent: opencode
inbox: 0 messages
[01:17:00] agent started (pid 80880)
[01:17:32] note: "Session 3: Rolled 1 (not 4), complained about 1:17 AM being an uncivilized hour, hibernating until 1:19 AM"
```

## Configuration

`cryo init` creates a `cryo.toml` file with project settings:

```toml
# cryo.toml — Cryochamber project configuration
agent = "opencode"        # Agent command (opencode, claude, codex, etc.)
max_retries = 1           # Max retry attempts on agent failure (1 = no retry)
max_session_duration = 0  # Session timeout in seconds (0 = no timeout)
watch_inbox = true        # Watch inbox for reactive wake
```

## Commands

### Operator (`cryo`)

```bash
cryo init [--agent <cmd>]           # Initialize working directory (writes cryo.toml)
cryo start [--agent <cmd>]          # Start the daemon (reads cryo.toml for config)
cryo start --max-retries 3          # Override max retries from cryo.toml
cryo start --max-session-duration 3600  # Override session timeout from cryo.toml
cryo status                         # Show current state
cryo ps [--kill-all]                # List (or kill) all running daemons
cryo restart                        # Kill running daemon and restart
cryo cancel                         # Stop the daemon and remove state
cryo watch [--all]                  # Watch session log in real-time
cryo log                            # Print session log
cryo send "<message>"               # Send a message to the agent's inbox
cryo receive                        # Read messages from the agent's outbox
cryo wake ["message"]               # Send a wake message to the daemon's inbox
cryo clean [--force]                # Remove runtime files (logs, state, messages)
```

### Agent IPC (`cryo-agent`)

```bash
cryo-agent hibernate --wake <ISO8601>  # Schedule next wake
cryo-agent hibernate --complete        # Mark plan as complete
cryo-agent note "text"                 # Leave a note for next session
cryo-agent send "message"              # Send message to human (writes to outbox)
cryo-agent receive                     # Read inbox messages from human
cryo-agent time "+30 minutes"          # Compute a future timestamp
cryo-agent alert <action> <target> "msg"  # Set dead-man switch
```

### GitHub Sync (`cryo-gh`)

Sync messages with a GitHub Discussion board for remote monitoring and two-way messaging. See [`docs/cryo-gh.md`](docs/cryo-gh.md) for setup and recommended workflow.

## FAQ

**What happens if my computer sleeps or shuts down during a scheduled wake?**

The daemon process is suspended along with everything else. When your machine wakes up, the daemon resumes and detects that the scheduled wake time has passed. It runs the session immediately and includes a "DELAYED WAKE" notice in the agent's prompt with the original scheduled time and how late the session is. The agent can then decide whether time-sensitive tasks need adjustment. Fallback alerts are not triggered prematurely — only if the session itself fails after running.

**How do I manually wake a sleeping daemon?**

Use `cryo wake` to send a message to the daemon's inbox. You can include a message: `cryo wake "Please check the latest PR"`. If inbox watching is enabled (the default), the daemon wakes immediately. You can also use `cryo send --wake` for the same effect. If inbox watching is disabled, `cryo wake` sends a SIGUSR1 signal to force the daemon awake. If no daemon is running, the message is queued for the next `cryo start`.

## License

[MIT](LICENSE)
