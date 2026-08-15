import { useEffect } from 'react'
import { isAuthError } from '../api/errors'
import { HubClient } from '../api/hubClient'
import { readSse } from '../api/sse'
import { useAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'

/** Thrown out of the SSE callback when the hub says the chamber index changed:
 * the loop unwinds to its top and re-registers. */
class ReregisterSignal extends Error {}

/** How long an SSE connection must stay open, with no event at all, before it
 * counts as healthy enough to reset the backoff. */
const SSE_HEALTHY_MS = 10_000

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function waitForVisible(): Promise<void> {
  return new Promise((resolve) => {
    const handler = () => {
      if (document.visibilityState === 'visible') {
        document.removeEventListener('visibilitychange', handler)
        resolve()
      }
    }
    document.addEventListener('visibilitychange', handler)
  })
}

export function useEventLoop(): void {
  const client = useAppStore((s) => s.client)

  useEffect(() => {
    if (!client) return
    let stopped = false
    const abort = new AbortController()
    let backoff = 1000
    const store = useAppStore

    // register() lists the chambers in scope; a single SSE stream carries
    // everything after that.
    async function run(client: HubClient) {
      while (!stopped) {
        if (document.visibilityState === 'hidden') {
          await waitForVisible()
          if (stopped) return
          continue
        }
        try {
          store.getState().setConnection('connecting')
          const init = await client.register()
          if (stopped) return
          // Re-registering clears loadedStreams, so an open conversation
          // re-fetches its history over whatever the events left behind.
          store.getState().applyInitialState(init)
          store.getState().setConnection('live')
          let seq = 1
          // A successful register() says nothing about the stream that follows:
          // resetting the backoff here made a connection that dies instantly
          // retry forever at one second. The reset waits for proof — a first
          // event, or SSE_HEALTHY_MS of staying open.
          let healthy = false
          const markHealthy = () => {
            healthy = true
            backoff = 1000
          }
          const healthTimer = setTimeout(markHealthy, SSE_HEALTHY_MS)
          try {
            await readSse(
              '/api/events',
              { Authorization: client.authHeaderValue() },
              (event, payload) => {
                markHealthy()
                if (event === 'index') throw new ReregisterSignal()
                if (event === 'status') {
                  // Fire and forget for transient failures — a stale banner
                  // heals on the next status event. A 401 is different: this
                  // refresh may be the first authenticated call after a
                  // revocation, and swallowing it would leave a signed-in UI
                  // behind a dead token.
                  client
                    .chamberStatuses()
                    .then((l) => store.getState().updateStreamStatus(l))
                    .catch((e) => {
                      if (isAuthError(e)) store.getState().logout(AUTH_LOGOUT_REASON)
                    })
                  return
                }
                if (event !== 'message') return
                try {
                  const m = JSON.parse(payload) as {
                    id?: string
                    chamber_id: string
                    from: string
                    subject: string
                    body: string
                    timestamp: string
                    is_question: boolean
                  }
                  const msg = client.toChamberEventMessage(m)
                  if (msg) {
                    store.getState().applyEvents([{ id: seq++, type: 'message', message: msg }])
                  }
                } catch {
                  // malformed payload: skip (the index signal is thrown above,
                  // outside this try, so it is never swallowed here)
                }
              },
              abort.signal,
            )
          } finally {
            clearTimeout(healthTimer)
          }
          if (stopped) return
          // Stream ended cleanly → the loop re-registers, but there is a gap
          // before it does and the user is not receiving anything during it, so
          // say so rather than leaving the banner claiming 'live'. A stream that
          // never proved healthy (proxy dropping it, server restarting) also
          // widens the wait, so the loop cannot spin.
          store.getState().setConnection('offline')
          await sleep(backoff)
          if (!healthy) backoff = Math.min(backoff * 2, 30000)
        } catch (e) {
          if (stopped) return
          if (e instanceof ReregisterSignal) continue
          if (isAuthError(e)) {
            store.getState().logout(AUTH_LOGOUT_REASON)
            return
          }
          store.getState().setConnection('offline')
          await sleep(backoff)
          backoff = Math.min(backoff * 2, 30000)
        }
      }
    }

    void run(client)
    return () => {
      stopped = true
      abort.abort()
    }
  }, [client])
}
