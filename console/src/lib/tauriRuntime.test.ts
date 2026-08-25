import { describe, it, expect, afterEach, vi } from 'vitest'
import { TauriHubsBackend, makeTauriRuntime } from './tauriRuntime'
import type { HubAccount } from '../store/hubs'

afterEach(() => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  delete (window as any).__TAURI__
})

function hub(url: string, trust: HubAccount['trust']): HubAccount {
  return {
    id: `id-${url}`,
    url,
    label: 'hub',
    token: 'deadbeef',
    name: 'human',
    role: 'owner',
    trust,
  }
}

/** A fake store plugin: records every call so a test can assert both what was
 * written and in which order it reached the store. */
function fakeStore(initial?: unknown) {
  const calls: string[] = []
  const values = new Map<string, unknown>()
  if (initial !== undefined) values.set('hubs', initial)
  const store = {
    get: vi.fn(async (key: string) => {
      calls.push(`get:${key}`)
      return values.get(key) ?? null
    }),
    set: vi.fn(async (key: string, value: unknown) => {
      calls.push(`set:${key}`)
      values.set(key, value)
    }),
    save: vi.fn(async () => {
      calls.push('save')
    }),
  }
  const load = vi.fn(async (file: string) => {
    calls.push(`load:${file}`)
    return store
  })
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  ;(window as any).__TAURI__ = {
    core: { invoke: vi.fn() },
    http: { fetch: vi.fn(async () => new Response('{}')) },
    store: { load },
  }
  return { calls, values, store, load }
}

describe('TauriHubsBackend', () => {
  it('loads hubs from the hubs.json store', async () => {
    const f = fakeStore([
      { url: 'https://a.example', token: 't1', label: 'A', name: 'me', role: 'owner', trust: { kind: 'https' } },
    ])
    const hubs = await new TauriHubsBackend().load()
    expect(f.load).toHaveBeenCalledWith('hubs.json')
    expect(f.store.get).toHaveBeenCalledWith('hubs')
    expect(hubs).toHaveLength(1)
    expect(hubs[0].url).toBe('https://a.example')
  })

  it('returns an empty list when nothing is stored yet', async () => {
    fakeStore()
    await expect(new TauriHubsBackend().load()).resolves.toEqual([])
  })

  it('drops malformed stored entries', async () => {
    fakeStore([{ url: 'https://a.example' }, 'nonsense'])
    await expect(new TauriHubsBackend().load()).resolves.toEqual([])
  })

  it('round-trips accounts through set + save', async () => {
    const f = fakeStore()
    const backend = new TauriHubsBackend()
    const account = hub('https://a.example', { kind: 'https' })
    await backend.save([account])
    expect(f.store.set).toHaveBeenCalledWith('hubs', [account])
    expect(f.store.save).toHaveBeenCalledOnce()
    expect(f.calls).toEqual(['load:hubs.json', 'set:hubs', 'save'])
    await expect(backend.load()).resolves.toHaveLength(1)
    // One handle for the life of the backend: a second would race the first
    // handle's in-memory copy of the file.
    expect(f.load).toHaveBeenCalledOnce()
  })

  it('retries the store load after a failed one instead of caching the failure', async () => {
    const f = fakeStore([
      { url: 'https://a.example', token: 't1', label: 'A', name: 'me', role: 'owner', trust: { kind: 'https' } },
    ])
    f.load.mockRejectedValueOnce(new Error('store permission denied'))
    const backend = new TauriHubsBackend()
    await expect(backend.load()).rejects.toThrow('store permission denied')
    // A cached rejection would fail every later call for the process lifetime.
    await expect(backend.load()).resolves.toHaveLength(1)
    await expect(backend.save([hub('https://b.example', { kind: 'https' })])).resolves.toBeUndefined()
  })

  it('awaits the store save before resolving', async () => {
    const f = fakeStore()
    let flushed = false
    f.store.save.mockImplementation(async () => {
      await Promise.resolve()
      flushed = true
    })
    await new TauriHubsBackend().save([hub('https://a.example', { kind: 'https' })])
    expect(flushed).toBe(true)
  })

  it('serializes writes so two rapid saves land in call order', async () => {
    const f = fakeStore()
    let release: () => void = () => {}
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    let first = true
    f.store.set.mockImplementation(async (key: string, value: unknown) => {
      if (first) {
        first = false
        f.calls.push(`set:${key}:first`)
        await gate
        return
      }
      f.calls.push(`set:${key}:second`)
      f.values.set(key, value)
    })

    const backend = new TauriHubsBackend()
    const a = backend.save([hub('https://a.example', { kind: 'https' })])
    const b = backend.save([hub('https://b.example', { kind: 'https' })])

    // The second write must not have touched the store while the first is
    // still in flight — that is what serialization means here.
    await Promise.resolve()
    await Promise.resolve()
    expect(f.calls).not.toContain('set:hubs:second')

    release()
    await Promise.all([a, b])
    expect(f.calls).toEqual([
      'load:hubs.json',
      'set:hubs:first',
      'save',
      'set:hubs:second',
      'save',
    ])
    expect((f.values.get('hubs') as HubAccount[])[0].url).toBe('https://b.example')
  })

  it('keeps the queue running after a failed write', async () => {
    const f = fakeStore()
    f.store.set.mockRejectedValueOnce(new Error('disk full'))
    const backend = new TauriHubsBackend()
    await expect(backend.save([hub('https://a.example', { kind: 'https' })])).rejects.toThrow(
      'disk full',
    )
    await expect(backend.save([hub('https://b.example', { kind: 'https' })])).resolves.toBeUndefined()
  })
})

describe('makeTauriRuntime', () => {
  it('uses the store-backed backend', () => {
    fakeStore()
    expect(makeTauriRuntime().backend).toBeInstanceOf(TauriHubsBackend)
  })

  it('routes https and plain-http hubs through the plugin fetch', async () => {
    const f = fakeStore()
    const rt = makeTauriRuntime()
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const pluginFetch = (window as any).__TAURI__.http.fetch
    await rt.transportFor(hub('https://a.example', { kind: 'https' }))('https://a.example/api/whoami')
    await rt.transportFor(hub('http://b.example', { kind: 'plain-http' }))('http://b.example/api/whoami')
    expect(pluginFetch).toHaveBeenCalledTimes(2)
    expect(f.load).not.toHaveBeenCalled()
  })

  it('refuses a pinned hub until the pinned transport ships', () => {
    fakeStore()
    const rt = makeTauriRuntime()
    expect(() => rt.transportFor(hub('https://c.example', { kind: 'pinned', sha256: 'a'.repeat(64) }))).toThrow(
      'pinned transport arrives in a later task',
    )
  })
})
