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

### 2. Try the example chambers

Clone the repo and start the hub over the bundled examples (`mr-lazy`, `chess-by-mail`, `personal-assistant`):

```bash
git clone https://github.com/GiggleLiu/cryochamber
cd cryochamber/examples/chambers
cryohub start --foreground
```

`cryohub` prints a `http://host:port` URL — open it in your browser to manage the example chambers from the web UI.

### 3. Write your own plan

If your AI coding agent supports custom skills, install the `make-plan` skill from the repo you just cloned (point your agent's skill installer at `<repo>/.claude/skills/make-plan`), then ask the agent:

> Invoke the `make-plan` skill to create a new cryochamber project here.

The skill will walk you through `plan.md` and `cryo.toml` interactively. Without skill support, copy one of the `examples/chambers/*` directories as a starting point and edit `plan.md` (the task) and `cryo.toml` (agent command, retry policy, inbox settings) by hand.

### 4. Manage the running chamber

From inside a chamber directory:

```bash
cryo start           # start the daemon (installs an OS service)
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

`cryohub` always operates on the current directory; it rejects starting from a chamber dir. The UI lists every chamber with a status dot, lets you send messages, wake the agent, and start/stop/restart daemons.

## Messaging Channels

Cryochamber supports external messaging channels that sync between a remote service and the local inbox/outbox.

| Channel | Binary | Backend | Docs |
|---------|--------|---------|------|
| GitHub Discussions | `cryo-gh` | GitHub GraphQL API | [GitHub Sync](https://giggleliu.github.io/cryochamber/github-sync.html) |
| Zulip | `cryo-zulip` | Zulip REST API | [Zulip Sync](https://giggleliu.github.io/cryochamber/zulip-sync.html) |

## License

[MIT](LICENSE)
