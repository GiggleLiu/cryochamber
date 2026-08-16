import { renderHook } from '@testing-library/react'
import { waitFor } from '@testing-library/react'
import { useEventLoop, sleep } from './useEventLoop'
import { useAppStore, resetAppStore } from '../store/appStore'
import { HubClient } from '../api/hubClient'
import type { Credentials } from '../api/types'

const creds: Credentials = { token: 'tok', name: 'Alice', role: 'owner' }

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

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({ creds })
})

afterEach(() => vi.unstubAllGlobals())

test('lists chambers and applies SSE message events under the hub id', async () => {
  const payload = JSON.stringify({
    id: 'outbox/1.md', chamber_id: 'cham-a', direction: 'outbox', from: 'agent', subject: '',
    body: 'done', timestamp: '2026-08-15T10:00:00', is_question: false,
  })
  const fetchMock = vi.fn(async (url: string) =>
    String(url).includes('/api/events')
      ? liveStream([`event: message\ndata: ${payload}\n\n`])
      : new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 }),
  )
  vi.stubGlobal('fetch', fetchMock)
  useAppStore.setState({ client: new HubClient({ token: creds.token }) })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(useAppStore.getState().chambers).toHaveLength(1))
  expect(useAppStore.getState().chambers[0].id).toBe('cham-a')
  await waitFor(() =>
    expect(useAppStore.getState().messagesByChamber['cham-a']?.[0]?.body).toBe('done'),
  )
  expect(useAppStore.getState().messagesByChamber['cham-a'][0].id).toBe('outbox/1.md')
  expect(useAppStore.getState().connection).toBe('live')
  // The token rides in the header, never in the events URL.
  const eventsCall = fetchMock.mock.calls.find(([u]) => String(u).includes('/api/events'))!
  expect(String(eventsCall[0])).toBe('/api/events')
  unmount()
})

test('a message for a chamber outside our scope is dropped', async () => {
  const payload = JSON.stringify({
    id: 'outbox/1.md', chamber_id: 'cham-zzz', direction: 'outbox', from: 'agent', subject: '',
    body: 'not ours', timestamp: '2026-08-15T10:00:00', is_question: false,
  })
  const fetchMock = vi.fn(async (url: string) =>
    String(url).includes('/api/events')
      ? liveStream([`event: message\ndata: ${payload}\n\n`])
      : new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 }),
  )
  vi.stubGlobal('fetch', fetchMock)
  useAppStore.setState({ client: new HubClient({ token: creds.token }) })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(useAppStore.getState().connection).toBe('live'))
  await new Promise((r) => setTimeout(r, 20))
  expect(useAppStore.getState().messagesByChamber).toEqual({})
  unmount()
})

test('a status event refreshes chamber liveness into the store', async () => {
  let indexReads = 0
  const fetchMock = vi.fn(async (url: string) => {
    if (String(url).includes('/api/events')) {
      return liveStream(['event: status\ndata: {"chamber_id":"cham-a"}\n\n'])
    }
    indexReads += 1
    // Awake at register; asleep by the time the status event lands.
    return new Response(
      JSON.stringify([
        { id: 'cham-a', name: 'alpha', running: true, agent_running: indexReads === 1, next_wake_display: 'in 2 h' },
      ]),
      { status: 200 },
    )
  })
  vi.stubGlobal('fetch', fetchMock)
  useAppStore.setState({ client: new HubClient({ token: creds.token }) })
  const { unmount } = renderHook(() => useEventLoop())
  // The banner would otherwise keep claiming the agent is awake until the next
  // register, which can be minutes away.
  await waitFor(() => expect(useAppStore.getState().chambers[0]?.agentRunning).toBe(false))
  expect(useAppStore.getState().chambers[0]).toMatchObject({
    name: 'alpha',
    nextWakeDisplay: 'in 2 h',
  })
  expect(indexReads).toBeGreaterThan(1)
  unmount()
})

test('a failed status refresh is swallowed and leaves the loop running', async () => {
  const fetchMock = vi.fn(async (url: string) => {
    if (String(url).includes('/api/events')) {
      return liveStream(['event: status\ndata: {"chamber_id":"cham-a"}\n\n'])
    }
    // Register succeeds once; the status refresh behind it fails.
    return fetchMock.mock.calls.filter(([u]) => !String(u).includes('/api/events')).length > 1
      ? new Response('', { status: 500 })
      : new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha', running: true, agent_running: true }]), { status: 200 })
  })
  vi.stubGlobal('fetch', fetchMock)
  useAppStore.setState({ client: new HubClient({ token: creds.token }) })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(useAppStore.getState().chambers[0]?.agentRunning).toBe(true))
  // The stale value survives, and the connection is not torn down.
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(useAppStore.getState().chambers[0].agentRunning).toBe(true)
  expect(useAppStore.getState().creds).not.toBeNull()
  unmount()
})

test('a 401 on the status refresh reaches the client\'s logout hook', async () => {
  // The refresh can be the first authenticated call after a revocation —
  // swallowing it like a transient failure would leave a signed-in UI
  // behind a dead token.
  const onAuthFailure = vi.fn()
  const fetchMock = vi.fn(async (url: string) => {
    if (String(url).includes('/api/events')) {
      return liveStream(['event: status\ndata: {"chamber_id":"cham-a"}\n\n'])
    }
    return fetchMock.mock.calls.filter(([u]) => !String(u).includes('/api/events')).length > 1
      ? new Response('', { status: 401 })
      : new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha', running: true, agent_running: true }]), { status: 200 })
  })
  vi.stubGlobal('fetch', fetchMock)
  useAppStore.setState({ client: new HubClient({ token: creds.token, onAuthFailure }) })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(onAuthFailure).toHaveBeenCalledTimes(1))
  unmount()
})

test('a 401 on the chamber index stops the loop instead of retrying a dead token', async () => {
  const onAuthFailure = vi.fn()
  const fetchMock = vi.fn(async () => new Response('', { status: 401 }))
  vi.stubGlobal('fetch', fetchMock)
  useAppStore.setState({ client: new HubClient({ token: creds.token, onAuthFailure }) })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(onAuthFailure).toHaveBeenCalledTimes(1))
  const attempts = fetchMock.mock.calls.length
  await new Promise((r) => setTimeout(r, 50))
  expect(fetchMock.mock.calls.length).toBe(attempts)
  unmount()
})

test('a 401 on the event stream ends the loop; signing out is the client\'s job', async () => {
  // The loop must not own logout — it stops, and the hook the client already
  // ran is what clears the session. A loop that retried would hammer a dead
  // token; a loop that logged out would double up with the client.
  const onAuthFailure = vi.fn()
  const logout = vi.spyOn(useAppStore.getState(), 'logout')
  let streams = 0
  const fetchMock = vi.fn(async (url: string) => {
    if (String(url).includes('/api/events')) {
      streams += 1
      return new Response('', { status: 401 })
    }
    return new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 })
  })
  vi.stubGlobal('fetch', fetchMock)
  useAppStore.setState({ client: new HubClient({ token: creds.token, onAuthFailure }) })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(onAuthFailure).toHaveBeenCalledTimes(1))
  await new Promise((r) => setTimeout(r, 50))
  expect(streams).toBe(1)
  expect(logout).not.toHaveBeenCalled()
  logout.mockRestore()
  unmount()
})

test.each(['index', 'resync'])(
  "a %s event re-reads the index so a changed chamber scope is picked up",
  async (event) => {
    let reads = 0
    const fetchMock = vi.fn(async (url: string) => {
      if (String(url).includes('/api/events')) {
        // First connection nudges us to re-read; the second stays open.
        return reads === 1 ? liveStream([`event: ${event}\ndata: changed\n\n`]) : liveStream([])
      }
      reads += 1
      return new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)
    useAppStore.setState({ client: new HubClient({ token: creds.token }) })
    const { unmount } = renderHook(() => useEventLoop())
    await waitFor(() => expect(reads).toBe(2))
    // Re-registering resets the loaded set, which is what makes an open
    // conversation refetch its history.
    expect(useAppStore.getState().loadedChambers).toEqual([])
    unmount()
  },
)

test('an immediately-closed stream backs off instead of spinning', async () => {
  let reads = 0
  const fetchMock = vi.fn(async (url: string) => {
    if (String(url).includes('/api/events')) {
      const stream = new ReadableStream({ start: (c) => c.close() })
      return new Response(stream, { status: 200 })
    }
    reads += 1
    return new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 })
  })
  vi.stubGlobal('fetch', fetchMock)
  useAppStore.setState({ client: new HubClient({ token: creds.token }) })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(reads).toBe(1))
  // Well inside the 1s backoff: a spinning loop would have re-registered many
  // times by now.
  await new Promise((resolve) => setTimeout(resolve, 50))
  expect(reads).toBe(1)
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
  useAppStore.setState({ client: new HubClient({ token: creds.token }) })
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
    useAppStore.setState({ client: new HubClient({ token: creds.token }) })
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
    useAppStore.setState({ client: new HubClient({ token: creds.token }) })
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

test('status and log events reach a per-chamber subscriber', async () => {
  const { subscribeChamberEvents, resetChamberEvents } = await import('../store/chamberEvents')
  resetChamberEvents()
  const heard: unknown[] = []
  subscribeChamberEvents('cham-a', (ev) => heard.push(ev))
  const fetchMock = vi.fn(async (url: string) =>
    String(url).includes('/api/events')
      ? liveStream([
          'event: status\ndata: {"chamber_id":"cham-a"}\n\n',
          'event: log\ndata: {"chamber_id":"cham-a","line":"session 5 started"}\n\n',
        ])
      : new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 }),
  )
  vi.stubGlobal('fetch', fetchMock)
  useAppStore.setState({ client: new HubClient({ token: creds.token }) })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(heard).toHaveLength(2))
  expect(heard[0]).toEqual({ type: 'status', chamberId: 'cham-a' })
  expect(heard[1]).toEqual({ type: 'log', chamberId: 'cham-a', line: 'session 5 started' })
  unmount()
})

test('a malformed log payload is skipped without killing the stream', async () => {
  const { subscribeChamberEvents, resetChamberEvents } = await import('../store/chamberEvents')
  resetChamberEvents()
  const heard: unknown[] = []
  subscribeChamberEvents('cham-a', (ev) => heard.push(ev))
  const fetchMock = vi.fn(async (url: string) =>
    String(url).includes('/api/events')
      ? liveStream([
          'event: log\ndata: not json\n\n',
          'event: log\ndata: {"chamber_id":"cham-a","line":"ok"}\n\n',
        ])
      : new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 }),
  )
  vi.stubGlobal('fetch', fetchMock)
  useAppStore.setState({ client: new HubClient({ token: creds.token }) })
  const { unmount } = renderHook(() => useEventLoop())
  await waitFor(() => expect(heard).toHaveLength(1))
  expect(heard[0]).toEqual({ type: 'log', chamberId: 'cham-a', line: 'ok' })
  unmount()
})

describe('sleep', () => {
  afterEach(() => vi.useRealTimers())

  test('resolves after its delay', async () => {
    vi.useFakeTimers()
    let done = false
    void sleep(500).then(() => { done = true })
    await vi.advanceTimersByTimeAsync(499)
    expect(done).toBe(false)
    await vi.advanceTimersByTimeAsync(1)
    expect(done).toBe(true)
  })

  test('resolves early when the page becomes visible', async () => {
    vi.useFakeTimers()
    let done = false
    void sleep(30_000).then(() => { done = true })
    await vi.advanceTimersByTimeAsync(100)
    expect(done).toBe(false)
    // jsdom's visibilityState is 'visible'; the transition event is what we
    // listen for.
    document.dispatchEvent(new Event('visibilitychange'))
    await vi.advanceTimersByTimeAsync(0)
    expect(done).toBe(true)
  })

  test('ignores a transition to hidden', async () => {
    vi.useFakeTimers()
    const original = Object.getOwnPropertyDescriptor(Document.prototype, 'visibilityState')
    Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true })
    try {
      let done = false
      void sleep(30_000).then(() => { done = true })
      document.dispatchEvent(new Event('visibilitychange'))
      await vi.advanceTimersByTimeAsync(0)
      expect(done).toBe(false)
    } finally {
      delete (document as { visibilityState?: string }).visibilityState
      if (original) Object.defineProperty(Document.prototype, 'visibilityState', original)
    }
  })

  test('resolves at once when the signal is already aborted', async () => {
    // A signal that aborted before the call never fires an 'abort' event, so
    // a listener-only sleep would sit out the whole backoff after teardown.
    vi.useFakeTimers()
    const ac = new AbortController()
    ac.abort()
    let done = false
    void sleep(30_000, ac.signal).then(() => { done = true })
    await vi.advanceTimersByTimeAsync(0)
    expect(done).toBe(true)
  })

  test('resolves when the signal aborts', async () => {
    vi.useFakeTimers()
    const ac = new AbortController()
    let done = false
    void sleep(30_000, ac.signal).then(() => { done = true })
    ac.abort()
    await vi.advanceTimersByTimeAsync(0)
    expect(done).toBe(true)
  })
})

test('a stalled stream reconnects immediately, without a backoff sleep', async () => {
  vi.useFakeTimers()
  try {
    const sleeps: number[] = []
    const realTimeout = globalThis.setTimeout
    vi.spyOn(globalThis, 'setTimeout').mockImplementation(((fn: () => void, ms?: number) => {
      if (ms !== undefined && ms >= 1000 && ms < 10_000) sleeps.push(ms)
      return realTimeout(fn, ms)
    }) as typeof setTimeout)

    let registers = 0
    const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
      if (String(url).includes('/api/events')) {
        // One keepalive, then a connection that neither speaks nor closes.
        const body = new ReadableStream({
          start(c) {
            c.enqueue(new TextEncoder().encode(': keepalive\n'))
            init?.signal?.addEventListener('abort', () =>
              c.error(new DOMException('The operation was aborted.', 'AbortError')),
            )
          },
        })
        return new Response(body, { status: 200 })
      }
      registers += 1
      return new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)
    useAppStore.setState({ client: new HubClient({ token: creds.token }) })
    const { unmount } = renderHook(() => useEventLoop())
    await vi.waitFor(() => expect(registers).toBe(1))
    // 30 s of silence trips the watchdog; the loop must be back on
    // register() right away, not after a 1 s (or longer) backoff.
    await vi.advanceTimersByTimeAsync(30_001)
    await vi.waitFor(() => expect(registers).toBe(2), { timeout: 500, interval: 10 })
    expect(sleeps).toEqual([])
    unmount()
  } finally {
    vi.restoreAllMocks()
    vi.useRealTimers()
  }
})

test('coming back to the foreground cuts the reconnect wait short', async () => {
  vi.useFakeTimers()
  try {
    let registers = 0
    const fetchMock = vi.fn(async (url: string) => {
      if (String(url).includes('/api/events')) {
        // Every stream closes at once, so the loop is always in its backoff.
        return new Response(new ReadableStream({ start: (c) => c.close() }), { status: 200 })
      }
      registers += 1
      return new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)
    useAppStore.setState({ client: new HubClient({ token: creds.token }) })
    const { unmount } = renderHook(() => useEventLoop())
    await vi.waitFor(() => expect(useAppStore.getState().connection).toBe('offline'))
    expect(registers).toBe(1)
    document.dispatchEvent(new Event('visibilitychange'))
    // waitFor advances fake time by `interval` per check: 500 ms in total is
    // well inside the 1 s backoff, so only the visibility cut can get us here.
    await vi.waitFor(() => expect(registers).toBe(2), { timeout: 500, interval: 10 })
    unmount()
  } finally {
    vi.useRealTimers()
  }
})
