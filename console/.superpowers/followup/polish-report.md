# Agent Console — polish report

Branch `main`, 8 commits on top of `5369d1b`. Working tree clean apart from
`.superpowers/`, which is untracked and was never staged (verified:
`git log --all --name-only | grep -c superpowers` → 0).

## Gate

| check | result |
|---|---|
| `npm test` | **178 passed / 18 files** (was 122 / 17) |
| `npm run build` | **clean** (`tsc --noEmit` + vite, 255 kB JS / 83 kB gzip) |
| `npm run e2e` | **14 passed** (3 smoke + layout contract, 11 screenshot states) |

No runtime dependencies were added. **Zero.** The icon pipeline reuses the
Chromium that `@playwright/test` already installs; the icon set is inline SVG;
the mono/UI type pairing uses system stacks.

## Commits

| SHA | subject |
|---|---|
| `548f2f7` | fix: harden sanitizer CSS/emoji handling |
| `9e8da1c` | feat: design system — tokens, icon family, app chrome |
| `0329c89` | feat: rebuild the projects list, conversation and composer |
| `10e0374` | feat: rebuild login and settings |
| `8cea767` | test: phone-viewport screenshot harness |
| `ad0f8b1` | feat: real app icon generated from one SVG source |
| `0674627` | fix: day-aware separators, skeleton fidelity, composer-growth scroll |
| `a493c1d` | docs: user-facing README with screenshots; release copy sweep |

## Phase 0 — sanitizer hardening

Extracted the sanitizer into `src/components/sanitize.ts` (MessageBody
re-exports it, so no caller changed). A security-critical pass deserves its own
module and its own test surface.

1. **Inline styles are now filtered, not passed through.** `filterStyleAttribute`
   allowlists the properties KaTeX actually needs — lengths (`height`, `width`,
   `min/max-width`, `top/left/right/bottom`, `margin-*`, `padding-*`,
   `font-size`, `vertical-align`, `border-*-width`), enumerated keywords
   (`position`, `display`, `font-style`), and `transform` — and validates every
   value against a numbers/units/keywords grammar. Any failing declaration drops
   the **whole** attribute; partial application of a hostile style is not a
   state worth reasoning about. Structural rejections come first: `url(`, CSS
   escapes (`\`), comments (`/* */`), at-rules, `!important`.
   - `position` admits only `static|relative|absolute` — `fixed` and `sticky`
     are deliberately absent, so message content cannot overlay the app chrome.
   - `transform` admits only the named functions with strictly numeric or
     angular arguments, so no `url(` can form inside parens.
2. **SVG paint attributes** (`fill`, `stroke`, `filter`, `mask`, `clip-path`)
   have `url(...)` values stripped.
3. **Emoji decoding cannot crash the view.** `decodeEmojiToken` requires ≤8
   groups, each 1–6 hex digits parsing to an integer ≤ `0x10FFFF` and outside
   the surrogate range, with the `fromCodePoint` call in a `try/catch` anyway.
   Malformed tokens leave the element untouched instead of throwing a
   `RangeError` that took the whole conversation down.
4. **Pygments fidelity** — bold keywords/tags/classes/namespaces/exceptions/
   entities/escapes, italic docstrings, and the canonical `border: 1px solid`
   on `.err` (it was a colour before).

Tests went 25 → 45 in `MessageBody.test.tsx`: hostile `background-image:url(...)`
beacon, `position:fixed;inset:0` overlay, partial-failure drop, escaped `url`,
comment smuggling, `!important`, bad transforms, SVG paint `url()`, and each
invalid emoji shape — plus a case asserting KaTeX-typical styles survive
verbatim.

One test changed meaning-preservingly: "keeps style attributes but still strips
event handlers" used `color:red`, which the allowlist (correctly) now rejects;
it uses `height:1.2em` instead and keeps the same assertion shape.

## Phase 1 — UI polish loop

Harness: `e2e/screenshots.spec.ts` + `e2e/fixtures.ts`, 390×844 @2x, frozen
clock, 16 states. Five rounds. Every round's shots were read as images.

### Design direction

The brief pins the palette (WeChat family) and the form (phone chat). The free
axes were spent as follows:

- **Type is the signature.** Platform UI face for prose, a monospace utility
  face for everything the machine produced or measured — timestamps, unread
  counts, day separators, avatar initials, code, the version string. That is
  the actual subject of this product (a console you talk to), and it costs zero
  bytes.
- **Chat lines have faces; reports have bylines.** A message carrying block
  content (`pre`, `table`, heading, blockquote, display math, inline image)
  renders as a full-width card with a sender byline and no avatar or tail,
  while short messages stay as bubbles with avatars. This is where the modern
  LLM-chat convention and WeChat meet, and it is also the fix for the real
  usability failure found in round 1 (wide code and tables silently clipped
  inside a 78 %-wide bubble).
- **Deliberate deviation:** bubbles use a 4 px notched corner rather than a CSS
  triangle tail. The old hard triangles were the crudest element on screen; the
  notch reads as the same gesture, and the bubble tail survives in the product
  mark instead.

### Round-by-round

**Round 1 (baseline critique → full rebuild).** Generic indigo chrome; floating
"material card" project list carrying almost no information; conversation
opened *at the top* rather than the newest message; sender labels at 2.4:1
contrast; wide code and tables clipped with no affordance; emoji-glyph attach
button; big slab Send at 0.5 opacity when disabled; native checkboxes and a
stranded outlined Log out in Settings; no logo, no focus states.

Built: the token system (palette, 4 px rhythm, type/radius/elevation/motion
scales, `--tap: 44px`, reduced-motion escape); inline SVG icon family; light
translucent top bar with fixed action slots; edge-to-edge project rows with
tiles, last-message preview, relative time and unread badge; conversation
auto-scroll + jump chip, sender-run grouping, day separators, rich-message
cards; auto-growing composer with Enter-to-send; skeletons and designed empty
states; login and settings rebuilt; version footer.

**Round 2.** Auto-scroll landed one message short (images grow the list after
mount) → `ResizeObserver` re-pin. Wide blocks still gave no hint they scrolled
→ pure-CSS scroll shadows (local cover gradients over fixed radial shadows) on
`pre`, `table` and `.katex-display`, plus a table header that uses weight and a
rule instead of a fill so the shadow reads across it. Rich cards still paid the
avatar gutter → they now drop the avatar entirely. Disabled Send dissolved into
the dock → inset hairline. Composer focus ring too heavy → softened. Settings
gained a Server row; error state gained a muted detail line and **Try again**.

**Round 3.** A three-line draft hid the message it was replying to → the
`ResizeObserver` now watches the scroller itself, so composer growth re-pins
too. Thread skeletons were nearly invisible on the darker canvas and did not
mirror real rows → own contrast range plus avatar+bubble shapes. The login
alert inherited the in-thread alert's inset and misaligned with the fields →
`.login-screen .alert`.

**Round 4.** A gap-triggered separator read `19:32` directly under
`YESTERDAY 20:32`, telling the reader nothing. `separatorLabel` now takes the
previous message's timestamp and names the day whenever the day changes
(`Today` / `Yesterday` / weekday / date / date+year), falling back to a bare
time inside a day. Report cards dropped the tail notch.

**Round 5.** Unread badge hung at the description baseline when a row had no
timestamp → centred via `:has()`. No other material critique: **loop
converged.**

### Behaviour contracts

Event loop, 401 handling, unread accounting, upload/download and mention
insertion syntax are untouched; their tests are unchanged except for markup
selectors. Test changes and why they preserve meaning:

| test | change | meaning kept |
|---|---|---|
| time pill regex | accepts `Today `/`Yesterday `/weekday/date prefixes | "a separator appears at the first message and after 300 s gaps, showing a readable time" |
| projects empty state | sets `connection: 'live'` | the empty state is what a *connected* client with no streams shows; a new test covers the skeleton for `'connecting'` |
| login secondary button | `/paste an api key/i` | copy reads as English; same button, same path |
| `color:red` style test | `height:1.2em` | style survives, handler stripped |

New behavioural tests (56 added overall): scroll pinning and the jump chip,
sender-run grouping, rich-vs-bubble classification, Enter-to-send across
hardware/touch keyboards and IME, the `format.ts` helpers, plus an e2e
**phone layout contract** asserting no horizontal overflow at 390 px with wide
code and a four-column table on screen, and ≥44 px on every bar, composer and
list control.

### App identity

`public/icons/icon.svg` is the single source: the product mark (a chat bubble
containing a shell prompt) on the brand green, sized inside the maskable safe
zone. `scripts/generate-icons.mjs` renders 180/192/512 PNGs through Playwright's
Chromium — no new dependency, PNGs committed so a plain build needs no browser.
Manifest gained description, scope, orientation, `maskable` purpose and an SVG
entry; `theme_color` and the document `theme-color` now match the light chrome.

## Phase 2 — release readiness

- README rewritten for users: what it is, three embedded screenshots generated
  by the harness, quick start, deploy, adding a server (with the "never add a
  catch-all proxy" warning repeated where it will be read), a five-question
  FAQ, project layout, and icon regeneration.
- Sweep: no `TODO`/`FIXME`/`HACK` in `src/` or `e2e/`; no `console.*` outside
  the icon CLI; no orphan CSS classes (checked programmatically); every
  exported icon is used; error copy aligned on "Couldn't …".
- Version footer reads `package.json` through a Vite `define`, so Settings
  cannot drift from the released version.
- The screenshot harness now defaults to the gitignored `test-results/` instead
  of a developer-specific path.

## Screenshot inventory

`.superpowers/followup/shots/` — 01-login, 02-login-focus (keyboard),
03-login-error, 04-projects, 05-projects-empty, 06-conversation-latest,
07-conversation-full, 08-conversation-markdown, 09-conversation-table,
10-conversation-loading, 11-conversation-error, 12-composer-multiline,
13-composer-mentions, 14-reconnecting, 15-settings, 16-conversation-top.

Committed subset for the README: `docs/screenshots/{projects,conversation,report}.png`.

## Consciously left for later

- **No dark mode.** `color-scheme: light` is declared so form controls stay
  consistent. The palette is fully tokenised, so a dark theme is a
  `@media (prefers-color-scheme: dark)` block redefining the colour tokens —
  but shipping it untested at a phone viewport would be worse than not
  shipping it.
- **`color` is not in the style allowlist**, following the mandate's list
  literally. `\textcolor{}` in LaTeX and coloured spans will render in the
  default ink. Adding `color`/`background-color` with a strict
  `#hex|rgb()|named` grammar would be safe and is the obvious next step if
  anyone notices.
- **The reconnect chip overlays content** rather than reserving space. That is
  what "must not jump layout" implies; on a full screen it briefly covers the
  top row.
- **Row previews only exist for streams already opened** — the app has no
  last-message-per-stream data until a conversation is fetched, so unopened
  rows show the stream description. Honest, but asymmetric.
- **`:has()` is used once** (unread-badge centring). It degrades to the
  previous, still-acceptable alignment on engines without it.
- **Reduced motion is a blanket CSS override**, not per-animation tuning, and
  is not covered by an automated test.
