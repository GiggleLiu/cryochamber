---
name: daemon-state-diagram
description: Use when the user wants visual state machines of the cryochamber daemon — the overall event loop around inbox messages and TODO-triggered wakes, plus per-item lifecycles for a message and for a reminder — rendered as Typst PDFs via the Fletcher diagram package
---

# Daemon State Diagrams (Typst + Fletcher)

## Overview

Produce up to three diagrams, each living in its own `.typ` / `.pdf` pair
under `docs/diagrams/`:

1. **`daemon-state`** — one-page overview of the daemon's event loop
   around inbox messages and past-due TODOs (the default output).
2. **`lifecycles`** — two pages: how a single inbox message travels from
   arrival to archive, and how a single TODO (reminder) travels from
   creation through due / consumed / cleanly-finished or retried.

Decide which diagrams to produce from the user's ask. "Show how the daemon
reacts" → diagram 1. "Show how a message / TODO is consumed" → diagram 2.
"Both" → both files.

All diagrams are drawn with [`fletcher`](https://typst.app/universe/package/fletcher),
a node-and-arrow diagram package built on CeTZ and designed for exactly
this kind of graph (state machines, commutative diagrams, flowcharts).

## Visual style (applies to every diagram)

- **State name on top, plain-language description below in small gray
  text** — never dump code identifiers into the node body. Helper:
  ```typst
  #let sub(body) = text(size: 7pt, fill: rgb("#666666"), body)
  #let state(title, desc) = align(center)[*#title* \ #sub(desc)]
  ```
- **Events live on the edges, not in the nodes.** Use a small white pill
  so labels cut through crossings cleanly:
  ```typst
  #let elabel(body) = box(
    fill: white, inset: (x: 3pt, y: 1pt), radius: 1.5pt,
    text(size: 7.5pt, body),
  )
  ```
- **Translate Rust identifiers into plain English.** Instead of
  `WakeFromSchedule` / `InboxChanged` / `consume_past_due` /
  `reschedule_consumed`, write "a reminder came due" / "new mail arrived" /
  "pick up every reminder whose time has passed" / "retry with a longer
  delay". The caption block is the right place for the one or two
  precise rules (hibernate precondition, unanswered-mail fallback,
  `2^k`-minute backoff cap).
- **Consistent palette.** Blue = idle / waiting, orange = in-flight /
  active, green = done / finalised, red = crash and retry branch
  (`rgb("#aa3333")`, dashed stroke).
- **Shape per role.** `node-shape: fletcher.shapes.rect` is the default
  for ordinary states; use `shape: fletcher.shapes.pill` for entry /
  exit states. Fletcher 0.5.8 has no `stadium` alias — it's `pill`.

Why fletcher rather than raw CeTZ:
- Named nodes plus `edge(<a>, <b>, "->")` syntax — no manual anchor math.
- Automatic layout via the `(x, y)` grid coordinate system.
- Built-in arrow mark shorthand (`"->"`, `"=>"`, `"-|>"`, etc.) — the
  `mark: (end: "straight")` stroke dict from `.claude/rules/typst.md` is still
  the fallback when a raw CeTZ `line` is needed.

## Workflow

1. **Analyse the daemon sources.** Read these files (in order) and summarise
   the state transitions around wake events, inbox handling, session run,
   and crash retry:
   - `src/daemon.rs` — top-level event loop, SIGUSR1 handling, inbox watcher
   - `src/daemon/schedule.rs` — how the next wake time is computed
   - `src/daemon/session.rs` — session launch and finalisation
   - `src/daemon/request.rs` — IPC request dispatch (`Receive`, `Reply`, `TodoAdd`, …)
   - `src/daemon/effects.rs` — `SessionEffects` / `FsSessionEffects`
   - `src/daemon/inbox.rs` — `SessionInboxState` (claim / reply / fallback)
   - `src/todo.rs` — `consume_past_due`, `reschedule_consumed`, `bump_attempt`,
     `retry_delay_minutes`
   - `src/message.rs` + `src/channel/store.rs` — `MessageStore` claim /
     archive / restore for `messages/inbox/pending/`
   Keep the summary in the conversation; do not invent a separate markdown file.

2. **Identify the states.** Minimum set (add more only if the summary
   surfaces them):
   - `Idle` — daemon sleeping on 250 ms ticks; serves `Ping`, `Hello`,
     `Receive`, `Todo*`; refuses `Send` / `Reply` / `Hibernate`.
   - `Triggered` — `WakeFromSchedule` / `InboxChanged` / SIGUSR1 sets `run_now`.
   - `ConsumeTodos` — `TodoFile::consume_past_due` marks due items `done` and
     returns `Vec<(text, at)>`.
   - `RunSession` + `Active IPC` — agent spawned; handles `Send`, `Receive`
     (claim into `inbox/pending/`), `Reply` (outbox + archive pending),
     `Hibernate`, `Todo*`.
   - `Exit / Interrupt` — child exited or shutdown or timeout
     (`resolve_child_exit` / `resolve_interrupted_session`).
   - `Finalize` — `finalize_human_replies` writes daemon fallback reply for
     unanswered inbox, archives the pending batch, `EventLogger::finish`.
   - `decide_next_step` — `PlanComplete` | `Hibernate` | `RotateProvider`.
   - `Crash path` — `reschedule_consumed_after_crash` → `bump_attempt` + `2^k`
     minutes (capped at 1 day) → new TODO feeds back into `ConsumeTodos`.
   Use the real identifiers from the code for transition labels
   (`InboxChanged`, `WakeFromSchedule`, `run_now`, `is_crash()`, `Hibernate`,
   `PlanComplete`, `ValidationFailed`, …).

3. **Write the Typst file** at `docs/diagrams/daemon-state.typ` (create the
   directory if missing). Use this fletcher skeleton:

   ```typst
   #import "@preview/fletcher:0.5.8" as fletcher: diagram, node, edge

   #set page(width: auto, height: auto, margin: 8mm)
   #set text(size: 9pt)

   #diagram(
     node-stroke: 0.6pt,
     node-corner-radius: 3pt,
     spacing: (14mm, 10mm),

     // column 0: idle/trigger/consume (top row)
     node((0, 0), [*Idle*\ 250 ms tick], name: <idle>, fill: rgb("#eef5ff")),
     node((1, 0), [*Triggered*\ WakeFromSchedule\ InboxChanged / SIGUSR1],
          name: <trig>, fill: rgb("#eef5ff")),
     node((2, 0), [*ConsumeTodos*\ consume_past_due],
          name: <consume>, fill: rgb("#eef5ff")),

     // middle row: active session
     node((0, 1), [*Active IPC*\ Send · Receive\ Reply · Todo\*],
          name: <ipc>, fill: rgb("#fff5e6")),
     node((1, 1), [*RunSession*\ spawn agent],
          name: <run>, fill: rgb("#fff5e6")),
     node((2, 1), [*Exit / Interrupt*\ child exit /\ shutdown / timeout],
          name: <exit>, fill: rgb("#fff5e6")),

     // bottom row: finalize/decide/done + crash
     node((0, 2), [*PlanComplete*\ break loop],
          name: <done>, fill: rgb("#eaf7ea")),
     node((1, 2), [*Finalize*\ fallback reply\ EventLogger::finish],
          name: <fin>, fill: rgb("#eaf7ea")),
     node((2, 2), [*decide_next_step*\ Hibernate / Rotate],
          name: <decide>, fill: rgb("#eaf7ea")),

     // crash box, offset below the decide node
     node((2, 3), [*Crash path*\ reschedule_consumed\ (attempt k), 2^k min],
          name: <crash>, fill: rgb("#fde8e8")),

     // forward transitions
     edge(<idle>, <trig>, "->", [wake]),
     edge(<trig>, <consume>, "->", [run_now]),
     edge(<consume>, <run>, "->", [spawn], bend: -20deg),
     edge(<run>, <ipc>, "<->", [handle_active_request]),
     edge(<run>, <exit>, "->", [try_wait]),
     edge(<exit>, <fin>, "->", [finalize_human_replies], bend: 20deg),
     edge(<fin>, <done>, "->", [PlanComplete]),
     edge(<fin>, <decide>, "->", [Hibernate / Fail]),

     // loop back to idle via a bent edge above the top row
     edge(<decide>, <idle>, "->", [next_wake_from_todos],
          bend: -50deg, label-pos: 0.5),

     // crash branch — red dashed
     edge(<decide>, <crash>, "->",
          stroke: (paint: rgb("#aa3333"), dash: "dashed"),
          [is_crash()]),
     edge(<crash>, <consume>, "->",
          stroke: (paint: rgb("#aa3333"), dash: "dashed"),
          [new TODO (attempt k)], bend: 40deg),
   )
   ```

   - Pin `@preview/fletcher:0.5.8` (or the latest resolvable version in the
     local typst cache — run a one-line probe if unsure).
   - Grid coordinates are `(column, row)` with y growing **downward**. Keep
     the layout tight; fletcher auto-sizes nodes to their content.
   - Use `bend: Ndeg` for curved edges, `label-pos: 0..1` to slide labels
     along the edge, and `"->"`, `"<->"`, `"=>"`, `"-|>"` for the common
     arrow shapes.
   - For crash / fallback styling, pass `stroke: (paint: ..., dash: "dashed")`
     to `edge`; node shading is plain `fill:`.
   - If you need anything fletcher cannot express (e.g. a decorative
     background shade over a subgraph), drop to a `fletcher.cetz-canvas` or
     embed `cetz.canvas({ … })` beneath the diagram.

4. **Compile.** From the project root:

   ```bash
   typst compile docs/diagrams/daemon-state.typ docs/diagrams/daemon-state.pdf
   ```

   Fletcher 0.5.8 pulls in cetz 0.3.4 and compiles with zero warnings. If
   you ever see a `path` → `curve` deprecation, it means fletcher < 0.5.8
   was resolved — bump the pin. Fix real errors in the `.typ` file; never
   ship a broken source.

5. **Ask the user whether to open the PDF.** After a successful compile,
   show the output path and ask a single yes/no question:

   > Compiled `docs/diagrams/daemon-state.pdf`. Open it now? (y/n)

   On `y` (macOS), run `open docs/diagrams/daemon-state.pdf`. On Linux try
   `xdg-open`. Do **not** open the PDF without explicit consent.

## Files Produced

- `docs/diagrams/daemon-state.typ` + `.pdf` — overall event-loop state machine.
- `docs/diagrams/lifecycles.typ` + `.pdf` — page 1 "How a message travels
  through the chamber" (Arrived → Waiting → Claimed → Answered by
  agent / chamber → Archived, with crash restoring the pending batch);
  page 2 "How a reminder (TODO) gets consumed" (Created → Waiting → Due
  → Consumed → Cleanly Finished, with crash creating a new reminder
  whose delay doubles each attempt).

The `.typ` files are commit-worthy; the generated `.pdf` files are
typically gitignored.

## Message lifecycle (for the `lifecycles` diagram)

States, in order:
1. **Arrived** — file landed in `messages/inbox/` (external sync or
   direct drop).
2. **Waiting to be Read** — file sits in the inbox; the daemon uses its
   existence to schedule a wake but does not preview the body.
3. **Claimed by Agent** — `cryo-agent receive` routed through the
   daemon moved the file to `messages/inbox/pending/`.
4a. **Answered by Agent** — `cryo-agent reply` wrote an outbox message
    and archived the pending batch.
4b. **Answered by Chamber** — session ended unanswered, so the daemon
    wrote a `from: cryochamber` fallback and archived the batch.
5. **Archived** — file rests under `messages/inbox/archive/` and will
   not be re-delivered.

Crash path: *Claimed* → *Waiting to be Read* via
`MessageStore::restore_pending_inbox` (the same call
`recover_pending_inbox` uses on daemon restart).

## TODO lifecycle (for the `lifecycles` diagram)

1. **Created** — agent called `cryo-agent todo add --at TIME`.
2. **Waiting** — item sits in `todo.json` with a future `at`.
3. **Due** — `at ≤ now`; the scheduler picks it up on the next tick.
4. **Consumed** — the daemon flagged it `done = true` and started a
   session.
5a. **Cleanly Finished** — the session hibernated politely; the item
    stays done.
5b. **Retried Later** — the session crashed; `reschedule_consumed`
    created a **new** item whose text is suffixed `(attempt k)` and
    whose `at = now + 2^k` minutes (capped at 1 day). That new item
    re-enters *Waiting*.

Early exits from *Waiting*: **Marked Done** (`cryo-agent todo done`)
and **Removed** (`cryo-agent todo remove`).

## Quick Reference

| Daemon concept | Code location |
|----------------|---------------|
| Event loop / wake | `src/daemon.rs::run_event_loop` |
| Next wake time | `src/daemon/schedule.rs::next_wake_from_todos` |
| Session IPC dispatch | `src/daemon.rs::handle_active_request` |
| Hibernate gate | `src/daemon/request.rs::resolve_hibernate_request` |
| Inbox claim / archive | `src/channel/store.rs::MessageStore` |
| TODO consume + retry | `src/todo.rs` (`consume_past_due`, `bump_attempt`, `retry_delay_minutes`) |
| EventLogger + cryo.log | `src/log.rs` |

| Fletcher need | Pattern |
|---------------|---------|
| State node | `node((col, row), [*Name*\ detail], name: <id>, fill: rgb("..."))` |
| Arrow | `edge(<a>, <b>, "->", [label])` |
| Bidirectional | `edge(<a>, <b>, "<->", [label])` |
| Curved edge | `edge(<a>, <b>, "->", [label], bend: 30deg)` |
| Dashed / coloured | `stroke: (paint: rgb("#aa3333"), dash: "dashed")` |
| Label position | `label-pos: 0.2` (0 = source, 1 = target, default 0.5) |
| Cetz escape hatch | `fletcher.cetz-canvas({ … })` when you need raw CeTZ primitives |

## Common Mistakes

- **Drawing from memory.** The daemon re-injects crashed TODOs with
  `2^k`-minute backoff capped at 1 day, and it does *not* preview inbox bodies
  in the wake prompt — verify in `src/todo.rs` and `src/daemon.rs` before
  labelling.
- **Forgetting node `name:`.** Edges reference nodes by label; unnamed nodes
  can't be connected.
- **Coordinate confusion.** Fletcher uses `(column, row)` with row growing
  downward; cetz uses `(x, y)` with y growing upward. Don't mix.
- **Using raw cetz for edges.** Use `edge(...)`; only drop to cetz for
  backgrounds or custom decorations.
- **Opening the PDF without asking.** The last step is a question, not an
  action.
- **Writing a markdown state-table next to the PDF.** The PDF is the artifact;
  extra prose belongs in the conversation or the diagram's caption block.
