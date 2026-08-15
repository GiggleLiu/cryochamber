import { renderMarkdown } from './markdown'

test('renders CommonMark basics', () => {
  const html = renderMarkdown('# Hi\n\n**bold** and `code`')
  expect(html).toContain('<h1>')
  expect(html).toContain('<strong>bold</strong>')
  expect(html).toContain('<code>code</code>')
})

test('renders tables and fenced code', () => {
  expect(renderMarkdown('| a | b |\n|---|---|\n| 1 | 2 |')).toContain('<table>')
  expect(renderMarkdown('```py\nx = 1\n```')).toContain('<pre>')
})

test('renders inline and display math via KaTeX', () => {
  expect(renderMarkdown('Euler: $e^{i\\pi}+1=0$')).toContain('katex')
  const display = renderMarkdown('$$\\int_0^1 x\\,dx$$')
  expect(display).toContain('katex-display')
})

test('dollar amounts are not eaten as math', () => {
  const html = renderMarkdown('costs $5 and $10 total')
  expect(html).toContain('$5')
  expect(html).toContain('$10')
  expect(html).not.toContain('katex')
})

test('raw HTML in markdown is not passed through', () => {
  const html = renderMarkdown('<img src=x onerror=alert(1)> hi')
  expect(html).not.toContain('<img src=x')
  expect(html).toContain('&lt;img')
})

test('invalid TeX degrades instead of throwing', () => {
  expect(() => renderMarkdown('$\\frobnicate{$')).not.toThrow()
})
