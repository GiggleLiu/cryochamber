import type { AppRuntime } from './appBoot'
import { tauriFetch, tauriLoadStore, type TauriStore } from './tauri'
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

  async load(): Promise<HubAccount[]> {
    const store = await this.handle()
    return parseHubAccounts(await store.get<unknown>(HUBS_KEY))
  }

  save(hubs: HubAccount[]): Promise<void> {
    // The chain is kept on an always-handled, always-resolving tail: one bad
    // write must not poison every later one, and a fire-and-forget caller must
    // not leave a rejection with no handler on it. The failure still reaches
    // the caller that owns it, through the promise this call returns.
    const next = this.queue.then(async () => {
      const store = await this.handle()
      await store.set(HUBS_KEY, hubs)
      await store.save()
    })
    this.queue = next.catch(() => {})
    return next
  }
}

/** What app mode gets inside the shell: hubs that survive a quit, and a
 * transport that is not the WebView's `fetch` — the plugin's requests come
 * from Rust, so no CORS preflight and no browser origin apply. */
export function makeTauriRuntime(): AppRuntime {
  const backend = new TauriHubsBackend()
  return {
    backend,
    transportFor(hub: HubAccount): typeof fetch {
      // Unreachable today: nothing can mint pinned trust until the probe
      // ships, and a silent fallback to the plain transport would drop the
      // certificate check the user asked for.
      if (hub.trust.kind === 'pinned') throw new Error('pinned transport arrives in a later task')
      return tauriFetch()
    },
  }
}
