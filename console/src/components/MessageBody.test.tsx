import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MessageBody, filenameFromHref, plainTextFallback, sanitizeZulipHtml } from './MessageBody'
import { useAppStore, resetAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'
import * as fx from '../test/fixtures/zulipHtml'

const PREFIX = '/zulip/qec'
const AUTH = 'Basic ' + btoa('me@b.c:k')

function okBlobResponse(): Response {
  return new Response(new Blob(['fake-image-bytes']), {
    status: 200,
    headers: { 'Content-Type': 'image/png' },
  })
}

let objectUrlCounter = 0
const originalCreateObjectURL = URL.createObjectURL
const originalRevokeObjectURL = URL.revokeObjectURL

beforeEach(() => {
  objectUrlCounter = 0
  URL.createObjectURL = (() => `blob:mock-${++objectUrlCounter}`) as typeof URL.createObjectURL
  URL.revokeObjectURL = vi.fn() as typeof URL.revokeObjectURL
  resetAppStore()
  // Signed in, so the 401 tests below can observe the session being cleared.
  useAppStore.setState({ creds: { prefix: PREFIX, email: 'me@b.c', apiKey: 'k', sendTopic: '' } })
})

afterEach(() => {
  URL.createObjectURL = originalCreateObjectURL
  URL.revokeObjectURL = originalRevokeObjectURL
  vi.unstubAllGlobals()
})

test('keeps code block structure and classes', () => {
  const out = sanitizeZulipHtml(fx.codeBlock, PREFIX)
  expect(out).toContain('codehilite')
  expect(out).toContain('<pre>')
  expect(out).toContain('class="k"')
})

test('keeps KaTeX spans', () => {
  expect(sanitizeZulipHtml(fx.katexMath, PREFIX)).toContain('katex')
})

describe('math / markdown rendering fidelity', () => {
  test('keeps inline styles that carry KaTeX layout', () => {
    const out = sanitizeZulipHtml(fx.katexDisplayMath, PREFIX)
    expect(out).toContain('style="height:0.8141em;"')
    expect(out).toContain('style="top:-3.063em;margin-right:0.05em;"')
  })

  test('removes the .katex-mathml fallback so raw TeX does not leak as text', () => {
    const out = sanitizeZulipHtml(fx.katexDisplayMath, PREFIX)
    expect(out).not.toContain('katex-mathml')
    expect(out).not.toContain('annotation')
    expect(out).not.toContain('x^2')
    expect(out).not.toContain('<math')
    expect(out).not.toContain('</math>')
  })

  test('keeps the .katex-html visible layout intact', () => {
    const out = sanitizeZulipHtml(fx.katexDisplayMath, PREFIX)
    expect(out).toContain('katex-html')
    expect(out).toContain('aria-hidden="true"')
    expect(out).toContain('msupsub')
    expect(out).toContain('katex-display')
  })

  test('keeps KaTeX SVG sqrt structure (svg, path, viewBox, preserveAspectRatio, d)', () => {
    const out = sanitizeZulipHtml(fx.katexSvgSqrt, PREFIX)
    expect(out).toContain('<svg')
    expect(out).toContain('<path')
    expect(out).toContain('viewBox="0 0 400000 1080"')
    expect(out).toContain('preserveAspectRatio="xMinYMin slice"')
    expect(out).toContain('d="M95,702c-2.7,0,-7.17,-2.7,-13.5,-8c-5.8,-5.3,-9.5,-10,-9.5,-14"')
    expect(out).toContain('xmlns="http://www.w3.org/2000/svg"')
    expect(out).toContain('style="min-width:0.853em;height:1.08em;"')
  })

  test('keeps markdown table structure (thead/tbody/th/td)', () => {
    const out = sanitizeZulipHtml(fx.tableMarkup, PREFIX)
    expect(out).toContain('<table>')
    expect(out).toContain('<thead>')
    expect(out).toContain('<tbody>')
    expect(out).toContain('<th>')
    expect(out).toContain('<td>')
  })

  test('converts spritesheet emoji spans to Unicode characters', () => {
    const out = sanitizeZulipHtml(fx.emojiThumbsUp, PREFIX)
    expect(out).toContain('👍')
    expect(out).not.toContain(':thumbs_up:')
    expect(out).not.toContain('emoji-1f44d')
  })

  test('converts multi-codepoint emoji spans to a combined Unicode sequence', () => {
    const out = sanitizeZulipHtml(fx.emojiFlagCn, PREFIX)
    expect(out).toContain('🇨🇳')
    expect(out).not.toContain(':cn:')
  })

  test('converts emoji img elements to their alt text', () => {
    const out = sanitizeZulipHtml(fx.emojiImg, PREFIX)
    expect(out).toContain('🎉')
    expect(out).not.toContain('<img')
  })

  test('keeps layout style attributes but still strips event handlers', () => {
    const out = sanitizeZulipHtml(
      '<div style="height:1.2em" onclick="alert(1)">hi</div>',
      PREFIX,
    )
    expect(out).toContain('style="height:1.2em;"')
    expect(out).not.toContain('onclick')
  })
})

describe('inline style filtering', () => {
  test('keeps KaTeX-typical declarations verbatim', () => {
    for (const style of [
      'height:0.8141em;',
      'top:-3.063em;margin-right:0.05em;',
      'min-width:0.853em;height:1.08em;',
      'vertical-align:-0.3em;',
      'position:relative;',
      'width:100%;padding:0 0.2em;',
      'transform:scale(-1);',
    ]) {
      const out = sanitizeZulipHtml(`<span style="${style}">x</span>`, PREFIX)
      expect(out, style).toContain(`style="${style.endsWith(';') ? style : style + ';'}"`)
    }
  })

  test('strips a style carrying a remote url() beacon', () => {
    const out = sanitizeZulipHtml(
      '<span style="background-image:url(https://x/beacon)">x</span>',
      PREFIX,
    )
    expect(out).not.toContain('url(')
    expect(out).not.toContain('beacon')
    expect(out).not.toContain('style=')
  })

  test('neutralizes an overlay style (position:fixed;inset:0)', () => {
    const out = sanitizeZulipHtml('<div style="position:fixed;inset:0">x</div>', PREFIX)
    expect(out).not.toContain('fixed')
    expect(out).not.toContain('inset')
    expect(out).not.toContain('style=')
  })

  test('drops the whole style attribute when any declaration fails', () => {
    const out = sanitizeZulipHtml(
      '<span style="height:1em;background:red">x</span>',
      PREFIX,
    )
    expect(out).not.toContain('style=')
  })

  test.each([
    ['non-allowlisted property', 'background-color:red'],
    ['sticky positioning', 'position:sticky'],
    ['escaped url', 'background-image:u\\rl(https://x/y)'],
    ['css comment smuggling', 'height:1em/*x*/;background:url(https://x/y)'],
    ['important override', 'height:1em !important'],
    ['transform with a non-numeric arg', 'transform:translate(attr(x))'],
    ['unknown transform function', 'transform:perspective(1px)'],
  ])('rejects style: %s', (_name, style) => {
    const out = sanitizeZulipHtml(`<span style="${style}">x</span>`, PREFIX)
    expect(out).not.toContain('style=')
  })

  test('strips url() paint references from SVG fill attributes', () => {
    const out = sanitizeZulipHtml(
      '<svg><path fill="url(https://x/y)" d="M0,0"/></svg>',
      PREFIX,
    )
    expect(out).not.toContain('url(')
    expect(out).not.toContain('fill=')
    expect(out).toContain('d="M0,0"')
  })

  test('keeps plain SVG fill colors', () => {
    const out = sanitizeZulipHtml('<svg><path fill="currentColor" d="M0,0"/></svg>', PREFIX)
    expect(out).toContain('fill="currentColor"')
  })
})

describe('emoji codepoint decoding', () => {
  test.each([
    ['out of Unicode range', 'emoji-110000'],
    ['lone surrogate', 'emoji-d800'],
    ['absurdly large hex', 'emoji-fffffffffffff'],
    ['too many codepoints', 'emoji-1f1e8-1f1e8-1f1e8-1f1e8-1f1e8-1f1e8-1f1e8-1f1e8-1f1e8'],
  ])('leaves the element untouched for %s', (_name, cls) => {
    const html = `<p><span class="emoji ${cls}">:x:</span></p>`
    expect(() => sanitizeZulipHtml(html, PREFIX)).not.toThrow()
    const out = sanitizeZulipHtml(html, PREFIX)
    expect(out).toContain(':x:')
  })

  test('still decodes valid codepoints at the range boundary', () => {
    const out = sanitizeZulipHtml('<p><span class="emoji-10ffff">:x:</span></p>', PREFIX)
    expect(out).not.toContain(':x:')
  })
})

test('rewrites relative upload links and images to the proxy prefix', () => {
  expect(sanitizeZulipHtml(fx.uploadLink, PREFIX)).toContain('href="/zulip/qec/user_uploads/2/ab/report.pdf"')
  expect(sanitizeZulipHtml(fx.inlineImage, PREFIX)).toContain('src="/zulip/qec/user_uploads/2/ab/plot.png"')
})

test('leaves absolute external links alone but adds rel/target', () => {
  const out = sanitizeZulipHtml(fx.externalLink, PREFIX)
  expect(out).toContain('href="https://arxiv.org/abs/2401.00001"')
  expect(out).toContain('target="_blank"')
  expect(out).toContain('rel="noopener noreferrer"')
})

test.each([
  ['script tag', fx.hostileScript],
  ['img onerror', fx.hostileImgHandler],
  ['javascript: href', fx.hostileJsHref],
])('strips hostile payload: %s', (_name, html) => {
  const out = sanitizeZulipHtml(html, PREFIX)
  expect(out).not.toContain('script')
  expect(out).not.toContain('onerror')
  expect(out).not.toContain('javascript:')
})

test('component renders sanitized HTML', () => {
  const { container } = render(<MessageBody html={fx.codeBlock} prefix={PREFIX} />)
  expect(container.querySelector('.message-body pre')).not.toBeNull()
})

test('keeps mention spans with their classes and data-user-id', () => {
  const out = sanitizeZulipHtml(fx.userMention, PREFIX)
  expect(out).toContain('class="user-mention"')
  expect(out).toContain('data-user-id="42"')
  expect(out).toContain('title="@Alice Doe"')
})

test('keeps group-mention spans', () => {
  const out = sanitizeZulipHtml(fx.userGroupMention, PREFIX)
  expect(out).toContain('class="user-group-mention"')
})

test('selfUserId match adds the mention-me highlight class', () => {
  const out = sanitizeZulipHtml(fx.userMention, PREFIX, 42)
  expect(out).toContain('mention-me')
  expect(out).toContain('user-mention mention-me')
  expect(out).toContain('data-user-id="42"')
})

test('non-matching selfUserId leaves mentions unhighlighted', () => {
  const out = sanitizeZulipHtml(fx.userMention, PREFIX, 7)
  expect(out).not.toContain('mention-me')
  expect(out).toContain('data-user-id="42"')
})

test('selfUserId does not highlight group mentions (no data-user-id)', () => {
  const out = sanitizeZulipHtml(fx.userGroupMention, PREFIX, 1)
  expect(out).not.toContain('mention-me')
})

test('component selfUserId prop highlights own mentions', () => {
  const { container } = render(
    <MessageBody html={fx.userMention} prefix={PREFIX} selfUserId={42} />,
  )
  const mention = container.querySelector('.user-mention')!
  expect(mention.className).toContain('mention-me')
  expect(mention.getAttribute('data-user-id')).toBe('42')
})

test('user_uploads image loads via authenticated fetch into a blob src', async () => {
  const fetchMock = vi.fn(async () => okBlobResponse())
  vi.stubGlobal('fetch', fetchMock)
  const { container } = render(
    <MessageBody html={fx.inlineImage} prefix={PREFIX} authHeader={AUTH} />,
  )
  const img = container.querySelector('img')!
  await waitFor(() => expect(img.getAttribute('src')).toBe('blob:mock-1'))
  expect(fetchMock).toHaveBeenCalledWith('/zulip/qec/user_uploads/2/ab/plot.png', {
    headers: { Authorization: AUTH },
  })
})

test('non-upload images are left untouched', () => {
  const fetchMock = vi.fn()
  vi.stubGlobal('fetch', fetchMock)
  const { container } = render(
    <MessageBody html='<img src="/static/logo.png">' prefix={PREFIX} authHeader={AUTH} />,
  )
  expect(container.querySelector('img')!.getAttribute('src')).toBe('/zulip/qec/static/logo.png')
  expect(fetchMock).not.toHaveBeenCalled()
})

test('user_uploads anchor click downloads the file via a blob anchor', async () => {
  const fetchMock = vi.fn(async () => okBlobResponse())
  vi.stubGlobal('fetch', fetchMock)
  const open = vi.fn()
  const originalOpen = window.open
  window.open = open as typeof window.open
  const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, 'click')
  try {
    render(<MessageBody html={fx.uploadLink} prefix={PREFIX} authHeader={AUTH} />)
    const linkEl = screen.getByText('report.pdf') as HTMLAnchorElement
    await userEvent.click(linkEl)
    await waitFor(() => expect(clickSpy).toHaveBeenCalledTimes(1))
    const clicked = clickSpy.mock.instances[0] as unknown as HTMLAnchorElement
    expect(clicked.download).toBe('report.pdf')
    expect(clicked.href).toBe('blob:mock-1')
    expect(fetchMock).toHaveBeenCalledWith('/zulip/qec/user_uploads/2/ab/report.pdf', {
      headers: { Authorization: AUTH },
    })
    expect(open).not.toHaveBeenCalled()
    // Safari-safe: the blob URL must NOT be revoked synchronously after click
    expect(URL.revokeObjectURL).not.toHaveBeenCalledWith('blob:mock-1')
  } finally {
    window.open = originalOpen
    clickSpy.mockRestore()
  }
})

test('REGRESSION: download still works after React replaces the message innerHTML', async () => {
  // The original per-anchor listeners were silently orphaned whenever
  // dangerouslySetInnerHTML re-set the subtree; delegation must survive it.
  const fetchMock = vi.fn(async () => okBlobResponse())
  vi.stubGlobal('fetch', fetchMock)
  const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, 'click')
  try {
    const { rerender } = render(
      <MessageBody html={fx.uploadLink} prefix={PREFIX} authHeader={AUTH} />,
    )
    // Force an innerHTML replacement by rendering different content, then back.
    rerender(<MessageBody html={'<p>interim</p>'} prefix={PREFIX} authHeader={AUTH} />)
    rerender(<MessageBody html={fx.uploadLink} prefix={PREFIX} authHeader={AUTH} />)
    await userEvent.click(screen.getByText('report.pdf'))
    await waitFor(() => expect(clickSpy).toHaveBeenCalledTimes(1))
    expect(fetchMock).toHaveBeenCalledWith('/zulip/qec/user_uploads/2/ab/report.pdf', {
      headers: { Authorization: AUTH },
    })
  } finally {
    clickSpy.mockRestore()
  }
})

test('clicking an upload image anchor opens the zoom lightbox, not a download', async () => {
  const fetchMock = vi.fn(async () => okBlobResponse())
  vi.stubGlobal('fetch', fetchMock)
  const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, 'click')
  try {
    const { container } = render(
      <MessageBody html={fx.inlineImage} prefix={PREFIX} authHeader={AUTH} />,
    )
    // wait for the authenticated swap so the lightbox reuses the blob
    await waitFor(() =>
      expect(container.querySelector('img')!.getAttribute('src')).toBe('blob:mock-1'),
    )
    await userEvent.click(container.querySelector('img')!)
    const dialog = await screen.findByRole('dialog')
    expect(dialog.querySelector('img')!.getAttribute('src')).toBe('blob:mock-1')
    expect(clickSpy).not.toHaveBeenCalled() // no download anchor was clicked
    // click closes it
    await userEvent.click(dialog)
    expect(screen.queryByRole('dialog')).toBeNull()
  } finally {
    clickSpy.mockRestore()
  }
})

test('clicking a plain (non-upload) message image zooms it', async () => {
  vi.stubGlobal('fetch', vi.fn())
  const { container } = render(
    <MessageBody html={'<img src="/static/logo.png" alt="logo">'} prefix={PREFIX} authHeader={AUTH} />,
  )
  await userEvent.click(container.querySelector('img')!)
  const dialog = await screen.findByRole('dialog')
  expect(dialog.querySelector('img')!.getAttribute('src')).toBe('/zulip/qec/static/logo.png')
})

test('filenameFromHref takes the last URL-decoded path segment', () => {
  expect(filenameFromHref('/zulip/qec/user_uploads/2/ab/report.pdf')).toBe('report.pdf')
  expect(filenameFromHref('/zulip/qec/user_uploads/2/ab/my%20file.pdf')).toBe('my file.pdf')
  expect(filenameFromHref('/zulip/qec/user_uploads/2/ab/report.pdf?download=1#x')).toBe('report.pdf')
})

test('sanitize strips CSS-escaped url() smuggled into SVG paint attributes', () => {
  const out = sanitizeZulipHtml(
    '<svg width="1em" height="1em"><path d="M0 0" fill="\\75\\72\\6c(https://attacker/x.svg#p)"/></svg>',
    PREFIX,
  )
  expect(out).not.toContain('fill=')
  expect(out).not.toContain('attacker')
})

test('failed download surfaces a visible error instead of silence', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('nope', { status: 503 })))
  render(<MessageBody html={fx.uploadLink} prefix={PREFIX} authHeader={AUTH} />)
  await userEvent.click(screen.getByText('report.pdf'))
  expect(await screen.findByRole('alert')).toHaveTextContent(/could not download report\.pdf/i)
  expect(useAppStore.getState().creds).not.toBeNull()
})

describe('a 401 on an attachment is a revoked session, not a broken file', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
  })

  test('downloading', async () => {
    render(<MessageBody html={fx.uploadLink} prefix={PREFIX} authHeader={AUTH} />)
    await userEvent.click(screen.getByText('report.pdf'))
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
  })

  test('opening the lightbox', async () => {
    render(
      <MessageBody
        html={`<p><a href="${PREFIX}/user_uploads/1/ab/shot.png">shot.png</a></p>`}
        prefix={PREFIX}
        authHeader={AUTH}
      />,
    )
    await userEvent.click(screen.getByText('shot.png'))
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
  })
})

test('inline image preview block keeps its Zulip classes for styling', () => {
  const out = sanitizeZulipHtml(fx.inlineImage, PREFIX)
  expect(out).toContain('message_inline_image')
})

describe('markdown format', () => {
  test('renders markdown content through the sanitizer', async () => {
    render(<MessageBody html={'**hi** $x^2$'} prefix="" format="markdown" />)
    // The renderer is a lazily-imported chunk, so the rendered form arrives a
    // microtask later than the plain-text placeholder.
    await waitFor(() =>
      expect(document.querySelector('.message-body strong')?.textContent).toBe('hi'),
    )
    expect(document.querySelector('.message-body .katex')).not.toBeNull()
  })

  test('hostile markdown cannot smuggle handlers past the sanitizer', async () => {
    const { container } = render(
      <MessageBody html={'[x](javascript:alert(1)) **rendered**'} prefix="" format="markdown" />,
    )
    // The bold marker proves the real renderer ran, not the plain-text fallback.
    await waitFor(() => expect(container.querySelector('strong')).not.toBeNull())
    const body = container.querySelector('.message-body')!
    expect(body.querySelector('a')).toBeNull()
    expect(body.innerHTML).not.toContain('href')
  })

  test('the pre-load placeholder escapes its source instead of injecting it', () => {
    // What the reader sees for the microsecond before the renderer chunk lands.
    // It is raw user/agent text, so it must be escaped, not parsed.
    const out = plainTextFallback('<img src=x onerror=alert(1)> & "quoted"')
    expect(out).not.toContain('<img')
    expect(out).toContain('&lt;img')
    expect(out).toContain('&amp;')
    expect(out).toMatch(/^<p>.*<\/p>$/)
  })

  test('hub file links download via delegation instead of navigating', async () => {
    // The rendering format is irrelevant here — what matters is the href shape.
    const fetchMock = vi.fn(async () => okBlobResponse())
    vi.stubGlobal('fetch', fetchMock)
    const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, 'click')
    try {
      render(
        <MessageBody
          html={'<p><a href="/api/chambers/c1/files/ab12_report.pdf">report.pdf</a></p>'}
          prefix=""
          authHeader={AUTH}
        />,
      )
      const link = screen.getByText('report.pdf')
      const event = new MouseEvent('click', { bubbles: true, cancelable: true })
      link.dispatchEvent(event)
      expect(event.defaultPrevented).toBe(true)
      await waitFor(() => expect(clickSpy).toHaveBeenCalledTimes(1))
      const clicked = clickSpy.mock.instances[0] as unknown as HTMLAnchorElement
      expect(clicked.download).toBe('ab12_report.pdf')
      expect(fetchMock).toHaveBeenCalledWith('/api/chambers/c1/files/ab12_report.pdf', {
        headers: { Authorization: AUTH },
      })
    } finally {
      clickSpy.mockRestore()
    }
  })
})

describe('same-origin absolute link normalization', () => {
  test('an absolute same-origin upload URL becomes a proxied relative link', () => {
    const abs = `${window.location.origin}/user_uploads/90996/abc/review.pdf`
    const out = sanitizeZulipHtml(`<p><a href="${abs}">review.pdf</a></p>`, PREFIX)
    expect(out).toContain(`href="${PREFIX}/user_uploads/90996/abc/review.pdf"`)
    expect(out).not.toContain(window.location.origin)
  })

  test('an already-prefixed same-origin URL is not double-prefixed', () => {
    const abs = `${window.location.origin}${PREFIX}/user_uploads/90996/abc/review.pdf`
    const out = sanitizeZulipHtml(`<p><a href="${abs}">review.pdf</a></p>`, PREFIX)
    expect(out).toContain(`href="${PREFIX}/user_uploads/90996/abc/review.pdf"`)
    expect(out).not.toContain(`${PREFIX}${PREFIX}`)
  })

  test('foreign-origin absolute URLs are left alone', () => {
    const out = sanitizeZulipHtml('<p><a href="https://arxiv.org/abs/1">x</a></p>', PREFIX)
    expect(out).toContain('href="https://arxiv.org/abs/1"')
  })
})

describe('code block copy button', () => {
  function stubClipboard() {
    const writeText = vi.fn(async () => {})
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    return writeText
  }

  test('every code block gets a copy button that copies its text', async () => {
    const writeText = stubClipboard()
    const { container } = render(
      <MessageBody html={'<pre><code>let x = 1</code></pre>'} prefix={PREFIX} />,
    )
    // The button is added by the MutationObserver pass, so it appears async.
    const btn = await waitFor(() => {
      const el = container.querySelector('button.code-copy')
      expect(el).not.toBeNull()
      return el as HTMLButtonElement
    })
    expect(btn).toHaveTextContent('Copy')
    await userEvent.click(btn)
    expect(writeText).toHaveBeenCalledWith('let x = 1')
    await waitFor(() => expect(btn).toHaveTextContent('Copied'))
  })

  test.each([
    ['a rejected write', () => {
      const writeText = vi.fn().mockRejectedValue(new Error('permission denied'))
      Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    }],
    ['no clipboard API at all', () => {
      Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true })
    }],
  ])('%s keeps the label on Copy and reports the failure', async (_name, stub) => {
    stub()
    const { container } = render(
      <MessageBody html={'<pre><code>let x = 1</code></pre>'} prefix={PREFIX} />,
    )
    const btn = await waitFor(() => {
      const el = container.querySelector('button.code-copy')
      expect(el).not.toBeNull()
      return el as HTMLButtonElement
    })
    await userEvent.click(btn)
    expect(await screen.findByRole('alert')).toHaveTextContent(/could not copy/i)
    // Nothing was put on the clipboard, so "Copied" would be a lie.
    expect(btn).toHaveTextContent('Copy')
    expect(btn).not.toHaveTextContent('Copied')
  })

  test('the button is wired without an auth header too', async () => {
    stubClipboard()
    const { container } = render(<MessageBody html={fx.codeBlock} prefix={PREFIX} />)
    await waitFor(() => expect(container.querySelector('button.code-copy')).not.toBeNull())
  })

  test('re-rendering never stacks up duplicate buttons', async () => {
    stubClipboard()
    const { container, rerender } = render(
      <MessageBody html={'<pre><code>a</code></pre>'} prefix={PREFIX} authHeader={AUTH} />,
    )
    await waitFor(() => expect(container.querySelector('button.code-copy')).not.toBeNull())
    rerender(<MessageBody html={'<pre><code>a</code></pre>'} prefix={PREFIX} authHeader={AUTH} />)
    await waitFor(() => expect(container.querySelectorAll('button.code-copy')).toHaveLength(1))
  })

  test('copying does not trip the attachment click handlers', async () => {
    const writeText = stubClipboard()
    const fetchMock = vi.fn(async () => okBlobResponse())
    vi.stubGlobal('fetch', fetchMock)
    const { container } = render(
      <MessageBody
        html={'<pre><code><a href="/zulip/qec/user_uploads/2/ab/report.pdf">x</a></code></pre>'}
        prefix={PREFIX}
        authHeader={AUTH}
      />,
    )
    const btn = await waitFor(() => {
      const el = container.querySelector('button.code-copy')
      expect(el).not.toBeNull()
      return el as HTMLButtonElement
    })
    await userEvent.click(btn)
    expect(writeText).toHaveBeenCalled()
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
