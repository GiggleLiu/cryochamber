# Product-release polish mandate — Agent Console PWA

You own polishing this repo (a Zulip-backed chat PWA for controlling AI agents; WeChat-inspired conversation UI) to product-release quality. Work on branch main. Before any UI work, invoke the `frontend-design:frontend-design` skill and let it calibrate your visual decisions.

## Phase 0 — mandatory correctness fixes first (from a pending code review; commit "fix: harden sanitizer CSS/emoji handling")

1. `src/components/MessageBody.tsx` — the `style` attribute is allowed globally, unfiltered. Add filtering in the rewrite pass: parse each styled element's declarations and keep ONLY properties in an allowlist covering KaTeX layout needs (height, width, min-width, max-width, top, left, right, bottom, margin*, padding*, vertical-align, position, display, border-*width*, font-size, font-style, transform) with values matching a conservative grammar (numbers, units, %, calc-free, keywords) and NO `url(` anywhere; strip the whole style attr if any declaration fails. Also strip `url(` values from SVG paint attributes (fill etc.). Tests: hostile `style="background-image:url(https://x/beacon)"` stripped; `position:fixed;inset:0` neutralized; KaTeX-typical styles survive verbatim.
2. Same file — emoji decoding can throw (`emoji-110000` → String.fromCodePoint RangeError → view crash). Validate: ≤8 hex groups, each parses to a finite integer ≤ 0x10FFFF and outside the surrogate range 0xD800–0xDFFF; on any failure leave the original element text untouched; wrap decode in try/catch regardless. Tests for each invalid case.
3. `src/styles.css` — Pygments default-theme fidelity: keywords bold (`.k*` per canonical theme), docstrings/comments italic where canonical, `.err` uses the canonical border treatment.

## Phase 1 — UI polish loop to the bar of a modern LLM chat app

The bar: a first-time user should place this next to ChatGPT/Claude/WeChat mobile UIs and not flinch. Iterate with your own eyes:

**Method (each round):**
1. Build a screenshot harness (e.g. `e2e/screenshots.spec.ts`, reusing the existing route-mock patterns from `e2e/smoke.spec.ts`) that captures, at iPhone-ish viewport (390×844): login; projects (with unreads); conversation with a rich thread — long markdown message with code block + display math + table, short agent replies, own messages, an image attachment placeholder, time pills; composer with the mention panel open; settings sheet; error banner state. Save PNGs to `.superpowers/followup/shots/`.
2. LOOK at every screenshot (Read them). Critique ruthlessly against the bar below. Write the critique to `.superpowers/followup/polish-log.md` (append per round).
3. Fix what the critique found. Keep tests green (adapt selectors/assertions to markup changes — behavior tests must keep their meaning).
4. Repeat until a round produces no material critique (typically 3–5 rounds). Do not stop after one pass.

**The bar (non-exhaustive — your eyes and the frontend-design skill govern):**
- Deliberate design system: CSS custom properties for the palette (WeChat-family greens/neutrals), consistent type scale, spacing rhythm, radius scale; no ad-hoc magic values scattered.
- Conversation: auto-scroll to newest on open and on new messages (unless user has scrolled up — then an unobtrusive "↓ new messages" chip); smooth message entrance; day separators; bubble refinement (tails, subtle shadow, pressed states); readable timestamps.
- Composer: auto-growing textarea (1→5 lines), Enter sends / Shift+Enter newline on hardware keyboards, disabled-state clarity, upload progress affordance, mention panel that looks native to the design.
- States: skeleton or spinner for loading; designed empty states (projects, conversation); human error copy; reconnecting banner that doesn't jump layout.
- Login: product-quality first impression (logo mark, spacing, input focus states, error presentation).
- Touch/mobile: ≥44px touch targets, safe-area insets, no horizontal overflow anywhere (test long code lines, wide tables), momentum scrolling.
- Accessibility: focus-visible rings, aria labels on icon buttons, prefers-reduced-motion respected, contrast ≥ 4.5:1 for text.
- App identity: replace the solid-square placeholder icons with a real designed SVG glyph (chat-bubble/agent motif), regenerate the 180/192/512 PNGs from it (script it), matching theme colors in the manifest.

## Phase 2 — whole-project release readiness
- README: user-facing quality (what it is, screenshot embedded from your harness, quick start, deploy, adding servers, FAQ stubs).
- Sweep for release blockers: console noise, dead code, TODO remnants, inconsistent copy (title-case vs sentence-case), version string in Settings ("Agent Console v0.1.0" footer reading from package.json).
- Final full gate: `npm test`, `npm run build`, `npm run e2e` all green.

## Constraints
- No new runtime dependencies without a written justification in your report (default: zero).
- Sanitizer safety is non-negotiable — nothing you do may weaken it.
- Behavior contracts stay intact: event loop, 401 handling, unread accounting, upload/download, mention insertion syntax.
- Granular commits with clear messages. Never touch `.superpowers/` in commits.
- You do not spawn subagents; do all work yourself.

## Report
Write `.superpowers/followup/polish-report.md`: per-round critique summary, what changed, final screenshot inventory, test/build/e2e evidence, anything consciously left for later. Final chat message ≤12 lines: rounds completed, commits, gate status, remaining known gaps.
