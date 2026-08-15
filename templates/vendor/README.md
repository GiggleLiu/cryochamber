# Vendored client-side rendering libraries

These files are committed so the hub renders message markdown + LaTeX math
without any network dependency (LAN-only or offline deployments). They are
served verbatim by `cryohub` under `/assets/vendor/*` (see
`src/hub/routes/pages.rs`).

| File           | Package                 | Version  | License                      | Source |
|----------------|-------------------------|----------|------------------------------|--------|
| `katex.min.css`| KaTeX                   | 0.16.11  | MIT                          | https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css |
| `katex.min.js` | KaTeX                   | 0.16.11  | MIT                          | https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js |
| `marked.min.js`| marked                  | 12.0.2   | MIT                          | https://cdn.jsdelivr.net/npm/marked@12.0.2/marked.min.js |
| `purify.min.js`| DOMPurify               | 3.1.6    | Apache-2.0 OR MPL-2.0        | https://cdn.jsdelivr.net/npm/dompurify@3.1.6/dist/purify.min.js |
| `fonts/*.woff2`| KaTeX font faces        | 0.16.11  | MIT (fonts SIL OFL 1.1)      | https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/fonts/ |

To upgrade, re-download from the pinned jsdelivr URLs above (same filename)
and bump the version in this table. The minified files carry their own
license banners; keep them intact when replacing.
