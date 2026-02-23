[![CI](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml/badge.svg)](https://github.com/GiggleLiu/cryochamber/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/GiggleLiu/cryochamber/graph/badge.svg?token=ucZRgZz154)](https://codecov.io/gh/GiggleLiu/cryochamber)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/logo/logo.svg" alt="cryochamber logo" width="500">
</p>

A long-term AI agent task scheduler. Cryochamber runs an AI agent session, parses structured `[CRYO:*]` markers from the agent's output, then uses OS-level timers (launchd on macOS, systemd on Linux) to hibernate and wake the agent at a future time.

This creates a daemon + agent architecture where the daemon orchestrates multi-session plans over hours or days — tasks that are too long for a single agent session and too irregular for cron.

## Quick Start

```bash
# Install
cargo install --path .

# Initialize a working directory
cryochamber init                    # for opencode (writes AGENTS.md)
cryochamber init --agent claude     # for Claude Code (writes CLAUDE.md)

# Edit the generated plan
vim plan.md

# Start the plan
cryochamber start plan.md
```

## How It Works

```
cmd_start() → run_session() → parse markers → validate → schedule timer → hibernate
                                                                              ↓
cmd_wake()  ← ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ (OS timer fires) ← ─ ─ ─ ─ ─ ─ ┘
     ↓
run_session() → parse markers → ...
```

1. The agent runs and writes `[CRYO:*]` markers at the end of its output
2. Cryochamber parses the markers to determine the next wake time
3. An OS timer (launchd/systemd) is scheduled to wake the agent
4. On wake, the agent gets the plan + previous session context and continues

## Markers

The agent communicates with cryochamber through markers in its stdout:

| Marker | Purpose | Example |
|--------|---------|---------|
| `[CRYO:EXIT <code>]` | Session result (0=success, 1=partial, 2=failure) | `[CRYO:EXIT 0] Reviewed 3 PRs` |
| `[CRYO:WAKE <time>]` | Next wake time (omit = plan complete) | `[CRYO:WAKE 2026-03-08T09:00]` |
| `[CRYO:CMD <cmd>]` | Command to run next session | `[CRYO:CMD opencode "check PRs"]` |
| `[CRYO:PLAN <note>]` | Memory for future sessions | `[CRYO:PLAN PR #41 needs fixes]` |
| `[CRYO:FALLBACK <action> <target> "<msg>"]` | Dead man's switch | `[CRYO:FALLBACK email user@ex.com "task failed"]` |

## Commands

```bash
cryochamber init [--agent <cmd>]     # Initialize working directory
cryochamber start <plan> [--agent <cmd>]  # Start a new plan
cryochamber wake                     # Called by OS timer
cryochamber status                   # Show current state
cryochamber cancel                   # Cancel all timers
cryochamber validate                 # Check if ready to hibernate
cryochamber log                      # Print session log
```

## Examples

See [`examples/`](examples/) for complete, runnable examples:

- **[Conference Chair](examples/conference-chair/)** — Manage a CS conference from CFP through author notification (~3 months)
- **[Mars Mission](examples/mars-mission/)** — Plan and monitor a simulated Mars probe mission (~100 days)

## Platform Support

| Platform | Timer Backend | Status |
|----------|--------------|--------|
| macOS | launchd (plist + launchctl) | Supported |
| Linux | systemd (user units + systemctl) | Supported |

## Development

```bash
cargo build                          # build
cargo test                           # run all tests
cargo clippy --all-targets -- -D warnings  # lint
make check                           # fmt + clippy + test
```
