import { readSse, isSseStall } from './sse'

const enc = (s: string) => new TextEncoder().encode(s)

function streamResponse(chunks: string[]): Response {
  const stream = new ReadableStream({
    start(controller) {
      for (const c of chunks) controller.enqueue(enc(c))
      controller.close()
    },
  })
  return new Response(stream, { status: 200 })
}

const noAbort = () => new AbortController().signal

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

test('parses events split across chunks', async () => {
  vi.stubGlobal('fetch', vi.fn(async () =>
    streamResponse(['event: message\ndata: {"a"', ':1}\n\nevent: index\ndata: changed\n\n']),
  ))
  const events: Array<[string, string]> = []
  await readSse('/api/events', {
    signal: noAbort(), headers: { Authorization: 'Bearer t' },
    onEvent: (e, d) => events.push([e, d]),
  })
  expect(events).toEqual([['message', '{"a":1}'], ['index', 'changed']])
})

test('non-2xx rejects with the status', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
  await expect(
    readSse('/api/events', { signal: noAbort(), headers: {}, onEvent: () => {} }),
  ).rejects.toMatchObject({ status: 401 })
})

test('joins multi-line data, tolerates CRLF, and ignores comment lines', async () => {
  vi.stubGlobal('fetch', vi.fn(async () =>
    streamResponse([': keepalive\r\n\r\nevent: log\r\ndata: one\r\ndata: two\r\n\r\n']),
  ))
  const events: Array<[string, string]> = []
  await readSse('/api/events', { signal: noAbort(), headers: {}, onEvent: (e, d) => events.push([e, d]) })
  expect(events).toEqual([['log', 'one\ntwo']])
})

test('sends the auth header; the caller abort reaches the fetch and is not a stall', async () => {
  const ac = new AbortController()
  const fetchMock = vi.fn(async (_url: string, init: RequestInit) => {
    const body = new ReadableStream({
      start(c) {
        // A real fetch rejects the pending read when its signal aborts.
        init.signal!.addEventListener('abort', () =>
          c.error(new DOMException('The operation was aborted.', 'AbortError')),
        )
      },
    })
    return new Response(body, { status: 200 })
  })
  vi.stubGlobal('fetch', fetchMock)
  const p = readSse('/api/events', {
    signal: ac.signal, headers: { Authorization: 'Bearer t' }, onEvent: () => {},
  })
  await vi.waitFor(() => expect(fetchMock).toHaveBeenCalled())
  const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
  expect(url).toBe('/api/events')
  expect(init.headers).toEqual({ Authorization: 'Bearer t' })
  expect(init.signal!.aborted).toBe(false)
  ac.abort()
  expect(init.signal!.aborted).toBe(true)
  const err = await p.then(() => null, (e: unknown) => e)
  expect(err).toBeInstanceOf(DOMException)
  expect(isSseStall(err)).toBe(false)
})

test('uses an injected fetch when given one, leaving the global untouched', async () => {
  // A HubClient built with an injected fetch must reach the hub through it
  // here too, or the stream is the single call that escapes to the global.
  const globalFetch = vi.fn(async () => streamResponse([]))
  vi.stubGlobal('fetch', globalFetch)
  const injected = vi.fn(async () => streamResponse(['event: log\ndata: hi\n\n']))
  const events: Array<[string, string]> = []
  await readSse('/api/events', {
    signal: noAbort(),
    headers: { Authorization: 'Bearer t' },
    onEvent: (e, d) => events.push([e, d]),
    fetch: injected as unknown as typeof fetch,
  })
  expect(events).toEqual([['log', 'hi']])
  expect(injected).toHaveBeenCalledTimes(1)
  expect(globalFetch).not.toHaveBeenCalled()
})

test('a stream that goes silent past stallMs rejects with "sse stalled" and aborts the fetch', async () => {
  vi.useFakeTimers()
  let aborted = false
  vi.stubGlobal('fetch', vi.fn(async (_url: string, init: RequestInit) => {
    init.signal!.addEventListener('abort', () => { aborted = true })
    // One keepalive, then silence — the connection never closes on its own.
    const body = new ReadableStream({
      start(c) {
        c.enqueue(enc(': keepalive\n'))
        init.signal!.addEventListener('abort', () =>
          c.error(new DOMException('The operation was aborted.', 'AbortError')),
        )
      },
    })
    return new Response(body, { status: 200 })
  }))
  const outcome = readSse('/api/events', {
    signal: noAbort(), headers: {}, stallMs: 1000, onEvent: () => {},
  }).then(() => 'resolved', (e: Error) => e.message)
  await vi.advanceTimersByTimeAsync(999)
  expect(aborted).toBe(false)
  await vi.advanceTimersByTimeAsync(2)
  expect(aborted).toBe(true)
  await expect(outcome).resolves.toBe('sse stalled')
})

test('keepalive comments reset the stall watchdog', async () => {
  vi.useFakeTimers()
  let controller!: ReadableStreamDefaultController<Uint8Array>
  vi.stubGlobal('fetch', vi.fn(async () =>
    new Response(new ReadableStream({ start(c) { controller = c } }), { status: 200 }),
  ))
  const events: string[] = []
  const p = readSse('/api/events', {
    signal: noAbort(), headers: {}, stallMs: 1000, onEvent: (e) => events.push(e),
  })
  let settled = false
  p.then(() => { settled = true }, () => { settled = true })
  for (let i = 0; i < 3; i += 1) {
    await vi.advanceTimersByTimeAsync(800)
    controller.enqueue(enc(': keepalive\n'))
  }
  // 2.4 s elapsed with a byte every 0.8 s: never silent for a full second.
  expect(settled).toBe(false)
  controller.enqueue(enc('event: index\ndata: changed\n\n'))
  controller.close()
  await vi.advanceTimersByTimeAsync(0)
  await p
  expect(events).toEqual(['index'])
})

test('a hung connect (no response at all) also trips the watchdog', async () => {
  vi.useFakeTimers()
  vi.stubGlobal('fetch', vi.fn((_url: string, init: RequestInit) =>
    new Promise<Response>((_, reject) => {
      init.signal!.addEventListener('abort', () =>
        reject(new DOMException('The operation was aborted.', 'AbortError')),
      )
    }),
  ))
  const outcome = readSse('/api/events', {
    signal: noAbort(), headers: {}, stallMs: 1000, onEvent: () => {},
  }).then(() => 'resolved', (e: Error) => e.message)
  await vi.advanceTimersByTimeAsync(1001)
  await expect(outcome).resolves.toBe('sse stalled')
})
