import type { Credentials } from '../api/types'

/**
 * The namespace every per-account local store hangs off: drafts, hidden
 * projects, and the hub's chamber/message id maps.
 *
 * Backend kind is part of it because numeric stream ids are not comparable
 * across backends — hub chambers are numbered from 1 and collide head-on with
 * ordinary Zulip stream ids, so a draft for Zulip stream 1 would otherwise open
 * in hub chamber 1, and hiding either project would hide the other. Prefix and
 * email complete it: two accounts on one server, or one account on two servers,
 * are different namespaces too.
 */
export function accountKey(c: Pick<Credentials, 'kind' | 'prefix' | 'email'>): string {
  return `${c.kind ?? 'zulip'}|${c.prefix}|${c.email}`
}
