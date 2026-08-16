import { readSse } from './sse'

function streamResponse(chunks: string[]): Response {
  const stream = new ReadableStream({
    start(controller) {
      for (const c of chunks) controller.enqueue(new TextEncoder().encode(c))
      controller.close()
    },
  })
  return new Response(stream, { status: 200 })
}

afterEach(() => vi.unstubAllGlobals())

test('parses events split across chunks', async () => {
  vi.stubGlobal('fetch', vi.fn(async () =>
    streamResponse(['event: message\ndata: {"a"', ':1}\n\nevent: index\ndata: changed\n\n']),
  ))
  const events: Array<[string, string]> = []
  await readSse('/api/events', { Authorization: 'Bearer t' }, (e, d) => events.push([e, d]), new AbortController().signal)
  expect(events).toEqual([['message', '{"a":1}'], ['index', 'changed']])
})

test('non-2xx rejects with the status', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
  await expect(
    readSse('/api/events', {}, () => {}, new AbortController().signal),
  ).rejects.toMatchObject({ status: 401 })
})

test('joins multi-line data, tolerates CRLF, and ignores comment lines', async () => {
  vi.stubGlobal('fetch', vi.fn(async () =>
    streamResponse([': keepalive\r\n\r\nevent: log\r\ndata: one\r\ndata: two\r\n\r\n']),
  ))
  const events: Array<[string, string]> = []
  await readSse('/api/events', {}, (e, d) => events.push([e, d]), new AbortController().signal)
  expect(events).toEqual([['log', 'one\ntwo']])
})

test('sends the auth header and the abort signal to fetch', async () => {
  const signal = new AbortController().signal
  const fetchMock = vi.fn(async () => streamResponse([]))
  vi.stubGlobal('fetch', fetchMock)
  await readSse('/api/events', { Authorization: 'Bearer t' }, () => {}, signal)
  expect(fetchMock).toHaveBeenCalledWith('/api/events', {
    headers: { Authorization: 'Bearer t' },
    signal,
  })
})
