You are implementing two approved changes to the Agent Console PWA (repo root = cwd, branch main, TDD required, two commits). Read the existing code first: `src/components/MessageBody.tsx`, `src/views/ConversationView.tsx`, `src/styles.css`, and their tests. App context: Zulip returns server-rendered HTML; MessageBody sanitizes it with DOMPurify allowlists, rewrites relative URLs to a proxy prefix, and loads user_uploads via authenticated fetch. KaTeX CSS is imported globally in main.tsx.

## Change 1 — Markdown/math rendering fidelity (commit: "feat: render Zulip math, code, and markdown with full fidelity")

Problem: the sanitizer strips `style` attributes and SVG, which KaTeX HTML depends on (inline styles carry all math layout; sqrt bars/stretchy delimiters are SVG). The MathML fallback's text (raw TeX from `<annotation>`) leaks as duplicate text because DOMPurify drops unknown tags but keeps their content. The Pygments palette covers only ~6 token classes, and message CSS lacks table/heading/list rules.

Fix in `src/components/MessageBody.tsx`:
1. Add `'style'` to ALLOWED_ATTR.
2. Add SVG support to the allowlists: tags `svg`, `path`, `line`, `g`; attributes `viewBox`, `d`, `width`, `height`, `preserveAspectRatio`, `xmlns`, `x1`, `y1`, `x2`, `y2`, `stroke-width`, `fill`. (DOMPurify lowercases via its own handling — ensure the config actually preserves `viewBox`/`preserveAspectRatio`; verify with the test below and adjust, e.g. via ALLOWED_ATTR entries, until it passes.)
3. In the existing DOMParser rewrite pass, remove every element matching `.katex-mathml` (kills the duplicate raw-TeX text; the visible math is the `.katex-html` part).
3b. **Emoji:** Zulip emits `<span class="emoji emoji-1f44d" title="thumbs up">:thumbs_up:</span>` (spritesheet-styled on zulip.com; we don't ship that CSS, so users see `:thumbs_up:`). In the rewrite pass, for every element whose class list contains a token matching `emoji-([0-9a-f]+(?:-[0-9a-f]+)*)`, replace the element with a text node of the Unicode character(s): split the hex codepoints on `-`, `String.fromCodePoint(...codepoints)` (multi-codepoint sequences like `emoji-1f1e8-1f1f3` → 🇨🇳 must work). Also handle the `<img class="emoji" alt="...">` variant some realms emit: replace the img with a text node of its `alt`. Plain Unicode emoji already in text pass through untouched.

Fix in `src/styles.css` (all scoped under `.message-body` / `.codehilite`):
4. `.katex-display`: block, `margin: 0.5em 0`, `overflow-x: auto`, centered content.
5. Replace the partial `.codehilite` palette with the complete standard Pygments "default" theme palette (all token classes: k, kc, kd, kn, kp, kr, kt, n, na, nb, nc, nd, ne, nf, ni, nl, nn, no, nt, nv, o, ow, p, c, ch, cm, cp, cpf, c1, cs, s, sa, sb, sc, dl, sd, s2, se, sh, si, sx, sr, s1, ss, m, mb, mf, mh, mi, mo, il, w, err, g classes gd/gi/etc. — use the canonical default-theme colors).
6. Markdown polish: `p { margin: 0.25em 0 }`; heading sizes h1 1.25em → h6 0.9em with 0.5em top margin; `ul, ol { margin: 0.25em 0; padding-left: 1.4em }`; tables — `table { border-collapse: collapse; display: block; overflow-x: auto; max-width: 100% }`, `th, td { border: 1px solid #d1d5db; padding: 0.3em 0.6em }`, `th { background: #f3f4f6 }`.

Tests (TDD — write failing first) in `src/components/MessageBody.test.tsx` with new fixtures in `src/test/fixtures/zulipHtml.ts`:
- A realistic KaTeX display-math fixture (span.katex-display > span.katex > span.katex-mathml with `<math><semantics><mrow><mi>x</mi></mrow><annotation encoding="application/x-tex">x^2</annotation></semantics></math>` + span.katex-html with nested spans carrying `style="height:0.8141em;"`, `style="top:-3.063em;margin-right:0.05em;"` etc.). Assert: style attributes survive; `.katex-mathml` content (the string `x^2` / `annotation`) is fully removed; `.katex-html` structure intact.
- An SVG fixture modeled on KaTeX sqrt: `<span class="hide-tail" style="min-width:0.853em;height:1.08em;"><svg xmlns="http://www.w3.org/2000/svg" width="400em" height="1.08em" viewBox="0 0 400000 1080" preserveAspectRatio="xMinYMin slice"><path d="M95,702c-2.7,0,-7.17,-2.7,-13.5,-8c-5.8,-5.3,-9.5,-10,-9.5,-14"/></svg></span>`. Assert svg, path, viewBox, preserveAspectRatio, d survive.
- A table fixture (thead/tbody/th/td). Assert structure survives.
- Emoji fixtures: `<p><span class="emoji emoji-1f44d" title="thumbs up">:thumbs_up:</span></p>` → output contains 👍 and no `:thumbs_up:`; multi-codepoint `<span class="emoji emoji-1f1e8-1f1f3">:cn:</span>` → 🇨🇳; `<img class="emoji" alt="🎉" src="/static/generated/emoji/tada.png">` → 🎉 with no img element.
- Keep every existing test passing (hostile payloads must still be stripped — style must NOT weaken script/handler stripping; add one assertion that `<div style="x" onclick="alert(1)">` loses onclick but keeps style).

## Change 2 — WeChat-style conversation view (commit: "feat: WeChat-style chat bubbles in conversation view")

Restyle `ConversationView`'s message list (logic for fetching/mark-read/load-older unchanged):
1. Determine `isSelf = m.sender_email === creds.email`.
2. **Time pills:** before any message whose timestamp is ≥300s after the previous message (and before the first message), render a centered gray pill `div.time-pill` — format: `HH:MM` if same calendar day as now, else `M/D HH:MM` (locale default is fine).
3. **Message row** `div.msg-row` + modifier class `msg-self` / `msg-other`: avatar + column. Avatar `div.avatar`: 38px rounded square (WeChat uses rounded squares, 4px radius) showing the sender's first character (uppercased), white text on a background color deterministically hashed from `sender_email` (pick from a fixed 8-color palette). Avatar left for others, right for self (flex-direction row-reverse for self).
4. **Column:** for others only, a small gray `div.sender-label` with `sender_full_name` above the bubble. Bubble `div.bubble`: background `#fff` for others, `#95ec69` for self; `border-radius: 8px`; padding 0.5em 0.75em; `max-width: 78%` of the row; MessageBody rendered inside. Add a small CSS triangle tail (`::before`) pointing at the avatar, vertically aligned near the top.
5. **Backdrop:** `.message-scroll` background `#ededed`. Keep load-earlier button, error, empty states as-is (they sit on the beige background).
6. Remove the old `.message` / `.message-meta` markup and CSS (fully replaced); keep `.message-body` styles (bubbles contain them). Ensure pre/code and tables inside bubbles stay within `max-width` (`.bubble .message-body pre { max-width: 100% }` etc. as needed).
7. Timestamps per-message are no longer shown outside pills — that is intentional WeChat behavior.

Tests (update `src/views/ConversationView.test.tsx`; write the new assertions first, watch them fail, then implement):
- Own vs other message rows get `msg-self` / `msg-other` classes (set creds email equal to one fixture message's sender).
- Sender label renders for others' messages, absent on own.
- Avatar shows the sender's first character.
- Time pill: two messages 10 minutes apart produce 2 pills (one before first message, one before the second); two messages 1 minute apart produce only the leading pill.
- Existing behavioral tests (initial load, load-earlier dedup, send/retry, 401 logout, mark-read retry) must keep passing — update selectors only where markup changed.
- Check `e2e/smoke.spec.ts` still passes; adjust its locators only if the markup change requires it.

## Gate before finishing
`npm test` all green, `npm run build` clean, `npm run e2e` passing. Do not create subagents or invoke other AI tools. If DOMPurify's SVG attribute handling fights you beyond the noted adjustment, STOP and report BLOCKED with specifics.

## Report
Full report to `.superpowers/followup/rendering-report.md` (per-change summary, TDD evidence RED/GREEN, files changed, commands + output, self-review, concerns). End your final console message with ONLY (≤10 lines): Status; Commits; one-line test summary; concerns.
