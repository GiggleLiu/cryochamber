import { ApiError } from './types'

/** Reject after this long with no bytes at all. The hub sends a keepalive
 * comment every 15 s (`KeepAlive::default()`), so twice that is silence a
 * live connection cannot produce. */
export const SSE_STALL_MS = 30_000

const STALLED = 'sse stalled'

/** True for the rejection `readSse` produces when its watchdog fired. */
export function isSseStall(e: unknown): boolean {
  return e instanceof Error && e.message === STALLED
}

export interface ReadSseOptions {
  /** The caller's stop signal: aborting it ends the read as an ordinary
   * abort — it is never reported as a stall. */
  signal: AbortSignal
  headers: Record<string, string>
  /** Silence window before the connection is declared dead. */
  stallMs?: number
  onEvent: (event: string, data: string) => void
  /** The caller's fetch: a `HubClient` built with an injected one must reach
   * the hub through it here too, or the stream is the single call that escapes
   * to the global — which in a test realm is a different fetch with a
   * different `AbortSignal` class. */
  fetch?: typeof fetch
}

/** Minimal SSE reader over fetch streaming — EventSource cannot send an
 * Authorization header, and the token must never ride in a query string.
 *
 * A half-open connection (network switch, iOS resume) delivers nothing and
 * never closes; without a watchdog the loop believes it is live forever. Any
 * byte — a keepalive comment included — re-arms the watchdog. */
export async function readSse(url: string, opts: ReadSseOptions): Promise<void> {
  const { signal, headers, onEvent, stallMs = SSE_STALL_MS } = opts
  // Bound to undefined for the same reason the client binds its own: a native
  // fetch called as a member throws.
  const fetchFn = (opts.fetch ?? fetch).bind(undefined)
  // Our own controller, so the watchdog can cut the socket without touching
  // the caller's signal — which means "stop the loop", not "reconnect".
  const local = new AbortController()
  const forwardAbort = () => local.abort(signal.reason)
  if (signal.aborted) forwardAbort()
  else signal.addEventListener('abort', forwardAbort, { once: true })

  let stalled = false
  let watchdog: ReturnType<typeof setTimeout> | undefined
  const arm = () => {
    if (watchdog !== undefined) clearTimeout(watchdog)
    watchdog = setTimeout(() => {
      stalled = true
      local.abort()
    }, stallMs)
  }

  try {
    arm() // a connect that never answers is a stall too
    const res = await fetchFn(url, { headers, signal: local.signal })
    if (!res.ok || !res.body) throw new ApiError(res.status, `HTTP ${res.status}`)
    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''
    let eventName = 'message'
    let data: string[] = []
    const dispatch = () => {
      if (data.length > 0) onEvent(eventName, data.join('\n'))
      eventName = 'message'
      data = []
    }
    try {
      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        arm()
        buffer += decoder.decode(value, { stream: true })
        let nl: number
        while ((nl = buffer.indexOf('\n')) >= 0) {
          const line = buffer.slice(0, nl).replace(/\r$/, '')
          buffer = buffer.slice(nl + 1)
          if (line === '') dispatch()
          else if (line.startsWith('event:')) eventName = line.slice(6).trim()
          else if (line.startsWith('data:')) data.push(line.slice(5).trimStart())
          // comments (`:keepalive`) and other fields are ignored — but they
          // did arrive, which is all the watchdog needs to know
        }
      }
    } finally {
      // Every exit path — stream end, abort, or a throw out of onEvent — must
      // release the connection, or a re-registering loop stacks up open streams.
      void reader.cancel().catch(() => {})
    }
    dispatch()
  } catch (e) {
    // A stall the caller's own abort raced is not a stall: the loop asked to
    // stop, and reporting a stall would send it reconnecting instead.
    if (stalled && !signal.aborted) throw new Error(STALLED)
    throw e
  } finally {
    if (watchdog !== undefined) clearTimeout(watchdog)
    signal.removeEventListener('abort', forwardAbort)
  }
}
