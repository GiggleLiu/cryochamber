import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MessageBody, filenameFromHref, plainTextFallback, sanitizeHtml } from './MessageBody'
import { useAppStore, resetAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'
import * as fx from '../test/fixtures/messageHtml'

// A hub session carries no path prefix, and chamber file paths are absolute app
// paths — the prefixing branch of the sanitizer is exercised on its own below.
const PREFIX = ''
const PROXY = '/hub'
const AUTH = 'Bearer tok'
const FILE_PATH = '/api/chambers/cham-a/files/ab_report.pdf'
const IMAGE_PATH = '/api/chambers/cham-a/files/ab_plot.png'

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
  useAppStore.setState({
    creds: { kind: 'hub', prefix: PREFIX, email: 'Alice', apiKey: 'tok', sendTopic: '' },
  })
})

afterEach(() => {
  URL.createObjectURL = originalCreateObjectURL
  URL.revokeObjectURL = originalRevokeObjectURL
  vi.unstubAllGlobals()
})

/** The renderer is a lazily-imported chunk, so the rendered form arrives a
 * microtask later than the escaped plain-text placeholder. */
async function renderBody(props: Parameters<typeof MessageBody>[0], selector: string) {
  const utils = render(<MessageBody {...props} />)
  await waitFor(() => expect(utils.container.querySelector(selector)).not.toBeNull())
  return utils
}

test('keeps code block structure and classes', () => {
  const out = sanitizeHtml(fx.codeBlock, PREFIX)
  expect(out).toContain('codehilite')
  expect(out).toContain('<pre>')
  expect(out).toContain('class="k"')
})

test('keeps KaTeX spans', () => {
  expect(sanitizeHtml(fx.katexMath, PREFIX)).toContain('katex')
})

describe('math / markdown rendering fidelity', () => {
  test('keeps inline styles that carry KaTeX layout', () => {
    const out = sanitizeHtml(fx.katexDisplayMath, PREFIX)
    expect(out).toContain('style="height:0.8141em;"')
    expect(out).toContain('style="top:-3.063em;margin-right:0.05em;"')
  })

  test('removes the .katex-mathml fallback so raw TeX does not leak as text', () => {
    const out = sanitizeHtml(fx.katexDisplayMath, PREFIX)
    expect(out).not.toContain('katex-mathml')
    expect(out).not.toContain('annotation')
    expect(out).not.toContain('x^2')
    expect(out).not.toContain('<math')
    expect(out).not.toContain('</math>')
  })

  test('keeps the .katex-html visible layout intact', () => {
    const out = sanitizeHtml(fx.katexDisplayMath, PREFIX)
    expect(out).toContain('katex-html')
    expect(out).toContain('aria-hidden="true"')
    expect(out).toContain('msupsub')
    expect(out).toContain('katex-display')
  })

  test('keeps KaTeX SVG sqrt structure (svg, path, viewBox, preserveAspectRatio, d)', () => {
    const out = sanitizeHtml(fx.katexSvgSqrt, PREFIX)
    expect(out).toContain('<svg')
    expect(out).toContain('<path')
    expect(out).toContain('viewBox="0 0 400000 1080"')
    expect(out).toContain('preserveAspectRatio="xMinYMin slice"')
    expect(out).toContain('d="M95,702c-2.7,0,-7.17,-2.7,-13.5,-8c-5.8,-5.3,-9.5,-10,-9.5,-14"')
    expect(out).toContain('xmlns="http://www.w3.org/2000/svg"')
    expect(out).toContain('style="min-width:0.853em;height:1.08em;"')
  })

  test('keeps markdown table structure (thead/tbody/th/td)', () => {
    const out = sanitizeHtml(fx.tableMarkup, PREFIX)
    expect(out).toContain('<table>')
    expect(out).toContain('<thead>')
    expect(out).toContain('<tbody>')
    expect(out).toContain('<th>')
    expect(out).toContain('<td>')
  })

  test('converts spritesheet emoji spans to Unicode characters', () => {
    const out = sanitizeHtml(fx.emojiThumbsUp, PREFIX)
    expect(out).toContain('👍')
    expect(out).not.toContain(':thumbs_up:')
    expect(out).not.toContain('emoji-1f44d')
  })

  test('converts multi-codepoint emoji spans to a combined Unicode sequence', () => {
    const out = sanitizeHtml(fx.emojiFlagCn, PREFIX)
    expect(out).toContain('🇨🇳')
    expect(out).not.toContain(':cn:')
  })

  test('converts emoji img elements to their alt text', () => {
    const out = sanitizeHtml(fx.emojiImg, PREFIX)
    expect(out).toContain('🎉')
    expect(out).not.toContain('<img')
  })

  test('keeps layout style attributes but still strips event handlers', () => {
    const out = sanitizeHtml('<div style="height:1.2em" onclick="alert(1)">hi</div>', PREFIX)
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
      const out = sanitizeHtml(`<span style="${style}">x</span>`, PREFIX)
      expect(out, style).toContain(`style="${style.endsWith(';') ? style : style + ';'}"`)
    }
  })

  test('strips a style carrying a remote url() beacon', () => {
    const out = sanitizeHtml(
      '<span style="background-image:url(https://x/beacon)">x</span>',
      PREFIX,
    )
    expect(out).not.toContain('url(')
    expect(out).not.toContain('beacon')
    expect(out).not.toContain('style=')
  })

  test('neutralizes an overlay style (position:fixed;inset:0)', () => {
    const out = sanitizeHtml('<div style="position:fixed;inset:0">x</div>', PREFIX)
    expect(out).not.toContain('fixed')
    expect(out).not.toContain('inset')
    expect(out).not.toContain('style=')
  })

  test('drops the whole style attribute when any declaration fails', () => {
    const out = sanitizeHtml('<span style="height:1em;background:red">x</span>', PREFIX)
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
    const out = sanitizeHtml(`<span style="${style}">x</span>`, PREFIX)
    expect(out).not.toContain('style=')
  })

  test('strips url() paint references from SVG fill attributes', () => {
    const out = sanitizeHtml('<svg><path fill="url(https://x/y)" d="M0,0"/></svg>', PREFIX)
    expect(out).not.toContain('url(')
    expect(out).not.toContain('fill=')
    expect(out).toContain('d="M0,0"')
  })

  test('keeps plain SVG fill colors', () => {
    const out = sanitizeHtml('<svg><path fill="currentColor" d="M0,0"/></svg>', PREFIX)
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
    expect(() => sanitizeHtml(html, PREFIX)).not.toThrow()
    expect(sanitizeHtml(html, PREFIX)).toContain(':x:')
  })

  test('still decodes valid codepoints at the range boundary', () => {
    const out = sanitizeHtml('<p><span class="emoji-10ffff">:x:</span></p>', PREFIX)
    expect(out).not.toContain(':x:')
  })
})

test('rewrites relative links and images to the server prefix', () => {
  expect(sanitizeHtml(fx.attachmentLink, PROXY)).toContain(`href="${PROXY}${FILE_PATH}"`)
  expect(sanitizeHtml(fx.attachmentImage, PROXY)).toContain(`src="${PROXY}${IMAGE_PATH}"`)
})

test('leaves absolute external links alone but adds rel/target', () => {
  const out = sanitizeHtml(fx.externalLink, PREFIX)
  expect(out).toContain('href="https://arxiv.org/abs/2401.00001"')
  expect(out).toContain('target="_blank"')
  expect(out).toContain('rel="noopener noreferrer"')
})

test.each([
  ['script tag', fx.hostileScript],
  ['img onerror', fx.hostileImgHandler],
  ['javascript: href', fx.hostileJsHref],
])('strips hostile payload: %s', (_name, html) => {
  const out = sanitizeHtml(html, PREFIX)
  expect(out).not.toContain('script')
  expect(out).not.toContain('onerror')
  expect(out).not.toContain('javascript:')
})

test('keeps mention spans with their classes and data-user-id', () => {
  const out = sanitizeHtml(fx.userMention, PREFIX)
  expect(out).toContain('class="user-mention"')
  expect(out).toContain('data-user-id="42"')
  expect(out).toContain('title="@Alice Doe"')
})

test('keeps group-mention spans', () => {
  const out = sanitizeHtml(fx.userGroupMention, PREFIX)
  expect(out).toContain('class="user-group-mention"')
})

test('selfUserId match adds the mention-me highlight class', () => {
  const out = sanitizeHtml(fx.userMention, PREFIX, 42)
  expect(out).toContain('mention-me')
  expect(out).toContain('user-mention mention-me')
  expect(out).toContain('data-user-id="42"')
})

test('non-matching selfUserId leaves mentions unhighlighted', () => {
  const out = sanitizeHtml(fx.userMention, PREFIX, 7)
  expect(out).not.toContain('mention-me')
  expect(out).toContain('data-user-id="42"')
})

test('selfUserId does not highlight group mentions (no data-user-id)', () => {
  const out = sanitizeHtml(fx.userGroupMention, PREFIX, 1)
  expect(out).not.toContain('mention-me')
})

describe('markdown rendering', () => {
  test('renders markdown content through the sanitizer', async () => {
    const { container } = await renderBody(
      { source: '**hi** $x^2$', prefix: PREFIX },
      '.message-body strong',
    )
    expect(container.querySelector('.message-body strong')?.textContent).toBe('hi')
    expect(container.querySelector('.message-body .katex')).not.toBeNull()
  })

  test('hostile markdown cannot smuggle handlers past the sanitizer', async () => {
    const { container } = await renderBody(
      { source: '[x](javascript:alert(1)) **rendered**', prefix: PREFIX },
      'strong',
    )
    const body = container.querySelector('.message-body')!
    expect(body.querySelector('a')).toBeNull()
    expect(body.innerHTML).not.toContain('href')
  })

  test('raw HTML in the source is escaped, never parsed', async () => {
    const { container } = await renderBody(
      { source: '<img src=x onerror=alert(1)> **after**', prefix: PREFIX },
      'strong',
    )
    const body = container.querySelector('.message-body')!
    expect(body.querySelector('img')).toBeNull()
    expect(body.textContent).toContain('<img src=x onerror=alert(1)>')
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
})

describe('chamber attachments', () => {
  test('an attachment image loads via authenticated fetch into a blob src', async () => {
    const fetchMock = vi.fn(async () => okBlobResponse())
    vi.stubGlobal('fetch', fetchMock)
    const { container } = await renderBody(
      { source: `![plot.png](${IMAGE_PATH})`, prefix: PREFIX, authHeader: AUTH },
      'img',
    )
    const img = container.querySelector('img')!
    await waitFor(() => expect(img.getAttribute('src')).toBe('blob:mock-1'))
    expect(fetchMock).toHaveBeenCalledWith(IMAGE_PATH, { headers: { Authorization: AUTH } })
  })

  test('non-attachment images are left untouched', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    const { container } = await renderBody(
      { source: '![logo](/static/logo.png)', prefix: PREFIX, authHeader: AUTH },
      'img',
    )
    expect(container.querySelector('img')!.getAttribute('src')).toBe('/static/logo.png')
    expect(fetchMock).not.toHaveBeenCalled()
  })

  test('an attachment anchor click downloads the file via a blob anchor', async () => {
    const fetchMock = vi.fn(async () => okBlobResponse())
    vi.stubGlobal('fetch', fetchMock)
    const open = vi.fn()
    const originalOpen = window.open
    window.open = open as typeof window.open
    const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, 'click')
    try {
      await renderBody(
        { source: `[report.pdf](${FILE_PATH})`, prefix: PREFIX, authHeader: AUTH },
        'a',
      )
      await userEvent.click(screen.getByText('report.pdf'))
      await waitFor(() => expect(clickSpy).toHaveBeenCalledTimes(1))
      const clicked = clickSpy.mock.instances[0] as unknown as HTMLAnchorElement
      expect(clicked.download).toBe('ab_report.pdf')
      expect(clicked.href).toBe('blob:mock-1')
      expect(fetchMock).toHaveBeenCalledWith(FILE_PATH, { headers: { Authorization: AUTH } })
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
    const link = `[report.pdf](${FILE_PATH})`
    try {
      const { rerender } = await renderBody(
        { source: link, prefix: PREFIX, authHeader: AUTH },
        'a',
      )
      // Force an innerHTML replacement by rendering different content, then back.
      rerender(<MessageBody source={'interim'} prefix={PREFIX} authHeader={AUTH} />)
      rerender(<MessageBody source={link} prefix={PREFIX} authHeader={AUTH} />)
      await userEvent.click(screen.getByText('report.pdf'))
      await waitFor(() => expect(clickSpy).toHaveBeenCalledTimes(1))
      expect(fetchMock).toHaveBeenCalledWith(FILE_PATH, { headers: { Authorization: AUTH } })
    } finally {
      clickSpy.mockRestore()
    }
  })

  test('clicking an attachment image anchor opens the zoom lightbox, not a download', async () => {
    const fetchMock = vi.fn(async () => okBlobResponse())
    vi.stubGlobal('fetch', fetchMock)
    const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, 'click')
    try {
      const { container } = await renderBody(
        { source: `[![plot](${IMAGE_PATH})](${IMAGE_PATH})`, prefix: PREFIX, authHeader: AUTH },
        'img',
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

  test('clicking a plain (non-attachment) message image zooms it', async () => {
    vi.stubGlobal('fetch', vi.fn())
    const { container } = await renderBody(
      { source: '![logo](/static/logo.png)', prefix: PREFIX, authHeader: AUTH },
      'img',
    )
    await userEvent.click(container.querySelector('img')!)
    const dialog = await screen.findByRole('dialog')
    expect(dialog.querySelector('img')!.getAttribute('src')).toBe('/static/logo.png')
  })

  test('failed download surfaces a visible error instead of silence', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('nope', { status: 503 })))
    await renderBody({ source: `[report.pdf](${FILE_PATH})`, prefix: PREFIX, authHeader: AUTH }, 'a')
    await userEvent.click(screen.getByText('report.pdf'))
    expect(await screen.findByRole('alert')).toHaveTextContent(/could not download ab_report\.pdf/i)
    expect(useAppStore.getState().creds).not.toBeNull()
  })
})

test('filenameFromHref takes the last URL-decoded path segment', () => {
  expect(filenameFromHref(FILE_PATH)).toBe('ab_report.pdf')
  expect(filenameFromHref('/api/chambers/cham-a/files/my%20file.pdf')).toBe('my file.pdf')
  expect(filenameFromHref(`${FILE_PATH}?download=1#x`)).toBe('ab_report.pdf')
})

test('sanitize strips CSS-escaped url() smuggled into SVG paint attributes', () => {
  const out = sanitizeHtml(
    '<svg width="1em" height="1em"><path d="M0 0" fill="\\75\\72\\6c(https://attacker/x.svg#p)"/></svg>',
    PREFIX,
  )
  expect(out).not.toContain('fill=')
  expect(out).not.toContain('attacker')
})

describe('a 401 on an attachment is a revoked session, not a broken file', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
  })

  test('downloading', async () => {
    await renderBody({ source: `[report.pdf](${FILE_PATH})`, prefix: PREFIX, authHeader: AUTH }, 'a')
    await userEvent.click(screen.getByText('report.pdf'))
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
  })

  test('opening the lightbox', async () => {
    await renderBody(
      { source: `[shot.png](/api/chambers/cham-a/files/shot.png)`, prefix: PREFIX, authHeader: AUTH },
      'a',
    )
    await userEvent.click(screen.getByText('shot.png'))
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
  })
})

describe('same-origin absolute link normalization', () => {
  test('an absolute same-origin URL becomes a proxied relative link', () => {
    const abs = `${window.location.origin}${FILE_PATH}`
    const out = sanitizeHtml(`<p><a href="${abs}">report.pdf</a></p>`, PROXY)
    expect(out).toContain(`href="${PROXY}${FILE_PATH}"`)
    expect(out).not.toContain(window.location.origin)
  })

  test('an already-prefixed same-origin URL is not double-prefixed', () => {
    const abs = `${window.location.origin}${PROXY}${FILE_PATH}`
    const out = sanitizeHtml(`<p><a href="${abs}">report.pdf</a></p>`, PROXY)
    expect(out).toContain(`href="${PROXY}${FILE_PATH}"`)
    expect(out).not.toContain(`${PROXY}${PROXY}`)
  })

  test('foreign-origin absolute URLs are left alone', () => {
    const out = sanitizeHtml('<p><a href="https://arxiv.org/abs/1">x</a></p>', PROXY)
    expect(out).toContain('href="https://arxiv.org/abs/1"')
  })
})

describe('code block copy button', () => {
  const FENCE = '```\nlet x = 1\n```'

  function stubClipboard() {
    const writeText = vi.fn(async () => {})
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    return writeText
  }

  test('every code block gets a copy button that copies its text', async () => {
    const writeText = stubClipboard()
    const { container } = await renderBody({ source: FENCE, prefix: PREFIX }, 'pre')
    // The button is added by the MutationObserver pass, so it appears async.
    const btn = await waitFor(() => {
      const el = container.querySelector('button.code-copy')
      expect(el).not.toBeNull()
      return el as HTMLButtonElement
    })
    expect(btn).toHaveTextContent('Copy')
    await userEvent.click(btn)
    expect(writeText).toHaveBeenCalledWith('let x = 1\n')
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
    const { container } = await renderBody({ source: FENCE, prefix: PREFIX }, 'pre')
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
    const { container } = await renderBody({ source: FENCE, prefix: PREFIX }, 'pre')
    await waitFor(() => expect(container.querySelector('button.code-copy')).not.toBeNull())
  })

  test('re-rendering never stacks up duplicate buttons', async () => {
    stubClipboard()
    const { container, rerender } = await renderBody(
      { source: FENCE, prefix: PREFIX, authHeader: AUTH },
      'pre',
    )
    await waitFor(() => expect(container.querySelector('button.code-copy')).not.toBeNull())
    rerender(<MessageBody source={FENCE} prefix={PREFIX} authHeader={AUTH} />)
    await waitFor(() => expect(container.querySelectorAll('button.code-copy')).toHaveLength(1))
  })

  test('copying does not trip the attachment click handlers', async () => {
    const writeText = stubClipboard()
    const fetchMock = vi.fn(async () => okBlobResponse())
    vi.stubGlobal('fetch', fetchMock)
    const { container } = await renderBody(
      { source: `\`\`\`\n[x](${FILE_PATH})\n\`\`\``, prefix: PREFIX, authHeader: AUTH },
      'pre',
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
