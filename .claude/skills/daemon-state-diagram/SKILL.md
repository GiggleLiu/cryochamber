---
name: daemon-state-diagram
description: Use when the user wants visual state machines of the cryochamber daemon — the overall event loop around inbox messages and TODO-triggered wakes, plus per-item lifecycles for a message and for a reminder — rendered as hand-written SVGs
---

# Daemon State Diagrams (hand-written SVG)

## Overview

Produce up to two SVG files under `docs/diagrams/`:

1. **`daemon-state.svg`** — one-page overview of the daemon's event loop
   around inbox messages and past-due TODOs (the default output).
2. **`lifecycles.svg`** — two stacked sections: how a single inbox
   message travels from arrival to reply, and how a single TODO
   (reminder) travels from creation through due / consumed /
   cleanly-finished or retried.

Decide which diagrams to produce from the user's ask. "Show how the daemon
reacts" → diagram 1. "Show how a message / TODO is consumed" → diagram 2.
"Both" → both files.

**Why hand-written SVG.** No build tool, no package pinning, no compile
step. The `.svg` drops straight into a browser, README, mdBook, or
GitHub preview. Fixed layouts like these state machines don't benefit
from auto-layout — pixel-perfect placement is actually an advantage
here. The cost is that the agent has to pick coordinates; the style
system below makes that cheap.

## Visual style (applies to every diagram)

- **State name on top, plain-language description below.** Use a `<g>`
  group per node with a `<rect>` for the shape and stacked `<text>`
  lines: one `.title` (bold, dark) and 1–3 `.desc` lines (10px, gray).
- **Events live on the edges, not in the nodes.** Every edge label is
  a `<text class="elabel">` with a paint-order halo so it cuts through
  crossing lines cleanly (`paint-order: stroke; stroke: white;
  stroke-width: 3.5;`).
- **Translate Rust identifiers into plain English.** Instead of
  `WakeFromSchedule` / `InboxChanged` / `consume_past_due` /
  `reschedule_consumed`, write "a reminder came due" / "new mail
  arrived" / "pick up every reminder whose time has passed" / "retry
  with a longer delay". The caption / legend block is the right place
  for the precise rules (hibernate precondition, unanswered-mail
  fallback, `2^k`-minute backoff cap).
- **Consistent palette.** Four semantic colours, each a (fill, stroke)
  pair:
  - `idle`   = `#eef5ff` / `#6b9bd8` — blue, daemon sleeping / waiting
  - `active` = `#fff5e6` / `#d8a15b` — orange, in-flight / agent running
  - `done`   = `#eaf7ea` / `#6bb06b` — green, finalised / terminal
  - `crash`  = `#fde8e8` / `#aa3333` — red, crash and retry branch
  - `entry`  = `#f3f3f3` / `#a0a0a0` — neutral pill for entry / exit
- **Shape per role.** Rounded rectangles (`rx="10"`) for ordinary
  states, pill shapes (`rx="32"` on a ~64px-tall rect) for entry /
  exit states. Crash branch edges use `stroke-dasharray: 5 3.5` and
  the red stroke colour.
- **Soft drop shadow on every node** via a single `<filter id="shadow">`
  — `feDropShadow dx="0.8" dy="1.6" stdDeviation="1.4" flood-opacity="0.2"`
  is the house style. Makes the diagram read as layered rather than
  flat, without looking skeuomorphic.

## Starter template

Every diagram starts from the same skeleton. Copy this, then edit
nodes and edges — do **not** rebuild the style block from scratch.

```xml
<svg xmlns="http://www.w3.org/2000/svg"
     viewBox="-40 -95 1010 790"
     font-family="-apple-system, system-ui, 'Segoe UI', sans-serif">
  <defs>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0.8" dy="1.6" stdDeviation="1.4" flood-opacity="0.2"/>
    </filter>
    <marker id="arrow" viewBox="0 0 10 10" refX="8" refY="5"
            markerWidth="8" markerHeight="8" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="#4a5568"/>
    </marker>
    <marker id="arrow-red" viewBox="0 0 10 10" refX="8" refY="5"
            markerWidth="8" markerHeight="8" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="#aa3333"/>
    </marker>
    <style><![CDATA[
      .node   { stroke-width: 1.3; }
      .entry  { fill: #f3f3f3; stroke: #a0a0a0; }
      .idle   { fill: #eef5ff; stroke: #6b9bd8; }
      .active { fill: #fff5e6; stroke: #d8a15b; }
      .done   { fill: #eaf7ea; stroke: #6bb06b; }
      .crash  { fill: #fde8e8; stroke: #aa3333; }
      .title  { font-size: 13px; font-weight: 600; fill: #1e2838; text-anchor: middle; }
      .desc   { font-size: 10px; fill: #6a6e7a; text-anchor: middle; }
      .edge     { stroke: #4a5568; stroke-width: 1.3; fill: none; }
      .edge-red { stroke: #aa3333; stroke-width: 1.3; fill: none;
                  stroke-dasharray: 5 3.5; }
      .elabel, .elabel-red {
        font-size: 10px; text-anchor: middle; font-weight: 500;
        paint-order: stroke; stroke: white; stroke-width: 3.5;
        stroke-linejoin: round;
      }
      .elabel     { fill: #222; }
      .elabel-red { fill: #aa3333; }
      .big-title { font-size: 17px; font-weight: 700; fill: #1e2838; text-anchor: middle; }
    ]]></style>
  </defs>

  <text x="450" y="-65" class="big-title">Diagram Title Here</text>

  <!-- Node template: rounded rect with two-line description -->
  <g transform="translate(CX,CY)">
    <rect class="node idle" x="-90" y="-35" width="180" height="70"
          rx="10" filter="url(#shadow)"/>
    <text class="title" y="-8">State Name</text>
    <text class="desc"  y="8">first line of description,</text>
    <text class="desc"  y="22">second line if needed</text>
  </g>

  <!-- Pill template (entry / exit): short, fully rounded -->
  <g transform="translate(CX,CY)">
    <rect class="node entry" x="-80" y="-32" width="160" height="64"
          rx="32" filter="url(#shadow)"/>
    <text class="title" y="-6">Entry / Exit</text>
    <text class="desc"  y="10">short description</text>
    <text class="desc"  y="23">(two lines max)</text>
  </g>

  <!-- Straight edge template -->
  <path class="edge" d="M X1,Y1 L X2,Y2" marker-end="url(#arrow)"/>
  <text class="elabel" x="LX" y="LY">event name</text>

  <!-- Curved edge template (Q = quadratic Bézier) -->
  <path class="edge" d="M X1,Y1 Q CX,CY X2,Y2" marker-end="url(#arrow)"/>

  <!-- Loop-back arc template (C = cubic Bézier, pulls above the row) -->
  <path class="edge" d="M X1,Y1 C X1,-55 X2,-55 X2,Y2" marker-end="url(#arrow)"/>

  <!-- Red dashed crash edge -->
  <path class="edge-red" d="M X1,Y1 L X2,Y2" marker-end="url(#arrow-red)"/>
  <text class="elabel-red" x="LX" y="LY">crashed</text>

</svg>
```

## Layout rules

- **Grid.** Use three columns for the daemon-state diagram (column
  centres ~140 / 450 / 760, ~310px apart) and four rows (centres
  ~90 / 270 / 450 / 620, ~180px apart). This keeps nodes visually
  aligned without a layout engine.
- **Anchor coordinates to node edges, not centres.** A node at
  centre `(cx, cy)` with width 180 has left edge at `cx-90` and right
  edge at `cx+90`. Edges should start and end at those anchors so the
  arrowhead doesn't overlap the rect — e.g. `M 230,90 L 358,90` for
  two nodes with centres at `(140,90)` and `(450,90)`, leaving 2–5 px
  of air before the arrowhead.
- **Curved edges** use `Q cx,cy x2,y2` (one control point) for gentle
  diagonals between neighbouring cells, and `C x,y x,y x,y` (two
  control points) for loop-back arcs that pass above or below the
  main grid.
- **Bidirectional edges** get both `marker-start` and `marker-end`
  pointing at the same marker. `orient="auto-start-reverse"` on the
  marker definition makes the start arrow flip automatically.
- **Label placement.** For horizontal edges, put the label ~8 px
  above the line. For curved edges, put the label at roughly the
  Bézier midpoint — if the curve crosses other lines, the white halo
  will still make the label readable.
- **viewBox with generous top margin** when a loop-back arc exits the
  top of the grid (daemon-state's `decide → idle` arc peaks around
  y=−35). `viewBox="-40 -95 1010 790"` is the working frame for the
  daemon-state diagram; the lifecycles diagram uses a shorter top
  margin because nothing loops above.

## Workflow

1. **Analyse the daemon sources.** Read these files (in order) and
   summarise the state transitions around wake events, inbox handling,
   session run, and crash retry:
   - `src/daemon.rs` — top-level event loop, inbox watcher, hibernate
     reply window (parked hibernate released by mail / due TODO / expiry)
   - `src/daemon/schedule.rs` — how the next wake time is computed
   - `src/daemon/session.rs` — session launch and finalisation
   - `src/daemon/request.rs` — IPC request dispatch (`Receive`, `Send`,
     `TodoAdd`, …)
   - `src/daemon/effects.rs` — `SessionEffects` / `FsSessionEffects`
   - `src/daemon/inbox.rs` — `SessionInboxState` (which tracks the
     *reply obligation* for a received batch; the file itself is
     archived immediately at receive time)
   - `src/todo.rs` — `consume_past_due`, `reschedule_consumed`,
     `bump_attempt`, `retry_delay_minutes`
   - `src/message.rs` + `src/channel/store.rs` — `MessageStore`
     `read_and_archive_inbox` (no pending area; receive → archive
     atomically)
   Keep the summary in the conversation; do not invent a separate
   markdown file.

2. **Identify the states.** Minimum set for `daemon-state.svg` (add
   more only if the summary surfaces them):
   - `Idle` — daemon sleeping on 250 ms ticks; serves `Ping`, `Hello`,
     `Receive`, `Todo*`; refuses `Send` / `Hibernate`.
   - `Triggered` — `WakeFromSchedule` / `InboxChanged`
     sets `run_now`.
   - `Collect Reminders` — `TodoFile::consume_past_due` marks due
     items `done` and returns `Vec<(text, at)>`.
   - `Session Running` + `Serving Agent` — agent spawned; handles
     `Send` (also finalises any claimed inbox batch), `Receive`
     (archives the current inbox batch and records filenames in
     `SessionInboxState`), `Hibernate`, `Todo*`.
   - `Session Ends` — child exited / shutdown / timeout
     (`resolve_child_exit` / `resolve_interrupted_session`).
   - `Wrap Up` — `finalize_human_replies` writes a
     `from: cryochamber` fallback reply for any still-claimed batch,
     then `EventLogger::finish`.
   - `Pick Next Step` — `decide_next_step` → `PlanComplete` |
     `Hibernate` | `RotateProvider`.
   - `Plan Complete` — terminal; daemon exits.
   - `Retry After Crash` — `reschedule_consumed_after_crash` →
     `bump_attempt` + `2^k` minutes (capped at 1 day) → new TODO
     feeds back into `Collect Reminders`.

3. **Write the SVG file** at `docs/diagrams/<name>.svg` (create the
   directory if missing). Start from the skeleton above and fill in
   nodes / edges by grid coordinates. Re-use the style block verbatim —
   don't invent new colours or font sizes without reason.

4. **Validate.** Run an XML well-formedness check (the SVG must parse
   as XML) and verify each `id`, `class`, and `url(#...)` reference
   resolves. One quick smoke test:
   ```bash
   python3 -c 'import xml.etree.ElementTree as ET; ET.parse("docs/diagrams/daemon-state.svg")'
   ```
   If `xmllint` is available, `xmllint --noout docs/diagrams/*.svg`
   is even better.

5. **Ask the user whether to open the SVG.** After producing the
   file(s), show the output paths and ask a single yes/no question:

   > Wrote `docs/diagrams/daemon-state.svg`. Open it now? (y/n)

   On `y` (macOS), run `open docs/diagrams/daemon-state.svg` — it
   opens in the default browser / Preview. On Linux try `xdg-open`.
   Do **not** open the SVG without explicit consent.

## Files produced

- `docs/diagrams/daemon-state.svg` — overall event-loop state machine.
- `docs/diagrams/lifecycles.svg` — two stacked sections:
  - *How a Message Travels Through the Chamber*: Arrived → Waiting
    to be Read → Received (file archived immediately, reply
    obligation still open) → Answered by Agent **or** Answered by
    Chamber. There is **no crash-restore arc** — consumption is
    terminal; even on crash, the chamber writes the fallback from
    the obligation tracked in `SessionInboxState`.
  - *How a Reminder (TODO) Gets Consumed*: Created → Waiting → Due
    → Consumed → Cleanly Finished **or** Retried Later, where the
    retry branch creates a *new* reminder with `(attempt k)` suffix
    and `2^k`-minute delay capped at 1 day. Early exits from
    *Waiting*: *Marked Done* (`cryo-agent todo done`) and *Removed*
    (`cryo-agent todo remove`).

The `.svg` files are committable source artifacts — there is no
separate build output to gitignore.

## Message lifecycle (for the `lifecycles` diagram)

States, in order:
1. **Arrived** — file landed in `messages/inbox/` (external sync or
   direct drop).
2. **Waiting to be Read** — file sits in the inbox; the daemon uses
   its existence to schedule a wake but does not preview the body.
3. **Received** — `cryo-agent receive` routed through the daemon
   triggered `MessageStore::read_and_archive_inbox`: the file is
   archived under `messages/inbox/archive/` in the same step, and
   `SessionInboxState` records the filenames so the chamber knows a
   reply is still owed.
4a. **Answered by Agent** — a subsequent `cryo-agent send` in the
    same session counts as the reply; `SessionInboxState` clears.
4b. **Answered by Chamber** — session ends (clean exit *or* crash)
    with the obligation still open, so
    `finalize_human_replies` writes a `from: cryochamber` fallback.

Crash semantics: consumption is terminal. There is no arc from
*Received* back to *Waiting*; the file is gone from the inbox the
moment the agent reads it, and the fallback path handles the
unanswered obligation on crash.

## TODO lifecycle (for the `lifecycles` diagram)

1. **Created** — agent called `cryo-agent todo add --at TIME`.
2. **Waiting** — item sits in `todo.json` with a future `at`.
3. **Due** — `at ≤ now`; the scheduler picks it up on the next tick.
4. **Consumed** — the daemon flagged it `done = true` and started a
   session.
5a. **Cleanly Finished** — the session hibernated politely; the
    item stays done.
5b. **Retried Later** — the session crashed;
    `reschedule_consumed` created a **new** item whose text is
    suffixed `(attempt k)` and whose `at = now + 2^k` minutes
    (capped at 1 day). That new item re-enters *Waiting*.

Early exits from *Waiting*: **Marked Done** (`cryo-agent todo done`)
and **Removed** (`cryo-agent todo remove`).

## Quick reference

| Daemon concept | Code location |
|----------------|---------------|
| Event loop / wake | `src/daemon.rs::run_event_loop` |
| Next wake time | `src/daemon/schedule.rs::next_wake_from_todos` |
| Session IPC dispatch | `src/daemon.rs::handle_active_request` |
| Hibernate gate | `src/daemon/request.rs::resolve_hibernate_request` |
| Inbox claim / archive (single step) | `src/channel/store.rs::MessageStore::read_and_archive_inbox` |
| Reply-obligation tracking | `src/daemon/inbox.rs::SessionInboxState` |
| Fallback reply on unanswered batch | `src/daemon.rs::finalize_human_replies` |
| TODO consume + retry | `src/todo.rs` (`consume_past_due`, `bump_attempt`, `retry_delay_minutes`) |
| EventLogger + cryo.log | `src/log.rs` |

| SVG need | Pattern |
|----------|---------|
| Rounded-rect node | `<g transform="translate(cx,cy)"><rect class="node idle" x="-90" y="-35" width="180" height="70" rx="10" filter="url(#shadow)"/>…</g>` |
| Pill node | Same pattern with `rx="32"` on a rect of height ~64 |
| Straight arrow | `<path class="edge" d="M x1,y1 L x2,y2" marker-end="url(#arrow)"/>` |
| Curved arrow | `<path class="edge" d="M x1,y1 Q cx,cy x2,y2" marker-end="url(#arrow)"/>` |
| Loop-back arc | `<path class="edge" d="M x1,y1 C x1,top x2,top x2,y2" marker-end="url(#arrow)"/>` with `top` well outside the main grid |
| Bidirectional | Add `marker-start="url(#arrow)"` with `orient="auto-start-reverse"` on the marker def |
| Dashed crash edge | `class="edge-red"` and `marker-end="url(#arrow-red)"` |
| Edge label with halo | `<text class="elabel" x="lx" y="ly">event</text>` — the halo comes from CSS |

## Common mistakes

- **Drawing from memory.** The daemon re-injects crashed TODOs with
  `2^k`-minute backoff capped at 1 day, and it does *not* preview
  inbox bodies in the wake prompt — verify in `src/todo.rs` and
  `src/daemon.rs` before labelling. Also: receive is archive —
  there is no `inbox/pending/` directory and no crash-restore arc in
  the current code.
- **Writing labels without the halo.** A bare `<text>` on top of an
  edge becomes illegible wherever the edge crosses anything. Always
  use `class="elabel"` (or `elabel-red`) so the `paint-order`
  halo kicks in.
- **Anchoring edges to node centres.** The arrowhead will overlap
  the rect and look muddy. Start and end at the edge of each node.
- **Building the style block from scratch.** Copy the skeleton. The
  palette, drop shadow, and halo values are already tuned.
- **Opening the SVG without asking.** The last workflow step is a
  question, not an action.
- **Writing a markdown state-table next to the SVG.** The SVG is
  the artifact; extra prose belongs in the conversation or in a
  legend block inside the SVG.
