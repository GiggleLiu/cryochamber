# Getting Started

## Prerequisites

- Rust toolchain ([rustup.rs](https://rustup.rs))
- macOS or Linux

## Install

```bash
cargo install --path .
```

## Initialize a Project

```bash
cryo init                      # for opencode (writes AGENTS.md + cryo.toml)
cryo init --agent claude       # for Claude Code (writes CLAUDE.md + cryo.toml)
```

Expected output:

```
Wrote cryo.toml
Wrote AGENTS.md
Wrote plan.md
```

## Edit the Generated Files

```bash
vim plan.md                    # your task plan
vim cryo.toml                  # agent, retries, timeout, inbox settings
```

## Start the Daemon

```bash
cryo start && cryo watch
```

Expected output:

```
Daemon started (PID 12345)
Watching cryo.log (Ctrl-C to stop)...
--- CRYO SESSION 1 | 2026-02-26T10:00:00Z ---
task: Continue the plan
agent: opencode
inbox: 0 messages
[10:00:00] agent started (pid 12346)
...
```

While the agent runs, you can interact from another terminal:

```bash
cryo send "Please also check issue #12"   # message appears in next session
cryo status                                # check current state
cryo cancel                                # stop the daemon
```

## What Happens

1. Cryochamber installs an OS service (launchd/systemd) that **survives reboots**
2. The daemon runs your agent with the plan and a task prompt
3. The agent does its work and calls `cryo-agent hibernate` to schedule the next wake
4. The daemon sleeps until the next wake time and repeats
5. New messages in `messages/inbox/` wake the daemon immediately

Session events are logged to `cryo.log`. Monitor progress with `cryo watch`. Check state with `cryo status`.

## Example Run

The [`examples/mr-lazy/plan.md`](https://github.com/GiggleLiu/cryochamber/blob/main/examples/mr-lazy/plan.md) example demonstrates the full daemon lifecycle. The agent has a 25% chance of waking up each session — otherwise it complains and goes back to sleep.

```bash
cd examples/mr-lazy && cryo init && cryo start && cryo watch
```

Here's a real run (from `cryo.log`):

```
--- CRYO SESSION 1 | 2026-02-25T01:13:12Z ---
task: Continue the plan
agent: opencode
inbox: 0 messages
[01:13:12] agent started (pid 75159)
[01:13:50] hibernate: wake=2026-02-25T01:16, exit=0,
           summary="Woke at 1:13 AM, rolled 2, complained about the indecency of early hours"
[01:13:55] note: "Session 1: Woke at 1:13 AM, rolled 2 (not 4), complained..."
[01:14:00] agent exited (code 0)
[01:14:00] session complete
--- CRYO END ---
```
