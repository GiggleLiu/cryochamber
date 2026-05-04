[![Crates.io](https://img.shields.io/crates/v/cryochamber)](https://crates.io/crates/cryochamber)
[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-mdbook-blue)](https://giggleliu.github.io/cryochamber/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

**Cryochamber** is a hibernation chamber for AI agents (Claude, OpenCode, Codex). It hibernates an AI agent between sessions and wakes it at the right time, not on a fixed schedule. The agent reads its plan, completes a task, and decides when to wake next. Cryochamber empowers AI agents to run tasks that span days, weeks, or even years, like interstellar travelers in stasis.

The goal is to automate long-running activities that are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, with a persistent daemon that manages the lifecycle.

<p align="center">
  <img src="https://github.com/user-attachments/assets/7be712b2-f704-4a39-a2a0-b0e70ae05109" alt="Cryochamber cartoon explaining make a plan, hibernate, wake at the right time, and choose the next wake" width="900">
</p>

## Quick start

Install Cryochamber:
```bash
cargo install cryochamber
```

Create a chamber directory:
```bash
mkdir my-chamber && cd my-chamber
```

Then start Claude Code, Codex, or OpenCode in that directory and ask:

```text
Follow the make-plan skill at https://github.com/GiggleLiu/cryochamber/blob/main/.claude/skills/make-plan/SKILL.md to create a new cryochamber project here.
```

The skill walks you through setup and can launch the chamber when ready.
Start Cryohub to monitor it in your browser:
```bash
cryohub start
```

Cryohub is the local web dashboard for Cryochamber. It runs once per user,
discovers chambers on this machine, and gives you a browser UI for status,
logs, messages, TODOs, notes, and start/stop controls.

→ **Full tutorial:** <https://giggleliu.github.io/cryochamber/tutorial.html>

## Remote monitoring

Admin can talk to a chamber from anywhere via GitHub Discussions, `cryo-gh`, or Zulip, `cryo-zulip`. See [Monitor and message a chamber](https://giggleliu.github.io/cryochamber/how-to/monitor-chambers.html).

## License

[MIT](LICENSE)
