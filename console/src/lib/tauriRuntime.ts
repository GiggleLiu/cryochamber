import type { AppRuntime } from './appBoot'
import { pinnedFetch } from './pinnedFetch'
import { tauriFetch, tauriInvoke, tauriLoadStore, type TauriStore } from './tauri'
import { parseHubAccounts, type HubAccount, type HubsBackend } from '../store/hubs'

/** The hub list lives in the shell's own store file, not `localStorage`: the
 * app must still know its hubs after the WebView data is cleared, and the
 * tokens in it are the only copy the user has. */
const HUBS_FILE = 'hubs.json'
const HUBS_KEY = 'hubs'

export class TauriHubsBackend implements HubsBackend {
  /** Loaded once and reused: `load` returns a handle to the same file, and a
   * second handle would race the first one's in-memory copy. A *failed* load is
   * deliberately not memoized — caching that rejection would make every later
   * save fail for the life of the process while the user believes their hub was
   * added, so a failure clears the memo and the next call retries. */
  private store: Promise<TauriStore> | null = null
  /** Tail of the write chain, always handled and always resolving. Callers fire
   * `save` from store subscriptions without awaiting, so two writes can be in
   * flight at once; without this the store could apply them out of order and
   * persist the older list. */
  private queue: Promise<void> = Promise.resolve()

  private handle(): Promise<TauriStore> {
    if (!this.store) {
      const loading = tauriLoadStore(HUBS_FILE)
      this.store = loading.catch((err) => {
        this.store = null
        throw err
      })
    }
    return this.store
  }

  private async tokens(): Promise<Record<string, string>> {
    const raw = await tauriInvoke<string | null>('load_credentials')
    if (raw == null) return {}
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)
      || Object.values(parsed).some((token) => typeof token !== 'string')) {
      throw new Error('Invalid native credential record')
    }
    return parsed as Record<string, string>
  }

  private async writeTokens(tokens: Record<string, string>): Promise<void> {
    const value = JSON.stringify(tokens)
    await tauriInvoke('save_credentials', { value })
    if (await tauriInvoke('load_credentials') !== value) {
      throw new Error('Native credential verification failed; previous hub settings were preserved')
    }
  }

  async load(): Promise<HubAccount[]> {
    const store = await this.handle()
    const raw = await store.get<unknown>(HUBS_KEY)
    if (!Array.isArray(raw)) return []
    const tokens = await this.tokens()
    const restored = raw.map((item: unknown) => {
      if (!item || typeof item !== 'object') return item
      const row = item as Record<string, unknown>
      if (typeof row.token === 'string') return row
      if (typeof row.id !== 'string') return row
      if (!tokens[row.id]) {
        throw new Error('A saved hub token is unavailable in device storage. Restore credential access before continuing.')
      }
      return { ...row, token: tokens[row.id] }
    })
    const hubs = parseHubAccounts(restored)
    // Verify the protected copy before removing any legacy plaintext token.
    if (raw.some((row) => row && typeof row === 'object' && typeof row.token === 'string')) {
      await this.save(hubs)
    }
    return hubs
  }

  save(hubs: HubAccount[]): Promise<void> {
    // The chain is kept on an always-handled, always-resolving tail: one bad
    // write must not poison every later one, and a fire-and-forget caller must
    // not leave a rejection with no handler on it. The failure still reaches
    // the caller that owns it, through the promise this call returns.
    const next = this.queue.then(async () => {
      const store = await this.handle()
      const previous = await this.tokens()
      const current = Object.fromEntries(hubs.map((hub) => [hub.id, hub.token]))
      // Retain old tokens until the metadata commit succeeds, so a failed
      // removal/save cannot strand the previous on-disk hub list.
      await this.writeTokens({ ...previous, ...current })
      await store.set(HUBS_KEY, hubs.map(({ token: _token, ...metadata }) => metadata))
      await store.save()
      await this.writeTokens(current)
    })
    this.queue = next.catch(() => {})
    return next
  }
}

/** One look at a hub's TLS, as `probe_hub` reports it. `fingerprint` is the
 * SHA-256 of the end-entity certificate in lowercase hex, present whenever a
 * handshake completed — trusted or not. */
export interface ProbeReport {
  https_valid: boolean
  fingerprint: string | null
}

/** Ask the shell what certificate a hub presents. Only Rust can answer this:
 * the WebView's `fetch` reports a bad certificate as an indistinguishable
 * network error and never says which certificate it saw. */
export function probeHub(url: string): Promise<ProbeReport> {
  return tauriInvoke<ProbeReport>('probe_hub', { url })
}

/** What app mode gets inside the shell: hubs that survive a quit, and a
 * transport that is not the WebView's `fetch` — the plugin's requests come
 * from Rust, so no CORS preflight and no browser origin apply. */
export function makeTauriRuntime(): AppRuntime {
  const backend = new TauriHubsBackend()
  return {
    backend,
    transportFor(hub: HubAccount): typeof fetch {
      // A pinned hub is one the system trust store rejects, so the plugin
      // fetch cannot reach it at all: its requests go out through Rust, where
      // the pinned fingerprint is what decides the handshake.
      if (hub.trust.kind === 'pinned') return pinnedFetch(hub)
      return tauriFetch()
    },
  }
}
