import { fnv1a } from './account'

/** Canonical spelling of a hub URL: http(s) only, lowercase scheme+host,
 * no trailing slash — so re-adding the same hub always mints the same id. */
export function normalizeHubUrl(raw: string): string {
  const u = new URL(raw.trim()) // throws on garbage
  if (u.protocol !== 'http:' && u.protocol !== 'https:') {
    throw new Error(`Hub URLs must be http or https, got ${u.protocol}`)
  }
  const path = u.pathname.replace(/\/+$/, '')
  return `${u.protocol}//${u.host}${path}`
}

/** 8-hex fingerprint of one saved access: URL plus bearer token. Two invite
 * links can point at the same hub but expose different chambers, so URL alone
 * must never make the second silently replace the first. */
export function hubIdFor(url: string, token: string): string {
  return fnv1a(`${normalizeHubUrl(url)}\n${token}`).toString(16).padStart(8, '0')
}

/** URL-only id written by app versions before access tokens became separate
 * records. Kept only so their local caches can be re-keyed once at boot. */
export function legacyHubIdFor(url: string): string {
  return fnv1a(normalizeHubUrl(url)).toString(16).padStart(8, '0')
}

const KEY_RE = /^([0-9a-f]{8}):([\s\S]*)$/

/** The console-side chamber key. Browser mode (`hubId === ''`) is the
 * identity, which is what keeps every browser-visible key byte-identical. */
export function chamberKey(hubId: string, chamberId: string): string {
  return hubId === '' ? chamberId : `${hubId}:${chamberId}`
}

/** Inverse of chamberKey for app-minted keys. Chamber ids may contain `:`
 * and `/`, but the prefix is ours and fixed-width, so the first match wins. */
export function splitChamberKey(key: string): { hubId: string; chamberId: string } {
  const m = KEY_RE.exec(key)
  return m ? { hubId: m[1], chamberId: m[2] } : { hubId: '', chamberId: key }
}
