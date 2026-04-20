# Mr. Lazy

The laziest cryochamber example: an AI agent that refuses to get out of bed.

Every time cryochamber wakes Mr. Lazy, he rolls a die — 25% chance he actually gets up. Otherwise, he delivers a dramatic, unique complaint and hits snooze for a few more minutes. Repeat until he finally rolls a 4.

**Demonstrates:** repeated wake cycles, `cryo-agent time` for scheduling, `NOTES.md` for cross-session memory, probabilistic plan completion.

## Quick Start

```bash
cd examples/chambers/mr-lazy
cryo init && cryo start
cd ../.. && cryohub start --foreground   # open the browser chat UI from the workspace dir
```

Or use the Makefile target (runs in daemon mode, Ctrl-C to stop):

```bash
make check-agent
```

## What You'll See

```
Session 1: "What is the point of consciousness this early? It's only 09:15..."
  → cryo-agent todo add "complain again" --at 2026-03-08T09:18
  → cryo-agent hibernate --summary "Too early, going back to sleep"

Session 2: "No hobbit ever woke up before second breakfast... and it's 09:18."
  → cryo-agent todo add "maybe get up" --at 2026-03-08T09:22
  → cryo-agent hibernate --summary "Still not ready, snoozing again"

Session 3: "Fine. FINE. I'm up. Are you happy now?"
  → cryo-agent hibernate --complete --summary "Mr. Lazy finally got out of bed"
```

Use `cryo cancel` if you can't wait for Mr. Lazy to roll a 4.
