// ─────────────────────────────────────────────────────────────────────────────
//  Cryochamber — Product Usage Guide
// ─────────────────────────────────────────────────────────────────────────────
#import "@preview/cetz:0.4.0"

#set page(paper: "a4", margin: (top: 2.5cm, bottom: 2.5cm, left: 2.4cm, right: 2.4cm))
#set text(size: 10.5pt, font: "New Computer Modern")
#set heading(numbering: "1.1.")
#set par(justify: true, leading: 0.65em)

// ── colour palette ────────────────────────────────────────────────────────────
#let ocean   = rgb("#1a73a7")
#let teal    = rgb("#2bb5a0")
#let amber   = rgb("#e08b2a")
#let rose    = rgb("#c0392b")
#let sage    = rgb("#5a9a6f")
#let slate   = rgb("#5d6d7e")
#let linen   = rgb("#fdf6ec")
#let icebg   = rgb("#edf4fb")

// ── helper: info / tip box ────────────────────────────────────────────────────
#let callout(color: ocean, icon: "ℹ", title: "", body) = block(
  stroke: (left: 3pt + color),
  inset: (left: 10pt, top: 6pt, bottom: 6pt, right: 8pt),
  radius: 2pt,
  fill: color.lighten(92%),
  width: 100%,
)[
  #if title != "" { text(weight: "bold", fill: color)[#icon  #title \ ] }
  #body
]

// ── helper: command line block ────────────────────────────────────────────────
#let cmd(body) = block(
  fill: rgb("#1e1e2e"), radius: 4pt, inset: 10pt, width: 100%,
)[#text(fill: rgb("#cdd6f4"), font: "Courier New", size: 9pt)[#body]]

// ── title page ────────────────────────────────────────────────────────────────
#align(center)[
  #v(2cm)
  #text(size: 28pt, weight: "bold", fill: ocean)[Cryochamber]
  #v(0.3cm)
  #text(size: 15pt, fill: slate)[_A hibernation chamber for AI agents_]
  #v(0.5cm)
  #line(length: 60%, stroke: 1.5pt + ocean)
  #v(0.4cm)
  #text(size: 12pt)[*Product Usage Guide*]
  #v(0.2cm)
  #text(size: 10pt, fill: slate)[For operators, developers, and curious readers]
  #v(3cm)
]

#pagebreak()

// ── table of contents ─────────────────────────────────────────────────────────
#outline(title: "Contents", depth: 2, indent: 1.5em)

#pagebreak()

// ─────────────────────────────────────────────────────────────────────────────
= What Is Cryochamber?
// ─────────────────────────────────────────────────────────────────────────────

*Cryochamber* is an open-source AI-agent scheduler written in Rust. It solves a
fundamental problem: _large tasks take longer than a single LLM context window._

Modern AI agents (Claude, OpenCode, GPT-4 …) can run code, browse the web, and
write software — but they time out, hit rate limits, or simply need to pause and
think overnight. Cryochamber gives them a persistent harness:

#grid(columns: (1fr, 1fr), gutter: 16pt)[
  #callout(color: rose, icon: "⚠", title: "The Problem")[
    - Agent times out mid-task
    - No memory between runs
    - Human must restart manually
    - API rate limits block overnight work
  ]
][
  #callout(color: sage, icon: "✓", title: "What Cryochamber Provides")[
    - Scheduled wake-ups via TODO list
    - Persistent logs & notes across sessions
    - Human↔agent messaging
    - Automatic retry on crash
  ]
]

The name reflects the metaphor: the agent is placed in a *cryochamber* between
sessions — state is frozen, resources are freed, and it wakes up exactly when
needed.

// ─────────────────────────────────────────────────────────────────────────────
= Core Concepts
// ─────────────────────────────────────────────────────────────────────────────

== Session
A *session* is one run of the agent process. Every session has a number, a task
description, a start timestamp, and an end outcome (`complete`, `exit <N>`,
`timeout`, …). Sessions are delimited in `cryo.log` by
`--- CRYO SESSION N ---` / `--- CRYO END ---` markers.

== Daemon
The *daemon* is a long-running background process managed by the OS service
layer (launchd on macOS, systemd on Linux, SCM on Windows). It:
- sleeps until the earliest pending TODO time;
- spawns the agent subprocess for each session;
- enforces a configurable session timeout;
- retries crashed agents with exponential backoff (5 s → 15 s → 60 s);
- watches `messages/inbox/` for reactive wakes.

== IPC
The agent subprocess communicates with the daemon over a platform IPC channel
(Unix domain socket on Unix, named pipe on Windows) using the `cryo-agent` CLI.
This keeps the agent decoupled: it just runs shell commands.

== TODO List
The agent controls its own wake schedule through `todo.json`. Adding a TODO with
a future `--at` time causes the daemon to sleep until that time and then
re-spawn the agent. If no TODO is pending the daemon idles.

== Inbox / Outbox
A simple file-based message bus lives in `messages/`. The human writes to
`inbox/` (via `cryo send`); the agent reads it at session start. The agent
writes replies to `outbox/`; the human reads them (via `cryo receive`). Channels
such as GitHub Discussions and Zulip mirror this bus over the internet.

// ─────────────────────────────────────────────────────────────────────────────
= Architecture
// ─────────────────────────────────────────────────────────────────────────────

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    // ── style helpers ──────────────────────────────────────────────────────
    let box-stroke   = (paint: ocean,  thickness: 1.2pt)
    let dash-stroke  = (paint: slate,  thickness: 0.8pt, dash: "dashed")
    let arrow        = (end: "straight")
    let bkg          = icebg
    let human-fill   = amber.lighten(75%)
    let daemon-fill  = ocean.lighten(82%)
    let agent-fill   = teal.lighten(80%)
    let fs-fill      = sage.lighten(80%)
    let ch-fill      = rose.lighten(82%)

    // ── HUMAN operator (top-left) ─────────────────────────────────────────
    rect((0, 7), (2.8, 8.4), fill: human-fill, stroke: box-stroke, name: "human")
    content("human.center", align(center)[*Human*\ Operator])

    // ── cryo CLI (top-centre) ─────────────────────────────────────────────
    rect((4, 7), (7.2, 8.4), fill: daemon-fill, stroke: box-stroke, name: "cli")
    content("cli.center", align(center)[*cryo* CLI\ `init / start / send / receive`])

    // ── DAEMON (centre) ───────────────────────────────────────────────────
    rect((4, 4.6), (7.2, 6.6), fill: daemon-fill, stroke: box-stroke, name: "daemon")
    content("daemon.center", align(center)[*Daemon Process*\ event loop\ IPC server])

    // ── AGENT subprocess (right) ──────────────────────────────────────────
    rect((8.8, 4.6), (12, 6.6), fill: agent-fill, stroke: box-stroke, name: "agent")
    content("agent.center", align(center)[*Agent Subprocess*\ (opencode / claude /\ any CLI tool)])

    // ── cryo-agent CLI (bottom-right) ─────────────────────────────────────
    rect((8.8, 2.2), (12, 3.8), fill: agent-fill, stroke: box-stroke, name: "cryo-agent")
    content("cryo-agent.center", align(center)[*cryo-agent* CLI\ `hibernate / note / todo`])

    // ── FILESYSTEM (bottom-centre) ────────────────────────────────────────
    rect((3.5, 0.3), (8.2, 2.2), fill: fs-fill, stroke: box-stroke, name: "fs")
    content("fs.center", align(center)[
      *Project Directory (filesystem)*\
      `cryo.toml  plan.md  cryo.log`\
      `messages/inbox/   messages/outbox/`
    ])

    // ── CHANNELS (bottom-left) ────────────────────────────────────────────
    rect((0, 2.2), (2.8, 3.8), fill: ch-fill, stroke: box-stroke, name: "channels")
    content("channels.center", align(center)[*Channels*\ GitHub Discussions\ Zulip])

    // ── ARROWS ────────────────────────────────────────────────────────────
    // Human → cryo CLI
    line("human.east", "cli.west",
         mark: arrow, stroke: (paint: ocean, thickness: 1.2pt))
    content(("human.east", 50%, "cli.west"), [  `cryo`\ commands  ],
            fill: white, frame: "rect", stroke: none, padding: 0.1)

    // cryo CLI → daemon (start / wake)
    line("cli.south", "daemon.north",
         mark: arrow, stroke: (paint: ocean, thickness: 1.2pt))
    content(("cli.south", 50%, "daemon.north"), [],
            fill: white, frame: "rect", stroke: none, padding: 0.04)

    // daemon → agent (spawn)
    line("daemon.east", "agent.west",
         mark: arrow, stroke: (paint: teal, thickness: 1.4pt))
    content(("daemon.east", 50%, "agent.west"), [spawn],
            fill: white, frame: "rect", stroke: none, padding: 0.08)

    // agent → cryo-agent
    line("agent.south", "cryo-agent.north",
         mark: arrow, stroke: (paint: teal, thickness: 1.2pt))
    content(("agent.south", 50%, "cryo-agent.north"), [calls],
            fill: white, frame: "rect", stroke: none, padding: 0.08)

    // cryo-agent → daemon (IPC)
    line("cryo-agent.west", (5.6, 2.8), "daemon.south",
         mark: arrow, stroke: (paint: slate, thickness: 1pt, dash: "dashed"))
    content(((5.6, 2.8)), [IPC socket],
            fill: white, frame: "rect", stroke: none, padding: 0.08)

    // daemon ↔ filesystem
    line("daemon.south", "fs.north",
         mark: (start: "straight", end: "straight"),
         stroke: (paint: sage, thickness: 1.2pt))
    content(("daemon.south", 50%, "fs.north"), [read/write],
            fill: white, frame: "rect", stroke: none, padding: 0.08)

    // channels ↔ filesystem (sync)
    line("channels.east", "fs.west",
         mark: (start: "straight", end: "straight"),
         stroke: (paint: rose, thickness: 1.2pt))
    content(("channels.east", 50%, "fs.west"), [sync],
            fill: white, frame: "rect", stroke: none, padding: 0.08)

    // human → channels
    line("human.south", "channels.north",
         mark: (start: "straight", end: "straight"),
         stroke: (paint: amber, thickness: 1.2pt))
    content(("human.south", 50%, "channels.north"), [messages],
            fill: white, frame: "rect", stroke: none, padding: 0.08)
  }),
  caption: [Component architecture of Cryochamber.],
)

The diagram shows the five main actors:

#table(
  columns: (auto, 1fr),
  stroke: 0.5pt + luma(180),
  inset: 7pt,
  fill: (col, row) => if row == 0 { ocean.lighten(88%) } else { white },
  [*Actor*], [*Role*],
  [`cryo` CLI], [Operator's entry point — initialise projects, start/stop the daemon, view logs, send messages.],
  [Daemon], [Long-running background process. Manages the session lifecycle, IPC server, and wake logic.],
  [Agent subprocess], [The actual AI tool (e.g. `opencode run`). Completely swappable — any CLI program works.],
  [`cryo-agent` CLI], [Agent's toolbox. Sends commands over IPC: hibernate, add notes, schedule todos, send messages.],
  [Channels], [Optional bridges (GitHub Discussions, Zulip) that mirror the inbox/outbox to internet services.],
)

// ─────────────────────────────────────────────────────────────────────────────
= Session Lifecycle
// ─────────────────────────────────────────────────────────────────────────────

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let st = (paint: ocean, thickness: 1.2pt)
    let arrow = (end: "stealth")
    let state-fill = ocean.lighten(85%)
    let act-fill   = teal.lighten(82%)
    let err-fill   = rose.lighten(82%)

    // states
    let states = (
      ("idle",     (5.5, 8.2), [*Idle / Sleeping*]),
      ("check",    (5.5, 6.2), [*Check wake condition*]),
      ("spawn",    (5.5, 4.2), [*Spawn agent*]),
      ("running",  (5.5, 2.2), [*Agent running*]),
      ("exited",   (9.5, 2.2), [*Agent exited*]),
      ("success",  (9.5, 0.2), [*Log success\ schedule next*]),
      ("crash",    (1.5, 2.2), [*Crashed /\ timed out*]),
      ("retry",    (1.5, 0.2), [*Backoff retry\  (5→15→60 s)*]),
    )

    for (name, pos, label) in states {
      let fill = if name == "crash" or name == "retry" { err-fill }
                 else if name == "spawn" or name == "running" { act-fill }
                 else { state-fill }
      let (x, y) = pos
      rect((x - 1.8, y - 0.65), (x + 1.8, y + 0.65),
           fill: fill, stroke: st, radius: 4pt, name: name)
      content(name + ".center", align(center, label))
    }

    // transitions
    // idle → check
    line("idle.south", "check.north", mark: arrow, stroke: st)
    content(("idle.south", 55%, "check.north"),
      [wake time\ or inbox event], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // check → spawn (yes)
    line("check.south", "spawn.north", mark: arrow, stroke: st)
    content(("check.south", 55%, "spawn.north"),
      [ready], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // check → idle (no)
    bezier("check.east", "idle.east",
           (9, 6.2), (9, 8.2),
           mark: arrow, stroke: (paint: slate, thickness: 0.9pt, dash: "dashed"))
    content((9.3, 7.2), [not yet], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // spawn → running
    line("spawn.south", "running.north", mark: arrow, stroke: st)

    // running → exited (normal exit)
    line("running.east", "exited.west", mark: arrow, stroke: (paint: sage, thickness: 1.2pt))
    content(("running.east", 50%, "exited.west"),
      [hibernate\ or exit 0], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // exited → success
    line("exited.south", "success.north", mark: arrow, stroke: (paint: sage, thickness: 1.2pt))

    // success → idle
    bezier("success.east", "idle.east",
           (12, 0.2), (12, 8.2),
           mark: arrow, stroke: (paint: sage, thickness: 1pt))
    content((12.6, 4.2), [next wake], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // running → crash (bad exit)
    line("running.west", "crash.east", mark: arrow, stroke: (paint: rose, thickness: 1.2pt))
    content(("running.west", 50%, "crash.east"),
      [non-zero\ exit], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // crash → retry
    line("crash.south", "retry.north", mark: arrow, stroke: (paint: rose, thickness: 1.2pt))

    // retry → spawn
    bezier("retry.north", "spawn.west",
           (1.5, 1.4), (3.7, 4.2),
           mark: arrow, stroke: (paint: rose, thickness: 1pt, dash: "dashed"))
    content((2.3, 2.8), [retry], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // retry → idle (max retries exhausted)
    bezier("retry.south", "idle.west",
           (1.5, -1.2), (-1.2, 8.2),
           mark: arrow, stroke: (paint: rose, thickness: 0.9pt, dash: "dotted"))
    content((-1.5, 3.5), text(size: 8pt)[exhausted\ → alert], fill: white, frame: "rect", stroke: none, padding: 0.08)
  }),
  caption: [Daemon session lifecycle state machine.],
)

The daemon loops indefinitely. Key points:

- *Normal exit*: the agent calls `cryo-agent hibernate`; the daemon logs the session and schedules the next wake from `todo.json`.
- *Crash/timeout*: the daemon retries with exponential backoff. After `max_retries` consecutive failures it fires a *fallback alert* (dead-man switch) to the outbox.
- *Reactive wake*: a new file in `messages/inbox/` or a `cryo wake` command can cut the sleep short.

// ─────────────────────────────────────────────────────────────────────────────
= Getting Started
// ─────────────────────────────────────────────────────────────────────────────

== Install

#cmd[
  ```
  cargo install cryochamber        # from crates.io
  # or, from source:
  git clone https://github.com/GiggleLiu/cryochamber
  cd cryochamber && cargo install --path .
  ```
]

This installs four binaries: `cryo`, `cryo-agent`, `cryo-gh`, `cryo-zulip`.

== Create a Project

#cmd[
  ```
  mkdir my-ai-project && cd my-ai-project
  echo "# Plan\nWrite the plan here." > plan.md
  cryo init --agent "opencode run"
  ```
]

`cryo init` creates:
- `cryo.toml` — project configuration
- `plan.md` — (already present) read by the agent at session start
- `CLAUDE.md` / `AGENTS.md` — cryochamber protocol injected into the agent's context

== Start the Daemon

#cmd[
  ```
  cryo start                        # uses agent from cryo.toml
  cryo start --agent "claude -p"    # override agent for this run
  ```
]

On macOS/Linux this installs a *launchd* / *systemd* service that survives reboots.
Set `CRYO_NO_SERVICE=1` to skip service installation and run as a plain background
process instead.

== Monitor Progress

#cmd[
  ```
  cryo status          # daemon state, session number, next wake time
  cryo watch           # live-tail of cryo.log (tail -f style)
  cryo watch --all     # from the beginning of the log
  cryo log             # dump the full structured log
  ```
]

== Send & Receive Messages

#cmd[
  ```
  cryo send "Please review the PR before the next session"
  cryo receive                     # read agent replies from outbox
  ```
]

`cryo send` places a Markdown message in `messages/inbox/`. The daemon wakes
the agent (if sleeping) and the agent reads the message at the top of its next
session.

== Stop

#cmd[
  ```
  cryo cancel          # stop daemon + remove service
  cryo clean --force   # cancel + delete all runtime files
  ```
]

// ─────────────────────────────────────────────────────────────────────────────
= The Agent Protocol
// ─────────────────────────────────────────────────────────────────────────────

When the daemon spawns the agent it injects a *cryochamber protocol block* at
the top of the prompt. This block instructs the agent to use `cryo-agent`
subcommands for structured communication.

#callout(color: amber, icon: "📋", title: "Session Workflow (as seen by the agent)")[
  #set list(marker: "→")
  - *Step 1 — Orient*: Read `plan.md`, run `cryo-agent todo list`, check inbox.
  - *Step 2 — Work*: Do the task. Reply to inbox messages with `cryo-agent reply`.
  - *Step 3 — Record*: `cryo-agent note "what I did and what's next"`.
  - *Step 4 — Schedule*: `cryo-agent todo add "next task" --at <TIME>` using `cryo-agent time "+2 hours"`.
  - *Step 5 — Hibernate*: `cryo-agent hibernate --summary "…"` (must be the last command).
]

#table(
  columns: (auto, 1fr),
  stroke: 0.5pt + luma(180),
  inset: 7pt,
  fill: (col, row) => if row == 0 { teal.lighten(82%) } else { white },
  [*cryo-agent command*], [*What it does*],
  [`hibernate --summary "…"`], [End the session. Daemon logs the outcome and schedules the next wake.],
  [`hibernate --complete`], [Mark the overall project as finished.],
  [`hibernate --exit 1 --summary "…"`], [Signal a failure / blocked state.],
  [`note "text"`], [Append a note to the session log. Readable by the next session via `parse_latest_session_notes`.],
  [`send "message"`], [Write a message to `messages/outbox/`.],
  [`reply "message"`], [Reply to the current inbox messages.],
  [`receive`], [Read `messages/inbox/` (local — no daemon needed).],
  [`alert <action> <target> "msg"`], [Set a dead-man switch: if the agent doesn't wake on time, fire `<action>` to `<target>`.],
  [`todo add "task" --at <ISO>`], [Add a scheduled TODO item. The daemon wakes at the earliest pending time.],
  [`todo list`], [List all pending TODO items.],
  [`todo done <id>`], [Mark a TODO item as completed.],
  [`time`], [Print current ISO8601 local time.],
  [`time "+2 hours"`], [Compute and print a future ISO8601 time.],
)

// ─────────────────────────────────────────────────────────────────────────────
= Human ↔ Agent Communication
// ─────────────────────────────────────────────────────────────────────────────

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let bst  = (paint: ocean,  thickness: 1.2pt)
    let chst = (paint: rose,   thickness: 1.1pt)
    let fst  = (paint: sage,   thickness: 1.1pt)
    let arrow = (end: "straight")

    // ── boxes ─────────────────────────────────────────────────────────────
    rect((0,  3.2), (2.6, 4.4), fill: amber.lighten(75%), stroke: bst, name: "human")
    content("human.center", align(center)[*Human*])

    rect((4.2, 3.2), (7.4, 4.4), fill: ocean.lighten(82%), stroke: bst, name: "daemon")
    content("daemon.center", align(center)[*Daemon*])

    rect((9.2, 3.2), (12, 4.4), fill: teal.lighten(80%), stroke: bst, name: "agent")
    content("agent.center", align(center)[*Agent*])

    // inbox folder
    rect((4.2, 1.2), (7.4, 2.4), fill: sage.lighten(80%), stroke: fst, name: "inbox")
    content("inbox.center", align(center)[`messages/inbox/`])

    // outbox folder
    rect((4.2, 5.2), (7.4, 6.4), fill: sage.lighten(80%), stroke: fst, name: "outbox")
    content("outbox.center", align(center)[`messages/outbox/`])

    // channels
    rect((0, 1.2), (2.6, 2.4), fill: rose.lighten(82%), stroke: chst, name: "ch-in")
    content("ch-in.center", align(center)[Channels\ (GitHub/Zulip)])

    rect((0, 5.2), (2.6, 6.4), fill: rose.lighten(82%), stroke: chst, name: "ch-out")
    content("ch-out.center", align(center)[Channels\ (GitHub/Zulip)])

    // ── arrows ────────────────────────────────────────────────────────────
    // human → inbox (direct)
    line("human.south", (1.3, 2.4), "inbox.west",
         mark: arrow, stroke: bst)
    content(((1.3, 2.8)), [`cryo send`], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // channels → inbox (sync pull)
    line("ch-in.east", "inbox.west",
         mark: arrow, stroke: chst)
    content(("ch-in.east", 50%, "inbox.west"), [pull], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // human → channels (post message)
    line("human.south", "ch-in.north",
         mark: arrow, stroke: chst)

    // inbox → daemon (watch / wake)
    line("inbox.north", "daemon.south",
         mark: arrow, stroke: fst)
    content(("inbox.north", 60%, "daemon.south"), [wake +\ read], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // daemon → agent (inject prompt)
    line("daemon.east", "agent.west",
         mark: arrow, stroke: bst)
    content(("daemon.east", 50%, "agent.west"), [prompt], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // agent → outbox
    line("agent.south", (10.6, 2.4), "outbox.east",
         mark: arrow, stroke: (paint: teal, thickness: 1.1pt))
    content(((10.6, 2.8)), [`cryo-agent send`], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // outbox → human
    line("outbox.west", (1.3, 5.8), "human.north",
         mark: arrow, stroke: bst)
    content(((1.5, 5.1)), [`cryo receive`], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // outbox → channels (push)
    line("outbox.west", "ch-out.east",
         mark: arrow, stroke: chst)
    content(("outbox.west", 50%, "ch-out.east"), [push], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // channels → human
    line("ch-out.north", "human.south",
         mark: arrow, stroke: chst)
  }),
  caption: [Human ↔ agent communication flow.],
)

== Direct Messaging (local)

The simplest channel — no external accounts needed:

#cmd[
  ```
  # Human writes
  cryo send "Please add unit tests for the auth module"

  # Human reads replies
  cryo receive
  ```
]

== GitHub Discussions Sync

#cmd[
  ```
  cryo-gh init --repo owner/repo --discussion 42
  cryo-gh sync          # start background sync daemon
  cryo-gh status        # check sync configuration
  cryo-gh unsync        # stop background sync
  ```
]

New comments on the GitHub Discussion are pulled to `messages/inbox/` on each
sync cycle. Session summaries are pushed back as new comments.

== Zulip Sync

#cmd[
  ```
  cryo-zulip init --config ~/.zuliprc --stream "dev"
  cryo-zulip sync       # start background sync daemon
  cryo-zulip status     # check sync state
  ```
]

Messages posted in the configured Zulip stream appear in the inbox; the agent's
replies are posted back to the stream.

// ─────────────────────────────────────────────────────────────────────────────
= Configuration Reference
// ─────────────────────────────────────────────────────────────────────────────

The project configuration lives in `cryo.toml`:

#cmd[
  ```toml
  agent = "opencode run"           # default agent command
  max_retries = 3                  # crash retries before fallback alert
  max_session_duration = 1800      # session timeout in seconds (0 = unlimited)
  watch_inbox = true               # inotify-based reactive wake on new inbox msg
  fallback_alert = "outbox"        # "outbox" | "notify" | "none"
  report_interval_hours = 24       # 0 = disable periodic reports
  report_time = "09:00"            # wall-clock time for daily report

  # Multiple providers for automatic rotation on failure:
  [[providers]]
  name = "primary"
  env  = { OPENAI_API_KEY = "sk-…" }

  [[providers]]
  name = "fallback"
  env  = { ANTHROPIC_API_KEY = "sk-ant-…" }
  ```
]

#table(
  columns: (auto, auto, 1fr),
  stroke: 0.5pt + luma(180),
  inset: 7pt,
  fill: (col, row) => if row == 0 { ocean.lighten(88%) } else { white },
  [*Key*], [*Default*], [*Description*],
  [`agent`], [`opencode run`], [Shell command used to spawn the agent subprocess.],
  [`max_retries`], [`3`], [Consecutive crash retries before giving up and sending a fallback alert.],
  [`max_session_duration`], [`0`], [Maximum seconds per session. `0` means unlimited.],
  [`watch_inbox`], [`true`], [Use filesystem events to wake the daemon immediately on new inbox message.],
  [`fallback_alert`], [`"outbox"`], [What to do when retries are exhausted: write to outbox, send desktop notification, or nothing.],
  [`report_interval_hours`], [`0`], [Period for periodic session-count desktop notifications. `0` disables.],
  [`report_time`], [`"09:00"`], [Wall-clock time to align daily reports to.],
  [`providers`], [`[]`], [List of AI provider configurations rotated on failure. Each entry can set env vars.],
)

// ─────────────────────────────────────────────────────────────────────────────
= Runtime File Reference
// ─────────────────────────────────────────────────────────────────────────────

#table(
  columns: (auto, 1fr),
  stroke: 0.5pt + luma(180),
  inset: 7pt,
  fill: (col, row) => if row == 0 { sage.lighten(70%) } else { white },
  [*File / Directory*], [*Purpose*],
  [`cryo.toml`], [Project config. Hand-edited. Created by `cryo init`.],
  [`plan.md`], [Task description injected into every agent session.],
  [`CLAUDE.md` / `AGENTS.md`], [Cryochamber protocol for the agent (auto-generated by `cryo init`).],
  [`timer.json`], [Runtime state: session number, PID lock, retry count, CLI overrides. Written by daemon.],
  [`cryo.log`], [Append-only structured event log. Read by `cryo log` / `cryo watch`.],
  [`cryo-agent.log`], [Raw stdout/stderr from the agent subprocess.],
  [`todo.json`], [Pending TODO items. Drives wake schedule.],
  [`messages/inbox/`], [Incoming messages for the agent. Written by `cryo send` and channel sync.],
  [`messages/outbox/`], [Agent replies. Read by `cryo receive` and pushed by channel sync.],
  [`messages/inbox/archive/`], [Processed (already read) inbox messages.],
  [`.cryo/cryo.sock`], [Unix domain socket for agent↔daemon IPC (Unix only).],
  [`gh-sync.json`], [GitHub Discussion sync state (last cursor, pushed session). Created by `cryo-gh init`.],
  [`zulip-sync.json`], [Zulip sync state (last message ID, pushed session). Created by `cryo-zulip init`.],
)

// ─────────────────────────────────────────────────────────────────────────────
= A Complete Example Walk-Through
// ─────────────────────────────────────────────────────────────────────────────

The following traces a single turn of a long-running coding task.

#callout(color: ocean, icon: "1", title: "Operator sets up the project")[
  #cmd[
    ```
    mkdir chess-engine && cd chess-engine
    cat > plan.md << 'EOF'
    # Chess Engine
    Implement a UCI-compatible chess engine in Rust.
    Focus on alpha-beta pruning this session.
    EOF
    cryo init --agent "opencode run"
    cryo start
    ```
  ]
  The daemon starts; the agent wakes immediately (session 1).
]

#callout(color: teal, icon: "2", title: "Agent works (session 1)")[
  #cmd[
    ```
    # Inside the agent process — cryo-agent commands issued by the LLM
    cryo-agent todo add "Implement move generation" --at 2026-03-07T09:00
    cryo-agent note "Completed board representation. Next: move generation."
    cryo-agent hibernate --summary "Board struct done. Scheduled move gen for tomorrow."
    ```
  ]
  Daemon logs `--- CRYO END ---`, reads the TODO, and sleeps until 09:00.
]

#callout(color: amber, icon: "3", title: "Human sends a mid-sleep message")[
  #cmd[
    ```
    cryo send "Also add perft tests for debugging"
    ```
  ]
  The daemon's inbox watcher fires and re-wakes the agent early (session 2 starts).
]

#callout(color: sage, icon: "4", title: "Operator checks progress")[
  #cmd[
    ```
    cryo status
    # → Session: 3 | Next wake: 2026-03-08T09:00 | Agent: opencode run
    cryo receive
    # → Message from agent: "Move generation complete. Perft(5) = 4865609 ✓"
    ```
  ]
]

// ─────────────────────────────────────────────────────────────────────────────
= Provider Rotation
// ─────────────────────────────────────────────────────────────────────────────

When multiple `[[providers]]` are defined in `cryo.toml`, the daemon rotates
through them after consecutive failures. This is useful for:

- Rate-limit resilience (rotate to a different API key or model)
- Multi-model experimentation (try GPT-4 then Claude on failure)
- Budget control (cheap model first, expensive model as fallback)

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *
    let st = (paint: ocean, thickness: 1.1pt)
    let arrow = (end: "stealth")

    let providers = ("Provider A", "Provider B", "Provider C")
    for (i, name) in providers.enumerate() {
      let x = float(i) * 4.0
      rect((x, 0), (x + 3.2, 1.2),
           fill: ocean.lighten(82%), stroke: st, name: "p" + str(i))
      content("p" + str(i) + ".center", align(center)[#name])
    }

    // arrows between providers
    line("p0.east", "p1.west", mark: arrow, stroke: (paint: rose, thickness: 1.2pt))
    content(("p0.east", 50%, "p1.west"), [fail], fill: white, frame: "rect", stroke: none, padding: 0.06)

    line("p1.east", "p2.west", mark: arrow, stroke: (paint: rose, thickness: 1.2pt))
    content(("p1.east", 50%, "p2.west"), [fail], fill: white, frame: "rect", stroke: none, padding: 0.06)

    // wrap-around
    bezier("p2.south", "p0.south",
           (11.2, -1.4), (0, -1.4),
           mark: arrow, stroke: (paint: rose, thickness: 1pt, dash: "dashed"))
    content((5.6, -1.8), [wrap-around (all exhausted → fallback alert)],
            fill: white, frame: "rect", stroke: none, padding: 0.08)
  }),
  caption: [Provider rotation sequence on consecutive failures.],
)

// ─────────────────────────────────────────────────────────────────────────────
= Tips for Developers
// ─────────────────────────────────────────────────────────────────────────────

#callout(color: slate, icon: "🔧", title: "Development workflow")[
  #cmd[
    ```
    # run all tests
    cargo test

    # lint (warnings are errors in CI)
    cargo clippy --all-targets -- -D warnings

    # measure coverage
    make coverage

    # run the included mr-lazy example end-to-end
    make example DIR=examples/mr-lazy
    make example-cancel DIR=examples/mr-lazy
    ```
  ]
]

Key entry points for contributors:

#table(
  columns: (auto, 1fr),
  stroke: 0.5pt + luma(180),
  inset: 7pt,
  fill: (col, row) => if row == 0 { slate.lighten(72%) } else { white },
  [*File*], [*Start here if you want to …*],
  [`src/daemon.rs`], [Understand or modify the session event loop, retry logic, wake detection.],
  [`src/bin/cryo.rs`], [Add or change operator-facing CLI commands.],
  [`src/bin/cryo_agent.rs`], [Add or change agent-facing IPC commands.],
  [`src/platform/`], [Port to a new OS or fix platform-specific behaviour.],
  [`src/channel/zulip.rs`], [Understand the Zulip HTTP client or add a new channel.],
  [`src/agent.rs`], [Change how agent prompts are built or how the subprocess is spawned.],
  [`templates/`], [Edit the default protocol, plan, or config injected by `cryo init`.],
  [`tests/`], [Add integration tests. CLI tests use `assert_cmd`; HTTP tests use a minimal inline mock server.],
)

#v(1cm)
#line(length: 100%, stroke: 0.5pt + luma(200))
#v(0.3cm)
#align(center)[
  #text(size: 9pt, fill: slate)[
    Cryochamber is MIT-licensed. Source: #link("https://github.com/GiggleLiu/cryochamber") \
    Documentation: #link("https://giggleliu.github.io/cryochamber")
  ]
]

