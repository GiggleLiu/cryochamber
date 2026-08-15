# UI polish log

One entry per round. Each round: shoot 390×844 with `e2e/screenshots.spec.ts`,
read every PNG, critique against the modern-LLM-chat bar, fix, repeat.

---

## Round 1 — baseline (pre-existing UI)

**Chrome.** Solid indigo `#4f46e5` top bar and buttons — generic SaaS, and it
contradicts the WeChat-family brief. No design tokens anywhere; every colour,
radius and space is a literal.

**Login.** Bold text wordmark, no mark, no statement of what the app is. Form
floats on flat grey at `10vh`. "Paste API key instead" is an underlined blue
hyperlink. No focus states.

**Projects.** Floating rounded cards with large gaps — a dated material list,
not a chat list. No per-project identity, no timestamp, no message preview: four
grey lines carrying almost nothing. Bare-sentence empty state.

**Conversation.**
- Opens **at the top of the thread**, not at the newest message. Worst defect
  on the screen.
- "Load earlier" sits in the reader's face on open.
- Sender labels `#9ca3af` on `#ededed` ≈ 2.4:1 — fails contrast.
- Hard CSS-triangle bubble tails; 8 px radius; crude.
- **Wide code blocks and tables are clipped with no affordance.** A four-column
  table inside a 78 %-wide bubble loses two columns silently. This is a real
  usability failure, not a taste issue.
- Repeated avatar + name on consecutive messages from one sender.
- Time pills only; no day separators.

**Composer.** `rows={2}` fixed box; 📎 emoji as the attach button (~24 px, fails
touch target); big indigo Send slab that at `opacity: .5` reads as broken
lavender rather than disabled.

**Settings.** Native checkboxes, ad-hoc spacing, a red outlined "Log out" button
stranded mid-page, no version string.

→ Rebuilt everything on a token system. See report for the design direction.

---

## Round 2

Big improvement; the remaining problems are specific.

1. **Auto-scroll lands one message short.** Images finish loading after mount
   and grow the list under the reader. → `ResizeObserver` re-pin.
2. **Sideways-scrolling blocks still give no hint they scroll.** The table now
   fits more, but the last column is cut mid-word. → pure-CSS scroll shadows
   (local cover gradients over fixed radial shadows) on `pre`, `table`,
   `.katex-display`; table header switches from a fill to weight + a rule so
   the shadow reads across the whole table.
3. Rich cards still pay the 42 px avatar gutter they do not use. → reports drop
   the avatar entirely; the sender label becomes the byline.
4. Disabled Send (`--surface-2` on a white dock) is invisible. → inset hairline.
5. Composer focus ring (`rgba(7,193,96,.38)`, 3 px) is a heavy blob on a text
   field. → softened to `.15`.
6. Error state is a lone alert on an empty canvas with a raw `Server error` in
   the same red as the headline. → muted detail line + **Try again**.
7. Settings has no server identity. → Server row.
8. Empty projects state is stranded at ~1/8 height. → scroller becomes a flex
   column so it centres.

---

## Round 3

1. **A three-line draft hides the message being replied to.** The composer
   grows, the scroller shrinks, nothing re-pins. → `ResizeObserver` now watches
   the scroller element itself, not only its children.
2. **Thread skeletons are nearly invisible** (`#e8e9ea` on the `#ededed`
   canvas) and are bare rounded rectangles that do not mirror real rows. → own
   contrast range via an inherited token override, plus avatar + bubble shapes.
   (First attempt set the token on `.skeleton` itself, which beats inheritance
   — moved the defaults to `:root`.)
3. **The login alert misaligns with the fields** — `.alert`'s 16 px margin wins
   over `.login-notice` by source order while the form is inset 24 px. →
   `.login-screen .alert`.
4. Added e2e guards while here: no horizontal overflow at 390 px with wide code
   and a four-column table on screen; ≥44 px on every bar/composer/list control.

---

## Round 4

1. **The day separator lies by omission.** Scrolled to the top, the thread reads
   `YESTERDAY 19:32` → `20:32` → `19:32`. The third is *today* but nothing says
   so. → `separatorLabel` takes the previous message's timestamp and names the
   day whenever the day changes (`Today` / `Yesterday` / weekday / date /
   date+year), bare time inside a day.
2. Full-width report cards still carry the bubble's 4 px tail notch. A report is
   not pointing at anyone. → uniform radius.

Keyboard-focus capture added to the harness (was capturing click focus, which
proves nothing about `:focus-visible`).

---

## Round 5

1. The unread badge hangs at the description's baseline when a row has no
   timestamp above it (the common case before any conversation is opened). →
   centred on the row via `.stream-card:not(:has(.stream-meta))`.

Everything else read clean: separators, grouping, report cards, scroll
affordances, skeletons, empty and error states, focus rings, touch targets,
composer growth, mention panel, settings, login.

**No material critique. Loop converged after 5 rounds.**
