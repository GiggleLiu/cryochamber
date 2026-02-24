# Deep-Space Mission Planner Example

Manage a simulated Mars probe mission spanning ~100 days of simulated mission time.

## Why Cryochamber

A cron job cannot handle this workflow because:

- Burn windows are determined by orbital mechanics, not calendar time
- A sensor failure cascades into science plan changes, pointing schedule changes, and shifted wake times
- Wake intervals vary from hours (near critical burns) to weeks (cruise phase)

The agent adapts the entire mission plan based on events encountered in each session.

## Running

```bash
cd examples/mars-mission
cryo start && cryo watch
```

`cryo start` auto-generates the protocol file (CLAUDE.md or AGENTS.md) and Makefile.
