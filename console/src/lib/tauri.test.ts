import { describe, it, expect, afterEach, vi } from 'vitest'
import { tauriChannel, tauriFetch, tauriLoadStore, tauriInvoke } from './tauri'

afterEach(() => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  delete (window as any).__TAURI__
})

describe('tauri global access', () => {
  it('throws a clear error outside the shell', () => {
    expect(() => tauriFetch()).toThrow('Tauri runtime not available')
  })

  it('returns the plugin fetch bound safely', async () => {
    const fetchSpy = vi.fn(async () => new Response('{}'))
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ;(window as any).__TAURI__ = { http: { fetch: fetchSpy }, core: { invoke: vi.fn() } }
    await tauriFetch()('http://x/api/whoami')
    expect(fetchSpy).toHaveBeenCalledOnce()
  })

  it('invoke delegates to core.invoke', async () => {
    const invoke = vi.fn(async () => 42)
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ;(window as any).__TAURI__ = { core: { invoke } }
    await expect(tauriInvoke<number>('probe_hub', { url: 'http://x' })).resolves.toBe(42)
    expect(invoke).toHaveBeenCalledWith('probe_hub', { url: 'http://x' })
  })

  it('builds a channel with the handler already attached', () => {
    class FakeChannel {
      onmessage: ((msg: unknown) => void) | undefined
    }
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ;(window as any).__TAURI__ = { core: { invoke: vi.fn(), Channel: FakeChannel } }
    const seen: unknown[] = []
    const ch = tauriChannel<{ n: number }>((msg) => seen.push(msg))
    expect(ch).toBeInstanceOf(FakeChannel)
    // The handler must be in place before the channel is ever handed to a
    // command: a message that arrives first would otherwise be dropped.
    ch.onmessage?.({ n: 1 })
    expect(seen).toEqual([{ n: 1 }])
  })

  it('says the runtime is missing when the global has no Channel', () => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ;(window as any).__TAURI__ = { core: { invoke: vi.fn() } }
    expect(() => tauriChannel(() => {})).toThrow('Tauri runtime not available')
  })

  it('loads a store through the store plugin global', async () => {
    const store = { get: vi.fn(), set: vi.fn(), save: vi.fn() }
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ;(window as any).__TAURI__ = { store: { load: vi.fn(async () => store) }, core: { invoke: vi.fn() } }
    await expect(tauriLoadStore('hubs.json')).resolves.toBe(store)
  })
})
