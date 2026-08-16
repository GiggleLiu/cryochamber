// The exact bytes that ship as /sw.js — `?raw` keeps this test pinned to the
// deployed file rather than a copy that could drift.
import SOURCE from '../public/sw.js?raw'

/**
 * The production service worker is plain JS shipped verbatim from public/, so
 * there is nothing to import normally. This loads the real file's source into a
 * sandbox with a mocked `self`/`caches`/`fetch`, captures the listeners it
 * registers, and drives them — the same code the browser runs, with the storage
 * layer observable.
 */

const HASH = 'abcd1234'
const CACHE = `agent-console-${HASH}`
const MANIFEST = { hash: HASH, files: ['/index.html', '/manifest.webmanifest', '/assets/index-abc.js'] }

interface FakeResponse {
  ok: boolean
  status: number
  clone: () => FakeResponse
  body?: string
}
function res(status = 200, body = 'net'): FakeResponse {
  const r: FakeResponse = { ok: status >= 200 && status < 300, status, body, clone: () => r }
  return r
}

interface FetchEvent {
  request: { url: string; method: string; mode: string }
  respondWith: ReturnType<typeof vi.fn>
}

function loadWorker(opts: {
  existingCaches?: string[]
  network?: (input: unknown) => Promise<FakeResponse>
  cached?: Record<string, FakeResponse>
} = {}) {
  const listeners: Record<string, (e: unknown) => void> = {}
  const cached = { ...(opts.cached ?? {}) }
  const entry = {
    put: vi.fn(async (req: { url: string } | string, r: FakeResponse) => {
      cached[typeof req === 'string' ? req : req.url] = r
    }),
    addAll: vi.fn(async () => {}),
    match: vi.fn(async (req: { url: string } | string) => cached[typeof req === 'string' ? req : req.url]),
  }
  const caches = {
    open: vi.fn(async () => entry),
    match: vi.fn(async (req: { url: string } | string) => cached[typeof req === 'string' ? req : req.url]),
    keys: vi.fn(async () => opts.existingCaches ?? ['agent-console-old', 'other']),
    delete: vi.fn(async () => true),
  }
  // `opts.network` models how the *app's* requests are answered (offline, 404).
  // /precache.json is written by the build and always served, so it is answered
  // here in every case — otherwise a test's override would have to know about it.
  const app = opts.network ?? (async () => res(200))
  const network = vi.fn(async (input: unknown) => {
    const url = typeof input === 'string' ? input : (input as { url: string }).url
    if (url.endsWith('/precache.json')) {
      const r = res(200) as FakeResponse & { json: () => Promise<unknown> }
      r.json = async () => MANIFEST
      return r
    }
    return app(input)
  })
  const self = {
    addEventListener: (type: string, fn: (e: unknown) => void) => {
      listeners[type] = fn
    },
    skipWaiting: vi.fn(),
    clients: { claim: vi.fn(async () => {}) },
    location: { origin: 'https://app.example' },
  }
  const Request = function (this: { url: string }, url: string) {
    this.url = url
  } as unknown as typeof globalThis.Request
  new Function('self', 'caches', 'fetch', 'URL', 'Response', 'Request', SOURCE)(
    self,
    caches,
    network,
    URL,
    { error: () => ({ error: true }) },
    Request,
  )
  return { listeners, caches, entry, network, self, cached }
}

function fetchEvent(url: string, method = 'GET', mode = 'cors'): FetchEvent {
  return { request: { url, method, mode }, respondWith: vi.fn() }
}

async function drive(listeners: Record<string, (e: unknown) => void>, type: string) {
  let waited: Promise<unknown> = Promise.resolve()
  listeners[type]({ waitUntil: (p: Promise<unknown>) => (waited = p) })
  await waited
}

describe('install', () => {
  test('precaches every file in precache.json under the hash-named cache', async () => {
    const { listeners, caches, entry, network } = loadWorker()
    await drive(listeners, 'install')
    expect(network).toHaveBeenCalledWith('/precache.json', { cache: 'reload' })
    expect(caches.open).toHaveBeenCalledWith(CACHE)
    const [urls] = entry.addAll.mock.calls[0] as unknown as [Array<{ url: string }>]
    expect(urls.map((r) => r.url)).toEqual(MANIFEST.files)
  })

  test('does not skip waiting on its own — the page decides when to update', async () => {
    const { listeners, self } = loadWorker()
    await drive(listeners, 'install')
    expect(self.skipWaiting).not.toHaveBeenCalled()
  })

  test('the hash-named cache scheme leaves no hardcoded version behind', () => {
    expect(SOURCE).not.toMatch(/agent-console-v\d/)
    expect(SOURCE).not.toContain('servers.json')
  })
})

test('activate deletes every cache but the current one, then claims clients', async () => {
  const { listeners, caches, self } = loadWorker({ existingCaches: ['agent-console-old', CACHE, 'other'] })
  await drive(listeners, 'install')
  await drive(listeners, 'activate')
  expect(caches.delete).toHaveBeenCalledWith('agent-console-old')
  expect(caches.delete).toHaveBeenCalledWith('other')
  expect(caches.delete).not.toHaveBeenCalledWith(CACHE)
  expect(self.clients.claim).toHaveBeenCalled()
})

test('a SKIP_WAITING message activates the waiting worker', () => {
  const { listeners, self } = loadWorker()
  listeners.message({ data: { type: 'SKIP_WAITING' } })
  expect(self.skipWaiting).toHaveBeenCalledTimes(1)
  listeners.message({ data: { type: 'something-else' } })
  expect(self.skipWaiting).toHaveBeenCalledTimes(1)
})

describe('authenticated requests are passed through uncached', () => {
  test.each([
    ['hub identity', 'https://app.example/api/whoami'],
    ['hub chamber list', 'https://app.example/api/chambers'],
    ['hub messages', 'https://app.example/api/chambers/cham-a/messages'],
    ['hub attachment', 'https://app.example/api/chambers/cham-a/files/report.pdf'],
    ['hub SSE stream', 'https://app.example/api/events'],
  ])('%s', async (_name, url) => {
    const { listeners, caches, entry } = loadWorker()
    const e = fetchEvent(url)
    listeners.fetch(e)
    await Promise.resolve()
    expect(e.respondWith).not.toHaveBeenCalled()
    expect(caches.open).not.toHaveBeenCalled()
    expect(caches.match).not.toHaveBeenCalled()
    expect(entry.put).not.toHaveBeenCalled()
  })
})

test('non-GET requests are never intercepted', () => {
  const { listeners } = loadWorker()
  const e = fetchEvent('https://app.example/assets/index-abc.js', 'POST')
  listeners.fetch(e)
  expect(e.respondWith).not.toHaveBeenCalled()
})

describe('/assets/* is cache-first', () => {
  test('a cached asset is served without touching the network', async () => {
    const hit = res(200, 'cached')
    const { listeners, network } = loadWorker({
      cached: { 'https://app.example/assets/index-abc.js': hit },
    })
    await drive(listeners, 'install')
    const e = fetchEvent('https://app.example/assets/index-abc.js')
    listeners.fetch(e)
    expect(await e.respondWith.mock.calls[0][0]).toBe(hit)
    expect(network).not.toHaveBeenCalledWith(expect.objectContaining({ url: 'https://app.example/assets/index-abc.js' }))
  })

  test('a miss goes to the network and is cached when ok', async () => {
    const { listeners, entry } = loadWorker()
    await drive(listeners, 'install')
    const e = fetchEvent('https://app.example/assets/new-def.js')
    listeners.fetch(e)
    const out = (await e.respondWith.mock.calls[0][0]) as FakeResponse
    expect(out.ok).toBe(true)
    await vi.waitFor(() => expect(entry.put).toHaveBeenCalled())
  })
})

describe('never caches a response that is not ok', () => {
  test.each([404, 503])('a %s is returned but not stored', async (status) => {
    const { listeners, entry } = loadWorker({ network: async () => res(status) })
    const e = fetchEvent('https://app.example/assets/gone-123.js')
    listeners.fetch(e)
    const out = (await e.respondWith.mock.calls[0][0]) as FakeResponse
    expect(out.status).toBe(status)
    await Promise.resolve()
    expect(entry.put).not.toHaveBeenCalled()
  })
})

describe('navigations are network-first', () => {
  test('online: the network answer wins', async () => {
    const fresh = res(200, 'fresh')
    const { listeners } = loadWorker({ network: async () => fresh })
    const e = fetchEvent('https://app.example/c/cham-a', 'GET', 'navigate')
    listeners.fetch(e)
    expect(await e.respondWith.mock.calls[0][0]).toBe(fresh)
  })

  test('offline: falls back to the cached app shell', async () => {
    const shell = res(200, 'shell')
    const { listeners } = loadWorker({
      network: async () => {
        throw new TypeError('offline')
      },
      cached: { '/index.html': shell },
    })
    const e = fetchEvent('https://app.example/c/cham-a', 'GET', 'navigate')
    listeners.fetch(e)
    expect(await e.respondWith.mock.calls[0][0]).toBe(shell)
  })
})

test('other same-origin GETs are network-first with cache fallback', async () => {
  const stale = res(200, 'stale')
  const { listeners } = loadWorker({
    network: async () => {
      throw new TypeError('offline')
    },
    cached: { 'https://app.example/icons/icon-192.png': stale },
  })
  const e = fetchEvent('https://app.example/icons/icon-192.png')
  listeners.fetch(e)
  expect(await e.respondWith.mock.calls[0][0]).toBe(stale)
})
