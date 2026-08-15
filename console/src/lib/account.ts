import type { Credentials } from '../api/types'

/**
 * The namespace every per-account local store hangs off: drafts, hidden
 * projects, and the chamber/message id maps.
 *
 * Numeric stream ids are only meaningful inside one namespace — every token
 * numbers its own chambers from 1 — so prefix and email are part of the key:
 * two accounts on one hub, or one account on two hubs, are different
 * namespaces. The backend kind stays in it so the shape of the key survives.
 */
export function accountKey(c: Pick<Credentials, 'kind' | 'prefix' | 'email'>): string {
  return `${c.kind}|${c.prefix}|${c.email}`
}
