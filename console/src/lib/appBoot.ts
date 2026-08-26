import { HubClient } from '../api/hubClient'
import { useAppStore } from '../store/appStore'
import { MemoryHubsBackend, parseHubAccounts, type HubAccount, type HubsBackend } from '../store/hubs'
import { normalizeHubUrl } from './hubKeys'

/** Everything app mode needs from the shell it runs in, resolved once at start:
 * where the hub list lives, and how to reach a hub. Plan 2 swaps in the Tauri
 * store and a trust-aware transport; tests hand in fakes. */
export interface AppRuntime {
  backend: HubsBackend
  transportFor(hub: HubAccount): typeof fetch
}

let runtime: AppRuntime | null = null

/** Called by the shell before the first render. */
export function setAppRuntime(rt: AppRuntime): void {
  runtime = rt
}

/** The runtime, or a browser-safe placeholder. The placeholder is only ever
 * reached under `isTauri()`, so a production browser build never sees it: it
 * exists so a test (or a `tauri dev` bundle without the store) still boots. */
export function appRuntime(): AppRuntime {
  if (!runtime) {
    runtime = { backend: new MemoryHubsBackend(), transportFor: () => fetch }
  }
  return runtime
}

/** One client per hub: its own origin, its own token, its own transport. A 401
 * is that hub's problem alone — it is noted on the hub's row and its liveness
 * drops, rather than signing the whole app out the way browser mode does. */
export function makeClientFactory(rt: AppRuntime): (hub: HubAccount) => HubClient {
  return (hub) =>
    new HubClient({
      token: hub.token,
      baseUrl: hub.url,
      fetch: rt.transportFor(hub),
      onAuthFailure: () => {
        const store = useAppStore.getState()
        store.markHubAuthFailed(hub.id)
        // Without this the row stays pinned at "connecting" forever: the event
        // loop's own reconnects also 401, and nothing else moves it.
        store.setConnectionForHub(hub.id, 'offline')
      },
    })
}

/** Shown on the Add Hub screen when the stored list could not be read. The
 * warning is the point: an empty list here is indistinguishable from a first
 * run, and adding a hub over it writes a new list where one already exists. */
export const HUB_LOAD_ERROR =
  'Could not read this device’s saved hubs. Adding a hub here replaces the saved list.'

/**
 * App-mode boot: read the remembered hubs, enter app mode over them, then ask
 * each hub who our token is. The identity refresh is fire-and-forget — the
 * list is already on screen from the per-hub caches, and a hub that is down
 * must not hold up the ones that are not.
 *
 * A store that cannot be read still enters app mode, with no hubs and the
 * reason on screen: staying in browser mode would leave the window on a blank
 * Add Hub screen that explains nothing, behind an unhandled rejection.
 */
export async function bootApp(rt: AppRuntime): Promise<void> {
  let hubs: HubAccount[] = []
  let loadError: string | null = null
  try {
    hubs = parseHubAccounts(await rt.backend.load())
  } catch {
    loadError = HUB_LOAD_ERROR
  }
  const makeClient = makeClientFactory(rt)
  useAppStore.getState().initApp(hubs, rt.backend, makeClient)
  if (loadError) useAppStore.setState({ loginReason: loadError })
  for (const hub of hubs) {
    void makeClient(hub)
      .whoami()
      .then((who) => {
        const store = useAppStore.getState()
        // The answer is about the hub as booted. If the user removed it or
        // re-added it with a fresh token while whoami was in flight, writing
        // it back would resurrect the removed entry — token included — or
        // clobber the fresh token with this stale one.
        const current = store.hubs.find((h) => h.id === hub.id)
        if (!current || current.token !== hub.token) return
        store.setHubIdentity(hub.id, {
          role: who.role,
          name: who.name,
          version: who.hub_version ?? null,
        })
        const name = who.name ?? hub.name
        // Only a changed answer is written back: `addHub` rebuilds the router,
        // which restarts every hub's event loop — a needless cost on a boot
        // where the hub said what the file already holds.
        if (who.role === hub.role && name === hub.name) return
        store.addHub({ ...hub, role: who.role, name }).catch(() => {})
      })
      // A refused token has already gone through the client's own hook; every
      // other failure leaves the stored identity in place.
      .catch(() => {})
  }
}

/** The access-link token grammar, as App.tsx's `takeInviteToken` reads it. */
const INVITE_TOKEN_RE = /^[0-9a-f]{32,}$/

/** A link Android can route straight to the app. The admin token stays in the
 * fragment, matching browser invite links, instead of becoming a query value
 * that a server or proxy might log. */
export function appAccessLink(url: string, token: string): string {
  const link = new URL('cryochamber://add')
  link.searchParams.set('hub', normalizeHubUrl(url))
  link.hash = `invite=${token}`
  return link.toString()
}

/**
 * A browser invite link or `cryochamber://add` app link, split into the hub it
 * points at and the token it carries. Anything else is `null`, so the caller
 * can leave the form alone.
 */
export function parseInviteLink(text: string): { url: string; token: string } | null {
  let parsed: URL
  try {
    parsed = new URL(text.trim())
  } catch {
    return null
  }
  if (!parsed.hash.startsWith('#invite=')) return null
  const token = parsed.hash.slice('#invite='.length)
  if (!INVITE_TOKEN_RE.test(token)) return null
  try {
    if (parsed.protocol === 'cryochamber:' && parsed.hostname === 'add') {
      const hub = parsed.searchParams.get('hub')
      return hub ? { url: normalizeHubUrl(hub), token } : null
    }
    if (parsed.protocol === 'http:' || parsed.protocol === 'https:') {
      return {
        url: normalizeHubUrl(`${parsed.protocol}//${parsed.host}${parsed.pathname}`),
        token,
      }
    }
  } catch {
    return null
  }
  return null
}
