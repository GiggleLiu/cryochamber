# Conference Program Chair Example

Manage a CS conference from call-for-papers through author notification — a ~3 month workflow where every deadline depends on human behavior.

## Why Cryochamber

A cron job cannot handle this workflow because:

- If submissions are low at the soft deadline, the agent extends it, shifting all downstream dates
- Reviewer reminder escalation (gentle -> urgent -> backup) depends on how many reviews are missing
- Wake intervals shrink as deadlines approach

The agent reasons about its past sessions to determine when to wake and what to do next.

## Running

```bash
cd examples/conference-chair
cryo start && cryo watch
```

`cryo start` auto-generates the protocol file (CLAUDE.md or AGENTS.md) and Makefile.
