/**
 * The page's half of the update flow. The service worker never activates a
 * new build on its own (see public/sw.js); this module notices a new worker
 * reaching `installed` while an old one is still controlling the page — that
 * is "an update is available" — and, once the user asks, tells the waiting
 * worker to take over. The resulting `controllerchange` reloads the page once,
 * so the new HTML and the new hashed chunks are picked up together.
 */

let waiting: ServiceWorker | null = null
let refreshing = false

/** Test seam: forget the waiting worker and the reload guard. */
export function _resetForTests(): void {
  waiting = null
  refreshing = false
}

function noteInstalled(reg: ServiceWorkerRegistration, onAvailable: () => void): void {
  const sw = reg.installing
  if (!sw) return
  sw.onstatechange = () => {
    if (sw.state === 'installed' && navigator.serviceWorker.controller) {
      waiting = sw
      onAvailable()
    }
  }
}

/**
 * Attach the update listeners to a registration. Safe to call once per app
 * boot. `onAvailable` fires at most once per new build.
 */
export function wireUpdateFlow(reg: ServiceWorkerRegistration, onAvailable: () => void): void {
  // A worker may already be waiting from a previous visit (the user never
  // reloaded); report it now rather than waiting for the next build.
  if (reg.waiting && navigator.serviceWorker.controller) {
    waiting = reg.waiting
    onAvailable()
  }
  reg.addEventListener('updatefound', () => noteInstalled(reg, onAvailable))
  navigator.serviceWorker.addEventListener('controllerchange', () => {
    if (refreshing) return
    refreshing = true
    location.reload()
  })
}

/** Activate the waiting worker. No-op if none is waiting. */
export function applyUpdate(): void {
  waiting?.postMessage({ type: 'SKIP_WAITING' })
}
