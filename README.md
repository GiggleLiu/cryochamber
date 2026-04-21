[![Crates.io](https://img.shields.io/crates/v/cryochamber)](https://crates.io/crates/cryochamber)
[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-mdbook-blue)](https://giggleliu.github.io/cryochamber/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

**Cryochamber** is a hibernation chamber for AI agents (Claude, OpenCode, Codex). It hibernates an AI agent between sessions and wakes it at the right time — not on a fixed schedule. The agent checks the plan and log, completes a task, and decides when to wake next. Cryochamber empowers AI agents to run tasks that span days, weeks, or even years, like interstellar travelers in stasis.

Our goal is to automate long-running activities that are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, with a persistent daemon that manages the lifecycle.

## Quick Start

**Prerequisites:** Rust toolchain ([rustup.rs](https://rustup.rs)), an AI coding agent ([OpenCode](https://github.com/opencode-ai/opencode), [Claude Code](https://docs.anthropic.com/en/docs/claude-code), or [Codex](https://github.com/openai/codex)), macOS or Linux.

### 1. Install cryochamber

```bash
cargo install cryochamber
```

This installs `cryo`, `cryo-agent`, `cryo-gh`, `cryo-zulip`, and `cryohub` binaries.

### Copy-Paste Onboarding Prompt

If you want your coding agent to set up a new Cryochamber project for you, paste this:

```text
Set up a new Cryochamber project for me in this directory.

1. If `cryo` is not installed, install it with `cargo install cryochamber`.
2. If the `make-plan` skill is not installed and your coding agent supports custom skills, install it from the Cryochamber repo: clone https://github.com/GiggleLiu/cryochamber somewhere local, then use your agent's skill installation mechanism to install `/path/to/cryochamber/skills/make-plan`.
3. Invoke the `make-plan` skill to create the Cryochamber project and generate the initial plan/config files.
4. Start the daemon with `cryo start`.
5. Tell me which files were created or updated, and whether the service started successfully.
```

### 2. Write your plan and configure

Edit `plan.md` with your task — describe the goal, step-by-step tasks, and notes about persistent state. Edit `cryo.toml` to configure the agent command, retry policy, and inbox settings. See [`examples/`](examples/) for reference (chess-by-mail, mr-lazy).

**Recommended:** If your AI coding agent supports custom skills, install `make-plan` from the Cryochamber repo:

> Add the make-plan skill from https://github.com/GiggleLiu/cryochamber

Then invoke the `make-plan` skill to create a new project interactively via guided Q&A.

### 3. Start the service

```bash
cryo start                                                    # start the daemon
```

Depending on the way you interact with your agent, start the corresponding service wtih:
```bash
cryo-zulip init --config ./zuliprc --stream "my-stream"       # if using Zulip
cryo-zulip sync
cryo-gh init --repo owner/repo                                # if using GitHub Discussions
cryo-gh sync
cd <chambers-parent-dir> && cryohub start                     # if using the web UI
```

### 4. Manage the running service

Go to the project folder and type:
```bash
cryo status          # check if the daemon is running
cryo watch           # follow the live log
cryo send "message"  # send a message to the agent
cryo cancel          # stop the daemon
```

## Cryohub (multi-chamber)

`cryohub` runs a directory-scoped dashboard. `cd` into a directory whose immediate subdirectories are chambers (each has its own `cryo.toml`), then start it:

```
~/my-chambers/
  chess-by-mail/
  mr-lazy/
  reports/
```

```bash
cd ~/my-chambers
cryohub start
```

`cryohub` always operates on the current directory; it rejects starting from a chamber dir. The UI lists every chamber with a status dot, lets you send messages, wake the agent, and start/stop/restart daemons. Running daemons registered elsewhere on the machine (outside the hub's cwd) appear as **external** chambers for monitoring only.

**Single-chamber layout:**

```bash
mkdir -p ~/cryo-chambers
ln -s $(pwd) ~/cryo-chambers/my-chamber
cd ~/cryo-chambers && cryohub start
```

## Messaging Channels

Cryochamber supports external messaging channels that sync between a remote service and the local inbox/outbox directories. The cryo daemon and agent remain unaware of the channel — all sync is handled by a dedicated binary. These are configured automatically when using `/make-plan`.

| Channel | Binary | Backend | Docs |
|---------|--------|---------|------|
| Hub (Web UI) | `cryohub` | Built-in HTTP server | [Hub](https://giggleliu.github.io/cryochamber/hub.html) |
| GitHub Discussions | `cryo-gh` | GitHub GraphQL API | [GitHub Sync](https://giggleliu.github.io/cryochamber/github-sync.html) |
| Zulip | `cryo-zulip` | Zulip REST API | [Zulip Sync](https://giggleliu.github.io/cryochamber/zulip-sync.html) |

## License

[MIT](LICENSE)
