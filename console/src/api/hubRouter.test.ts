import { describe, it, expect } from 'vitest'
import { HubRouter } from './hubRouter'
import { HubClient } from './hubClient'
import { makeHubAccount } from '../store/hubs'
import { chamberKey } from '../lib/hubKeys'

function fakeHub(url: string, chambers: string[]) {
  const hub = makeHubAccount({ url, token: `tok-${url}`, trust: { kind: 'https' } })
  const calls: string[] = []
  const fetchFn = (async (input: RequestInfo | URL) => {
    const u = String(input)
    calls.push(u)
    if (u.endsWith('/api/chambers')) {
      return new Response(JSON.stringify(chambers.map((id) => ({ id, name: id }))), { status: 200 })
    }
    if (/\/messages$/.test(u)) {
      return new Response(
        JSON.stringify([{ id: 'inbox/1.md', direction: 'inbox', from: 'agent', body: 'hi', timestamp: '2026-08-26T10:00:00' }]),
        { status: 200 },
      )
    }
    return new Response(JSON.stringify({}), { status: 200 })
  }) as typeof fetch
  const client = new HubClient({ token: hub.token, baseUrl: hub.url, fetch: fetchFn })
  return { hub, client, calls }
}

describe('HubRouter', () => {
  it('lists chambers per hub with composite ids', async () => {
    const a = fakeHub('http://a.local:1', ['alpha'])
    const router = new HubRouter([{ hub: a.hub, client: a.client }])
    const chambers = await router.listChambersFor(a.hub.id)
    expect(chambers.map((c) => c.id)).toEqual([chamberKey(a.hub.id, 'alpha')])
  })

  it('routes per-chamber calls to the owning hub and remaps message keys', async () => {
    const a = fakeHub('http://a.local:1', ['alpha'])
    const b = fakeHub('http://b.local:2', ['beta'])
    const router = new HubRouter([
      { hub: a.hub, client: a.client },
      { hub: b.hub, client: b.client },
    ])
    const msgs = await router.getMessages(chamberKey(b.hub.id, 'beta'))
    expect(msgs[0].chamberId).toBe(chamberKey(b.hub.id, 'beta'))
    expect(b.calls.some((u) => u.includes('/api/chambers/beta/messages'))).toBe(true)
    expect(a.calls.every((u) => !u.includes('/messages'))).toBe(true)
  })

  it('rejects calls for an unknown hub', async () => {
    const a = fakeHub('http://a.local:1', [])
    const router = new HubRouter([{ hub: a.hub, client: a.client }])
    await expect(router.getMessages('ffffffff:ghost')).rejects.toThrow('Unknown hub')
  })

  it('remaps SSE message payloads to composite keys', () => {
    const a = fakeHub('http://a.local:1', ['alpha'])
    const router = new HubRouter([{ hub: a.hub, client: a.client }])
    const m = router.toEventMessageFor(a.hub.id, {
      chamber_id: 'alpha', id: 'inbox/2.md', direction: 'inbox', from: 'agent',
      body: 'x', timestamp: '2026-08-26T10:01:00',
    })
    expect(m?.chamberId).toBe(chamberKey(a.hub.id, 'alpha'))
  })
})
