/**
 * Per-chamber fan-out for the two SSE events a sheet cares about.
 *
 * The app keeps exactly one `/api/events` stream (see `useEventLoop`); a
 * Controls sheet must not open a second one just to learn that its chamber
 * woke up. So the loop forwards `status` and `log` here, and whatever is on
 * screen subscribes for the chamber it is showing.
 *
 * Module scope, not the Zustand store: these are transient notifications, not
 * state, and putting a log line in the store would re-render every subscriber
 * of every slice for each of the thousands of lines a session emits.
 */
export type ChamberEvent =
  | { type: 'status'; chamberId: string }
  | { type: 'log'; chamberId: string; line: string }

type Listener = (event: ChamberEvent) => void

const listeners = new Map<string, Set<Listener>>()

/** Returns the unsubscribe function, so an effect can `return subscribe(...)`. */
export function subscribeChamberEvents(chamberId: string, listener: Listener): () => void {
  const set = listeners.get(chamberId) ?? new Set<Listener>()
  set.add(listener)
  listeners.set(chamberId, set)
  return () => {
    const current = listeners.get(chamberId)
    if (!current) return
    current.delete(listener)
    if (current.size === 0) listeners.delete(chamberId)
  }
}

export function emitChamberEvent(event: ChamberEvent): void {
  const set = listeners.get(event.chamberId)
  if (!set) return
  // A copy, because a listener may unsubscribe itself while we iterate; and
  // each call is isolated, because one sheet throwing must not silence the
  // rest of the app's live updates.
  for (const listener of Array.from(set)) {
    try {
      listener(event)
    } catch (e) {
      // A broken subscriber must not silence the others — but not silently.
      console.warn('chamber event listener threw', e)
    }
  }
}

/** Test hook: drops every subscription. Called by `resetAppStore`. */
export function resetChamberEvents(): void {
  listeners.clear()
}
