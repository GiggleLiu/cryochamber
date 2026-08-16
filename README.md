[![Crates.io](https://img.shields.io/crates/v/cryochamber)](https://crates.io/crates/cryochamber)
[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-mdbook-blue)](https://giggleliu.github.io/cryochamber/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

**Cryochamber** is a hibernation chamber for AI agents (Claude, OpenCode, Codex, Pi, Kimi Code). It hibernates an AI agent between sessions and wakes it at the right time, not on a fixed schedule. The agent reads its plan, completes a task, and decides when to wake next. Cryochamber empowers AI agents to run tasks that span days, weeks, or even years, like interstellar travelers in stasis.

The goal is to automate long-running activities that are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, with a persistent daemon that manages the lifecycle.

<p align="center">
  <img src="https://github.com/user-attachments/assets/7be712b2-f704-4a39-a2a0-b0e70ae05109" alt="Cryochamber cartoon explaining make a plan, hibernate, wake at the right time, and choose the next wake" width="900">
</p>

## Quick start

Install Cryochamber:

> **Platform support:** macOS and Linux only. Windows is not supported — the daemon relies on Unix domain sockets, POSIX signals, and launchd/systemd services (see issue #27).

```bash
cargo install cryochamber
```

Create and start a chamber:
```bash
mkdir my-chamber && cd my-chamber
cryo init          # scaffold plan.md and cryo.toml — edit plan.md to describe the goal
cryo start
```

If your AI agent supports custom skills, you can skip the manual `plan.md` step: open the agent in the chamber directory and ask it to invoke the [make-plan skill](.claude/skills/make-plan/SKILL.md), which walks you through setup interactively.

Start Cryohub to monitor the chamber in your browser:
```bash
cryohub start
```

Cryohub runs once per user, discovers chambers on this machine, and serves the
**Agent Console** — a browser UI for status, logs, messages, TODOs, notes, and
lifecycle controls, on a desktop or a phone. It is embedded in the binary;
there is nothing else to install.

→ **CLI reference:** <https://giggleliu.github.io/cryochamber/reference/cli.html>  
→ **中文文档:** <https://giggleliu.github.io/cryochamber/zh/>

## Agent Console

The Agent Console is what cryohub serves, and it is also an installable phone
app (Android + iOS) for reading and steering your chambers from anywhere over
the same authenticated `/api`. One chamber is one flat conversation; a friend
gets in through an invite link scoped to the one chamber you minted it from.

To reach it beyond loopback, mint an owner token and start the hub with auth
enforced, then put a TLS proxy in front:

```bash
cryohub token owner     # prints the token once — this is your login
cryohub start --public
```

`--public` puts every `/api` route behind a bearer token; the console's own
pages stay public — they are the login screen. Sign-in, invites, PWA install
and the Caddy deployment are in the
[Agent Console guide](https://giggleliu.github.io/cryochamber/agent-console.html);
developer notes live in [`console/README.md`](console/README.md).

## Features

- **Agent-guided hibernation**: the agent decides when to wake next instead of running on a fixed cron schedule.
- **Folder watching**: configure `watch_dirs` to wake the agent when a folder changes, such as `messages/inbox/` or another project directory.
- **Crash recovery and reboot persistence**: `cryo start` installs a launchd/systemd service by default, so scheduled sessions survive machine reboots; failed TODO sessions are rescheduled as visible retry attempts with backoff.
- **Configurable agents**: run OpenCode, Claude Code, Codex, Pi, Kimi Code, or another command from `cryo.toml`.
- **Local and remote monitoring**: use the Cryohub dashboard to monitor chamber status, logs, messages, TODOs, notes, and lifecycle controls in a local browser UI, or talk to a chamber from anywhere via Zulip with `cryo-zulip`. See the [CLI reference](https://giggleliu.github.io/cryochamber/reference/cli.html).

## License

[MIT](LICENSE)
