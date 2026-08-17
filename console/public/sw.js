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

/**
 * The current cache name, resolved once per worker from the build manifest.
 *
 * A failed lookup is *not* memoized: a rejected `activate` still activates, so a
 * worker that restarted while offline would otherwise never learn its cache name
 * again — old caches would survive forever and the global `caches.match` (oldest
 * first) would keep serving the previous build's shell. Clearing the memo lets
 * the next request retry the fetch.
 */
let cacheNamePromise = null
function cacheName() {
  cacheNamePromise ??= fetch('/precache.json', { cache: 'reload' })
    .then((r) => r.json())
    .then((m) => ({ name: PREFIX + m.hash, files: m.files }))
    .catch((e) => {
      cacheNamePromise = null
      throw e
    })
  return cacheNamePromise
}

/**
 * Read `req` from *this build's* cache first, then from any cache.
 *
 * Preferring the named cache keeps `/index.html` matched to the running build:
 * the global lookup scans oldest-first, so it would hand back a stale shell left
 * behind by an activate that could not resolve the manifest.
 *
 * Falling back on a miss matters because the name is derived from the *live*
 * manifest, not from the bytes this worker shipped. Once a new build is
 * installed and waiting, a restarted old controller resolves the new hash and
 * would miss its own `/assets/*` chunks, which still live under the old one.
 * Hashed asset names are content-addressed, so whichever cache holds them the
 * bytes are right. Same fallback when the name cannot be resolved at all:
 * offline, a possibly-old shell beats nothing.
 */
function matchCurrent(req) {
  return cacheName()
    .then(({ name }) => caches.open(name).then((c) => c.match(req)))
    .catch(() => undefined)
    .then((hit) => hit || caches.match(req))
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
  // Fire-and-forget: a cache write that fails (quota, unknown name) must not
  // reject into nowhere — the response has already been handed to the page.
  cacheName()
    .then(({ name }) => caches.open(name).then((c) => c.put(request, copy)))
    .catch(() => {})
  return res
}

self.addEventListener('fetch', (e) => {
  const req = e.request
  if (req.method !== 'GET') return
  const url = new URL(req.url)
  if (isPrivate(url.pathname)) return

  if (url.pathname.startsWith('/assets/')) {
    e.respondWith(
      matchCurrent(req).then((hit) => hit || fetch(req).then((res) => storeIfOk(req, res))),
    )
    return
  }

  if (req.mode === 'navigate') {
    e.respondWith(
      fetch(req)
        .then((res) => storeIfOk(req, res))
        .catch(() => matchCurrent('/index.html').then((hit) => hit || Response.error())),
    )
    return
  }

  e.respondWith(
    fetch(req)
      .then((res) => storeIfOk(req, res))
      .catch(() => matchCurrent(req).then((hit) => hit || Response.error())),
  )
})
