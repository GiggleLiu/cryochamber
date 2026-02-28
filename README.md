[![Crates.io](https://img.shields.io/crates/v/cryochamber)](https://crates.io/crates/cryochamber)
[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-mdbook-blue)](https://giggleliu.github.io/cryochamber/)
[![API Docs](https://docs.rs/cryochamber/badge.svg)](https://docs.rs/cryochamber)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

**Cryochamber** is a hibernation chamber for AI agents (Claude, OpenCode, Codex). It hibernates an AI agent between sessions and wakes it at the right time — not on a fixed schedule. The agent checks the plan and log, completes a task, and decides when to wake next. Cryochamber empowers AI agents to run tasks that span days, weeks, or even years, like interstellar travelers in stasis.

Our goal is to automate long-running activities that are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, with a persistent daemon that manages the lifecycle.

## Quick Start

**Prerequisites:** Rust toolchain ([rustup.rs](https://rustup.rs)), [Claude Code](https://docs.anthropic.com/en/docs/claude-code), macOS or Linux.

### 1. Install cryochamber and the make-plan skill

```bash
cargo install cryochamber
claude skill install --path skills/make-plan
```

### 2. Create a project with `/make-plan`

Open Claude Code and run `/make-plan`. The skill walks you through a guided Q&A to create your `plan.md` and `cryo.toml` — covering the task, schedule, messaging channels, provider setup, and validation. It tests the agent, verifies the config, and optionally starts the daemon for you.

### 3. Start the service

If the skill didn't start the daemon for you:

```bash
cryo start                                                    # start the daemon
cryo-zulip init --config ./zuliprc --stream "my-stream"       # if using Zulip
cryo-zulip sync
cryo-gh init --repo owner/repo                                # if using GitHub Discussions
cryo-gh sync
cryo web                                                      # if using the web UI
cryo watch                                                    # follow the live log
```

### 4. Manage the running service

```bash
cryo status          # check if the daemon is running
cryo watch           # follow the live log
cryo send "message"  # send a message to the agent
cryo cancel          # stop the daemon
```

See [`examples/`](examples/) for complete, runnable examples (chess-by-mail, mr-lazy).

### Manual setup

If you prefer to set up without the skill:

```bash
mkdir my-project && cd my-project
cryo init                          # creates plan.md, cryo.toml, AGENTS.md, README.md
vim plan.md                        # write your task plan
vim cryo.toml                      # configure agent, retries, inbox
cryo start && cryo watch           # start and monitor
```

## Messaging Channels

Cryochamber supports external messaging channels that sync between a remote service and the local inbox/outbox directories. The cryo daemon and agent remain unaware of the channel — all sync is handled by a dedicated binary. These are configured automatically when using `/make-plan`.

| Channel | Binary | Backend | Docs |
|---------|--------|---------|------|
| GitHub Discussions | `cryo-gh` | GitHub GraphQL API | [GitHub Sync](https://giggleliu.github.io/cryochamber/github-sync.html) |
| Zulip | `cryo-zulip` | Zulip REST API | [Zulip Sync](https://giggleliu.github.io/cryochamber/zulip-sync.html) |

## Documentation

Full documentation is available at **[giggleliu.github.io/cryochamber](https://giggleliu.github.io/cryochamber/)** or build locally:

```bash
make book-serve   # opens http://localhost:3000 with live reload
```

## License

[MIT](LICENSE)
