/** Access to the Tauri runtime via the `withGlobalTauri` global — deliberately
 * NOT via `@tauri-apps/*` npm packages, so the browser bundle carries nothing
 * and `console/package.json` stays Tauri-free. Callers gate on `isTauri()`;
 * a throw here means a gate is missing, and it should be loud. */

interface TauriGlobal {
  core: {
    invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>
    // Channel lives under core in the global API — typed loosely on purpose:
    // its one job here is to be constructed, have `onmessage` set, and be
    // handed to a command as an argument.
    Channel?: new () => TauriChannel<unknown>
  }
  http?: { fetch: typeof fetch }
  store?: { load: (file: string) => Promise<TauriStore> }
}

/** A Rust→JS one-way stream. The shell serializes it to a command argument, so
 * it is passed by value and never awaited. */
export interface TauriChannel<T> {
  onmessage?: (msg: T) => void
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

/** A channel a Rust command can push messages down, with the handler attached
 * before the caller can hand it anywhere: a message that arrived before
 * `onmessage` was assigned would be delivered to the constructor's no-op and
 * lost. */
export function tauriChannel<T>(onmessage: (msg: T) => void): TauriChannel<T> {
  const ctor = tauriGlobal().core.Channel
  if (!ctor) throw new Error('Tauri runtime not available')
  const channel = new ctor() as TauriChannel<T>
  channel.onmessage = onmessage
  return channel
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
