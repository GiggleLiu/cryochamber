# Hub Client PWA (Plan B: Agent Console) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Teach the Agent Console PWA to speak to cryohub — invite-link onboarding, client-side markdown+KaTeX rendering, SSE live updates, a Share screen — plus the v1 chat-UX features (send states, drafts, copy button, dark mode).

**Architecture:** A `HubClient` (`src/api/hubClient.ts`) lives beside `ZulipClient` and adapts hub chambers/messages onto the app's existing numeric-id store types via deterministic id mapping; `servers.json` entries gain `kind: 'zulip' | 'hub'` and everything downstream branches on `creds.kind`. Hub messages are raw markdown rendered client-side (`src/lib/markdown.ts`, markdown-it + KaTeX) and fed through the **existing** DOMPurify sanitizer. Live updates arrive over a fetch-based SSE reader in `useEventLoop`'s hub branch. The store, cache, conversation view, and WeChat rendering are untouched except where named.

**Tech Stack:** React 18 + TS + Vite + zustand + vitest (jsdom) + Playwright. New deps: `markdown-it`, `@types/markdown-it` (dev). `katex` must be present (it ships the CSS/fonts already in the build — add the package explicitly if it is only a transitive dep).

**Working repo:** `~/agentic/zulip-app` (this repo).

**Spec:** `docs/superpowers/specs/2026-08-15-chamber-hub-design.md`. Server counterpart: `docs/superpowers/plans/2026-08-15-chamber-hub-auth.md` (Plan A — defines the API this client consumes).

## Global Constraints

- Hub API (from Plan A): `GET /api/chambers`, `GET /api/chambers/{id}/messages` → `[{id, direction, from, subject, body, timestamp, session, is_question}]`, `POST /api/chambers/{id}/send` `{body, from?}`, `POST /api/chambers/{id}/uploads` (multipart `file`) → `{ok, name, markdown}`, `GET /api/chambers/{id}/files/{name}`, `GET /api/events` (SSE: `message`/`status`/`log`/`index` events), `GET /api/whoami`, `GET/POST /api/tokens`, `POST /api/tokens/{name}/revoke`.
- Auth: `Authorization: Bearer <token>`; every non-GET also sends `X-Cryo-CSRF: 1` (hub's CSRF middleware requires a custom header).
- Tokens live in localStorage at the same trust level as the Zulip API key. 401 anywhere → clear creds → login with reason (existing pattern).
- Invite links: `#invite=<token>` in the URL fragment, consumed and stripped at boot.
- The token must never appear in a query string (SSE included).
- All existing tests keep passing; run with `npx vitest run`; build with `npm run build`; e2e with `npx playwright test`.
- Never touch the `/zulip/qec` dev proxy target in `vite.config.ts`.

---

### Task 1: Markdown + KaTeX renderer (`src/lib/markdown.ts`)

**Files:**
- Create: `src/lib/markdown.ts`
- Create: `src/lib/markdown.test.ts`
- Modify: `package.json` (add `markdown-it`; dev `@types/markdown-it`; ensure `katex` is a direct dep)

**Interfaces:**
- Produces: `renderMarkdown(md: string): string` — HTML string, **not sanitized** (callers must pass it through `sanitizeZulipHtml`; MessageBody does in Task 2). Supports CommonMark + tables + fenced code (`<pre><code>`), inline math `$…$`, display math `$$…$$` via `katex.renderToString(..., { throwOnError: false })`. Raw HTML in the markdown is disabled (`html: false`).

- [ ] **Step 1: Install deps**

```bash
npm i markdown-it && npm i -D @types/markdown-it && npm i katex
```

- [ ] **Step 2: Write the failing tests**

```ts
// src/lib/markdown.test.ts
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `npx vitest run src/lib/markdown.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 4: Implement `src/lib/markdown.ts`**

```ts
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run src/lib/markdown.test.ts`
Expected: all 6 PASS. (If "dollar amounts" fails: the `$5 and $10` case has a space after `5`? No — `5 and ` starts with `5`; the rule rejects because content `5 and ` ends with space. Verify the rule logic rather than loosening the test.)

- [ ] **Step 6: Commit**

```bash
git add package.json package-lock.json src/lib/markdown.ts src/lib/markdown.test.ts
git commit -m "feat: client-side markdown+KaTeX renderer for hub messages"
```

---

### Task 2: MessageBody `format` prop + hub attachment links

**Files:**
- Modify: `src/components/MessageBody.tsx`
- Modify: `src/App.tsx` (interceptor also catches hub file links)
- Modify: `src/components/MessageBody.test.tsx`, `src/App.test.tsx`

**Interfaces:**
- Consumes: `renderMarkdown` (Task 1).
- Produces: `MessageBody` accepts `format?: 'html' | 'markdown'` (default `'html'`). Markdown is rendered *then* sanitized: `sanitizeZulipHtml(renderMarkdown(html), prefix, selfUserId)`. A shared `export const HUB_FILES_RE = /^\/api\/chambers\/[^/]+\/files\//` (put it in `src/lib/download.ts`); anchor/img handling treats an href/src as an authenticated attachment when it starts with the Zulip `uploadPrefix` **or** matches `HUB_FILES_RE`.

- [ ] **Step 1: Write the failing tests**

Add to `src/components/MessageBody.test.tsx` (reuse the file's existing render helpers and fetch stubs):

```ts
describe('markdown format', () => {
  test('renders markdown content through the sanitizer', () => {
    render(<MessageBody html={'**hi** $x^2$'} prefix="" format="markdown" />)
    expect(document.querySelector('.message-body strong')?.textContent).toBe('hi')
    expect(document.querySelector('.message-body .katex')).not.toBeNull()
  })

  test('hostile markdown cannot smuggle handlers past the sanitizer', () => {
    render(<MessageBody html={'[x](javascript:alert(1))'} prefix="" format="markdown" />)
    const a = document.querySelector('.message-body a')
    expect(a?.getAttribute('href') ?? '').not.toContain('javascript:')
  })

  test('hub file links download via delegation instead of navigating', async () => {
    // html format is irrelevant here — what matters is the href shape
    const html = '<p><a href="/api/chambers/c1/files/ab12_report.pdf">report.pdf</a></p>'
    // render with authHeader, click the anchor, assert fetch was called with
    // that URL + Authorization header and the event was default-prevented
    // (copy the structure of the existing "downloads upload links" test).
  })
})
```

And in `src/App.test.tsx`, clone the existing interceptor test with
`anchor.href = '/api/chambers/c1/files/x_y.pdf'` asserting fetch to that path.

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/components/MessageBody.test.tsx src/App.test.tsx`
Expected: FAIL — `format` prop unknown / hub link not intercepted.

- [ ] **Step 3: Implement**

`src/lib/download.ts` — add:

```ts
/** Hub attachment routes (Plan A): /api/chambers/{id}/files/{name}. */
export const HUB_FILES_RE = /^\/api\/chambers\/[^/]+\/files\//
```

`MessageBody.tsx`:

```tsx
import { renderMarkdown } from '../lib/markdown'
import { fetchBlob, filenameFromHref, triggerBlobDownload, HUB_FILES_RE } from '../lib/download'
// prop: format?: 'html' | 'markdown'  (default 'html')
const sanitized = sanitizeZulipHtml(
  format === 'markdown' ? renderMarkdown(html) : html,
  prefix,
  selfUserId,
)
// helper used by BOTH the click delegation and the img-swap observer:
const isAttachment = (path: string) => path.startsWith(uploadPrefix) || HUB_FILES_RE.test(path)
// in onClick: replace `if (!href.startsWith(uploadPrefix)) return` with
//   `if (!isAttachment(href)) return`
// in the swap() observer: replace `if (!src.startsWith(uploadPrefix)) continue`
//   with `if (!isAttachment(src)) continue`
```

`App.tsx` interceptor — extend the path match:

```ts
let path: string | null = null
if (href.startsWith('/user_uploads/')) path = href
else if (href.startsWith(`${creds.prefix}/user_uploads/`)) path = href.slice(creds.prefix.length)
else if (HUB_FILES_RE.test(href)) path = href  // hub: prefix is '', use as-is
if (!path) return
// download URL: hub paths are already absolute app paths — do not re-prefix:
const url = HUB_FILES_RE.test(path) ? path : `${creds.prefix}${path}`
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/components/MessageBody.test.tsx src/App.test.tsx`
Expected: PASS (all pre-existing tests too).

- [ ] **Step 5: Commit**

```bash
git add src/components/MessageBody.tsx src/App.tsx src/lib/download.ts src/components/MessageBody.test.tsx src/App.test.tsx
git commit -m "feat: markdown message format and hub attachment link handling"
```

---

### Task 3: `HubClient` (`src/api/hubClient.ts`)

**Files:**
- Create: `src/api/hubClient.ts`
- Create: `src/api/hubClient.test.ts`
- Modify: `src/api/types.ts` (add `kind` to `Credentials` and the server entry type)

**Interfaces:**
- Consumes: hub API (Global Constraints).
- Produces (types first — `src/api/types.ts`):
  - `Credentials` gains `kind?: 'zulip' | 'hub'` (absent = `'zulip'`; every existing call site keeps working).
  - Server entry type (wherever `servers.ts` defines it) gains `kind?: 'zulip' | 'hub'`.
- Produces (`hubClient.ts`):

```ts
export class HubClient {
  constructor(creds: Credentials, fetchFn?: typeof fetch)
  authHeaderValue(): string                 // `Bearer <apiKey>`
  whoami(): Promise<{ role: 'owner' | 'invite'; name?: string }>
  // Zulip-compatible surface consumed by the views/store:
  register(): Promise<InitialState>         // chambers → subscriptions; queueId 'hub'; unread []
  getMessages(streamName: string, anchor: number | 'newest', numBefore?: number): Promise<ZulipMessage[]>
  sendMessage(streamName: string, content: string): Promise<number>
  markStreamRead(_streamId: number): Promise<void>   // no-op (unread is client-local on hub)
  getOwnUser(): Promise<{ user_id: number }>         // { user_id: 0 }
  getUsers(): Promise<ZulipUser[]>                   // [] — mentions fall back to seen senders
  uploadFile(file: File, streamName?: string): Promise<string>  // returns the /api/.../files/... URL
  // hub-specific:
  chamberIdFor(streamId: number): string | undefined
  listInvites(): Promise<Array<{ name: string; chambers: string[]; created_at: string; revoked_at: string | null }>>
  createInvite(name: string, chambers: string[]): Promise<{ token: string }>
  revokeInvite(name: string): Promise<void>
}
export function numericStreamId(chamberId: string): number   // stable per-browser (persisted map)
export function numericMessageId(id: string, timestampMs: number): number
```

- Id mapping rules (the store keys everything by number):
  - **Streams**: persisted map in localStorage `zulip-app.hub-ids` → `{ next: number, byChamber: Record<string, number> }`, ids assigned from 1 upward on first sight. Stable across reloads so the message cache keys stay valid.
  - **Messages**: `numericMessageId(id, tsMs) = tsMs + (fnv1a(id) % 997)` where `fnv1a` is 32-bit FNV-1a. Deterministic (same message → same number after reload, so dedupe works) and time-ordered (the store sorts by id).
- Errors: non-2xx responses throw `ZulipApiError` (reuse it from `client.ts`) with the HTTP status, so the app's existing `isAuthError` 401 handling works unchanged.
- Every request sends `Authorization`; every POST also sends `X-Cryo-CSRF: 1`.

- [ ] **Step 1: Write the failing tests**

```ts
// src/api/hubClient.test.ts
import { HubClient, numericMessageId } from './hubClient'
import type { Credentials } from './types'

const creds: Credentials = { kind: 'hub', prefix: '', email: 'Alice', apiKey: 'tok123', sendTopic: '' }

function mockFetch(handler: (url: string, init?: RequestInit) => object | Response) {
  return vi.fn(async (url: string, init?: RequestInit) => {
    const out = handler(url, init)
    return out instanceof Response ? out : new Response(JSON.stringify(out), { status: 200 })
  }) as unknown as typeof fetch
}

beforeEach(() => localStorage.removeItem('zulip-app.hub-ids'))

test('register maps chambers to subscriptions with stable numeric ids', async () => {
  const fetchFn = mockFetch(() => [
    { id: 'cham-b', name: 'beta', task: null },
    { id: 'cham-a', name: 'alpha', task: null },
  ])
  const c = new HubClient(creds, fetchFn)
  const init = await c.register()
  expect(init.subscriptions.map((s) => s.name).sort()).toEqual(['alpha', 'beta'])
  const idA = init.subscriptions.find((s) => s.name === 'alpha')!.stream_id
  // stable across a second client instance (persisted map)
  const again = await new HubClient(creds, fetchFn).register()
  expect(again.subscriptions.find((s) => s.name === 'alpha')!.stream_id).toBe(idA)
  expect(vi.mocked(fetchFn).mock.calls[0][1]?.headers).toMatchObject({ Authorization: 'Bearer tok123' })
})

test('getMessages maps ChamberMessage to ZulipMessage with markdown content', async () => {
  const fetchFn = mockFetch((url) =>
    url.includes('/messages')
      ? [{ id: 'm1', direction: 'outbox', from: 'agent', subject: 's', body: '**hi**',
           timestamp: '2026-08-15T10:00:00', session: 1, is_question: false }]
      : [{ id: 'cham-a', name: 'alpha' }],
  )
  const c = new HubClient(creds, fetchFn)
  await c.register()
  const msgs = await c.getMessages('alpha', 'newest')
  expect(msgs).toHaveLength(1)
  expect(msgs[0].sender_email).toBe('agent')
  expect(msgs[0].content).toBe('**hi**')
  expect(msgs[0].timestamp).toBe(Math.floor(Date.parse('2026-08-15T10:00:00') / 1000))
})

test('numericMessageId is deterministic and time-ordered', () => {
  expect(numericMessageId('m1', 1000_000)).toBe(numericMessageId('m1', 1000_000))
  expect(numericMessageId('m1', 1000_000)).not.toBe(numericMessageId('m2', 1000_000))
  expect(numericMessageId('any', 2000_000)).toBeGreaterThan(numericMessageId('other', 1000_000))
})

test('sendMessage posts body with CSRF header and 401 throws an auth error', async () => {
  const fetchFn = mockFetch((url, init) => {
    if (url.endsWith('/send')) {
      expect((init?.headers as Record<string, string>)['X-Cryo-CSRF']).toBe('1')
      expect(JSON.parse(String(init?.body)).body).toBe('do it')
      return { ok: true }
    }
    return [{ id: 'cham-a', name: 'alpha' }]
  })
  const c = new HubClient(creds, fetchFn)
  await c.register()
  await c.sendMessage('alpha', 'do it')

  const denied = new HubClient(creds, mockFetch(() => new Response('', { status: 401 })))
  await expect(denied.register()).rejects.toMatchObject({ httpStatus: 401 })
})

test('uploadFile posts multipart and returns the files URL', async () => {
  const fetchFn = mockFetch((url, init) => {
    if (url.endsWith('/uploads')) {
      expect(init?.body).toBeInstanceOf(FormData)
      return { ok: true, name: 'ab_report.pdf', markdown: '[report.pdf](/api/chambers/cham-a/files/ab_report.pdf)' }
    }
    return [{ id: 'cham-a', name: 'alpha' }]
  })
  const c = new HubClient(creds, fetchFn)
  await c.register()
  const url = await c.uploadFile(new File(['x'], 'report.pdf'), 'alpha')
  expect(url).toBe('/api/chambers/cham-a/files/ab_report.pdf')
})

test('invite management wrappers hit the token routes', async () => {
  const calls: string[] = []
  const fetchFn = mockFetch((url, init) => {
    calls.push(`${init?.method ?? 'GET'} ${url}`)
    if (url.endsWith('/api/tokens') && init?.method === 'POST') return { ok: true, token: 'ff'.repeat(32) }
    if (url.endsWith('/api/tokens')) return { invites: [{ name: 'Bob', chambers: [], created_at: 't', revoked_at: null }] }
    return { ok: true }
  })
  const c = new HubClient(creds, fetchFn)
  expect((await c.listInvites())[0].name).toBe('Bob')
  expect((await c.createInvite('Cara', ['cham-a'])).token).toHaveLength(64)
  await c.revokeInvite('Cara')
  expect(calls).toContain('POST /api/tokens/Cara/revoke')
})
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/api/hubClient.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `src/api/hubClient.ts`**

```ts
import { ZulipApiError } from './client'
import type { Credentials, InitialState, StreamSub, ZulipMessage, ZulipUser } from './types'

const IDS_KEY = 'zulip-app.hub-ids'

interface IdMap { next: number; byChamber: Record<string, number> }

function loadIdMap(): IdMap {
  try {
    const raw = localStorage.getItem(IDS_KEY)
    if (raw) return JSON.parse(raw) as IdMap
  } catch { /* fall through */ }
  return { next: 1, byChamber: {} }
}

export function numericStreamId(chamberId: string): number {
  const map = loadIdMap()
  const existing = map.byChamber[chamberId]
  if (existing !== undefined) return existing
  const id = map.next
  map.byChamber[chamberId] = id
  map.next = id + 1
  try { localStorage.setItem(IDS_KEY, JSON.stringify(map)) } catch { /* quota */ }
  return id
}

function fnv1a(s: string): number {
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i += 1) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  return h >>> 0
}

/** Deterministic, time-ordered numeric id for the store (which sorts and
 * dedupes by number). Same mailbox message → same number after any reload. */
export function numericMessageId(id: string, timestampMs: number): number {
  return timestampMs + (fnv1a(id) % 997)
}

interface ChamberMessage {
  id: string; direction: string; from: string; subject: string
  body: string; timestamp: string; session?: number | null; is_question: boolean
}

export class HubClient {
  private byName = new Map<string, string>()   // stream name -> chamber id
  private byStreamId = new Map<number, string>()
  private fetchFn: typeof fetch

  constructor(private creds: Credentials, fetchFn: typeof fetch = fetch) {
    this.fetchFn = fetchFn.bind(undefined)
  }

  authHeaderValue(): string {
    return `Bearer ${this.creds.apiKey}`
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private async request(path: string, init: RequestInit = {}): Promise<any> {
    const headers: Record<string, string> = {
      Authorization: this.authHeaderValue(),
      ...((init.headers as Record<string, string>) ?? {}),
    }
    if (init.method && init.method !== 'GET') headers['X-Cryo-CSRF'] = '1'
    const res = await this.fetchFn(`${this.creds.prefix}${path}`, { ...init, headers })
    if (!res.ok) throw new ZulipApiError(`HTTP ${res.status}`, res.status)
    return res.json()
  }

  async whoami(): Promise<{ role: 'owner' | 'invite'; name?: string }> {
    return this.request('/api/whoami')
  }

  async register(): Promise<InitialState> {
    const chambers = (await this.request('/api/chambers')) as Array<{ id: string; name: string }>
    this.byName.clear()
    this.byStreamId.clear()
    const subscriptions: StreamSub[] = chambers.map((c) => {
      const sid = numericStreamId(c.id)
      this.byName.set(c.name, c.id)
      this.byStreamId.set(sid, c.id)
      return { stream_id: sid, name: c.name, description: '' }
    })
    return { queueId: 'hub', lastEventId: 0, subscriptions, unread: [] }
  }

  chamberIdFor(streamId: number): string | undefined {
    return this.byStreamId.get(streamId)
  }

  private chamberByName(streamName: string): string {
    const id = this.byName.get(streamName)
    if (!id) throw new ZulipApiError(`unknown project ${streamName}`, 404)
    return id
  }

  toZulipMessage(m: ChamberMessage, chamberId: string): ZulipMessage {
    const tsMs = Date.parse(m.timestamp) || 0
    return {
      id: numericMessageId(m.id, tsMs),
      sender_full_name: m.from,
      sender_email: m.from,
      timestamp: Math.floor(tsMs / 1000),
      content: m.body,
      stream_id: numericStreamId(chamberId),
      subject: m.subject,
    }
  }

  async getMessages(streamName: string, _anchor: number | 'newest', _numBefore = 50): Promise<ZulipMessage[]> {
    const chamberId = this.chamberByName(streamName)
    const msgs = (await this.request(
      `/api/chambers/${encodeURIComponent(chamberId)}/messages`,
    )) as ChamberMessage[]
    // The mailbox returns full history; anchor/window semantics are not
    // needed (and "Load earlier" finds nothing further).
    return msgs.map((m) => this.toZulipMessage(m, chamberId))
  }

  async sendMessage(streamName: string, content: string): Promise<number> {
    const chamberId = this.chamberByName(streamName)
    await this.request(`/api/chambers/${encodeURIComponent(chamberId)}/send`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ body: content, from: this.creds.email }),
    })
    return Date.now()
  }

  async markStreamRead(_streamId: number): Promise<void> {
    // Unread state is client-local on hub; nothing to sync.
  }

  async getOwnUser(): Promise<{ user_id: number }> {
    return { user_id: 0 }
  }

  async getUsers(): Promise<ZulipUser[]> {
    return [] // mention autocomplete falls back to senders seen in messages
  }

  async uploadFile(file: File, streamName?: string): Promise<string> {
    if (!streamName) throw new ZulipApiError('upload needs a project', 400)
    const chamberId = this.chamberByName(streamName)
    const form = new FormData()
    form.append('file', file)
    const body = await this.request(`/api/chambers/${encodeURIComponent(chamberId)}/uploads`, {
      method: 'POST',
      body: form,
    })
    const match = /\(([^)]+)\)$/.exec(body.markdown as string)
    return match ? match[1] : `/api/chambers/${chamberId}/files/${body.name}`
  }

  async listInvites() {
    const body = await this.request('/api/tokens')
    return body.invites as Array<{ name: string; chambers: string[]; created_at: string; revoked_at: string | null }>
  }

  async createInvite(name: string, chambers: string[]): Promise<{ token: string }> {
    return this.request('/api/tokens', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, chambers }),
    })
  }

  async revokeInvite(name: string): Promise<void> {
    await this.request(`/api/tokens/${encodeURIComponent(name)}/revoke`, { method: 'POST' })
  }
}
```

In `src/api/types.ts` add `kind?: 'zulip' | 'hub'` to `Credentials`, and the
same optional field to the server-entry type in `src/api/servers.ts`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/api/hubClient.test.ts`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/api/hubClient.ts src/api/hubClient.test.ts src/api/types.ts src/api/servers.ts
git commit -m "feat: HubClient with stable id mapping and invite management"
```

---

### Task 4: SSE reader + hub branch of the event loop

**Files:**
- Create: `src/api/sse.ts`
- Create: `src/api/sse.test.ts`
- Modify: `src/store/appStore.ts` (store the client union), `src/hooks/useEventLoop.ts`, `src/hooks/useEventLoop.test.ts`

**Interfaces:**
- Consumes: `HubClient` (Task 3), store actions (`applyInitialState`, `applyEvents`, `setConnection`).
- Produces:
  - `src/api/sse.ts`: `readSse(url: string, headers: Record<string, string>, onEvent: (event: string, data: string) => void, signal: AbortSignal): Promise<void>` — fetch-streaming SSE parser (handles `event:`/`data:` lines, multi-line data, blank-line dispatch, CRLF); resolves when the stream ends, rejects on network/HTTP error (`ZulipApiError` with status on non-2xx so 401 handling works).
  - Store: `client` field type becomes `ZulipClient | HubClient | null` (structural typing keeps existing call sites compiling — both expose the shared surface). `setCreds` constructs `creds.kind === 'hub' ? new HubClient(c) : new ZulipClient(c)`.
  - `useEventLoop` hub branch: `register()` → `applyInitialState`, then loop `readSse('/api/events', …)`; each `message` SSE event whose payload parses becomes a Zulip-shaped message event fed to `applyEvents`; `index` events trigger a re-`register()`. Reconnect = re-register (which clears `loadedStreams`, so open conversations re-fetch) + backoff, mirroring the Zulip branch. 401 → `logout(AUTH_LOGOUT_REASON)`.

- [ ] **Step 1: Write the failing tests**

```ts
// src/api/sse.test.ts
import { readSse } from './sse'

function streamResponse(chunks: string[]): Response {
  const stream = new ReadableStream({
    start(controller) {
      for (const c of chunks) controller.enqueue(new TextEncoder().encode(c))
      controller.close()
    },
  })
  return new Response(stream, { status: 200 })
}

test('parses events split across chunks', async () => {
  vi.stubGlobal('fetch', vi.fn(async () =>
    streamResponse(['event: message\ndata: {"a"', ':1}\n\nevent: index\ndata: changed\n\n']),
  ))
  const events: Array<[string, string]> = []
  await readSse('/api/events', { Authorization: 'Bearer t' }, (e, d) => events.push([e, d]), new AbortController().signal)
  expect(events).toEqual([['message', '{"a":1}'], ['index', 'changed']])
})

test('non-2xx rejects with the status', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
  await expect(
    readSse('/api/events', {}, () => {}, new AbortController().signal),
  ).rejects.toMatchObject({ httpStatus: 401 })
})
```

For `useEventLoop.test.ts`, add a hub-mode test following the file's existing
mocking style: store creds `{ kind: 'hub', … }`, mock `fetch` so
`/api/chambers` returns one chamber and `/api/events` returns a stream carrying
one `message` event `{chamber_id, direction: 'outbox', from: 'agent', subject: '',
body: 'done', timestamp: '2026-08-15T10:00:00', is_question: false}` — assert
the store ends up with one message whose `content` is `'done'` and connection
is `'live'`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/api/sse.test.ts src/hooks/useEventLoop.test.ts`
Expected: FAIL — module not found / no hub branch.

- [ ] **Step 3: Implement**

`src/api/sse.ts`:

```ts
import { ZulipApiError } from './client'

/** Minimal SSE reader over fetch streaming — EventSource cannot send an
 * Authorization header, and the token must never ride in a query string. */
export async function readSse(
  url: string,
  headers: Record<string, string>,
  onEvent: (event: string, data: string) => void,
  signal: AbortSignal,
): Promise<void> {
  const res = await fetch(url, { headers, signal })
  if (!res.ok || !res.body) throw new ZulipApiError(`HTTP ${res.status}`, res.status)
  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let eventName = 'message'
  let data: string[] = []
  const dispatch = () => {
    if (data.length > 0) onEvent(eventName, data.join('\n'))
    eventName = 'message'
    data = []
  }
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    let nl: number
    while ((nl = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, nl).replace(/\r$/, '')
      buffer = buffer.slice(nl + 1)
      if (line === '') dispatch()
      else if (line.startsWith('event:')) eventName = line.slice(6).trim()
      else if (line.startsWith('data:')) data.push(line.slice(5).trimStart())
      // comments (`:keepalive`) and other fields are ignored
    }
  }
  dispatch()
}
```

`useEventLoop.ts` — split `run()` by kind. Keep the Zulip branch byte-for-byte;
add:

```ts
async function runHub(client: HubClient) {
  while (!stopped) {
    if (document.visibilityState === 'hidden') {
      await waitForVisible()
      if (stopped) return
      continue
    }
    try {
      store.getState().setConnection('connecting')
      const init = await client.register()
      if (stopped) return
      store.getState().applyInitialState(init)
      store.getState().setConnection('live')
      backoff = 1000
      let seq = 1
      await readSse(
        '/api/events',
        { Authorization: client.authHeaderValue() },
        (event, payload) => {
          if (event === 'index') throw new ReregisterSignal()
          if (event !== 'message') return
          try {
            const m = JSON.parse(payload) as {
              chamber_id: string; from: string; subject: string
              body: string; timestamp: string; is_question: boolean
            }
            const msg = client.toChamberEventMessage(m)
            if (msg) store.getState().applyEvents([{ id: seq++, type: 'message', message: msg }])
          } catch { /* malformed payload: skip */ }
        },
        abort.signal,
      )
      // stream ended cleanly → loop re-registers
    } catch (e) {
      if (stopped) return
      if (e instanceof ReregisterSignal) continue
      if (isAuthError(e)) {
        store.getState().logout(AUTH_LOGOUT_REASON)
        return
      }
      store.getState().setConnection('offline')
      await sleep(backoff)
      backoff = Math.min(backoff * 2, 30000)
    }
  }
}
```

with `class ReregisterSignal extends Error {}` at module level, and in the
effect body: `creds/client` kind check dispatches `runHub` vs the existing
`run`. Add to `HubClient` (exact signature — the loop depends on it):

```ts
/** Map an SSE message payload to a store message, or null if the chamber is
 * unknown (e.g. scope changed since register). */
toChamberEventMessage(m: { chamber_id: string; from: string; subject: string; body: string; timestamp: string; is_question: boolean }): ZulipMessage | null {
  if (!Array.from(this.byStreamId.values()).includes(m.chamber_id)) return null
  return this.toZulipMessage(
    { id: `${m.chamber_id}:${m.timestamp}:${m.from}:${fnv1a(m.body)}`, direction: 'event',
      from: m.from, subject: m.subject, body: m.body, timestamp: m.timestamp,
      is_question: m.is_question },
    m.chamber_id,
  )
}
```

(The SSE payload has no message id; the synthesized key is deterministic, so
the same event re-delivered after a reconnect dedupes to the same numeric id.
The subsequent history re-fetch reconciles content: `numericMessageId` for the
mailbox row differs from the event's synthetic id only until the register-loop
clears `loadedStreams` and the fetched history replaces the thread — the
store's full-window replacement from the cache work handles exactly this.)

Store `setCreds`:

```ts
const client = c.kind === 'hub' ? new HubClient(c) : new ZulipClient(c)
```

and widen the `client` field type. `ConversationView` passes
`format={creds.kind === 'hub' ? 'markdown' : 'html'}` to `MessageBody`, and
`Composer`'s upload call adds the `streamName` argument (`ZulipClient` gains
the ignored optional parameter so both compile against one signature).

Two spec-mandated behaviors also land in this task's wiring:

1. **Mention fallback (spec §4)** — `Composer`'s autocomplete candidate list,
   when the store's `users` list is empty (hub mode: `getUsers()` returns
   `[]`), derives from senders seen in the current conversation instead:

```ts
const messages = useAppStore((s) => s.messagesByStream[streamId])
const candidates: ZulipUser[] =
  users && users.length > 0
    ? users
    : [...new Map(
        (messages ?? []).map((m) => [
          m.sender_email,
          { user_id: 0, full_name: m.sender_full_name, email: m.sender_email, is_bot: false },
        ]),
      ).values()]
```

   (feed `candidates` into the existing filtering exactly where `users` was
   used; add a Composer test: with `users: null` and two messages from
   `agent`, typing `@ag` suggests `agent`).

2. **Revoked scope mid-session (spec: 404 on a previously-visible chamber →
   drop it)** — in `ConversationView`'s history-fetch `.catch`:

```ts
} else if (creds.kind === 'hub' && e instanceof ZulipApiError && e.httpStatus === 404) {
  // Scope was revoked while we were looking at it: leave quietly.
  navigate({ name: 'projects' })
} else {
```

   plus a ConversationView test (hub creds, `getMessages` rejecting with a
   404 `ZulipApiError` → the view navigates back to projects instead of
   showing the error panel).

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run`
Expected: whole suite PASS (type widening must not break existing tests).

- [ ] **Step 5: Commit**

```bash
git add src/api/sse.ts src/api/sse.test.ts src/api/hubClient.ts src/hooks/useEventLoop.ts src/hooks/useEventLoop.test.ts src/store/appStore.ts src/views/ConversationView.tsx src/components/Composer.tsx src/api/client.ts
git commit -m "feat: hub SSE event loop with reconnect and scope-aware mapping"
```

---

### Task 5: Invite onboarding + hub login

**Files:**
- Modify: `src/App.tsx`, `src/views/LoginView.tsx`, `src/store/appStore.ts`
- Modify: `src/App.test.tsx`, `src/views/LoginView.test.tsx`

**Interfaces:**
- Consumes: `HubClient.whoami()`, existing `setCreds` / `loginReason`.
- Produces:
  - `export function takeInviteToken(): string | null` in `App.tsx`: reads `location.hash`, matches `/^#invite=([0-9a-f]{16,})$/`, strips the fragment via `history.replaceState`, returns the token.
  - Boot effect: if a token was taken and no creds are stored → find the first `kind: 'hub'` entry in `servers.json` → `whoami()` with the token → `setCreds({ kind: 'hub', prefix: entry.prefix, email: name ?? 'human', apiKey: token, sendTopic: '' })`. `whoami` failure (revoked/invalid) → login view with reason `'This invite link is no longer valid.'`.
  - Store gains `hubRole: 'owner' | 'invite' | null` (set from the same `whoami` call; owner-token pastes get `'owner'`), reset on logout. The Share screen (Task 6) renders only when `hubRole === 'owner'`.
  - `LoginView`: server entries with `kind: 'hub'` render a single "Access token" password-type input (paste the owner token — or an invite token — there) instead of email+password; submit runs the same whoami→setCreds flow.

- [ ] **Step 1: Write the failing tests**

```ts
// additions to src/App.test.tsx
test('an #invite fragment signs in via whoami and lands on projects', async () => {
  window.location.hash = '#invite=' + 'ab'.repeat(16)
  vi.stubGlobal('fetch', vi.fn(async (url: string) => {
    if (String(url).endsWith('/api/whoami'))
      return new Response(JSON.stringify({ role: 'invite', name: 'Alice' }), { status: 200 })
    return new Response(JSON.stringify([]), { status: 200 })
  }))
  render(<App />)
  expect(await screen.findByRole('heading', { name: 'Projects' })).toBeInTheDocument()
  expect(window.location.hash).toBe('')
  const saved = JSON.parse(localStorage.getItem('zulip-app.credentials')!)
  expect(saved).toMatchObject({ kind: 'hub', email: 'Alice', apiKey: 'ab'.repeat(16) })
})

test('a revoked invite link shows login with a reason', async () => {
  window.location.hash = '#invite=' + 'cd'.repeat(16)
  vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
  render(<App />)
  expect(await screen.findByText(/no longer valid/i)).toBeInTheDocument()
})
```

(The `servers.json` mock at the top of `App.test.tsx` gains a hub entry:
`{ name: 'Chamber Hub', prefix: '', kind: 'hub', sendTopic: '' }`.)

For `LoginView.test.tsx`: selecting the hub server shows an "Access token"
field; submitting it with a whoami mock stores hub creds (same assertions as
above, via the form instead of the fragment).

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/App.test.tsx src/views/LoginView.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Implement**

`App.tsx`:

```tsx
export function takeInviteToken(): string | null {
  const match = /^#invite=([0-9a-f]{16,})$/.exec(window.location.hash)
  if (!match) return null
  window.history.replaceState(null, '', window.location.pathname)
  return match[1]
}
```

Boot effect (alongside the existing saved-creds effect):

```tsx
const [inviteToken] = useState<string | null>(takeInviteToken)
useEffect(() => {
  if (!inviteToken || useAppStore.getState().creds) return
  void (async () => {
    const servers = await loadServers()
    const hub = servers.find((s) => s.kind === 'hub')
    if (!hub) return
    const probe = new HubClient({ kind: 'hub', prefix: hub.prefix, email: '', apiKey: inviteToken, sendTopic: '' })
    try {
      const who = await probe.whoami()
      useAppStore.getState().setHubRole(who.role)
      useAppStore.getState().setCreds({
        kind: 'hub', prefix: hub.prefix, email: who.name ?? 'human',
        apiKey: inviteToken, sendTopic: '',
      })
    } catch {
      useAppStore.getState().logout('This invite link is no longer valid.')
    }
  })()
}, [inviteToken])
```

Store: add `hubRole: 'owner' | 'invite' | null` to state + `setHubRole`,
reset in `logout` (it is part of `initialData`). `LoginView`: for a selected
hub server, render the token field and run the identical whoami→setCreds flow
on submit. On app boot with saved hub creds, `App` re-runs `whoami()` once to
repopulate `hubRole` (stored creds don't carry the role).

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/App.tsx src/views/LoginView.tsx src/store/appStore.ts src/App.test.tsx src/views/LoginView.test.tsx public/servers.json
git commit -m "feat: invite-link onboarding and hub token login"
```

---

### Task 6: Share screen (owner-only invite management)

**Files:**
- Create: `src/views/ShareSheet.tsx`
- Create: `src/views/ShareSheet.test.tsx`
- Modify: `src/views/SettingsSheet.tsx` (a "Share access" row, visible when `hubRole === 'owner'`), `src/store/appStore.ts` (`shareOpen` flag mirroring `settingsOpen`), `src/App.tsx` (render), `src/styles.css` (reuse the sheet styles — follow SettingsSheet's classes)

**Interfaces:**
- Consumes: `HubClient.listInvites / createInvite / revokeInvite / chamberIdFor`, store `streams`, `hubRole`.
- Produces: a bottom sheet listing invites (name, project names resolved via the streams list, created date, revoked badge), a create form (name input + project checkboxes), and per-row Revoke. After create, the full link `${location.origin}/#invite=${token}` is shown once in a readonly input with a Copy button (`navigator.clipboard.writeText`); it is not retrievable later (the API never returns token strings again).

- [ ] **Step 1: Write the failing tests**

```tsx
// src/views/ShareSheet.test.tsx — mock the client on the store like
// SettingsSheet.test does; store streams [{stream_id: 1, name: 'alpha', …}].
test('lists invites with project names', async () => {
  // listInvites → [{name:'Alice', chambers:['cham-a'], created_at:'…', revoked_at:null}]
  // chamberIdFor(1) → 'cham-a'
  // render; expect 'Alice' and 'alpha' visible
})

test('create shows the invite link exactly once with copy', async () => {
  // fill name 'Bob', check 'alpha', submit; createInvite resolves {token:'ff…'}
  // expect input value `${location.origin}/#invite=ff…`
  // click Copy → clipboard.writeText called with the link
})

test('revoke calls the API and refreshes the list', async () => {
  // click Revoke on Alice → revokeInvite('Alice') called, list re-fetched
})
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/views/ShareSheet.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `ShareSheet.tsx`**

Structure (follow SettingsSheet's sheet/backdrop markup and class names):

```tsx
export function ShareSheet() {
  const client = useAppStore((s) => s.client)
  const streams = useAppStore((s) => s.streams)
  const setShareOpen = useAppStore((s) => s.setShareOpen)
  const [invites, setInvites] = useState<InviteRow[] | null>(null)
  const [name, setName] = useState('')
  const [checked, setChecked] = useState<number[]>([])
  const [createdLink, setCreatedLink] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const hub = client instanceof HubClient ? client : null

  const refresh = useCallback(() => {
    hub?.listInvites().then(setInvites).catch(() => setError('Could not load invites.'))
  }, [hub])
  useEffect(refresh, [refresh])

  async function create() {
    if (!hub || !name.trim()) return
    const chambers = checked
      .map((sid) => hub.chamberIdFor(sid))
      .filter((x): x is string => !!x)
    try {
      const { token } = await hub.createInvite(name.trim(), chambers)
      setCreatedLink(`${window.location.origin}/#invite=${token}`)
      setName(''); setChecked([]); refresh()
    } catch { setError('Could not create the invite (name in use?).') }
  }
  // list rendering: project names via streams.find matching chamberIdFor;
  // Revoke button → hub.revokeInvite(name).then(refresh)
  // createdLink block: <input readOnly value={createdLink}/> + Copy button →
  //   navigator.clipboard.writeText(createdLink)
}
```

Store: `shareOpen: boolean` + `setShareOpen` (exactly like `settingsOpen`).
SettingsSheet gains the "Share access" row (only when
`useAppStore((s) => s.hubRole) === 'owner'`) that closes settings and opens
share. App renders `{shareOpen && <ShareSheet />}`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/views/ShareSheet.tsx src/views/ShareSheet.test.tsx src/views/SettingsSheet.tsx src/store/appStore.ts src/App.tsx src/styles.css
git commit -m "feat: owner Share screen for creating and revoking invite links"
```

---

### Task 7: Send states + retry + per-project drafts

**Files:**
- Modify: `src/store/appStore.ts` (outbox slice), `src/components/Composer.tsx`, `src/views/ConversationView.tsx`, `src/styles.css`
- Modify: `src/store/appStore.test.ts`, `src/components/Composer.test.tsx`, `src/views/ConversationView.test.tsx`

**Interfaces:**
- Produces (store):

```ts
export interface OutboxItem {
  localId: number            // negative, monotonically decreasing — never collides with real ids
  streamId: number
  content: string
  state: 'sending' | 'failed'
}
// state: outboxByStream: Record<number, OutboxItem[]>
// actions:
//   enqueueOutbox(streamId, content): number        // returns localId, state 'sending'
//   resolveOutbox(streamId, localId): void          // remove (send succeeded; the real
//                                                   // message arrives via event/refetch)
//   failOutbox(streamId, localId): void             // state → 'failed'
//   retryOutbox(streamId, localId): void            // state → 'sending' (caller re-sends)
// outbox is session-local: NOT persisted to the cache, cleared on logout.
```

- Composer: `send()` becomes enqueue → `client.sendMessage` → resolve on success / fail on error (textarea clears immediately on enqueue — optimistic). Draft persistence: textarea initializes from `localStorage['zulip-app.draft.<streamId>']`, writes on every change, clears the key on successful enqueue.
- ConversationView: renders outbox items after the real messages as self-bubbles with a state line — `Sending…` or `Failed — tap to retry` (button, `onClick` → `retryOutbox` + re-send via client).

- [ ] **Step 1: Write the failing store tests**

```ts
// appStore.test.ts additions
test('outbox lifecycle: enqueue → fail → retry → resolve', () => {
  const id = useAppStore.getState().enqueueOutbox(1, 'hello')
  expect(id).toBeLessThan(0)
  expect(useAppStore.getState().outboxByStream[1][0].state).toBe('sending')
  useAppStore.getState().failOutbox(1, id)
  expect(useAppStore.getState().outboxByStream[1][0].state).toBe('failed')
  useAppStore.getState().retryOutbox(1, id)
  expect(useAppStore.getState().outboxByStream[1][0].state).toBe('sending')
  useAppStore.getState().resolveOutbox(1, id)
  expect(useAppStore.getState().outboxByStream[1]).toEqual([])
})
```

Composer tests: a send whose `client.sendMessage` rejects leaves a failed
outbox item and does NOT restore the textarea (the retry affordance owns the
content now); a resolving send calls `resolveOutbox`. Draft tests: type →
unmount → remount shows the draft; successful send clears it.
ConversationView test: a failed outbox item renders text matching
`/failed — tap to retry/i` and clicking it calls `sendMessage` again.

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/store src/components/Composer.test.tsx src/views/ConversationView.test.tsx`
Expected: FAIL on the new tests only.

- [ ] **Step 3: Implement**

Store actions (inside `create`, after the existing actions):

```ts
enqueueOutbox: (streamId, content) => {
  const localId = nextLocalId--          // module-level: let nextLocalId = -1
  set((state) => ({
    outboxByStream: {
      ...state.outboxByStream,
      [streamId]: [...(state.outboxByStream[streamId] ?? []), { localId, streamId, content, state: 'sending' as const }],
    },
  }))
  return localId
},
resolveOutbox: (streamId, localId) =>
  set((state) => ({
    outboxByStream: {
      ...state.outboxByStream,
      [streamId]: (state.outboxByStream[streamId] ?? []).filter((o) => o.localId !== localId),
    },
  })),
failOutbox: (streamId, localId) => set(/* map state → 'failed' */),
retryOutbox: (streamId, localId) => set(/* map state → 'sending' */),
```

(`outboxByStream: {}` joins `initialData` so logout clears it; it is **not**
added to `saveCachedState`.)

Composer `send`:

```ts
const localId = useAppStore.getState().enqueueOutbox(streamId, text)
setText('')
localStorage.removeItem(`zulip-app.draft.${streamId}`)
client.sendMessage(streamName, text).then(
  () => useAppStore.getState().resolveOutbox(streamId, localId),
  () => useAppStore.getState().failOutbox(streamId, localId),
)
```

(Composer needs the `streamId` — pass it as a prop from ConversationView next
to the existing `streamName`.) Draft wiring: `useState(() =>
localStorage.getItem(draftKey) ?? '')`, a `useEffect` on `text` writing/removing
the key. ConversationView appends after the message rows:

```tsx
{(outbox ?? []).map((o) => (
  <div key={o.localId} className="msg-row msg-self msg-pending">
    <div className="msg-col">
      <div className="bubble">
        <MessageBody html={o.content} prefix={creds.prefix} format="markdown" />
      </div>
      {o.state === 'sending' ? (
        <div className="send-state">Sending…</div>
      ) : (
        <button className="send-state send-failed" onClick={() => retry(o)}>
          Failed — tap to retry
        </button>
      )}
    </div>
  </div>
))}
```

with `retry(o)` = `retryOutbox` + the same `sendMessage` promise chain.
(Outbox bubbles render the raw text as markdown for both kinds — for a
pending bubble the approximation is fine and disappears on resolve.) CSS:
`.send-state` small muted caption; `.send-failed` in the danger color,
tappable.

- [ ] **Step 4: Run tests + full gate**

Run: `npx vitest run && npm run build`
Expected: all PASS, build clean.

- [ ] **Step 5: Commit**

```bash
git add src/store/appStore.ts src/components/Composer.tsx src/views/ConversationView.tsx src/styles.css src/store/appStore.test.ts src/components/Composer.test.tsx src/views/ConversationView.test.tsx
git commit -m "feat: optimistic send states with retry and per-project drafts"
```

---

### Task 8: Copy button on code blocks + dark mode

**Files:**
- Modify: `src/components/MessageBody.tsx`, `src/views/SettingsSheet.tsx`, `src/styles.css`
- Modify: `src/components/MessageBody.test.tsx`, `src/views/SettingsSheet.test.tsx`

**Interfaces:**
- Produces:
  - Every `<pre>` in a message gets a floating `button.code-copy` (added by the existing MutationObserver pass — same idempotency pattern as the image swap: skip if `pre.dataset.copyWired === '1'`). Click (via the existing delegation handler, catching `.code-copy` **before** the anchor/img branches) → `navigator.clipboard.writeText(pre.innerText)` → button text flips to `Copied` for 1.5 s.
  - Theme: `data-theme` on `<html>` — `'light' | 'dark' | ''` (empty = follow system). Persisted at `localStorage['zulip-app.theme']`, applied at boot in `main.tsx`, toggled from a three-way control in SettingsSheet. CSS: the existing token block stays as-is for light; add `[data-theme='dark']` and `@media (prefers-color-scheme: dark) { :root:not([data-theme='light']) }` blocks that re-assign the same custom properties (`--canvas`, `--bubble-self`, `--ink`, etc. — every token in the current `:root` gets a dark value; keep `--bubble-self` a darkened WeChat green `#3eb575`).

- [ ] **Step 1: Write the failing tests**

MessageBody: render html `<pre><code>let x = 1</code></pre>` with a clipboard
spy (`vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: vi.fn() } })`
— or `Object.assign` if navigator is read-only in jsdom); `await waitFor` for
`button.code-copy` to appear (MutationObserver is async); click it; expect
`writeText` called with `'let x = 1'`.
SettingsSheet: clicking the "Dark" theme option sets
`document.documentElement.dataset.theme === 'dark'` and persists
`zulip-app.theme` = `'dark'`; "System" clears both.

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/components/MessageBody.test.tsx src/views/SettingsSheet.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Implement**

MessageBody — inside the existing MutationObserver callback (rename `swap` to
`decorate` and keep both jobs there):

```ts
for (const pre of Array.from(root.querySelectorAll('pre'))) {
  if (pre.dataset.copyWired === '1') continue
  pre.dataset.copyWired = '1'
  const btn = document.createElement('button')
  btn.className = 'code-copy'
  btn.type = 'button'
  btn.textContent = 'Copy'
  pre.appendChild(btn)
}
```

(The observer effect must now run even without `authHeader` — split the guard
so image swapping still requires `authHeader` but copy-wiring does not.)
Delegation, first branch of `onClick`:

```ts
const copyBtn = target.closest('button.code-copy')
if (copyBtn && root?.contains(copyBtn)) {
  const pre = copyBtn.closest('pre')
  const code = pre?.querySelector('code') ?? pre
  void navigator.clipboard?.writeText(code?.textContent ?? '')
  copyBtn.textContent = 'Copied'
  setTimeout(() => { copyBtn.textContent = 'Copy' }, 1500)
  return
}
```

CSS: `pre { position: relative }`, `.code-copy` absolute top-right, subtle
until hover/tap. Theme helper in `main.tsx`:

```ts
const savedTheme = localStorage.getItem('zulip-app.theme')
if (savedTheme) document.documentElement.dataset.theme = savedTheme
```

SettingsSheet three-way segmented control writing the key and the dataset
(removing both for "System").

- [ ] **Step 4: Run tests + visual check**

Run: `npx vitest run && npm run build`
Expected: PASS. Then a quick manual look at `npm run dev` in dark mode for
contrast disasters (tokens only — no structural CSS edits).

- [ ] **Step 5: Commit**

```bash
git add src/components/MessageBody.tsx src/views/SettingsSheet.tsx src/styles.css src/main.tsx src/components/MessageBody.test.tsx src/views/SettingsSheet.test.tsx
git commit -m "feat: code-block copy button and dark mode"
```

---

### Task 9: e2e against a mock hub + deployment artifacts

**Files:**
- Create: `e2e/hub.spec.ts`
- Modify: `vite.config.ts` (dev proxy `/api` → `http://127.0.0.1:8765`; DO NOT touch `/zulip/qec`), `deploy/Caddyfile` (hub site block), `README.md` (hub setup section)

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the e2e spec**

`e2e/hub.spec.ts`, using Playwright `page.route` to mock the hub API (mirror
the mocking style of the existing e2e specs):

```ts
// route table:
//  GET  /api/whoami   → { role: 'invite', name: 'Alice' }   (or owner for the share test)
//  GET  /api/chambers → [{ id: 'cham-a', name: 'autoresearch' }]
//  GET  /api/chambers/cham-a/messages → one outbox message with markdown body '**done** $x^2$'
//  POST /api/chambers/cham-a/send     → { ok: true }  (capture the body)
//  GET  /api/events   → a never-resolving response (SSE not exercised here)
test('invite link → scoped project → markdown thread → send', async ({ page }) => {
  await page.goto('/#invite=' + 'ab'.repeat(16))
  await page.getByText('autoresearch').click()
  await expect(page.locator('.message-body strong')).toHaveText('done')
  await expect(page.locator('.message-body .katex')).toBeVisible()
  await page.getByRole('textbox').fill('continue')
  await page.keyboard.press('Enter')
  // assert the captured send body was {"body":"continue","from":"Alice"}
})

test('revoked invite shows login with reason', async ({ page }) => {
  // whoami route → 401; goto '/#invite=…'; expect /no longer valid/i
})
```

- [ ] **Step 2: Run e2e**

Run: `npx playwright test e2e/hub.spec.ts`
Expected: PASS.

- [ ] **Step 3: Deployment artifacts**

`vite.config.ts` — alongside the existing `/zulip/qec` proxy entry:

```ts
'/api': { target: 'http://127.0.0.1:8765', changeOrigin: false },
```

`deploy/Caddyfile` — add (with a placeholder hostname the README explains):

```
agents.example.com {
    encode gzip
    handle /api/* {
        reverse_proxy 127.0.0.1:8765
    }
    handle {
        root * /srv/agent-console
        try_files {path} /index.html
        file_server
    }
}
```

`README.md` — a "Chamber Hub mode" section: `cryohub token owner`,
`cryohub start --public`, servers.json hub entry, invite-link flow, the DNS
prerequisite, and the note that Zulip mode keeps working side by side.

- [ ] **Step 4: Full gate**

Run: `npx vitest run && npm run build && npx playwright test`
Expected: everything green.

- [ ] **Step 5: Commit**

```bash
git add e2e/hub.spec.ts vite.config.ts deploy/Caddyfile README.md
git commit -m "feat: hub e2e coverage, dev proxy, and deployment config"
```
