# Getting Started

## Prerequisites

- Rust toolchain ([rustup.rs](https://rustup.rs))
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code)
- macOS or Linux

## Install

```bash
cargo install --path .
claude skill install --path skills/make-plan
```

This installs the `cryo` CLI and the `make-plan` skill for Claude Code.

## Create a Project with `/make-plan` (Recommended)

The fastest way to set up a cryochamber application is through the `/make-plan` skill. Open Claude Code and type `/make-plan`. The skill guides you through four phases:

1. **Brainstorm** — answer questions about your task, schedule, tools, messaging, and providers
2. **Configure** — generates `plan.md` and `cryo.toml` from your answers
3. **Validate** — tests the agent, verifies files, and runs a smoke test
4. **Start** — optionally starts the daemon immediately

The skill handles everything: writing the plan, configuring the agent, setting up Zulip or GitHub sync, and verifying the agent can respond. When it finishes, your project is ready to run.

## Manual Setup

If you prefer to set up without Claude Code:

### Initialize a Project

```bash
mkdir my-project && cd my-project
cryo init                      # for opencode (writes AGENTS.md + cryo.toml + README.md)
cryo init --agent claude       # for Claude Code (writes CLAUDE.md + cryo.toml + README.md)
```

### Edit the Generated Files

Write your task plan in `plan.md` — describe the goal, step-by-step tasks, and notes about persistent state. See the [Mr. Lazy](./examples/mr-lazy.md) and [Chess by Mail](./examples/chess-by-mail.md) examples for reference.

Review `cryo.toml` and adjust the agent command, retry policy, and inbox settings as needed.

### Start the Daemon

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
