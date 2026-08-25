import { describe, it, expect } from 'vitest'
import { makeHubAccount, parseHubAccounts, MemoryHubsBackend } from './hubs'
import { hubIdFor } from '../lib/hubKeys'

describe('makeHubAccount', () => {
  it('normalizes the url, mints the id, defaults label and identity', () => {
    const h = makeHubAccount({ url: 'HTTP://Hub.Local:8765/', token: 't0', trust: { kind: 'plain-http' } })
    expect(h.url).toBe('http://hub.local:8765')
    expect(h.id).toBe(hubIdFor('http://hub.local:8765'))
    expect(h.label).toBe('hub.local:8765')
    expect(h.name).toBe('human')
    expect(h.role).toBe('invite')
  })
})

describe('parseHubAccounts', () => {
  it('keeps well-formed entries and drops malformed ones', () => {
    const good = makeHubAccount({ url: 'https://a.example', token: 'tok', trust: { kind: 'https' } })
    const out = parseHubAccounts([good, { url: 42 }, null, { ...good, token: '' }])
    expect(out).toEqual([good])
  })
  it('returns [] for non-arrays', () => {
    expect(parseHubAccounts('junk')).toEqual([])
    expect(parseHubAccounts(undefined)).toEqual([])
  })
  it('accepts all three trust kinds and rejects unknown ones', () => {
    const base = makeHubAccount({ url: 'https://a.example', token: 'tok', trust: { kind: 'https' } })
    const pinned = { ...base, trust: { kind: 'pinned', sha256: 'ab'.repeat(32) } }
    const bogus = { ...base, trust: { kind: 'trust-me' } }
    expect(parseHubAccounts([pinned, bogus])).toEqual([pinned])
  })
})

describe('MemoryHubsBackend', () => {
  it('round-trips', async () => {
    const b = new MemoryHubsBackend()
    const h = makeHubAccount({ url: 'https://a.example', token: 'tok', trust: { kind: 'https' } })
    await b.save([h])
    expect(await b.load()).toEqual([h])
  })
  it('starts empty', async () => {
    expect(await new MemoryHubsBackend().load()).toEqual([])
  })
})
