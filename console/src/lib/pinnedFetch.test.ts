import { describe, it, expect, afterEach, vi } from 'vitest'
import { b64decode, b64encode, formDataToMultipart, pinnedFetch } from './pinnedFetch'
import type { HubAccount } from '../store/hubs'

const SHA = 'ab'.repeat(32)

function pinnedHub(): HubAccount {
  return {
    id: 'hub-1',
    url: 'https://hub.example',
    label: 'hub',
    token: 'deadbeef',
    name: 'human',
    role: 'owner',
    trust: { kind: 'pinned', sha256: SHA },
  }
}

/** The shell's `Channel`, as the console uses it: constructed with no
 * arguments, handed to a command, and driven through `onmessage`. */
class FakeChannel {
  onmessage: (msg: unknown) => void = () => {}
}

/** Installs a fake Tauri global and hands back the invoke spy. Every test
 * drives the real `tauri.ts` accessors through it, so the Channel plumbing is
 * exercised rather than stubbed out. */
function fakeShell() {
  const invoke = vi.fn()
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  ;(window as any).__TAURI__ = { core: { invoke, Channel: FakeChannel } }
  return invoke
}

/** The arguments of the one `pinned_fetch`/`pinned_sse` call made so far. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function callArgs(invoke: any, cmd: string): Record<string, any> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const call = invoke.mock.calls.find((c: any[]) => c[0] === cmd)
  if (!call) throw new Error(`no ${cmd} call: ${JSON.stringify(invoke.mock.calls.map((c: unknown[]) => c[0]))}`)
  return call[1]
}

function utf8(s: string): Uint8Array {
  return new TextEncoder().encode(s)
}

afterEach(() => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  delete (window as any).__TAURI__
  vi.restoreAllMocks()
})

describe('base64 helpers', () => {
  it('round-trips every byte value', () => {
    const all = new Uint8Array(256).map((_, i) => i)
    expect(Array.from(b64decode(b64encode(all)))).toEqual(Array.from(all))
  })

  it('round-trips each padding length', () => {
    for (const n of [0, 1, 2, 3, 4, 5]) {
      const bytes = new Uint8Array(n).map((_, i) => (i * 37) & 0xff)
      expect(Array.from(b64decode(b64encode(bytes)))).toEqual(Array.from(bytes))
    }
  })

  it('agrees with the platform encoder', () => {
    const bytes = utf8('hello, chamber — 你好')
    const platform = btoa(String.fromCharCode(...bytes))
    expect(b64encode(bytes)).toBe(platform)
  })

  it('survives a payload far past the argument-list limit', () => {
    // A spread-based encoder (`String.fromCharCode(...bytes)`) overflows the
    // call stack here; an upload of this size is entirely ordinary.
    const big = new Uint8Array(300_000).map((_, i) => (i * 31) & 0xff)
    const round = b64decode(b64encode(big))
    expect(round.length).toBe(big.length)
    expect(round[299_999]).toBe(big[299_999])
  })
})

describe('formDataToMultipart', () => {
  it('names its boundary in the content type and disposes every part', async () => {
    const fd = new FormData()
    fd.append('file', new File([new Uint8Array([1, 2, 3, 4])], 'shot.png', { type: 'image/png' }))
    fd.append('note', 'hello')
    const { bytes, contentType } = await formDataToMultipart(fd)

    const boundary = /boundary=(.+)$/.exec(contentType)?.[1]
    expect(contentType.startsWith('multipart/form-data; boundary=')).toBe(true)
    expect(boundary).toBeTruthy()

    const text = new TextDecoder().decode(bytes)
    expect(text).toContain(`--${boundary}\r\n`)
    expect(text).toContain('Content-Disposition: form-data; name="file"; filename="shot.png"')
    expect(text).toContain('Content-Type: image/png')
    expect(text).toContain('Content-Disposition: form-data; name="note"')
    expect(text).toContain('hello')
    expect(text.endsWith(`--${boundary}--\r\n`)).toBe(true)
    // The file's bytes ride through untouched.
    expect(Array.from(bytes).join(',')).toContain('1,2,3,4')
  })

  it('cannot be talked into injecting a header line', async () => {
    const fd = new FormData()
    fd.append('file', new File([new Uint8Array([9])], 'a"\r\nX-Evil: 1.png', { type: 'image/png' }))
    const { bytes } = await formDataToMultipart(fd)
    const text = new TextDecoder().decode(bytes)
    // The quote and the line break are encoded, so the name stays one quoted
    // string on one header line — `X-Evil` never starts a header of its own.
    expect(text).not.toContain('\r\nX-Evil: 1')
    expect(text).toContain('filename="a%22%0D%0AX-Evil: 1.png"')
  })
})

describe('pinnedFetch — buffered requests', () => {
  it('round-trips a JSON POST through pinned_fetch', async () => {
    const invoke = fakeShell()
    invoke.mockResolvedValueOnce({
      status: 200,
      headers: [['content-type', 'application/json']],
      body_b64: b64encode(utf8('{"id":"inbox/1.md"}')),
    })

    const res = await pinnedFetch(pinnedHub())('https://hub.example/api/chambers/a/send', {
      method: 'POST',
      headers: { Authorization: 'Bearer deadbeef', 'Content-Type': 'application/json' },
      body: JSON.stringify({ body: 'hi' }),
    })

    const req = callArgs(invoke, 'pinned_fetch').req
    expect(req.url).toBe('https://hub.example/api/chambers/a/send')
    expect(req.method).toBe('POST')
    expect(req.sha256).toBe(SHA)
    expect(new TextDecoder().decode(b64decode(req.body_b64))).toBe('{"body":"hi"}')
    const sent = new Map<string, string>(req.headers.map((h: [string, string]) => [h[0].toLowerCase(), h[1]]))
    expect(sent.get('authorization')).toBe('Bearer deadbeef')
    expect(sent.get('content-type')).toBe('application/json')

    expect(res.status).toBe(200)
    expect(res.ok).toBe(true)
    expect(res.headers.get('content-type')).toBe('application/json')
    await expect(res.json()).resolves.toEqual({ id: 'inbox/1.md' })
  })

  it('sends a GET with no body and reads a blob back', async () => {
    const invoke = fakeShell()
    invoke.mockResolvedValueOnce({
      status: 200,
      headers: [['content-type', 'image/png']],
      body_b64: b64encode(new Uint8Array([137, 80, 78, 71])),
    })
    const res = await pinnedFetch(pinnedHub())('https://hub.example/api/chambers/a/files/x.png')
    const req = callArgs(invoke, 'pinned_fetch').req
    expect(req.method).toBe('GET')
    expect(req.body_b64).toBeNull()
    // A hub image comes back through `fetchBlob`, so the blob path is the one
    // that matters; the bytes are checked on a clone because this realm's
    // `Blob` cannot read itself back.
    const blob = await res.clone().blob()
    expect(blob.size).toBe(4)
    expect(new Uint8Array(await res.arrayBuffer())[0]).toBe(137)
  })

  it('serializes a FormData upload as multipart and says so in the header', async () => {
    const invoke = fakeShell()
    invoke.mockResolvedValueOnce({ status: 200, headers: [], body_b64: b64encode(utf8('{}')) })
    const fd = new FormData()
    fd.append('file', new File([new Uint8Array([7, 7])], 'up.bin', { type: 'application/octet-stream' }))
    await pinnedFetch(pinnedHub())('https://hub.example/api/chambers/a/uploads', {
      method: 'POST',
      body: fd,
    })
    const req = callArgs(invoke, 'pinned_fetch').req
    const ct = req.headers.find((h: [string, string]) => h[0].toLowerCase() === 'content-type')?.[1]
    expect(ct).toMatch(/^multipart\/form-data; boundary=/)
    const boundary = /boundary=(.+)$/.exec(ct)![1]
    const body = new TextDecoder().decode(b64decode(req.body_b64))
    expect(body).toContain(`--${boundary}`)
    expect(body).toContain('Content-Disposition: form-data; name="file"; filename="up.bin"')
  })

  it('keeps a 204 body-less rather than throwing on the Response constructor', async () => {
    const invoke = fakeShell()
    invoke.mockResolvedValueOnce({ status: 204, headers: [], body_b64: '' })
    const res = await pinnedFetch(pinnedHub())('https://hub.example/api/chambers/a/stop', {
      method: 'POST',
    })
    expect(res.status).toBe(204)
    expect(res.body).toBeNull()
  })

  it('refuses to carry a hub that was never pinned', async () => {
    const invoke = fakeShell()
    const transport = pinnedFetch({ trust: { kind: 'https' } })
    await expect(transport('https://hub.example/api/whoami')).rejects.toThrow('no pinned certificate')
    expect(invoke).not.toHaveBeenCalled()
  })

  it('reports a refused certificate as a rejected fetch', async () => {
    const invoke = fakeShell()
    invoke.mockRejectedValueOnce('pinned fingerprint mismatch')
    await expect(
      pinnedFetch(pinnedHub())('https://hub.example/api/whoami'),
    ).rejects.toThrow('pinned fingerprint mismatch')
  })
})

describe('pinnedFetch — the events stream', () => {
  /** Opens `/api/events` and hands back the pieces a test drives it with. */
  function openStream(signal?: AbortSignal) {
    const invoke = fakeShell()
    let settle: (v: unknown) => void = () => {}
    const command = new Promise((resolve) => {
      settle = resolve
    })
    invoke.mockImplementation((cmd: string) => (cmd === 'pinned_sse' ? command : Promise.resolve()))
    const res = pinnedFetch(pinnedHub())('https://hub.example/api/events', {
      headers: { Authorization: 'Bearer deadbeef' },
      signal,
    })
    const args = callArgs(invoke, 'pinned_sse')
    return { invoke, res, args, channel: args.onEvent as FakeChannel, settle }
  }

  it('opens a streaming Response and yields chunks in order', async () => {
    const s = openStream()
    expect(s.args.url).toBe('https://hub.example/api/events')
    expect(s.args.sha256).toBe(SHA)
    expect(typeof s.args.streamId).toBe('number')
    expect(s.args.headers).toEqual([['Authorization', 'Bearer deadbeef']])

    s.channel.onmessage({ status: 200 })
    const res = await s.res
    expect(res.ok).toBe(true)
    expect(res.body).toBeTruthy()

    const reader = res.body!.getReader()
    s.channel.onmessage({ chunk_b64: b64encode(utf8('event: message\n')) })
    s.channel.onmessage({ chunk_b64: b64encode(utf8('data: one\n\n')) })
    const decoder = new TextDecoder()
    expect(decoder.decode((await reader.read()).value)).toBe('event: message\n')
    expect(decoder.decode((await reader.read()).value)).toBe('data: one\n\n')

    s.channel.onmessage({ done: true })
    expect((await reader.read()).done).toBe(true)
  })

  it('cancels the host stream with the same id when the caller aborts', async () => {
    const ctrl = new AbortController()
    const s = openStream(ctrl.signal)
    s.channel.onmessage({ status: 200 })
    const res = await s.res
    const reader = res.body!.getReader()
    const pending = reader.read()

    ctrl.abort()
    expect(s.invoke).toHaveBeenCalledWith('pinned_sse_cancel', { streamId: s.args.streamId })
    await expect(pending).rejects.toBeTruthy()
  })

  it('cancels the host stream when the reader is released', async () => {
    const s = openStream()
    s.channel.onmessage({ status: 200 })
    const res = await s.res
    await res.body!.cancel()
    expect(s.invoke).toHaveBeenCalledWith('pinned_sse_cancel', { streamId: s.args.streamId })
  })

  it('errors the stream when the command fails mid-flight', async () => {
    const s = openStream()
    s.channel.onmessage({ status: 200 })
    const res = await s.res
    const reader = res.body!.getReader()
    const pending = reader.read()
    s.settle(Promise.reject(new Error('connection reset')))
    await expect(pending).rejects.toThrow('connection reset')
  })

  it('rejects the fetch when the connection fails before it opens', async () => {
    const s = openStream()
    s.settle(Promise.reject(new Error('pinned fingerprint mismatch')))
    await expect(s.res).rejects.toThrow('pinned fingerprint mismatch')
  })

  it('refuses to start when the signal is already aborted', async () => {
    const invoke = fakeShell()
    const ctrl = new AbortController()
    ctrl.abort()
    await expect(
      pinnedFetch(pinnedHub())('https://hub.example/api/events', { signal: ctrl.signal }),
    ).rejects.toBeTruthy()
    expect(invoke).not.toHaveBeenCalled()
  })
})
