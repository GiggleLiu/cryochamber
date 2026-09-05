# How it works

A five-minute walkthrough of a chamber's moving parts.

## A chamber is just a directory

`cryo init` creates three files:

- **`plan.md`** — the agent's mission: goal, tasks, and rules. The agent re-reads it at the start of every session.
- **`cryo.toml`** — chamber configuration: which agent command to run, session timeout, inbox watching. See [Configuration](./reference/configuration.md).
- **`NOTES.md`** — the agent's memory across sessions. It reads and appends to this file directly.

While the daemon runs, runtime state appears alongside them: logs, `todo.json`, and `messages/inbox/` + `messages/outbox/`.

## The plan is plain markdown

A chamber that watches a GitHub repo for new releases:

```markdown
# Release watcher

## Goal
Tell me when acme/widgets publishes a new release.

## Tasks
1. Run `gh release list --repo acme/widgets --limit 1` and compare
   the version with the one recorded in NOTES.md.
2. If it changed: send me the release notes with `cryo-agent send`,
   then record the new version in NOTES.md.
3. Schedule the next check with `cryo-agent todo add` — every 2 hours
   on weekdays, once a day on weekends.
4. Hibernate with `cryo-agent hibernate --summary "..."`.
```

No code, no cron expression — the agent reads the situation (weekday vs. weekend here) and decides the next wake itself.

## The session loop

```text
daemon wakes agent        <- earliest TODO due, or inbox message
    │
    v
agent reads plan.md + NOTES.md
    │
    v
does the work
    │
    v
cryo-agent send "..."                    <- a visible message, never silent
    │
    v
cryo-agent todo add "..." --at <when>    <- declares the next wake
    │
    v
cryo-agent hibernate                     <- daemon sleeps until that wake
    │
    └────────────── back to the top ──────────────┘
```

One wake, one agent run, one return to sleep — that is a **session**:

1. The daemon wakes the agent when the earliest pending TODO comes due, or immediately when an inbox message arrives.
2. The agent reads `plan.md` and `NOTES.md`, then does the work.
3. It sends at least one visible message with `cryo-agent send`. If it exits without sending, the daemon writes a fallback message — a session is never silent.
4. It declares its own next wake with `cryo-agent todo add "..." --at <time>`. The daemon's next wake is always the earliest pending TODO — no TODO, no wake.
5. It calls `cryo-agent hibernate` and exits. The daemon sleeps until the next trigger. `hibernate --complete` ends the plan for good.

## Talking to the agent

Send a message from the terminal (`cryo send "..."`) or the Cryohub dashboard.
It lands in `messages/inbox/` and, with the default `watch_dirs`, wakes the agent
immediately. The agent's replies appear in `messages/outbox/` and in the
dashboard's message history.

The dashboard opens each thread in a focused view. One `cryo-agent receive` or
`cryo-agent dialog` call claims one conversation: either the first pending
thread or the pending messages in the unthreaded main stream. Once claimed,
that conversation must receive a reply before the agent can claim another one.
For a thread follow-up, the agent receives the root and earlier replies as
context, and its next send returns to the same thread automatically.

Sharing a thread reply to the main stream creates an outbox display copy. It
does not add anything to the local inbox, so it neither wakes the agent nor
creates work for it.

## Next

- [CLI reference](./reference/cli.md) — every `cryo`, `cryohub`, `cryo-agent`, and `cryo-zulip` command.
- [Configuration](./reference/configuration.md) — every `cryo.toml` and `cryohub.toml` field.

## Interrupted conversations

Before the daemon archives a claimed conversation, it saves a durable reply obligation.
After a hard daemon stop, the next start writes an interruption notice if no reply
was saved. The message may have caused partial external work. Review the history
and resend only if you still want that work performed. Claimed messages stay
archived and are never replayed automatically. An unread batch remains pending.
A corrupt recovery journal stops startup with an error instead of discarding it.

See [backup and restore](./operations.md) for recovery and upgrade procedures.
