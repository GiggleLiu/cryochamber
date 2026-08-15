import type { Message, StreamSub } from '../api/types'

/**
 * Per-account local cache of streams and recent messages, so a reload paints
 * the projects list and open conversations instantly while the network
 * refresh (register + history fetch) reconciles in the background.
 *
 * Same trust level as the stored credentials: anyone who can read
 * localStorage already holds the API key, so caching message content adds no
 * new exposure. Cleared on logout alongside the credentials.
 */

export const CACHE_PREFIX = 'agent-console.cache.'

/** Kept per stream; message bodies are bulky and localStorage quota is ~5MB,
 * so cache only what one screenful of catch-up needs. */
export const MAX_CACHED_MESSAGES = 30

export interface CachedState {
  streams: StreamSub[]
  messagesByStream: Record<number, Message[]>
}

export function cacheKey(creds: { prefix: string; email: string }): string {
  return `${CACHE_PREFIX}${creds.prefix}|${creds.email}`
}

export function loadCachedState(creds: { prefix: string; email: string }): CachedState | null {
  try {
    const raw = localStorage.getItem(cacheKey(creds))
    if (!raw) return null
    const parsed = JSON.parse(raw) as CachedState
    if (!Array.isArray(parsed.streams) || typeof parsed.messagesByStream !== 'object') {
      return null
    }
    return parsed
  } catch {
    return null
  }
}

export function saveCachedState(
  creds: { prefix: string; email: string },
  streams: StreamSub[],
  messagesByStream: Record<number, Message[]>,
): void {
  const trimmed: Record<number, Message[]> = {}
  for (const [id, msgs] of Object.entries(messagesByStream)) {
    if (msgs.length > 0) trimmed[Number(id)] = msgs.slice(-MAX_CACHED_MESSAGES)
  }
  const key = cacheKey(creds)
  try {
    localStorage.setItem(key, JSON.stringify({ streams, messagesByStream: trimmed }))
  } catch {
    // Quota exceeded (or storage unavailable): a partial cache is worse than
    // none, so drop this account's entry and carry on — the app works without
    // it, just without the instant first paint.
    try {
      localStorage.removeItem(key)
    } catch {
      /* storage is entirely unavailable; nothing to do */
    }
  }
}

export function clearCachedState(creds: { prefix: string; email: string }): void {
  try {
    localStorage.removeItem(cacheKey(creds))
  } catch {
    /* storage unavailable */
  }
}
