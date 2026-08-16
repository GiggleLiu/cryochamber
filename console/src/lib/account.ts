import type { Credentials } from '../api/types'

/** 32-bit FNV-1a. Shared with the message-id fallback in hubClient. */
export function fnv1a(s: string): number {
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i += 1) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  return h >>> 0
}

/** Non-reversing fingerprint of a bearer token, for storage key names. Two
 * FNV passes over distinct inputs give 64 bits — collisions across the
 * handful of tokens one browser ever sees are not a real event, and 64 bits
 * of digest leak nothing useful about a 256-bit token. */
function tokenFingerprint(token: string): string {
  return `${fnv1a(token).toString(16)}${fnv1a(`cryo|${token}`).toString(16)}`
}

/**
 * The namespace every per-account local store hangs off: drafts, hidden
 * projects, the chamber/message id maps, and the message cache.
 *
 * Keyed by the token itself (fingerprinted), never by the display name:
 * invite names are reusable after revocation, so a later "Alice" token on
 * the same browser must not inherit the old Alice's drafts, id maps, or
 * hidden projects. The `hub|` prefix keeps the key's shape stable — the
 * console only ever talks to the hub that served it.
 */
export function accountKey(c: Pick<Credentials, 'token'>): string {
  return `hub|${tokenFingerprint(c.token)}`
}
