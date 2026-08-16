/**
 * Agent Console service worker.
 *
 * Cache name = `agent-console-<hash>` where <hash> comes from /precache.json,
 * written by the build. A new build ⇒ new hash ⇒ new cache; `activate`
 * deletes every other one, so old assets never pile up.
 *
 * Update policy: install does NOT skipWaiting. The page notices the waiting
 * worker (main.tsx) and shows "Update available · Reload"; only when the user
 * taps does it post SKIP_WAITING, and the resulting controllerchange reloads
 * the page once. Swapping code under a live session is what broke lazy chunks.
 *
 * Fetch policy:
 *   /api/*        never touched — answered per bearer identity, cannot be
 *                 replayed to another token, and /api/events is a stream.
 *   /assets/*     cache-first: hashed names, so a hit is always the right bytes.
 *   navigations   network-first, cached /index.html as the offline shell.
 *   everything    network-first with cache fallback.
 * Nothing is stored unless the response is ok: a cached 404 or the hub's
 * "not installed" 503 would otherwise replay as the app while offline.
 */
const PREFIX = 'agent-console-'

/** The current cache name, resolved once per worker from the build manifest. */
let cacheNamePromise = null
function cacheName() {
  cacheNamePromise ??= fetch('/precache.json', { cache: 'reload' })
    .then((r) => r.json())
    .then((m) => ({ name: PREFIX + m.hash, files: m.files }))
  return cacheNamePromise
}

self.addEventListener('install', (e) => {
  e.waitUntil(
    cacheName().then(({ name, files }) =>
      caches
        .open(name)
        .then((c) => c.addAll(files.map((f) => new Request(f, { cache: 'reload' })))),
    ),
  )
})

self.addEventListener('activate', (e) => {
  e.waitUntil(
    cacheName()
      .then(({ name }) =>
        caches
          .keys()
          .then((keys) => Promise.all(keys.filter((k) => k !== name).map((k) => caches.delete(k)))),
      )
      .then(() => self.clients.claim()),
  )
})

self.addEventListener('message', (e) => {
  if (e.data && e.data.type === 'SKIP_WAITING') self.skipWaiting()
})

function isPrivate(pathname) {
  return pathname === '/api' || pathname.startsWith('/api/')
}

/** Store `res` under the current cache iff it is a successful response. */
function storeIfOk(request, res) {
  if (!res || !res.ok) return res
  const copy = res.clone()
  cacheName().then(({ name }) => caches.open(name).then((c) => c.put(request, copy)))
  return res
}

self.addEventListener('fetch', (e) => {
  const req = e.request
  if (req.method !== 'GET') return
  const url = new URL(req.url)
  if (isPrivate(url.pathname)) return

  if (url.pathname.startsWith('/assets/')) {
    e.respondWith(
      caches.match(req).then((hit) => hit || fetch(req).then((res) => storeIfOk(req, res))),
    )
    return
  }

  if (req.mode === 'navigate') {
    e.respondWith(
      fetch(req)
        .then((res) => storeIfOk(req, res))
        .catch(() => caches.match('/index.html').then((hit) => hit || Response.error())),
    )
    return
  }

  e.respondWith(
    fetch(req)
      .then((res) => storeIfOk(req, res))
      .catch(() => caches.match(req).then((hit) => hit || Response.error())),
  )
})
