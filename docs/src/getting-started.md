# Getting Started

## Prerequisites

- Rust toolchain ([rustup.rs](https://rustup.rs))
- An AI coding agent: [OpenCode](https://github.com/opencode-ai/opencode) (default), [Claude Code](https://docs.anthropic.com/en/docs/claude-code), or [Codex](https://github.com/openai/codex)
- macOS or Linux

## Install

```bash
cargo install cryochamber
```

This installs `cryo`, `cryo-agent`, `cryo-gh`, and `cryo-zulip` binaries.

## Initialize a Project

```bash
mkdir my-project && cd my-project
cryo init                      # for opencode (writes AGENTS.md + cryo.toml + README.md)
cryo init --agent claude       # for Claude Code (writes CLAUDE.md + cryo.toml + README.md)
```

## Write Your Plan

Edit `plan.md` — describe the goal, step-by-step tasks, and notes about persistent state. See the [Mr. Lazy](./examples/mr-lazy.md) and [Chess by Mail](./examples/chess-by-mail.md) examples for reference.

Review `cryo.toml` and adjust the agent command, retry policy, and inbox settings as needed.

**Recommended:** Tell your AI coding agent to install the skill instead of editing files manually:

> Add the make-plan skill from https://github.com/GiggleLiu/cryochamber

Then run `/make-plan` to create a new project interactively via guided Q&A.

## Start the Daemon

```bash
cryo start                                                    # start the daemon
cryo-zulip init --config ./zuliprc --stream "my-stream"       # if using Zulip
cryo-zulip sync
cryo-gh init --repo owner/repo                                # if using GitHub Discussions
cryo-gh sync
cryo web                                                      # if using the web UI
cryo watch                                                    # follow the live log
```

## What Happens

1. Cryochamber installs an OS service (launchd/systemd) that **survives reboots**
2. The daemon runs your agent with the plan and a task prompt
3. The agent does its work and calls `cryo-agent hibernate` to schedule the next wake
4. The daemon sleeps until the next wake time and repeats
5. New messages in `messages/inbox/` wake the daemon immediately

Session events are logged to `cryo.log`. Monitor progress with `cryo watch`. Check state with `cryo status`.

## Verify It's Working

After starting, confirm the first session completes:

```bash
cryo status    # should show "Daemon: running" with session number and PID
cryo watch     # follow the live log — look for "agent hibernated" to confirm success
```

Here's what a successful session looks like in `cryo.log`:

```
--- CRYO SESSION 1 | 2026-02-25T01:13:12Z ---
task: Continue the plan
agent: opencode
inbox: 0 messages
[01:13:12] agent started (pid 75159)
[01:13:50] hibernate: wake=2026-02-25T01:16, exit=0,
           summary="Completed first task, scheduling next check"
[01:14:00] agent exited (code 0)
[01:14:00] session complete
--- CRYO END ---
```

## Day-to-Day Usage

```bash
cryo status          # quick health check
cryo log             # read full session history
cryo web             # visual overview in the browser
cryo send "message"  # send a message to the agent's inbox
cryo receive         # read messages the agent sent you
cryo wake            # force an immediate wake
cryo restart         # restart the daemon
cryo cancel          # stop the daemon and clean up
cryo ps              # list all running daemons
```

## Example Run

The [`examples/mr-lazy/`](https://github.com/GiggleLiu/cryochamber/tree/main/examples/mr-lazy) example demonstrates the full daemon lifecycle. The agent has a 25% chance of waking up each session — otherwise it complains and goes back to sleep.

```bash
cd examples/mr-lazy && cryo init && cryo start && cryo watch
```
