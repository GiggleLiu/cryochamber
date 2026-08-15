# Follow-up work ledger (post-merge features)
Rendering change (69c3835..05c5f7d) codex review: Needs fixes.
- Important 1: style attr unfiltered (MessageBody.tsx:15) — fix: property-allowlist CSS filter in rewrite pass; strip styles with url()/disallowed props; reject url() SVG paint values. 
- Important 2: emoji decoder RangeError DoS (MessageBody.tsx:37) — fix: validate codepoints <=0x10FFFF, no surrogates, cap 8 groups, try/catch fallback to original text.
- Minor: pygments default-theme bold/italic (.k bold, docstring italics, .err border) — fold into fix.
Queued: combined fix dispatch after mentions/files implementer (bzyntid3m) reports; then one codex review over the whole post-merge range.
