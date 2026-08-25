/** Access to the Tauri runtime via the `withGlobalTauri` global — deliberately
 * NOT via `@tauri-apps/*` npm packages, so the browser bundle carries nothing
 * and `console/package.json` stays Tauri-free. Callers gate on `isTauri()`;
 * a throw here means a gate is missing, and it should be loud. */

interface TauriGlobal {
  core: { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> }
  http?: { fetch: typeof fetch }
  store?: { load: (file: string) => Promise<TauriStore> }
  // Channel lives under core in the global API:
  // new window.__TAURI__.core.Channel() — typed loosely on purpose.
}

export interface TauriStore {
  get<T>(key: string): Promise<T | null>
  set(key: string, value: unknown): Promise<void>
  save(): Promise<void>
}

function tauriGlobal(): TauriGlobal {
  const t = (window as unknown as { __TAURI__?: TauriGlobal }).__TAURI__
  if (!t) throw new Error('Tauri runtime not available')
  return t
}

export function tauriInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return tauriGlobal().core.invoke(cmd, args) as Promise<T>
}

export function tauriFetch(): typeof fetch {
  const t = tauriGlobal()
  if (!t.http) throw new Error('Tauri runtime not available')
  return t.http.fetch.bind(undefined)
}

export function tauriLoadStore(file: string): Promise<TauriStore> {
  const t = tauriGlobal()
  if (!t.store) throw new Error('Tauri runtime not available')
  return t.store.load(file)
}
