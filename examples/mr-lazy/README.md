# Mr. Lazy

The laziest cryochamber example: an AI agent that refuses to get out of bed.

Every time cryochamber wakes Mr. Lazy, he rolls a die — 25% chance he
actually gets up. Otherwise, he delivers a dramatic, unique complaint
and hits snooze for a few more minutes. Repeat until he finally rolls a 4.

Demonstrates: repeated wake cycles, `make time` for scheduling, PLAN markers
for cross-session memory, probabilistic plan completion.

## Quick Start

```bash
cd examples/mr-lazy
cryo start --agent opencode && cryo watch
```

Or use the health check target (runs in daemon mode, Ctrl-C to stop):

```bash
make check-agent
```

## What You'll See

```
Session 1: "What is the point of consciousness this early? It's only 09:15..."
  → [CRYO:WAKE 2026-03-08T09:18]

Session 2: "No hobbit ever woke up before second breakfast... and it's 09:18."
  → [CRYO:WAKE 2026-03-08T09:22]

Session 3: "Fine. FINE. I'm up. Are you happy now?"
  → [CRYO:EXIT 0] Mr. Lazy finally got out of bed (plan complete)
```

Use `cryo cancel` if you can't wait for Mr. Lazy to roll a 4.

## Also Used By

`make check-agent` uses this example to verify your AI agent is set up correctly.
