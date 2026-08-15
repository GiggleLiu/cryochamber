import DOMPurify from 'dompurify'

const ALLOWED_TAGS = [
  'a', 'p', 'br', 'span', 'div', 'strong', 'em', 'del', 'code', 'pre',
  'blockquote', 'ul', 'ol', 'li', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  'table', 'thead', 'tbody', 'tr', 'th', 'td', 'img', 'hr', 'sup', 'sub',
  'time', 'details', 'summary',
  // SVG: KaTeX renders stretchy delimiters / sqrt bars as inline SVG
  'svg', 'path', 'line', 'g',
]
const ALLOWED_ATTR = [
  'href', 'src', 'alt', 'title', 'class', 'start', 'datetime', 'aria-hidden', 'data-code-language',
  // mentions carry the referenced user id so we can highlight the signed-in user
  'data-user-id',
  // inline styles carry all of KaTeX's math layout; SVG attrs keep the vector glyphs
  'style', 'viewBox', 'd', 'width', 'height', 'preserveAspectRatio', 'xmlns',
  'x1', 'y1', 'x2', 'y2', 'stroke-width', 'fill',
]

// Zulip spritesheet emoji: class tokens like `emoji-1f44d` / `emoji-1f1e8-1f1f3`
// encode one or more hex codepoints (Zulip.com ships the spritesheet CSS; we
// don't, so without this the user would see the raw `:thumbs_up:` marker).
const EMOJI_CLASS_RE = /^emoji-([0-9a-f]+(?:-[0-9a-f]+)*)$/
const MAX_EMOJI_CODEPOINTS = 8
const MAX_CODEPOINT = 0x10ffff

/**
 * Decodes an `emoji-<hex>[-<hex>…]` class token to its Unicode string, or null
 * when the token is not a well-formed codepoint sequence. Zulip is the source
 * of these tokens, but a malicious message can carry any hex the class regex
 * accepts — `emoji-110000` or `emoji-d800` would make String.fromCodePoint
 * throw a RangeError and take the whole conversation view down with it.
 */
export function decodeEmojiToken(token: string): string | null {
  const groups = token.split('-')
  if (groups.length === 0 || groups.length > MAX_EMOJI_CODEPOINTS) return null
  const codepoints: number[] = []
  for (const group of groups) {
    if (!/^[0-9a-f]{1,6}$/.test(group)) return null
    const cp = parseInt(group, 16)
    if (!Number.isInteger(cp) || cp < 0 || cp > MAX_CODEPOINT) return null
    // Lone surrogates are not scalar values; fromCodePoint accepts them but the
    // resulting string is ill-formed UTF-16.
    if (cp >= 0xd800 && cp <= 0xdfff) return null
    codepoints.push(cp)
  }
  try {
    return String.fromCodePoint(...codepoints)
  } catch {
    return null
  }
}

// ---------------------------------------------------------------------------
// Inline style filtering
//
// KaTeX carries all of its math layout in inline styles, so we cannot drop the
// `style` attribute — but an unfiltered one is an injection surface all by
// itself (`background-image:url(https://tracker/beacon)` exfiltrates the fact
// that a message was read; `position:fixed;inset:0` lets message content
// overlay the whole app and phish taps). So: allowlist the properties KaTeX
// needs, and validate every value against a grammar that admits nothing but
// numbers, units and known keywords.
// ---------------------------------------------------------------------------

/** Properties whose value may be a whitespace-separated list of lengths. */
const LENGTH_PROPS = new Set([
  'height', 'width', 'min-width', 'max-width', 'min-height', 'max-height',
  'top', 'left', 'right', 'bottom',
  'margin', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left',
  'padding', 'padding-top', 'padding-right', 'padding-bottom', 'padding-left',
  'font-size', 'vertical-align',
  'border-width', 'border-top-width', 'border-right-width',
  'border-bottom-width', 'border-left-width',
])

/** Properties restricted to a fixed set of keywords. */
const KEYWORD_PROPS: Record<string, Set<string>> = {
  // `fixed`/`sticky` would let message content escape its bubble and overlay
  // the app chrome, so they are deliberately absent.
  position: new Set(['static', 'relative', 'absolute']),
  display: new Set([
    'inline', 'inline-block', 'block', 'none',
    'table', 'table-row', 'table-cell', 'flex', 'inline-flex',
  ]),
  'font-style': new Set(['normal', 'italic', 'oblique']),
  'vertical-align': new Set([
    'baseline', 'top', 'middle', 'bottom', 'sub', 'super',
    'text-top', 'text-bottom',
  ]),
  'font-size': new Set([
    'xx-small', 'x-small', 'small', 'medium', 'large', 'x-large', 'xx-large',
    'smaller', 'larger',
  ]),
  'border-width': new Set(['thin', 'medium', 'thick']),
}

const LENGTH_UNITS = 'em|rem|ex|ch|px|pt|pc|in|cm|mm|%'
const LENGTH_RE = new RegExp(`^[+-]?(?:\\d+\\.?\\d*|\\.\\d+)(?:${LENGTH_UNITS})?$`)
const ANGLE_RE = /^[+-]?(?:\d+\.?\d*|\.\d+)(?:deg|rad|grad|turn)?$/
const TRANSFORM_FNS = new Set([
  'scale', 'scalex', 'scaley', 'translate', 'translatex', 'translatey',
  'rotate', 'skewx', 'skewy', 'matrix',
])
const TRANSFORM_CALL_RE = /([a-z]+)\(([^()]*)\)/gi

/** A bare number, length, or `auto` — the vocabulary of KaTeX's layout styles. */
function isLengthValue(value: string): boolean {
  const tokens = value.split(/\s+/).filter(Boolean)
  if (tokens.length === 0 || tokens.length > 4) return false
  return tokens.every((t) => t === 'auto' || LENGTH_RE.test(t))
}

/** `scale(-1)`, `translate(0,0.5em)` … with strictly numeric arguments. */
function isTransformValue(value: string): boolean {
  const compact = value.replace(/\s+/g, '')
  TRANSFORM_CALL_RE.lastIndex = 0
  let consumed = 0
  let calls = 0
  let match: RegExpExecArray | null
  while ((match = TRANSFORM_CALL_RE.exec(compact)) !== null) {
    if (!TRANSFORM_FNS.has(match[1].toLowerCase())) return false
    const args = match[2].split(',').filter(Boolean)
    if (args.length === 0 || args.length > 6) return false
    if (!args.every((a) => LENGTH_RE.test(a) || ANGLE_RE.test(a))) return false
    consumed += match[0].length
    calls += 1
  }
  // Reject anything outside the recognised calls (stray text, extra parens).
  return calls > 0 && consumed === compact.length
}

function isAllowedDeclaration(prop: string, value: string): boolean {
  if (value.length === 0) return false
  if (KEYWORD_PROPS[prop]?.has(value)) return true
  if (prop === 'transform') return isTransformValue(value)
  return LENGTH_PROPS.has(prop) && isLengthValue(value)
}

/**
 * Returns the subset of `style` that is safe to keep, or null when any
 * declaration fails the allowlist (in which case the caller drops the whole
 * attribute — partial application of a hostile style is not a state we want to
 * reason about).
 */
export function filterStyleAttribute(style: string): string | null {
  // Cheap structural rejections first: URLs, CSS escapes, comments, at-rules
  // and `!important` have no place in the styles KaTeX emits.
  if (/url\s*\(/i.test(style)) return null
  if (/[\\<>{}@!]/.test(style)) return null
  if (style.includes('/*') || style.includes('*/')) return null

  const kept: string[] = []
  for (const raw of style.split(';')) {
    const decl = raw.trim()
    if (!decl) continue
    const colon = decl.indexOf(':')
    if (colon <= 0) return null
    const prop = decl.slice(0, colon).trim().toLowerCase()
    const value = decl.slice(colon + 1).trim().toLowerCase()
    if (!isAllowedDeclaration(prop, value)) return null
    kept.push(`${prop}:${value}`)
  }
  return kept.length > 0 ? kept.join(';') + ';' : null
}

/** SVG presentation attributes that accept a `url(#…)` paint reference. */
const PAINT_ATTRS = ['fill', 'stroke', 'filter', 'mask', 'clip-path']

export function sanitizeZulipHtml(html: string, prefix: string, selfUserId?: number): string {
  const clean = DOMPurify.sanitize(html, { ALLOWED_TAGS, ALLOWED_ATTR })
  const doc = new DOMParser().parseFromString(clean, 'text/html')
  // The visible math is .katex-html; .katex-mathml is the MathML fallback whose
  // raw-TeX <annotation> would otherwise leak as duplicate text after the
  // (unallowlisted) math tags are stripped.
  for (const el of Array.from(doc.querySelectorAll('.katex-mathml'))) {
    el.remove()
  }
  // Filter inline styles down to the KaTeX layout allowlist.
  for (const el of Array.from(doc.querySelectorAll('[style]'))) {
    const filtered = filterStyleAttribute(el.getAttribute('style') ?? '')
    if (filtered === null) {
      el.removeAttribute('style')
    } else {
      el.setAttribute('style', filtered)
    }
  }
  // Paint attributes can smuggle a URL past the style filter.
  for (const attr of PAINT_ATTRS) {
    for (const el of Array.from(doc.querySelectorAll(`[${attr}]`))) {
      const value = el.getAttribute(attr) ?? ''
      // CSS escapes (e.g. `\75\72\6c(...)`) tokenize back into `url(...)`;
      // any backslash in a paint value is hostile enough to drop outright.
      if (/url\s*\(/i.test(value) || value.includes('\\')) el.removeAttribute(attr)
    }
  }
  // Replace spritesheet emoji spans with their Unicode characters.
  for (const el of Array.from(doc.querySelectorAll('[class]'))) {
    const token = Array.from(el.classList).find((c) => EMOJI_CLASS_RE.test(c))
    if (!token) continue
    const decoded = decodeEmojiToken(token.slice('emoji-'.length))
    // Malformed codepoints: leave the element exactly as it is rather than
    // crash the view.
    if (decoded === null) continue
    el.replaceWith(document.createTextNode(decoded))
  }
  // Some realms emit <img class="emoji" alt="..."> instead of spans.
  for (const img of Array.from(doc.querySelectorAll('img.emoji'))) {
    img.replaceWith(document.createTextNode(img.getAttribute('alt') ?? ''))
  }
  // Highlight the signed-in user's own mentions (Zulip renders these as
  // spans with data-user-id; group mentions carry no user id).
  if (selfUserId !== undefined) {
    for (const el of Array.from(
      doc.querySelectorAll('span.user-mention, span.user-group-mention'),
    )) {
      if (el.getAttribute('data-user-id') === String(selfUserId)) {
        el.classList.add('mention-me')
      }
    }
  }
  // Same-origin absolute URLs (e.g. a pasted `https://app.host/user_uploads/…`
  // link) are folded back to app-relative paths so they get the proxy prefix
  // like any other relative link — otherwise a click navigates the SPA itself.
  const origin = typeof window !== 'undefined' ? window.location.origin : ''
  const relativize = (url: string): string =>
    origin && url.startsWith(origin + '/') ? url.slice(origin.length) : url
  const proxied = (path: string): string =>
    path.startsWith(prefix + '/') ? path : prefix + path
  for (const a of Array.from(doc.querySelectorAll('a'))) {
    const href = relativize(a.getAttribute('href') ?? '')
    if (href.startsWith('/')) a.setAttribute('href', proxied(href))
    a.setAttribute('target', '_blank')
    a.setAttribute('rel', 'noopener noreferrer')
  }
  for (const img of Array.from(doc.querySelectorAll('img'))) {
    const src = relativize(img.getAttribute('src') ?? '')
    if (src.startsWith('/')) img.setAttribute('src', proxied(src))
  }
  return doc.body.innerHTML
}
