# Rendering dispatch report

Repo: /Users/liujinguo/agentic/zulip-app (branch main)
Two approved changes, TDD, two commits. Gate passed: `npm test` 90/90, `npm run build` clean, `npm run e2e` 1/1.

## Change 1 — Markdown/math rendering fidelity
Commit: `7bbb35a feat: render Zulip math, code, and markdown with full fidelity`

### Summary
- `src/components/MessageBody.tsx`
  - Added `'style'` to `ALLOWED_ATTR` (KaTeX layout lives in inline styles).
  - Added SVG allowlist: tags `svg/path/line/g`; attrs `viewBox`, `d`, `width`, `height`, `preserveAspectRatio`, `xmlns`, `x1/y1/x2/y2`, `stroke-width`, `fill`.
  - DOMPurify handling: config entries are lowercased internally (`viewBox` → `viewbox` key) and the original attribute-name case is preserved on clone, so `viewBox="0 0 400000 1080"` and `preserveAspectRatio="xMinYMin slice"` survive verbatim — verified empirically with a jsdom probe and locked in by the SVG test. No adjustment beyond listing the attrs was needed.
  - Rewrite pass: removed every `.katex-mathml` subtree (kills the duplicate raw-TeX text from `<annotation>`; visible math is `.katex-html`).
  - Emoji: span with class token `emoji-([0-9a-f]+(?:-[0-9a-f]+)*)` → replaced by a text node of `String.fromCodePoint(...hex.split('-'))` (multi-codepoint works: `emoji-1f1e8-1f1f3` → 🇨🇳); `<img class="emoji" alt="…">` → text node of `alt`. Plain Unicode emoji pass through untouched (no class token, no match).
- `src/styles.css`
  - `.message-body .katex-display`: block, 0.5em margins, overflow-x auto, centered.
  - Replaced the 6-class partial palette with the complete Pygments "default" theme (all token classes: k kc kd kn kp kr kt, n na nb nc nd ne nf ni nl nn no nt nv, o ow, p (default), c ch cm cp cpf c1 cs, s sa sb sc dl sd s2 se sh si sx sr s1 ss, m mb mf mh mi mo il, w, err, gd ge gr gh gi go gp gs gu gt gc) using canonical default-theme colors.
  - Markdown polish: p margins, h1 1.25em → h6 0.9em with 0.5em top margin, ul/ol, tables (collapse, block+overflow-x, th/td borders, th background).

### TDD evidence
- RED: `npx vitest run src/components/MessageBody.test.tsx` → 7 failed / 13 passed (all new fidelity tests failed: style stripped, mathml leaked, svg gone, img.emoji kept, onclick kept).
- GREEN: same command → 20/20 passed; full suite 85/85.

### Files changed
`src/components/MessageBody.tsx`, `src/components/MessageBody.test.tsx`, `src/test/fixtures/zulipHtml.ts`, `src/styles.css` (197 insertions, 8 deletions).

## Change 2 — WeChat-style conversation view
Commit: `05c5f7d feat: WeChat-style chat bubbles in conversation view`

### Summary
- `src/views/ConversationView.tsx` (fetch/mark-read/load-older logic untouched)
  - `isSelf = m.sender_email === creds.email`; rows get `msg-self` / `msg-other`.
  - Time pills: before first message and before any message whose timestamp is ≥300s after the previous; format `HH:MM` (same calendar day, `hour12: false` for a deterministic 2-digit hour) else `M/D HH:MM`.
  - Avatar: 38px rounded square (4px radius), first character of `sender_full_name` uppercased, white text, background hashed from `sender_email` over a fixed 8-color palette; row-reverse puts it right for self.
  - Sender label (`sender_full_name`) above the bubble for others only.
  - Bubble: `#fff` (other) / `#95ec69` (self), radius 8px, padding 0.5em 0.75em, max-width 78%, MessageBody inside, `::before` triangle tail pointing at the avatar.
  - Per-message timestamps removed (intentional WeChat behavior).
- `src/styles.css`
  - `.message-scroll` background `#ededed`; added `.time-pill`, `.msg-row`, `.avatar`, `.msg-col`, `.sender-label`, `.bubble` (+ tail, self/other variants); removed `.message` / `.message-meta` rules; kept `.message-body` block; added `.bubble .message-body pre, table { max-width: 100% }` so code/tables stay inside bubble width.
- e2e: `e2e/smoke.spec.ts` passes unchanged (message text still matches via the `<p>` inside the bubble; Playwright text matching uses direct text nodes, same as before).

### TDD evidence
- RED: `npx vitest run src/views/ConversationView.test.tsx` → 5 failed / 7 passed (new bubble tests failed; behavioral tests untouched).
- GREEN: full suite 90/90.

### Files changed
`src/views/ConversationView.tsx`, `src/views/ConversationView.test.tsx`, `src/styles.css` (168 insertions, 15 deletions).

### Test fix during TDD
The avatar determinism assertion I first wrote wrongly required two *different* senders to share a color; corrected to assert the same sender ('bot@b.c') across two messages gets the same color, and different senders get valid rgb values. Implementation was correct; the test was wrong.

## Commands + output
- `npm test` → Test Files 15 passed (15), Tests 90 passed (90)
- `npm run build` → `✓ built in 468ms` (tsc --noEmit clean, vite build clean)
- `npm run e2e` → `1 passed (1.1s)`
- `npx vitest run <file>` RED runs as documented above.

## Self-review
- Hostile payload tests still pass; added one assertion that `<div style="color:red" onclick="alert(1)">` keeps style but loses onclick. `javascript:` hrefs, `<script>`, img onerror all still stripped.
- Emoji span replacement happens before the img src-rewrite loop, so replaced imgs never get proxy-prefixed (correct: they are gone from output).
- `.katex-mathml` removal is safe: it is a sibling of `.katex-html` inside `.katex`, so the visible math is untouched (asserted structure intact).
- Time-pill format regex in tests covers both same-day (`HH:MM`) and cross-day (`M/D HH:MM`) output, so the suite is date-agnostic.
- Working tree clean except `.superpowers/` (this dispatch + report).

## Concerns
- Allowing `style` is a deliberate widening (needed for KaTeX). DOMPurify still strips event handlers; no CSS-injection vector is exercised (jsdom/browser render inert styles), but a future hostile-style CSS payload could theoretically exploit browser CSS parsing bugs — acceptable for a PWA rendering trusted-ish Zulip content.
- Avatar colors hash to an 8-color palette, so distinct senders can collide on the same color — cosmetic only.
- `String.fromCodePoint` on a malformed token (e.g. `emoji-zz`) would throw, but the regex only accepts `[0-9a-f]+` groups, so it cannot occur from Zulip output; a hostile message could still carry a valid-looking token that decodes to a private-use codepoint (renders as tofu box) — harmless.
