import {
  bootApp,
  HUB_LOAD_ERROR,
  makeClientFactory,
  parseInviteLink,
  type AppRuntime,
} from './appBoot'
import { MemoryHubsBackend, makeHubAccount } from '../store/hubs'
import { useAppStore, resetAppStore } from '../store/appStore'

beforeEach(() => {
  resetAppStore()
  localStorage.clear()
})

/** A transport that answers whoami with `who` and everything else with `[]`. */
function transport(who: unknown, status = 200): typeof fetch {
  return (async (input: RequestInfo | URL) =>
    String(input).endsWith('/api/whoami')
      ? new Response(JSON.stringify(who), { status })
      : new Response('[]', { status: 200 })) as typeof fetch
}

describe('parseInviteLink', () => {
  it('extracts hub origin and token from a full invite link', () => {
    expect(parseInviteLink('http://hub.local:8765/#invite=' + 'a'.repeat(32))).toEqual({
      url: 'http://hub.local:8765',
      token: 'a'.repeat(32),
    })
  })

  it('keeps a path the hub is served under', () => {
    expect(parseInviteLink('https://box.example/console/#invite=' + 'b'.repeat(32))).toEqual({
      url: 'https://box.example/console',
      token: 'b'.repeat(32),
    })
  })

  it('rejects links without a plausible token', () => {
    expect(parseInviteLink('http://hub.local:8765/#invite=xyz')).toBeNull()
    expect(parseInviteLink('not a link')).toBeNull()
    expect(parseInviteLink('http://hub.local:8765/')).toBeNull()
    expect(parseInviteLink('ftp://hub.local/#invite=' + 'a'.repeat(32))).toBeNull()
  })
})

describe('bootApp', () => {
  it('loads hubs, enters app mode, and refreshes identity from whoami', async () => {
    const hub = makeHubAccount({
      url: 'http://a.local:1',
      token: 'ta',
      trust: { kind: 'plain-http' },
    })
    const backend = new MemoryHubsBackend()
    await backend.save([hub])
    const rt: AppRuntime = {
      backend,
      transportFor: () => transport({ role: 'owner', name: 'liu', hub_version: '9.9.9' }),
    }
    await bootApp(rt)
    await vi.waitFor(() => {
      const s = useAppStore.getState()
      expect(s.mode).toBe('app')
      expect(s.roleByHub[hub.id]).toBe('owner')
      expect(s.versionByHub[hub.id]).toBe('9.9.9')
    })
    expect(useAppStore.getState().selfNameByHub[hub.id]).toBe('liu')
    // The stored record heals: the next boot starts from what the hub said.
    await vi.waitFor(async () => {
      const [saved] = await backend.load()
      expect(saved.role).toBe('owner')
      expect(saved.name).toBe('liu')
    })
  })

  it('leaves the stored list alone when whoami agrees with it', async () => {
    const hub = makeHubAccount({
      url: 'http://a.local:1',
      token: 'ta',
      name: 'liu',
      role: 'owner',
      trust: { kind: 'plain-http' },
    })
    const backend = new MemoryHubsBackend()
    await backend.save([hub])
    const save = vi.spyOn(backend, 'save')
    const rt: AppRuntime = {
      backend,
      transportFor: () => transport({ role: 'owner', name: 'liu', hub_version: '9.9.9' }),
    }
    await bootApp(rt)
    await vi.waitFor(() => expect(useAppStore.getState().versionByHub[hub.id]).toBe('9.9.9'))
    // addHub would rebuild the router and tear down every SSE loop the boot
    // just started, for a write that changes nothing.
    expect(save).not.toHaveBeenCalled()
  })

  it('marks a hub whose token the hub refuses, without failing the boot', async () => {
    const hub = makeHubAccount({
      url: 'http://a.local:1',
      token: 'ta',
      trust: { kind: 'plain-http' },
    })
    const backend = new MemoryHubsBackend()
    await backend.save([hub])
    const rt: AppRuntime = { backend, transportFor: () => transport('', 401) }
    await bootApp(rt)
    await vi.waitFor(() => {
      const s = useAppStore.getState()
      expect(s.authFailedHubs).toContain(hub.id)
      // A revoked token must not leave the row pinned at "connecting".
      expect(s.connectionByHub[hub.id]).toBe('offline')
    })
  })

  it('still enters app mode, and says why, when the stored list cannot be read', async () => {
    const backend = new MemoryHubsBackend()
    vi.spyOn(backend, 'load').mockRejectedValue(new Error('store unavailable'))
    const rt: AppRuntime = { backend, transportFor: () => transport({ role: 'owner' }) }
    // A rejection used to escape as an unhandled promise: the app stayed in
    // browser mode behind a blank Add Hub screen with nothing said.
    await bootApp(rt)
    const s = useAppStore.getState()
    expect(s.mode).toBe('app')
    expect(s.hubs).toEqual([])
    expect(s.loginReason).toBe(HUB_LOAD_ERROR)
  })

  it('enters app mode with an empty list when nothing is stored', async () => {
    const backend = new MemoryHubsBackend()
    const rt: AppRuntime = { backend, transportFor: () => transport({ role: 'owner' }) }
    await bootApp(rt)
    expect(useAppStore.getState().mode).toBe('app')
    expect(useAppStore.getState().hubs).toEqual([])
  })
})

describe('makeClientFactory', () => {
  it('addresses the hub it was built for and carries its token', async () => {
    const hub = makeHubAccount({
      url: 'http://a.local:1',
      token: 'ta',
      trust: { kind: 'plain-http' },
    })
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ role: 'invite' })))
    const client = makeClientFactory({
      backend: new MemoryHubsBackend(),
      transportFor: () => fetchMock as unknown as typeof fetch,
    })(hub)
    await client.whoami()
    expect(fetchMock).toHaveBeenCalledWith(
      'http://a.local:1/api/whoami',
      expect.objectContaining({
        headers: expect.objectContaining({ Authorization: 'Bearer ta' }),
      }),
    )
  })
})
