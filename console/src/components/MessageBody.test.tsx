import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MessageBody, filenameFromHref, plainTextFallback, sanitizeHtml } from './MessageBody'
import { filterStyleAttribute } from './sanitize'
import { HubClient } from '../api/hubClient'
import * as fx from '../test/fixtures/messageHtml'

const FILE_PATH = '/api/chambers/cham-a/files/ab_report.pdf'
const IMAGE_PATH = '/api/chambers/cham-a/files/ab_plot.png'

/** The real authenticated fetcher the component is handed in the app: a
 * `HubClient.fetchBlob` over a stubbed transport, so the bearer header and the
 * ApiError on failure are the client's own and not a test invention. */
function fetcher(respond: () => Response | Promise<Response>, onAuthFailure?: () => void) {
  const fetchFn = vi.fn(async () => respond()) as unknown as typeof fetch
  const client = new HubClient({ token: 'tok', fetch: fetchFn, onAuthFailure })
  return { fetchBlob: (url: string) => client.fetchBlob(url), fetchFn }
}

/** What `fetchFn` sees for an authenticated attachment GET. */
const AUTH_GET = { headers: { Authorization: 'Bearer tok' } }

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
  const out = sanitizeHtml(fx.codeBlock)
  expect(out).toContain('codehilite')
  expect(out).toContain('<pre>')
  expect(out).toContain('class="k"')
})

test('keeps KaTeX spans', () => {
  expect(sanitizeHtml(fx.katexMath)).toContain('katex')
})

describe('math / markdown rendering fidelity', () => {
  test('keeps inline styles that carry KaTeX layout', () => {
    const out = sanitizeHtml(fx.katexDisplayMath)
    expect(out).toContain('style="height:0.8141em;"')
    expect(out).toContain('style="top:-3.063em;margin-right:0.05em;"')
  })

  test('removes the .katex-mathml fallback so raw TeX does not leak as text', () => {
    const out = sanitizeHtml(fx.katexDisplayMath)
    expect(out).not.toContain('katex-mathml')
    expect(out).not.toContain('annotation')
    expect(out).not.toContain('x^2')
    expect(out).not.toContain('<math')
    expect(out).not.toContain('</math>')
  })

  test('keeps the .katex-html visible layout intact', () => {
    const out = sanitizeHtml(fx.katexDisplayMath)
    expect(out).toContain('katex-html')
    expect(out).toContain('aria-hidden="true"')
    expect(out).toContain('msupsub')
    expect(out).toContain('katex-display')
  })

  test('keeps KaTeX SVG sqrt structure (svg, path, viewBox, preserveAspectRatio, d)', () => {
    const out = sanitizeHtml(fx.katexSvgSqrt)
    expect(out).toContain('<svg')
    expect(out).toContain('<path')
    expect(out).toContain('viewBox="0 0 400000 1080"')
    expect(out).toContain('preserveAspectRatio="xMinYMin slice"')
    expect(out).toContain('d="M95,702c-2.7,0,-7.17,-2.7,-13.5,-8c-5.8,-5.3,-9.5,-10,-9.5,-14"')
    expect(out).toContain('xmlns="http://www.w3.org/2000/svg"')
    expect(out).toContain('style="min-width:0.853em;height:1.08em;"')
  })

  test('keeps markdown table structure (thead/tbody/th/td)', () => {
    const out = sanitizeHtml(fx.tableMarkup)
    expect(out).toContain('<table>')
    expect(out).toContain('<thead>')
    expect(out).toContain('<tbody>')
    expect(out).toContain('<th>')
    expect(out).toContain('<td>')
  })

  test('keeps layout style attributes but still strips event handlers', () => {
    const out = sanitizeHtml('<div style="height:1.2em" onclick="alert(1)">hi</div>')
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
      const out = sanitizeHtml(`<span style="${style}">x</span>`)
      expect(out, style).toContain(`style="${style.endsWith(';') ? style : style + ';'}"`)
    }
  })

  test('strips a style carrying a remote url() beacon', () => {
    const out = sanitizeHtml('<span style="background-image:url(https://x/beacon)">x</span>')
    expect(out).not.toContain('url(')
    expect(out).not.toContain('beacon')
    expect(out).not.toContain('style=')
  })

  test('neutralizes an overlay style (position:fixed;inset:0)', () => {
    const out = sanitizeHtml('<div style="position:fixed;inset:0">x</div>')
    expect(out).not.toContain('fixed')
    expect(out).not.toContain('inset')
    expect(out).not.toContain('style=')
  })

  test('drops the whole style attribute when any declaration fails', () => {
    const out = sanitizeHtml('<span style="height:1em;background:red">x</span>')
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
    const out = sanitizeHtml(`<span style="${style}">x</span>`)
    expect(out).not.toContain('style=')
  })

  test('style lengths above 100 units are rejected', () => {
    expect(filterStyleAttribute('height:1000em;')).toBeNull()
    expect(filterStyleAttribute('height:99.5em;')).toBe('height:99.5em;')
    expect(filterStyleAttribute('margin:-0.5em 100px;')).toBe('margin:-0.5em 100px;')
  })

  test('strips url() paint references from SVG fill attributes', () => {
    const out = sanitizeHtml('<svg><path fill="url(https://x/y)" d="M0,0"/></svg>')
    expect(out).not.toContain('url(')
    expect(out).not.toContain('fill=')
    expect(out).toContain('d="M0,0"')
  })

  test('keeps plain SVG fill colors', () => {
    const out = sanitizeHtml('<svg><path fill="currentColor" d="M0,0"/></svg>')
    expect(out).toContain('fill="currentColor"')
  })
})

test('leaves app-relative links and images exactly as written', () => {
  // The console is served by the hub it talks to, so a chamber file path is
  // already the URL to fetch — there is no prefix to graft on.
  expect(sanitizeHtml(fx.attachmentLink)).toContain(`href="${FILE_PATH}"`)
  expect(sanitizeHtml(fx.attachmentImage)).toContain(`src="${IMAGE_PATH}"`)
})

test('leaves absolute external links alone but adds rel/target', () => {
  const out = sanitizeHtml(fx.externalLink)
  expect(out).toContain('href="https://arxiv.org/abs/2401.00001"')
  expect(out).toContain('target="_blank"')
  expect(out).toContain('rel="noopener noreferrer"')
})

test.each([
  ['script tag', fx.hostileScript],
  ['img onerror', fx.hostileImgHandler],
  ['javascript: href', fx.hostileJsHref],
])('strips hostile payload: %s', (_name, html) => {
  const out = sanitizeHtml(html)
  expect(out).not.toContain('script')
  expect(out).not.toContain('onerror')
  expect(out).not.toContain('javascript:')
})

test('unknown class tokens and data-user-id survive untouched — no emoji or mention rewriting', () => {
  const html = sanitizeHtml(
    '<span class="emoji-1f44d">:thumbs_up:</span>' +
      '<span class="user-mention" data-user-id="7">@me</span>',
  )
  expect(html).toContain('emoji-1f44d')
  expect(html).toContain(':thumbs_up:')
  expect(html).not.toContain('mention-me')
})

describe('markdown rendering', () => {
  test('renders markdown content through the sanitizer', async () => {
    const { container } = await renderBody(
      { source: '**hi** $x^2$' },
      '.message-body strong',
    )
    expect(container.querySelector('.message-body strong')?.textContent).toBe('hi')
    expect(container.querySelector('.message-body .katex')).not.toBeNull()
  })

  test('hostile markdown cannot smuggle handlers past the sanitizer', async () => {
    const { container } = await renderBody(
      { source: '[x](javascript:alert(1)) **rendered**' },
      'strong',
    )
    const body = container.querySelector('.message-body')!
    expect(body.querySelector('a')).toBeNull()
    expect(body.innerHTML).not.toContain('href')
  })

  test('raw HTML in the source is escaped, never parsed', async () => {
    const { container } = await renderBody(
      { source: '<img src=x onerror=alert(1)> **after**' },
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
    const { fetchBlob, fetchFn } = fetcher(okBlobResponse)
    const { container } = await renderBody(
      { source: `![plot.png](${IMAGE_PATH})`, fetchBlob },
      'img',
    )
    const img = container.querySelector('img')!
    await waitFor(() => expect(img.getAttribute('src')).toBe('blob:mock-1'))
    expect(fetchFn).toHaveBeenCalledWith(IMAGE_PATH, AUTH_GET)
  })

  test('an attachment image never carries the raw hub URL as its src', async () => {
    // Before the swap the browser must have nothing to fetch on its own: a
    // bare `src` would be requested without the bearer token, 401, and paint
    // a broken image until the blob arrived.
    let release!: (r: Response) => void
    const gate = new Promise<Response>((r) => {
      release = r
    })
    const { fetchBlob, fetchFn } = fetcher(() => gate)
    const { container } = await renderBody(
      { source: `![plot.png](${IMAGE_PATH})`, fetchBlob },
      'img',
    )
    const img = container.querySelector('img')!
    expect(img.hasAttribute('src')).toBe(false)
    expect(img.dataset.uploadSrc).toBe(IMAGE_PATH)
    expect(fetchFn).toHaveBeenCalledWith(IMAGE_PATH, AUTH_GET)
    release(okBlobResponse())
    await waitFor(() => expect(img.getAttribute('src')).toBe('blob:mock-1'))
  })

  test('a swap in flight still lands when the fetcher identity changes under it', async () => {
    // Same markdown, new fetcher. React re-sets the innerHTML, so the <img> is
    // a fresh node; whichever fetch answers first has to fill it in — the
    // first one's result used to be dropped as belonging to a stale effect,
    // and the thumbnail stayed blank until tapped.
    let release!: (r: Response) => void
    const gate = new Promise<Response>((r) => {
      release = r
    })
    const first = fetcher(() => gate)
    const { container, rerender } = await renderBody(
      { source: `![plot.png](${IMAGE_PATH})`, fetchBlob: first.fetchBlob },
      'img',
    )
    // The second fetcher never answers: only the first result can do it.
    const second = fetcher(() => new Promise<Response>(() => {}))
    rerender(<MessageBody source={`![plot.png](${IMAGE_PATH})`} fetchBlob={second.fetchBlob} />)
    release(okBlobResponse())
    await waitFor(() =>
      expect(container.querySelector('img')!.getAttribute('src')).toBe('blob:mock-1'),
    )
  })

  test('without a fetcher the plain src stays, as the only way the image can load', async () => {
    const { container } = await renderBody({ source: `![plot.png](${IMAGE_PATH})` }, 'img')
    expect(container.querySelector('img')!.getAttribute('src')).toBe(IMAGE_PATH)
  })

  test('non-attachment images are left untouched', async () => {
    const { fetchBlob, fetchFn } = fetcher(okBlobResponse)
    const { container } = await renderBody(
      { source: '![logo](/static/logo.png)', fetchBlob },
      'img',
    )
    expect(container.querySelector('img')!.getAttribute('src')).toBe('/static/logo.png')
    expect(fetchFn).not.toHaveBeenCalled()
  })

  test('an attachment anchor click downloads the file via a blob anchor', async () => {
    const { fetchBlob, fetchFn } = fetcher(okBlobResponse)
    const open = vi.fn()
    const originalOpen = window.open
    window.open = open as typeof window.open
    const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, 'click')
    try {
      await renderBody(
        { source: `[report.pdf](${FILE_PATH})`, fetchBlob },
        'a',
      )
      await userEvent.click(screen.getByText('report.pdf'))
      await waitFor(() => expect(clickSpy).toHaveBeenCalledTimes(1))
      const clicked = clickSpy.mock.instances[0] as unknown as HTMLAnchorElement
      expect(clicked.download).toBe('ab_report.pdf')
      expect(clicked.href).toBe('blob:mock-1')
      expect(fetchFn).toHaveBeenCalledWith(FILE_PATH, AUTH_GET)
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
    const { fetchBlob, fetchFn } = fetcher(okBlobResponse)
    const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, 'click')
    const link = `[report.pdf](${FILE_PATH})`
    try {
      const { rerender } = await renderBody(
        { source: link, fetchBlob },
        'a',
      )
      // Force an innerHTML replacement by rendering different content, then back.
      rerender(<MessageBody source={'interim'} fetchBlob={fetchBlob} />)
      rerender(<MessageBody source={link} fetchBlob={fetchBlob} />)
      await userEvent.click(screen.getByText('report.pdf'))
      await waitFor(() => expect(clickSpy).toHaveBeenCalledTimes(1))
      expect(fetchFn).toHaveBeenCalledWith(FILE_PATH, AUTH_GET)
    } finally {
      clickSpy.mockRestore()
    }
  })

  test('clicking an attachment image anchor opens the zoom lightbox, not a download', async () => {
    const { fetchBlob, fetchFn } = fetcher(okBlobResponse)
    const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, 'click')
    try {
      const { container } = await renderBody(
        { source: `[![plot](${IMAGE_PATH})](${IMAGE_PATH})`, fetchBlob },
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

  test('a plain link to an attachment image previews inline and zooms on click', async () => {
    const { fetchBlob, fetchFn } = fetcher(okBlobResponse)
    const { container } = await renderBody(
      { source: `[ab_plot.png](${IMAGE_PATH})`, fetchBlob },
      'img',
    )
    const img = container.querySelector('img')!
    expect(img.closest('a')!.getAttribute('href')).toBe(IMAGE_PATH)
    await waitFor(() => expect(img.getAttribute('src')).toBe('blob:mock-1'))
    expect(fetchFn).toHaveBeenCalledWith(IMAGE_PATH, AUTH_GET)
    await userEvent.click(img)
    const dialog = await screen.findByRole('dialog')
    expect(dialog.querySelector('img')!.getAttribute('src')).toBe('blob:mock-1')
  })

  test('a plain link to a non-image attachment stays a link', async () => {
    const { fetchBlob } = fetcher(okBlobResponse)
    const { container } = await renderBody({ source: `[report.pdf](${FILE_PATH})`, fetchBlob }, 'a')
    expect(container.querySelector('img')).toBeNull()
    expect(screen.getByText('report.pdf')).toBeTruthy()
  })

  test('clicking a plain (non-attachment) message image zooms it', async () => {
    const { fetchBlob } = fetcher(okBlobResponse)
    const { container } = await renderBody(
      { source: '![logo](/static/logo.png)', fetchBlob },
      'img',
    )
    await userEvent.click(container.querySelector('img')!)
    const dialog = await screen.findByRole('dialog')
    expect(dialog.querySelector('img')!.getAttribute('src')).toBe('/static/logo.png')
  })

  test('failed download surfaces a visible error instead of silence', async () => {
    const { fetchBlob } = fetcher(() => new Response('nope', { status: 503 }))
    await renderBody({ source: `[report.pdf](${FILE_PATH})`, fetchBlob }, 'a')
    await userEvent.click(screen.getByText('report.pdf'))
    expect(await screen.findByRole('alert')).toHaveTextContent(/could not download ab_report\.pdf/i)
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
  )
  expect(out).not.toContain('fill=')
  expect(out).not.toContain('attacker')
})

describe('a 401 on an attachment is a revoked session, not a broken file', () => {
  // The client owns the logout; the body must stay silent rather than blaming
  // the file for a session that no longer exists.
  const denied = () => new Response('', { status: 401 })

  test('downloading', async () => {
    const onAuthFailure = vi.fn()
    const { fetchBlob } = fetcher(denied, onAuthFailure)
    await renderBody({ source: `[report.pdf](${FILE_PATH})`, fetchBlob }, 'a')
    await userEvent.click(screen.getByText('report.pdf'))
    await waitFor(() => expect(onAuthFailure).toHaveBeenCalledTimes(1))
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('opening the lightbox', async () => {
    const onAuthFailure = vi.fn()
    const { fetchBlob } = fetcher(denied, onAuthFailure)
    await renderBody(
      { source: `[shot.png](/api/chambers/cham-a/files/shot.png)`, fetchBlob },
      'a',
    )
    // The link renders as a thumbnail, whose own authenticated fetch is denied
    // first; clicking it asks again for the lightbox and must stay just as
    // quiet.
    await userEvent.click(await screen.findByAltText('shot.png'))
    await waitFor(() => expect(onAuthFailure).toHaveBeenCalled())
    expect(screen.queryByRole('alert')).toBeNull()
  })
})

describe('same-origin absolute link normalization', () => {
  test('an absolute same-origin URL is folded back to an app path', () => {
    // Otherwise a click navigates the SPA itself and the attachment handlers
    // never see the href.
    const abs = `${window.location.origin}${FILE_PATH}`
    const out = sanitizeHtml(`<p><a href="${abs}">report.pdf</a></p>`)
    expect(out).toContain(`href="${FILE_PATH}"`)
    expect(out).not.toContain(window.location.origin)
  })

  test('foreign-origin absolute URLs are left alone', () => {
    const out = sanitizeHtml('<p><a href="https://arxiv.org/abs/1">x</a></p>')
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
    const { container } = await renderBody({ source: FENCE }, 'pre')
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
    const { container } = await renderBody({ source: FENCE }, 'pre')
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

  test('the button is wired without an authenticated fetcher too', async () => {
    stubClipboard()
    const { container } = await renderBody({ source: FENCE }, 'pre')
    await waitFor(() => expect(container.querySelector('button.code-copy')).not.toBeNull())
  })

  test('re-rendering never stacks up duplicate buttons', async () => {
    stubClipboard()
    const { fetchBlob } = fetcher(okBlobResponse)
    const { container, rerender } = await renderBody(
      { source: FENCE, fetchBlob },
      'pre',
    )
    await waitFor(() => expect(container.querySelector('button.code-copy')).not.toBeNull())
    rerender(<MessageBody source={FENCE} fetchBlob={fetchBlob} />)
    await waitFor(() => expect(container.querySelectorAll('button.code-copy')).toHaveLength(1))
  })

  test('copying does not trip the attachment click handlers', async () => {
    const writeText = stubClipboard()
    const { fetchBlob, fetchFn } = fetcher(okBlobResponse)
    const { container } = await renderBody(
      { source: `\`\`\`\n[x](${FILE_PATH})\n\`\`\``, fetchBlob },
      'pre',
    )
    const btn = await waitFor(() => {
      const el = container.querySelector('button.code-copy')
      expect(el).not.toBeNull()
      return el as HTMLButtonElement
    })
    await userEvent.click(btn)
    expect(writeText).toHaveBeenCalled()
    expect(fetchFn).not.toHaveBeenCalled()
  })
})

describe('markdown chunk loading', () => {
  test('a rejected import is retried on a later mount instead of being memoised', async () => {
    // Fresh module instance so `markdownModule`/`markdownPending` start empty.
    vi.resetModules()
    let attempts = 0
    vi.doMock('../lib/markdown', async () => {
      attempts++
      if (attempts === 1) throw new Error('chunk 404 after deploy')
      return await vi.importActual<typeof import('../lib/markdown')>('../lib/markdown')
    })
    const { MessageBody: FreshBody } = await import('./MessageBody')

    const first = render(<FreshBody source="**bold**" />)
    // First mount: import rejected → stays on the plain-text fallback.
    await waitFor(() => expect(attempts).toBe(1))
    expect(first.container.querySelector('strong')).toBeNull()
    first.unmount()

    // Second mount: a fresh attempt succeeds and the markdown renders.
    render(<FreshBody source="**bold**" />)
    await waitFor(() => expect(attempts).toBe(2))
    await waitFor(() => expect(document.querySelector('strong')).toHaveTextContent('bold'))
    vi.doUnmock('../lib/markdown')
  })
})
