import MarkdownIt from 'markdown-it'
import katex from 'katex'

/**
 * Markdown renderer for hub messages (which arrive as raw markdown, unlike
 * Zulip's server-rendered HTML). Output is NOT sanitized — every caller must
 * feed it through sanitizeZulipHtml, which stays the single choke point for
 * HTML entering the DOM. Raw HTML in the source is disabled outright.
 */
const md = new MarkdownIt({ html: false, linkify: true, breaks: true })

function renderTex(tex: string, displayMode: boolean): string {
  return katex.renderToString(tex, { throwOnError: false, displayMode })
}

// Display math: a paragraph-level $$…$$ block.
md.block.ruler.before('fence', 'math_block', (state, start, end, silent) => {
  const line = state.getLines(start, start + 1, 0, false).trim()
  if (!line.startsWith('$$')) return false
  let last = start
  let content = ''
  if (line.length > 2 && line.endsWith('$$')) {
    content = line.slice(2, -2)
  } else {
    for (last = start + 1; last <= end; last += 1) {
      if (last === end) return false
      const l = state.getLines(last, last + 1, 0, false).trim()
      if (l.endsWith('$$')) {
        content = state.getLines(start, last + 1, 0, false).trim().slice(2, -2)
        break
      }
    }
  }
  if (silent) return true
  const token = state.push('math_block', 'div', 0)
  token.content = content
  state.line = last + 1
  return true
})
md.renderer.rules.math_block = (tokens, i) => renderTex(tokens[i].content, true)

// Inline math: $…$ with no space just inside the delimiters (so "$5 and $10"
// stays money, mirroring Pandoc's rule).
md.inline.ruler.after('escape', 'math_inline', (state, silent) => {
  const src = state.src
  if (src[state.pos] !== '$') return false
  const close = src.indexOf('$', state.pos + 1)
  if (close < 0) return false
  const content = src.slice(state.pos + 1, close)
  if (content.length === 0 || /^\s|\s$/.test(content)) return false
  if (!silent) {
    const token = state.push('math_inline', 'span', 0)
    token.content = content
  }
  state.pos = close + 1
  return true
})
md.renderer.rules.math_inline = (tokens, i) => renderTex(tokens[i].content, false)

export function renderMarkdown(source: string): string {
  return md.render(source)
}
