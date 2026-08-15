## Prior findings

- **STILL OPEN** — Style/SVG URL filtering: inline declarations now have a property/value allowlist, but SVG paint filtering only detects literal `url(`. An escaped value such as `fill="\75\72\6c(https://attacker/paint.svg#x)"` survives DOMPurify and the check, then CSS tokenization interprets it as `url(...)`. [src/components/sanitize.ts:188](/Users/liujinguo/agentic/zulip-app/src/components/sanitize.ts:188)
- **CLOSED** — Emoji decoder crash: hex groups are bounded, Unicode range and surrogate values are validated, and `String.fromCodePoint` has a catch fallback. [src/components/sanitize.ts:34](/Users/liujinguo/agentic/zulip-app/src/components/sanitize.ts:34), [src/components/sanitize.ts:47](/Users/liujinguo/agentic/zulip-app/src/components/sanitize.ts:47)

## Findings

- **Important** — Completing an upload can discard text typed while the request was pending. The async handler resumes with its pre-upload `text` closure, while `insertLink` combines that stale value with the textarea’s current caret. [src/components/Composer.tsx:99](/Users/liujinguo/agentic/zulip-app/src/components/Composer.tsx:99), [src/components/Composer.tsx:122](/Users/liujinguo/agentic/zulip-app/src/components/Composer.tsx:122)
- **Minor** — Mention detection lacks a boundary before `@`, so text such as `foo@` opens the panel and can redirect Enter into mention confirmation. [src/components/Composer.tsx:22](/Users/liujinguo/agentic/zulip-app/src/components/Composer.tsx:22)
- **Minor** — The open-panel keyboard branch intercepts Enter during IME composition because `isComposing` is checked only when the panel is hidden. [src/components/Composer.tsx:167](/Users/liujinguo/agentic/zulip-app/src/components/Composer.tsx:167)
- **Minor** — Download filename derivation includes query strings and fragments; `/report.pdf?download=1#x` becomes that entire suffix rather than `report.pdf`. [src/components/MessageBody.tsx:14](/Users/liujinguo/agentic/zulip-app/src/components/MessageBody.tsx:14)

## Verdict

**Needs fixes**

The CSS-escaped SVG URL bypass keeps a prior Important finding open, and upload completion can lose draft text. No additional `dangerouslySetInnerHTML`, blob-URL, or mention-panel injection was found; FormData handling and `uri`/`url` fallback are correct, and all README images resolve to committed files.