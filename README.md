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

This installs `cryo`, `cryo-agent`, `cryo-gh`, and `cryo-zulip` binaries.

### 2. Write your plan and configure

Edit `plan.md` with your task — describe the goal, step-by-step tasks, and notes about persistent state. Edit `cryo.toml` to configure the agent command, retry policy, and inbox settings. See [`examples/`](examples/) for reference (chess-by-mail, mr-lazy).

**Recommended:** Tell your AI coding agent to install the skill:

> Add the make-plan skill from https://github.com/GiggleLiu/cryochamber

Then run `/make-plan` to create a new project interactively via guided Q&A.

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
cryo web                                                      # if using the web UI
```

### 4. Manage the running service

Go to the project folder and type:
```bash
cryo status          # check if the daemon is running
cryo watch           # follow the live log
cryo send "message"  # send a message to the agent
cryo cancel          # stop the daemon
```

## Messaging Channels

Cryochamber supports external messaging channels that sync between a remote service and the local inbox/outbox directories. The cryo daemon and agent remain unaware of the channel — all sync is handled by a dedicated binary. These are configured automatically when using `/make-plan`.

| Channel | Binary | Backend | Docs |
|---------|--------|---------|------|
| Web UI | `cryo web` | Built-in HTTP server | [Web UI](https://giggleliu.github.io/cryochamber/web-ui.html) |
| GitHub Discussions | `cryo-gh` | GitHub GraphQL API | [GitHub Sync](https://giggleliu.github.io/cryochamber/github-sync.html) |
| Zulip | `cryo-zulip` | Zulip REST API | [Zulip Sync](https://giggleliu.github.io/cryochamber/zulip-sync.html) |

## License

[MIT](LICENSE)
