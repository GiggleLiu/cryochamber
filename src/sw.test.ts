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

interface FetchEvent {
  request: { url: string; method: string }
  respondWith: ReturnType<typeof vi.fn>
}

function loadWorker(existingCaches: string[] = ['zulip-app-v1', 'zulip-app-v2', 'other']) {
  const listeners: Record<string, (e: unknown) => void> = {}
  const entry = { put: vi.fn(async () => {}) }
  const caches = {
    open: vi.fn(async () => entry),
    match: vi.fn(async () => undefined),
    keys: vi.fn(async () => existingCaches),
    delete: vi.fn(async () => true),
  }
  const network = vi.fn(async () => ({ clone: () => ({ body: 'copy' }) }))
  const self = {
    addEventListener: (type: string, fn: (e: unknown) => void) => {
      listeners[type] = fn
    },
    skipWaiting: vi.fn(),
    clients: { claim: vi.fn(async () => {}) },
  }
  new Function('self', 'caches', 'fetch', 'URL', 'Response', SOURCE)(
    self,
    caches,
    network,
    URL,
    { error: () => ({ error: true }) },
  )
  return { listeners, caches, entry, network, self }
}

function fetchEvent(url: string, method = 'GET'): FetchEvent {
  return { request: { url, method }, respondWith: vi.fn() }
}

test('the cache name is bumped past the version that cached /api responses', () => {
  expect(SOURCE).toContain('zulip-app-v2')
  expect(SOURCE).not.toContain('zulip-app-v1')
})

test('activate deletes every cache but the current one', async () => {
  const { listeners, caches } = loadWorker()
  let waited: Promise<unknown> = Promise.resolve()
  listeners.activate({ waitUntil: (p: Promise<unknown>) => (waited = p) })
  await waited
  expect(caches.delete).toHaveBeenCalledWith('zulip-app-v1')
  expect(caches.delete).toHaveBeenCalledWith('other')
  expect(caches.delete).not.toHaveBeenCalledWith('zulip-app-v2')
})

describe('authenticated requests are passed through uncached', () => {
  test.each([
    ['hub identity', 'https://app.example/api/whoami'],
    ['hub chamber list', 'https://app.example/api/chambers'],
    ['hub messages', 'https://app.example/api/chambers/cham-a/messages'],
    ['hub attachment', 'https://app.example/api/chambers/cham-a/files/report.pdf'],
    ['hub SSE stream', 'https://app.example/api/events'],
    ['zulip proxy', 'https://app.example/zulip/qec/api/v1/messages'],
    ['server list', 'https://app.example/servers.json'],
  ])('%s', async (_name, url) => {
    const { listeners, caches, entry } = loadWorker()
    const e = fetchEvent(url)
    listeners.fetch(e)
    await Promise.resolve()
    // Untouched by the worker: the browser performs the request itself, so the
    // response can neither be stored nor replayed to another bearer.
    expect(e.respondWith).not.toHaveBeenCalled()
    expect(caches.open).not.toHaveBeenCalled()
    expect(caches.match).not.toHaveBeenCalled()
    expect(entry.put).not.toHaveBeenCalled()
  })
})

test('app shell assets are still cached', async () => {
  const { listeners, caches, entry } = loadWorker()
  const e = fetchEvent('https://app.example/assets/index-abc.js')
  listeners.fetch(e)
  expect(e.respondWith).toHaveBeenCalledTimes(1)
  await e.respondWith.mock.calls[0][0]
  await Promise.resolve()
  expect(caches.open).toHaveBeenCalledWith('zulip-app-v2')
  await vi.waitFor(() => expect(entry.put).toHaveBeenCalled())
})

test('non-GET requests are never intercepted', () => {
  const { listeners } = loadWorker()
  const e = fetchEvent('https://app.example/assets/index-abc.js', 'POST')
  listeners.fetch(e)
  expect(e.respondWith).not.toHaveBeenCalled()
})
