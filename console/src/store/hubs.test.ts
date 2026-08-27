import { describe, it, expect } from 'vitest'
import { makeHubAccount, parseHubAccounts, MemoryHubsBackend } from './hubs'
import { hubIdFor } from '../lib/hubKeys'

describe('makeHubAccount', () => {
  it('normalizes the url, mints the id, defaults label and identity', () => {
    const h = makeHubAccount({ url: 'HTTP://Hub.Local:8765/', token: 't0', trust: { kind: 'plain-http' } })
    expect(h.url).toBe('http://hub.local:8765')
    expect(h.id).toBe(hubIdFor('http://hub.local:8765', 't0'))
    expect(h.label).toBe('hub.local:8765')
    expect(h.name).toBe('human')
    expect(h.role).toBe('invite')
  })

  it('keeps two access links to the same hub distinct', () => {
    const owner = makeHubAccount({
      url: 'https://hub.example', token: 'owner-token', trust: { kind: 'https' },
    })
    const invite = makeHubAccount({
      url: 'https://hub.example', token: 'invite-token', trust: { kind: 'https' },
    })
    expect(owner.id).not.toBe(invite.id)
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
