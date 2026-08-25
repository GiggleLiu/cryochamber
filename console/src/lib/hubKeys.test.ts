import { describe, it, expect } from 'vitest'
import { normalizeHubUrl, hubIdFor, chamberKey, splitChamberKey } from './hubKeys'

describe('normalizeHubUrl', () => {
  it('lowercases scheme and host and strips trailing slashes', () => {
    expect(normalizeHubUrl('HTTP://Hub.Local:8765/')).toBe('http://hub.local:8765')
    expect(normalizeHubUrl('https://example.com///')).toBe('https://example.com')
  })
  it('keeps an explicit path prefix (reverse-proxy hubs)', () => {
    expect(normalizeHubUrl('https://example.com/cryo/')).toBe('https://example.com/cryo')
  })
  it('rejects non-http schemes and garbage', () => {
    expect(() => normalizeHubUrl('ftp://x')).toThrow()
    expect(() => normalizeHubUrl('not a url')).toThrow()
  })
})

describe('hubIdFor', () => {
  it('is 8 hex chars and stable across equivalent spellings', () => {
    const a = hubIdFor('http://hub.local:8765')
    expect(a).toMatch(/^[0-9a-f]{8}$/)
    expect(hubIdFor('HTTP://HUB.local:8765/')).toBe(a)
  })
  it('differs for different hubs', () => {
    expect(hubIdFor('http://a:1')).not.toBe(hubIdFor('http://b:1'))
  })
})

describe('chamberKey / splitChamberKey', () => {
  it('empty hubId is the identity (browser mode)', () => {
    expect(chamberKey('', 'proj')).toBe('proj')
  })
  it('prefixes and round-trips, including chamber ids containing separators', () => {
    const id = hubIdFor('http://hub.local:8765')
    const key = chamberKey(id, 'a/b:c')
    expect(splitChamberKey(key)).toEqual({ hubId: id, chamberId: 'a/b:c' })
  })
  it('an unprefixed key splits as the empty hub', () => {
    expect(splitChamberKey('plain-chamber')).toEqual({ hubId: '', chamberId: 'plain-chamber' })
  })
})
