import type { Chamber, ChamberMessage } from '../api/types'
import { accountKey } from '../lib/account'

/**
 * Per-account local cache: the chamber list, the tail of each conversation,
 * and the read watermark per chamber, so a reload paints instantly and unread
 * counts survive it. Same trust level as the stored token; cleared on logout.
 */

/** Distinct from the pre-cutover `agent-console.cache.` prefix, which is not
 * a prefix of this one, so purging the old keys cannot touch the new. */
export const CACHE_PREFIX = 'agent-console.cache2.'

/** Kept per chamber; message bodies are bulky and localStorage quota is ~5MB,
 * so cache only what one screenful of catch-up needs. */
export const MAX_CACHED_MESSAGES = 30

/** Keys the pre-cutover build wrote, removed once at boot. */
const LEGACY_PREFIXES = [
  'agent-console.cache.',
  'agent-console.hub-ids.',
  'agent-console.hub-msgids.',
]

export interface CachedState {
  chambers: Chamber[]
  messagesByChamber: Record<string, ChamberMessage[]>
  lastReadByChamber: Record<string, string>
}

/** Per token, like every other per-account store: a name is reusable, a token
 * is not, so a later invite of the same name never reads the old one's cache. */
export function cacheKey(creds: { token: string }): string {
  return CACHE_PREFIX + accountKey(creds)
}

export function loadCachedState(creds: { token: string }): CachedState | null {
  try {
    const raw = localStorage.getItem(cacheKey(creds))
    if (!raw) return null
    const p = JSON.parse(raw) as Partial<CachedState>
    if (!Array.isArray(p.chambers) || !p.messagesByChamber || typeof p.messagesByChamber !== 'object') {
      return null
    }
    return {
      chambers: p.chambers,
      messagesByChamber: p.messagesByChamber,
      lastReadByChamber:
        p.lastReadByChamber && typeof p.lastReadByChamber === 'object' ? p.lastReadByChamber : {},
    }
  } catch {
    return null
  }
}

export function saveCachedState(creds: { token: string }, state: CachedState): void {
  const messagesByChamber: Record<string, ChamberMessage[]> = {}
  for (const [id, msgs] of Object.entries(state.messagesByChamber)) {
    if (msgs.length > 0) messagesByChamber[id] = msgs.slice(-MAX_CACHED_MESSAGES)
  }
  const key = cacheKey(creds)
  try {
    localStorage.setItem(key, JSON.stringify({ ...state, messagesByChamber }))
  } catch {
    // Quota or no storage: a partial cache is worse than none.
    try {
      localStorage.removeItem(key)
    } catch {
      /* storage entirely unavailable */
    }
  }
}

/** A stream can land a dozen messages a second, and every store mutation asks
 * to persist; localStorage writes are synchronous, so they are coalesced. */
export const PERSIST_DEBOUNCE_MS = 250

/** Pending per cache key, not one slot for the app: app mode persists every
 * hub in the same turn, and a single slot would let the last hub's write
 * silently discard the rest. One key still coalesces to its newest state, so
 * the single-account path writes exactly what it always did. */
const pending = new Map<string, { creds: { token: string }; state: CachedState }>()
let timer: ReturnType<typeof setTimeout> | null = null

/** Coalesce a burst of store mutations (a chatty stream) into one write. */
export function saveCachedStateDebounced(creds: { token: string }, state: CachedState): void {
  pending.set(cacheKey(creds), { creds, state })
  if (timer) return
  timer = setTimeout(() => {
    timer = null
    flushCachedState()
  }, PERSIST_DEBOUNCE_MS)
}

/** Write whatever is pending now — before the page hides, before logout. */
export function flushCachedState(): void {
  if (timer) {
    clearTimeout(timer)
    timer = null
  }
  if (pending.size === 0) return
  const writes = [...pending.values()]
  pending.clear()
  for (const { creds, state } of writes) saveCachedState(creds, state)
}

/** Drop a pending write without saving (logout must not resurrect a cache). */
export function cancelPendingCachedState(): void {
  if (timer) {
    clearTimeout(timer)
    timer = null
  }
  pending.clear()
}

export function clearCachedState(creds: { token: string }): void {
  try {
    localStorage.removeItem(cacheKey(creds))
  } catch {
    /* storage unavailable */
  }
}

/** Drop everything the pre-cutover build persisted except the credentials
 * record (which `loadCredentials` migrates). Idempotent; runs once at boot. */
export function purgeLegacyStorage(): void {
  try {
    for (const key of Object.keys(localStorage)) {
      if (LEGACY_PREFIXES.some((p) => key.startsWith(p))) localStorage.removeItem(key)
    }
  } catch {
    /* storage unavailable */
  }
}
