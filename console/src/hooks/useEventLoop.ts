import { useEffect } from 'react'
import { isUnauthorized, type Chamber, type ChamberMessage } from '../api/types'
import { isSseStall } from '../api/sse'
import { HubClient } from '../api/hubClient'
import { HubRouter, type ConsoleClient } from '../api/hubRouter'
import { chamberKey } from '../lib/hubKeys'
import { useAppStore } from '../store/appStore'
import { emitChamberEvent } from '../store/chamberEvents'

/** Thrown out of the SSE callback when the hub says the chamber index changed:
 * the loop unwinds to its top and re-registers. */
class ReregisterSignal extends Error {}

/** How long an SSE connection must stay open, with no event at all, before it
 * counts as healthy enough to reset the backoff. */
const SSE_HEALTHY_MS = 10_000

/** Floor between a re-register and the index read that follows it. A hub that
 * answers every connection with `resync` (or an `index` the client keeps
 * lagging behind) would otherwise spin the loop as fast as the network
 * answers; half a second is invisible to a person and fatal to a tight loop. */
const REREGISTER_FLOOR_MS = 500

/**
 * Wait `ms` — but return early when the page becomes visible (a phone coming
 * back to the foreground should not sit out the rest of a 30 s backoff) or
 * when `signal` aborts (the loop is being torn down).
 */
export function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  // An already-aborted signal never fires 'abort', so a listener alone would
  // sit out the whole wait after teardown.
  if (signal?.aborted) return Promise.resolve()
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

/** This hub's chamber rows, keyed the way the store expects them. Browser
 * mode's one hub is the anonymous `''`, whose keys are the hub's own raw ids;
 * behind a router each row is stamped with its hub and re-keyed, which is the
 * only spelling of that mapping the loop needs to know about. */
function chambersFor(
  hubId: string,
  client: HubClient,
  storeClient: ConsoleClient,
): Promise<Chamber[]> {
  return hubId !== '' && storeClient instanceof HubRouter
    ? storeClient.listChambersFor(hubId)
    : client.listChambers()
}

/** An SSE `message` payload → store message, under the same keys. */
function eventMessageFor(
  hubId: string,
  client: HubClient,
  storeClient: ConsoleClient,
  payload: unknown,
): ChamberMessage | null {
  return hubId !== '' && storeClient instanceof HubRouter
    ? storeClient.toEventMessageFor(hubId, payload)
    : client.toEventMessage(payload)
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
    // Teardown is the whole app's: one flag and one controller stop every
    // loop. A hub's backoff is its own, so a hub that is down cannot widen
    // the wait of a hub that is up.
    let stopped = false
    const abort = new AbortController()
    const store = useAppStore

    // listChambers() is the scope read; a single SSE stream per hub carries
    // everything after that. `storeClient` is what the store holds — the
    // router in app mode — and is what maps this hub's ids into store keys.
    async function run(hubId: string, client: HubClient, storeClient: ConsoleClient) {
      let backoff = 1000
      while (!stopped) {
        if (document.visibilityState === 'hidden') {
          await waitForVisible()
          if (stopped) return
          continue
        }
        try {
          store.getState().setConnectionForHub(hubId, 'connecting')
          const chambers = await chambersFor(hubId, client, storeClient)
          if (stopped) return
          // Re-reading the index clears loadedChambers, so an open conversation
          // re-fetches its history over whatever the events left behind.
          store.getState().setChambersForHub(hubId, chambers)
          store.getState().setConnectionForHub(hubId, 'live')
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
                  // The sheet listens on the store's key, which names the hub.
                  if (chamber_id) {
                    emitChamberEvent({ type: 'status', chamberId: chamberKey(hubId, chamber_id) })
                  }
                } catch {
                  /* malformed payload: the index refresh below still runs */
                }
                // Fire and forget: a stale banner heals on the next status
                // event, and a 401 has already signed the app out inside the
                // client — there is nothing left for this catch to do.
                chambersFor(hubId, client, storeClient)
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
                    emitChamberEvent({
                      type: 'log',
                      chamberId: chamberKey(hubId, chamber_id),
                      line: line ?? '',
                    })
                  }
                } catch {
                  /* malformed payload: skip the line, keep the stream */
                }
                return
              }
              if (event !== 'message') return
              try {
                const msg = eventMessageFor(hubId, client, storeClient, JSON.parse(payload))
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
          store.getState().setConnectionForHub(hubId, 'offline')
          await sleep(backoff, abort.signal)
          if (!healthy) backoff = Math.min(backoff * 2, 30000)
        } catch (e) {
          if (stopped) return
          if (e instanceof ReregisterSignal) {
            // Pace the re-read (see REREGISTER_FLOOR_MS); teardown cuts it short.
            await sleep(REREGISTER_FLOOR_MS, abort.signal)
            if (stopped) return
            continue
          }
          if (isSseStall(e)) {
            // A half-open connection: nothing arrived for SSE_STALL_MS but the
            // socket never closed. The hub is not known to be down, so this is
            // not a failure to back off from — reconnect now, at the floor.
            backoff = 1000
            store.getState().setConnectionForHub(hubId, 'connecting')
            continue
          }
          // A 401 already signed the app out inside the client; this loop's
          // job is only to stop — this hub's, not the other hubs'.
          if (isUnauthorized(e)) return
          store.getState().setConnectionForHub(hubId, 'offline')
          await sleep(backoff, abort.signal)
          backoff = Math.min(backoff * 2, 30000)
        }
      }
    }

    // Browser mode is one hub, the anonymous `''`. App mode is one loop per
    // hub in the router — and a hub added or removed rebuilds that router,
    // which restarts this effect over the new set.
    const loops =
      client instanceof HubRouter
        ? client.entries().map((e) => ({ hubId: e.hub.id, hubClient: e.client }))
        : client instanceof HubClient
          ? [{ hubId: '', hubClient: client }]
          : []
    for (const l of loops) void run(l.hubId, l.hubClient, client)
    return () => {
      stopped = true
      abort.abort()
    }
  }, [client])
}
