import { renderHook } from '@testing-library/react'
import { waitFor } from '@testing-library/react'
import { useEventLoop } from './useEventLoop'
import { useAppStore, resetAppStore } from '../store/appStore'
import { ZulipApiError, type ZulipClient } from '../api/client'
import { HubClient } from '../api/hubClient'
import type { Credentials, InitialState } from '../api/types'

const init: InitialState = {
  queueId: 'q1',
  lastEventId: 0,
  subscriptions: [{ stream_id: 1, name: 'alpha', description: 'A' }],
  unread: [],
}

const NEVER = new Promise<never>(() => {}) // parks the loop

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    creds: { prefix: '/zulip/qec', email: 'me@b.c', apiKey: 'k', sendTopic: '' },
  })
})

test('registers, applies initial state, applies polled events, goes live', async () => {
  const client = {
    register: vi.fn(async () => init),
    pollEvents: vi
      .fn()
      .mockResolvedValueOnce([
        { id: 1, type: 'message', message: {
          id: 5, sender_full_name: 'Bot', sender_email: 'bot@b.c',
          timestamp: 1755100000, content: '<p>x</p>', stream_id: 1, subject: '',
        } },
      ])
      .mockReturnValue(NEVER),
  } as unknown as ZulipClient
  useAppStore.setState({ client })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(useAppStore.getState().streams).toHaveLength(1))
  await waitFor(() => expect(useAppStore.getState().unreadByStream[1]).toEqual([5]))
  expect(useAppStore.getState().connection).toBe('live')
  unmount()
})

test('BAD_EVENT_QUEUE_ID triggers transparent re-register', async () => {
  const client = {
    register: vi.fn(async () => init),
    pollEvents: vi
      .fn()
      .mockRejectedValueOnce(new ZulipApiError('Bad event queue ID', 400, 'BAD_EVENT_QUEUE_ID'))
      .mockReturnValue(NEVER),
  } as unknown as ZulipClient
  useAppStore.setState({ client })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(client.register).toHaveBeenCalledTimes(2))
  unmount()
})

test('401 logs the user out', async () => {
  const client = {
    register: vi.fn(async () => {
      throw new ZulipApiError('Invalid API key', 401)
    }),
    pollEvents: vi.fn(),
  } as unknown as ZulipClient
  useAppStore.setState({ client })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
  unmount()
})

test('network errors set connection offline and back off', async () => {
  vi.useFakeTimers()
  const client = {
    register: vi.fn(async () => init),
    pollEvents: vi
      .fn()
      .mockRejectedValueOnce(new TypeError('Failed to fetch'))
      .mockResolvedValueOnce([]) // recovery poll after backoff
      .mockReturnValue(NEVER),
  } as unknown as ZulipClient
  useAppStore.setState({ client })
  const { unmount } = renderHook(() => useEventLoop())
  await vi.waitFor(() => expect(useAppStore.getState().connection).toBe('offline'))
  await vi.advanceTimersByTimeAsync(1000)
  await vi.waitFor(() => expect(useAppStore.getState().connection).toBe('live'))
  unmount()
  vi.useRealTimers()
})

describe('hub mode', () => {
  const hubCreds: Credentials = {
    kind: 'hub', prefix: '', email: 'Alice', apiKey: 'tok', sendTopic: '',
  }

  /** A live SSE response: chunks are delivered, then the stream stays open the
   * way a real connection does (closing it would make the loop re-register). */
  function liveStream(chunks: string[]): Response {
    const stream = new ReadableStream({
      start(controller) {
        for (const c of chunks) controller.enqueue(new TextEncoder().encode(c))
      },
    })
    return new Response(stream, { status: 200 })
  }

  afterEach(() => vi.unstubAllGlobals())

  test('registers chambers and applies SSE message events', async () => {
    const payload = JSON.stringify({
      chamber_id: 'cham-a', direction: 'outbox', from: 'agent', subject: '',
      body: 'done', timestamp: '2026-08-15T10:00:00', is_question: false,
    })
    const fetchMock = vi.fn(async (url: string) =>
      String(url).includes('/api/events')
        ? liveStream([`event: message\ndata: ${payload}\n\n`])
        : new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 }),
    )
    vi.stubGlobal('fetch', fetchMock)
    useAppStore.setState({ creds: hubCreds, client: new HubClient(hubCreds) })
    const { unmount } = renderHook(() => useEventLoop())
    await waitFor(() => expect(useAppStore.getState().streams).toHaveLength(1))
    const streamId = useAppStore.getState().streams[0].stream_id
    await waitFor(() =>
      expect(useAppStore.getState().messagesByStream[streamId]?.[0]?.content).toBe('done'),
    )
    expect(useAppStore.getState().connection).toBe('live')
    // The token rides in the header, never in the events URL.
    const eventsCall = fetchMock.mock.calls.find(([u]) => String(u).includes('/api/events'))!
    expect(String(eventsCall[0])).toBe('/api/events')
    unmount()
  })

  test('401 on the hub register logs the user out', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
    useAppStore.setState({ creds: hubCreds, client: new HubClient(hubCreds) })
    const { unmount } = renderHook(() => useEventLoop())
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    unmount()
  })

  test('an index event re-registers so a changed chamber scope is picked up', async () => {
    let registers = 0
    const fetchMock = vi.fn(async (url: string) => {
      if (String(url).includes('/api/events')) {
        // First connection nudges us to re-register; the second stays open.
        return registers === 1
          ? liveStream(['event: index\ndata: changed\n\n'])
          : liveStream([])
      }
      registers += 1
      return new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)
    useAppStore.setState({ creds: hubCreds, client: new HubClient(hubCreds) })
    const { unmount } = renderHook(() => useEventLoop())
    await waitFor(() => expect(registers).toBe(2))
    unmount()
  })

  test('an immediately-closed stream backs off instead of spinning', async () => {
    let registers = 0
    const fetchMock = vi.fn(async (url: string) => {
      if (String(url).includes('/api/events')) {
        const stream = new ReadableStream({ start: (c) => c.close() })
        return new Response(stream, { status: 200 })
      }
      registers += 1
      return new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)
    useAppStore.setState({ creds: hubCreds, client: new HubClient(hubCreds) })
    const { unmount } = renderHook(() => useEventLoop())
    await waitFor(() => expect(registers).toBe(1))
    // Well inside the 1s backoff: a spinning loop would have re-registered many
    // times by now.
    await new Promise((resolve) => setTimeout(resolve, 50))
    expect(registers).toBe(1)
    unmount()
  })

  test('the reconnect gap is reported, not hidden behind a live banner', async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (String(url).includes('/api/events')) {
        return new Response(new ReadableStream({ start: (c) => c.close() }), { status: 200 })
      }
      return new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)
    useAppStore.setState({ creds: hubCreds, client: new HubClient(hubCreds) })
    const { unmount } = renderHook(() => useEventLoop())
    // A clean EOF used to sleep with the connection still marked 'live', so the
    // user saw no reconnecting banner during the gap.
    await waitFor(() => expect(useAppStore.getState().connection).toBe('offline'))
    unmount()
  })

  test('repeated SSE failures grow the wait instead of retrying at one second', async () => {
    vi.useFakeTimers()
    try {
      // Backoff sleeps only: the 10s SSE health timer is not one of them.
      const sleeps: number[] = []
      const realTimeout = globalThis.setTimeout
      vi.spyOn(globalThis, 'setTimeout').mockImplementation(((fn: () => void, ms?: number) => {
        if (ms !== undefined && ms >= 1000 && ms < 10_000) sleeps.push(ms)
        return realTimeout(fn, ms)
      }) as typeof setTimeout)

      const fetchMock = vi.fn(async (url: string) => {
        // Every stream dies the instant it opens: the connection never proves
        // healthy, so the backoff must keep widening.
        if (String(url).includes('/api/events')) {
          return new Response(new ReadableStream({ start: (c) => c.close() }), { status: 200 })
        }
        return new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 })
      })
      vi.stubGlobal('fetch', fetchMock)
      useAppStore.setState({ creds: hubCreds, client: new HubClient(hubCreds) })
      const { unmount } = renderHook(() => useEventLoop())

      for (const expected of [1000, 2000, 4000]) {
        await vi.waitFor(() => expect(sleeps.at(-1)).toBe(expected))
        await vi.advanceTimersByTimeAsync(expected)
      }
      expect(sleeps.slice(0, 3)).toEqual([1000, 2000, 4000])
      unmount()
    } finally {
      vi.restoreAllMocks()
      vi.useRealTimers()
    }
  })

  test('a stream that delivers an event resets the wait to its floor', async () => {
    vi.useFakeTimers()
    try {
      const sleeps: number[] = []
      const realTimeout = globalThis.setTimeout
      vi.spyOn(globalThis, 'setTimeout').mockImplementation(((fn: () => void, ms?: number) => {
        if (ms !== undefined && ms >= 1000 && ms < 10_000) sleeps.push(ms)
        return realTimeout(fn, ms)
      }) as typeof setTimeout)

      const payload = JSON.stringify({
        chamber_id: 'cham-a', from: 'agent', subject: '', body: 'done',
        timestamp: '2026-08-15T10:00:00', is_question: false,
      })
      let opens = 0
      const fetchMock = vi.fn(async (url: string) => {
        if (String(url).includes('/api/events')) {
          opens += 1
          // First two die empty (backoff climbs), the third delivers.
          const body =
            opens >= 3
              ? new ReadableStream({
                  start(c) {
                    c.enqueue(new TextEncoder().encode(`event: message\ndata: ${payload}\n\n`))
                    c.close()
                  },
                })
              : new ReadableStream({ start: (c) => c.close() })
          return new Response(body, { status: 200 })
        }
        return new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 })
      })
      vi.stubGlobal('fetch', fetchMock)
      useAppStore.setState({ creds: hubCreds, client: new HubClient(hubCreds) })
      const { unmount } = renderHook(() => useEventLoop())

      for (const expected of [1000, 2000]) {
        await vi.waitFor(() => expect(sleeps.at(-1)).toBe(expected))
        await vi.advanceTimersByTimeAsync(expected)
      }
      // The third connection actually carried a message, which is the proof the
      // reset waits for.
      await vi.waitFor(() => expect(sleeps.length).toBe(3))
      expect(sleeps[2]).toBe(1000)
      unmount()
    } finally {
      vi.restoreAllMocks()
      vi.useRealTimers()
    }
  })
})
