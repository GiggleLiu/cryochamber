// Bumped whenever the caching rules change: v1 cached authenticated /api
// responses, so every v1 cache must be deleted, not merely stopped being
// written to.
const CACHE = 'zulip-app-v2'

/**
 * Requests that must never touch CacheStorage.
 *
 * Everything under /zulip/ and /api/ is answered per bearer identity: an
 * owner's chamber list, another invite's messages, an attachment only one token
 * may read. CacheStorage is keyed by URL alone, so a cached response would be
 * replayed to whatever identity asks for the same URL next — and logout does
 * not clear it. /api/events is an unbounded SSE stream on top of that.
 * /servers.json must stay fresh so a moved backend is picked up.
 */
function isPrivate(pathname) {
  return (
    pathname.startsWith('/zulip/') ||
    pathname.startsWith('/api/') ||
    pathname === '/servers.json'
  )
}

self.addEventListener('install', () => self.skipWaiting())

self.addEventListener('activate', (e) =>
  e.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  ),
)

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url)
  if (e.request.method !== 'GET') return
  // Not respondWith'd at all: the request goes straight to the network, and
  // nothing about it is read from or written to the cache.
  if (isPrivate(url.pathname)) return
  e.respondWith(
    fetch(e.request)
      .then((res) => {
        const copy = res.clone()
        caches.open(CACHE).then((c) => c.put(e.request, copy))
        return res
      })
      .catch(() => caches.match(e.request).then((hit) => hit ?? Response.error())),
  )
})
