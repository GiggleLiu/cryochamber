[![Crates.io](https://img.shields.io/crates/v/cryochamber)](https://crates.io/crates/cryochamber)
[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-mdbook-blue)](https://giggleliu.github.io/cryochamber/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

**Cryochamber** is a hibernation chamber for AI agents (Claude, OpenCode, Codex). It hibernates an AI agent between sessions and wakes it at the right time — not on a fixed schedule. The agent checks the plan and log, completes a task, and decides when to wake next. Cryochamber empowers AI agents to run tasks that span days, weeks, or even years, like interstellar travelers in stasis.

Our goal is to automate long-running activities that are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, with a persistent daemon that manages the lifecycle.

<p align="center">
  <img src="https://github.com/user-attachments/assets/7be712b2-f704-4a39-a2a0-b0e70ae05109" alt="Cryochamber cartoon explaining make a plan, hibernate, wake at the right time, and choose the next wake" width="900">
</p>

## Quick start

### Prerequisites

- The Rust toolchain. Install from [rustup.rs](https://rustup.rs).
- An AI coding agent on your `PATH`: [OpenCode](https://github.com/opencode-ai/opencode), [Claude Code](https://docs.anthropic.com/en/docs/claude-code), or [Codex](https://github.com/openai/codex).
- macOS or Linux.

### Step 1: Install cryochamber

Install the binaries from crates.io:

```bash
cargo install cryochamber
```

This installs `cryo`, `cryo-agent`, `cryo-gh`, `cryo-zulip`, and `cryohub`.

### Step 2: Try the example chambers

Register and run a bundled example in the web dashboard.

1. Clone the repository:

   ```bash
   git clone https://github.com/GiggleLiu/cryochamber
   ```

2. Change into the repository and run one example:

   ```bash
   cd cryochamber
   make example DIR=examples/chambers/mr-lazy
   ```

3. Open the `http://host:port` URL that `cryohub` prints in your browser.

### Step 3: Create your own chamber

You have two options.

**Option A — Use the `make-plan` skill (recommended).**
If your agent supports custom skills, install the bundled skill (point your agent's skill installer at `<repo>/.claude/skills/make-plan`), then prompt your agent:

> Invoke the `make-plan` skill to create a new cryochamber project here.

The skill walks you through `plan.md` and `cryo.toml` interactively.

**Option B — Scaffold by hand.**
Copy one of `examples/chambers/*` as a starting point, or run `cryo init` in an empty directory and edit `plan.md` and `cryo.toml`.

### Step 4: Manage the running chamber

Run these from inside the chamber directory:

| Command | What it does |
|---------|--------------|
| `cryo start` | Start the daemon (installs an OS service that survives reboots). |
| `cryo status` | Show whether the daemon is running. |
| `cryo watch` | Follow the live log. |
| `cryo send "message"` | Send a message to the agent. |
| `cryo cancel` | Stop the daemon. |

For the full guide, see [Getting Started](https://giggleliu.github.io/cryochamber/getting-started.html).

## Cryohub (multi-chamber dashboard)

Cryohub is a global web dashboard that manages chambers remembered by `cryo start` or created in the dashboard.

![cryohub dashboard with the mr-lazy chamber selected](docs/src/images/cryohub-dashboard.png)

1. Start the hub:

   ```bash
   cryohub start
   ```

2. Open the printed URL in your browser. The UI lists registered chambers, including stopped chambers, with status dots and controls to send messages, wake the agent, and start, stop, or restart daemons.

3. New chambers created in the UI live under the configured hub root. The default is `~/.cryo/chambers`; edit `~/.config/cryo/cryohub.toml` (or `$XDG_CONFIG_HOME/cryo/cryohub.toml`) to set `chamber_root`.

See [Hub](https://giggleliu.github.io/cryochamber/hub.html) for the full reference.

## Messaging channels

Cryochamber syncs the local inbox and outbox with external messaging services through a dedicated binary per channel. The cryo daemon and agent stay channel-agnostic.

| Channel            | Binary       | Backend            | Docs                                                                          |
|--------------------|--------------|--------------------|-------------------------------------------------------------------------------|
| GitHub Discussions | `cryo-gh`    | GitHub GraphQL API | [GitHub Sync](https://giggleliu.github.io/cryochamber/github-sync.html)       |
| Zulip              | `cryo-zulip` | Zulip REST API     | [Zulip Sync](https://giggleliu.github.io/cryochamber/zulip-sync.html)         |

## License

[MIT](LICENSE)
