import { useEffect } from 'react'
import { isUnauthorized } from '../api/types'
import { isSseStall } from '../api/sse'
import { HubClient } from '../api/hubClient'
import { useAppStore } from '../store/appStore'
import { emitChamberEvent } from '../store/chamberEvents'

/** Thrown out of the SSE callback when the hub says the chamber index changed:
 * the loop unwinds to its top and re-registers. */
class ReregisterSignal extends Error {}

/** How long an SSE connection must stay open, with no event at all, before it
 * counts as healthy enough to reset the backoff. */
const SSE_HEALTHY_MS = 10_000

/**
 * Wait `ms` — but return early when the page becomes visible (a phone coming
 * back to the foreground should not sit out the rest of a 30 s backoff) or
 * when `signal` aborts (the loop is being torn down).
 */
export function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    let timer: ReturnType<typeof setTimeout>
    const finish = () => {
      clearTimeout(timer)
      document.removeEventListener('visibilitychange', onVisibility)
      signal?.removeEventListener('abort', finish)
      resolve()
    }
    const onVisibility = () => {
      if (document.visibilityState === 'visible') finish()
    }
    timer = setTimeout(finish, ms)
    document.addEventListener('visibilitychange', onVisibility)
    signal?.addEventListener('abort', finish, { once: true })
  })
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

    // listChambers() is the scope read; a single SSE stream carries
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
          const chambers = await client.listChambers()
          if (stopped) return
          // Re-reading the index clears loadedChambers, so an open conversation
          // re-fetches its history over whatever the events left behind.
          store.getState().setChambers(chambers)
          store.getState().setConnection('live')
          // A successful index read says nothing about the stream that follows:
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
            await client.events((event, payload) => {
              markHealthy()
              // `resync` is the hub asking for the same thing `index` does:
              // read the scope again from the top.
              if (event === 'index' || event === 'resync') throw new ReregisterSignal()
              if (event === 'status') {
                // Two audiences: the projects list, refreshed from the index,
                // and whatever sheet is open on this chamber, which re-reads
                // its own detail. Parse first — a payload we cannot read
                // still deserves the index refresh.
                try {
                  const { chamber_id } = JSON.parse(payload) as { chamber_id: string }
                  if (chamber_id) emitChamberEvent({ type: 'status', chamberId: chamber_id })
                } catch {
                  /* malformed payload: the index refresh below still runs */
                }
                // Fire and forget: a stale banner heals on the next status
                // event, and a 401 has already signed the app out inside the
                // client — there is nothing left for this catch to do.
                client
                  .listChambers()
                  .then((l) => store.getState().updateChamberStatus(l))
                  .catch(() => {})
                return
              }
              if (event === 'log') {
                try {
                  const { chamber_id, line } = JSON.parse(payload) as {
                    chamber_id: string
                    line: string
                  }
                  if (chamber_id) {
                    emitChamberEvent({ type: 'log', chamberId: chamber_id, line: line ?? '' })
                  }
                } catch {
                  /* malformed payload: skip the line, keep the stream */
                }
                return
              }
              if (event !== 'message') return
              try {
                const msg = client.toEventMessage(JSON.parse(payload))
                // A message for a chamber outside our scope has no row to land
                // in; dropping it keeps the store's keys and the list agreeing.
                if (msg && store.getState().chambers.some((c) => c.id === msg.chamberId)) {
                  store.getState().applyMessage(msg)
                }
              } catch {
                // malformed payload: skip (the index signal is thrown above,
                // outside this try, so it is never swallowed here)
              }
            }, abort.signal)
          } finally {
            clearTimeout(healthTimer)
          }
          if (stopped) return
          // Stream ended cleanly → the loop reconnects, but there is a gap
          // before it does and the user is not receiving anything during it, so
          // say so rather than leaving the banner claiming 'live'. A stream that
          // never proved healthy (proxy dropping it, server restarting) also
          // widens the wait, so the loop cannot spin.
          store.getState().setConnection('offline')
          await sleep(backoff, abort.signal)
          if (!healthy) backoff = Math.min(backoff * 2, 30000)
        } catch (e) {
          if (stopped) return
          if (e instanceof ReregisterSignal) continue
          if (isSseStall(e)) {
            // A half-open connection: nothing arrived for SSE_STALL_MS but the
            // socket never closed. The hub is not known to be down, so this is
            // not a failure to back off from — reconnect now, at the floor.
            backoff = 1000
            store.getState().setConnection('connecting')
            continue
          }
          // A 401 already signed the app out inside the client; this loop's
          // job is only to stop.
          if (isUnauthorized(e)) return
          store.getState().setConnection('offline')
          await sleep(backoff, abort.signal)
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
