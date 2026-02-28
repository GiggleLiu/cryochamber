[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-mdbook-blue)](https://giggleliu.github.io/cryochamber/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

**Cryochamber** is a hibernation chamber for AI agents (Claude, OpenCode, Codex). It hibernates an AI agent between sessions and wakes it at the right time — not on a fixed schedule. The agent checks the plan and log, completes a task, and decides when to wake next. Cryochamber empowers AI agents to run tasks that span days, weeks, or even years, like interstellar travelers in stasis.

Our goal is to automate long-running activities that are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, with a persistent daemon that manages the lifecycle.

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
```

See [`examples/`](examples/) for complete, runnable examples (chess-by-mail, mr-lazy).

## Messaging Channels

Cryochamber supports external messaging channels that sync between a remote service and the local inbox/outbox directories. The cryo daemon and agent remain unaware of the channel — all sync is handled by a dedicated binary.

| Channel | Binary | Backend | Docs |
|---------|--------|---------|------|
| GitHub Discussions | `cryo-gh` | GitHub GraphQL API | [GitHub Sync](https://giggleliu.github.io/cryochamber/github-sync.html) |
| Zulip | `cryo-zulip` | Zulip REST API | [Zulip Sync](https://giggleliu.github.io/cryochamber/zulip-sync.html) |

## Claude Code Skill

This repo includes a `make-plan` skill for Claude Code that guides you through creating a new cryochamber application via conversational Q&A. To install it:

```bash
claude skill install --path skills/make-plan
```

Then invoke it with `/make-plan` in Claude Code.

## Documentation

Full documentation is available at **[giggleliu.github.io/cryochamber](https://giggleliu.github.io/cryochamber/)** or build locally:

```bash
make book-serve   # opens http://localhost:3000 with live reload
```

## License

[MIT](LICENSE)
