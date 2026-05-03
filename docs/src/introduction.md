# Cryochamber

**Cryochamber** is a hibernation chamber for AI agents (Claude, OpenCode, Codex). It hibernates an AI agent between sessions and wakes it at the right time — not on a fixed schedule. The agent checks the plan and log, completes a task, and decides when to wake next. Cryochamber empowers AI agents to run tasks that span days, weeks, or even years, like interstellar travelers in stasis.

Our goal is to automate long-running activities that are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, with a persistent daemon that manages the lifecycle.

## How It Works

```text
cryo start → spawn daemon → run agent → agent calls cryo-agent hibernate → sleep
                                                                                    ↓
                  inbox message → (immediate wake) ← ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┤
                                                                                    ↓
                                  (wake time reached) ← ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘
                                       ↓
                                  run agent → agent calls cryo-agent hibernate → ...
```

**The daemon** (cryochamber) handles lifecycle: sleeping until wake time, watching the inbox for reactive wake, noting when inbox messages exist, enforcing session timeout, consuming past-due TODOs before each session, and — when the agent fails to hibernate cleanly — marking each consumed TODO done and adding a fresh retry TODO with an attempt-bumped suffix and `2^k`-minute delay (capped at 1 day). The agent still decides when to read mail. During a session, agent-side `cryo-agent receive` routes through daemon IPC so the daemon can archive that inbox batch and remember its reply obligation for the current session.

**The agent** (any AI coding agent — opencode, Claude Code, etc.) handles reasoning: reading the plan, doing the work, and deciding when to wake up next. It communicates with the daemon via `cryo-agent` CLI commands over a Unix domain socket.

**Sessions** are the unit of work. Each session gets the plan, any new inbox messages, and the previous session's event log as context. The agent reads and appends `NOTES.md` for cross-session memory.
