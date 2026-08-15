import { render, screen } from '@testing-library/react'
import { HtmlTab } from './HtmlTab'

test('renders the server HTML it was given', () => {
  const { container } = render(
    <HtmlTab html="<p>Step <strong>one</strong></p>" empty="No plan.md in this chamber." />,
  )
  expect(container.querySelector('.tab-html strong')).toHaveTextContent('one')
})

test('empty HTML shows the per-tab empty copy', () => {
  render(<HtmlTab html="" empty="No NOTES.md in this chamber." />)
  expect(screen.getByText('No NOTES.md in this chamber.')).toBeInTheDocument()
})

test('whitespace-only HTML counts as empty', () => {
  // An expression, not an attribute literal: JSX does not decode `\n` in
  // `html="   \n "`, so that form would pass a literal backslash-n — real
  // text, and rightly not empty.
  render(<HtmlTab html={'   \n '} empty="No plan.md in this chamber." />)
  expect(screen.getByText('No plan.md in this chamber.')).toBeInTheDocument()
})

test('the client sanitizes again even though the server already escaped', () => {
  const { container } = render(
    <HtmlTab
      html={'<p onclick="steal()">hi</p><script>steal()</script><a href="javascript:steal()">x</a>'}
      empty="No plan.md in this chamber."
    />,
  )
  const html = container.querySelector('.tab-html')!.innerHTML
  expect(html).not.toContain('script')
  expect(html).not.toContain('onclick')
  expect(html).not.toContain('javascript:')
  expect(container.querySelector('.tab-html')).toHaveTextContent('hi')
})

test('markup that sanitizes down to nothing shows the empty copy', () => {
  render(<HtmlTab html="<script>alert(1)</script>" empty="No plan.md in this chamber." />)
  expect(screen.getByText('No plan.md in this chamber.')).toBeInTheDocument()
  expect(document.querySelector('.tab-html')).toBeNull()
})
