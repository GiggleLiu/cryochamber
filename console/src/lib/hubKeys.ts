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

/** 8-hex fingerprint of the normalized URL. FNV over the handful of hubs one
 * user ever adds — collisions are not a real event, and the id leaks nothing. */
export function hubIdFor(url: string): string {
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
