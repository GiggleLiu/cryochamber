import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ConversationView, isRichMessage } from './ConversationView'
import { useAppStore, resetAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'
import { ApiError } from '../api/errors'
import { HubClient } from '../api/hubClient'
import { ECHO_TIMEOUT_MS, sendViaOutbox } from '../lib/outbox'
import type { Credentials, Message } from '../api/types'

const creds: Credentials = { kind: 'hub', prefix: '', email: 'me@b.c', apiKey: 'k', sendTopic: '' }

function makeMsg(id: number, overrides: Partial<Message> = {}): Message {
  return {
    id, sender_full_name: 'Agent', sender_email: 'bot@b.c',
    timestamp: 1755100000 + id, content: `msg-${id}`, stream_id: 1, subject: '',
    ...overrides,
  }
}

function fakeClient(overrides: Partial<Record<keyof HubClient, unknown>> = {}) {
  return {
    getMessages: vi.fn(async () => [makeMsg(1), makeMsg(2)]),
    sendMessage: vi.fn(async () => 99),
    markStreamRead: vi.fn(async () => {}),
    authHeaderValue: vi.fn(() => 'Bearer k'),
    getOwnUser: vi.fn(async () => ({ user_id: 7 })),
    chamberIdFor: vi.fn(() => 'cham-a'),
    ...overrides,
  } as unknown as HubClient
}

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    creds,
    streams: [{ stream_id: 1, name: 'alpha', description: 'A' }],
  })
})

describe('WeChat-style chat bubbles', () => {
  test('marks own messages msg-self and other messages msg-other', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { sender_email: 'bot@b.c', sender_full_name: 'Agent' }),
        makeMsg(2, { sender_email: 'me@b.c', sender_full_name: 'Human' }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    const rows = container.querySelectorAll('.msg-row')
    expect(rows).toHaveLength(2)
    expect(rows[0].className).toContain('msg-other')
    expect(rows[1].className).toContain('msg-self')
  })

  test('shows a sender label only above other people\'s bubbles', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { sender_email: 'bot@b.c', sender_full_name: 'Agent' }),
        makeMsg(2, { sender_email: 'me@b.c', sender_full_name: 'Human' }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    const labels = container.querySelectorAll('.sender-label')
    expect(labels).toHaveLength(1)
    expect(labels[0].textContent).toBe('Agent')
  })

  test('avatar shows the sender\'s first character uppercased', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { sender_full_name: 'ada lovelace' }),
        makeMsg(2, { sender_email: 'me@b.c', sender_full_name: 'Human' }),
        makeMsg(3, { sender_full_name: 'Another' }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    const avatars = container.querySelectorAll('.avatar')
    expect(avatars).toHaveLength(3)
    expect(avatars[0].textContent).toBe('A')
    expect(avatars[1].textContent).toBe('H')
    // deterministic per-sender color: msg-1 and msg-3 share sender 'bot@b.c'
    expect((avatars[2] as HTMLElement).style.backgroundColor).toBe(
      (avatars[0] as HTMLElement).style.backgroundColor,
    )
    expect((avatars[1] as HTMLElement).style.backgroundColor).toMatch(/^rgb\(/)
  })

  test('renders a time pill before the first message and before gaps of 300s+', async () => {
    const base = 1755100000
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { timestamp: base }),
        makeMsg(2, { timestamp: base + 60 }), // 1 minute later: no new pill
        makeMsg(3, { timestamp: base + 60 + 600 }), // 10 minutes later: new pill
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-3')
    const pills = container.querySelectorAll('.time-pill')
    expect(pills).toHaveLength(2)
    for (const pill of pills) {
      // "19:32", "Today 19:32", "Yesterday 19:32", "Fri 19:32", "13 Aug 19:32", "13 Aug 2025 19:32"
      expect(pill.textContent!.trim()).toMatch(
        /^(Today |Yesterday |[A-Z][a-z]{2} |\d{1,2} [A-Z][a-z]{2} (\d{4} )?)?\d{2}:\d{2}$/,
      )
    }
  })

  test('only the leading time pill when messages are close together', async () => {
    const base = 1755100000
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { timestamp: base }),
        makeMsg(2, { timestamp: base + 30 }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-2')
    expect(container.querySelectorAll('.time-pill')).toHaveLength(1)
  })
})

test('loads newest messages on mount and renders them sanitized', async () => {
  const client = fakeClient()
  useAppStore.setState({ client })
  render(<ConversationView streamId={1} />)
  expect(await screen.findByText('msg-1')).toBeInTheDocument()
  expect(client.getMessages).toHaveBeenCalledWith('alpha')
  await waitFor(() => expect(client.markStreamRead).toHaveBeenCalledWith(1))
  expect(useAppStore.getState().unreadByStream[1] ?? []).toEqual([])
})

test('fetches own user id on mount', async () => {
  const client = fakeClient({ getOwnUser: vi.fn(async () => ({ user_id: 7 })) })
  useAppStore.setState({ client })
  render(<ConversationView streamId={1} />)
  await waitFor(() => expect(useAppStore.getState().ownUserId).toBe(7))
})

test('does not refetch own user once known', async () => {
  const client = fakeClient({ getOwnUser: vi.fn(async () => ({ user_id: 7 })) })
  useAppStore.setState({ client, ownUserId: 7 })
  render(<ConversationView streamId={1} />)
  await screen.findByText('msg-1')
  expect(client.getOwnUser).not.toHaveBeenCalled()
})

test('auth error while fetching own user logs out', async () => {
  const client = fakeClient({
    getOwnUser: vi.fn().mockRejectedValue(new ApiError('HTTP 401', 401)),
  })
  useAppStore.setState({ client })
  render(<ConversationView streamId={1} />)
  await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
  expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
})

test('non-auth error while fetching own user is ignored silently', async () => {
  const client = fakeClient({ getOwnUser: vi.fn().mockRejectedValue(new Error('boom')) })
  useAppStore.setState({ client })
  render(<ConversationView streamId={1} />)
  await screen.findByText('msg-1')
  expect(useAppStore.getState().creds).not.toBeNull()
  expect(useAppStore.getState().ownUserId).toBeNull()
})

test('re-fetches history when the stream is not in loadedStreams even if messages exist', async () => {
  const client = fakeClient({ getMessages: vi.fn(async () => [makeMsg(1), makeMsg(2)]) })
  useAppStore.setState({
    client,
    messagesByStream: { 1: [makeMsg(2)] },
    loadedStreams: [],
  })
  render(<ConversationView streamId={1} />)
  await waitFor(() => expect(client.getMessages).toHaveBeenCalledWith('alpha'))
  await waitFor(() =>
    expect(useAppStore.getState().messagesByStream[1].map((m) => m.id)).toEqual([1, 2]),
  )
  expect(useAppStore.getState().loadedStreams).toEqual([1])
})

test('send clears composer on success', async () => {
  const client = fakeClient()
  useAppStore.setState({ client })
  render(<ConversationView streamId={1} />)
  await screen.findByText('msg-1')
  const box = screen.getByRole('textbox')
  await userEvent.type(box, 'do the thing')
  await userEvent.click(screen.getByRole('button', { name: /^send$/i }))
  await waitFor(() => expect(box).toHaveValue(''))
  expect(client.sendMessage).toHaveBeenCalledWith('alpha', 'do the thing')
})

test('a failed send becomes a retryable bubble and empties the composer', async () => {
  const client = fakeClient({
    sendMessage: vi.fn().mockRejectedValueOnce(new Error('boom')).mockResolvedValueOnce(99),
  })
  useAppStore.setState({ client })
  render(<ConversationView streamId={1} />)
  await screen.findByText('msg-1')
  const box = screen.getByRole('textbox')
  await userEvent.type(box, 'important command')
  await userEvent.click(screen.getByRole('button', { name: /^send$/i }))
  // The message is in the thread, not stuck in the box.
  expect(await screen.findByText('important command')).toBeInTheDocument()
  expect(box).toHaveValue('')
  await userEvent.click(await screen.findByRole('button', { name: /failed — tap to retry/i }))
  await waitFor(() => expect(screen.getByText('Sent')).toBeInTheDocument())
  expect(client.sendMessage).toHaveBeenCalledTimes(2)
})

test('back button returns to projects', async () => {
  useAppStore.setState({ client: fakeClient(), view: { name: 'conversation', streamId: 1 } })
  render(<ConversationView streamId={1} />)
  await userEvent.click(screen.getByRole('button', { name: /back/i }))
  expect(useAppStore.getState().view).toEqual({ name: 'projects' })
})

test('markStreamRead retries once after 3s on non-auth failure', async () => {
  vi.useFakeTimers()
  try {
    const client = fakeClient({
      markStreamRead: vi
        .fn()
        .mockRejectedValueOnce(new Error('boom'))
        .mockResolvedValueOnce(undefined),
    })
    useAppStore.setState({ client })
    render(<ConversationView streamId={1} />)
    await vi.waitFor(() => expect(client.markStreamRead).toHaveBeenCalledTimes(1))
    await vi.advanceTimersByTimeAsync(3000)
    await vi.waitFor(() => expect(client.markStreamRead).toHaveBeenCalledTimes(2))
    expect(useAppStore.getState().creds).not.toBeNull()
  } finally {
    vi.useRealTimers()
  }
})

describe('staying with the newest message', () => {
  /** jsdom has no layout, so the scroll geometry has to be declared. */
  function stubGeometry(el: Element, { scrollHeight = 2000, clientHeight = 800, scrollTop = 0 }) {
    Object.defineProperty(el, 'scrollHeight', { value: scrollHeight, configurable: true })
    Object.defineProperty(el, 'clientHeight', { value: clientHeight, configurable: true })
    Object.defineProperty(el, 'scrollTop', {
      value: scrollTop, writable: true, configurable: true,
    })
  }

  test('opening a conversation lands on the newest message', async () => {
    useAppStore.setState({ client: fakeClient() })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-2')
    const scroller = container.querySelector('.message-scroll')!
    // Bottom-parked: no jump affordance is offered.
    expect(container.querySelector('.jump-latest')).toBeNull()
    expect(scroller.scrollTop).toBe(scroller.scrollHeight)
  })

  test('scrolling away offers a jump chip; using it returns to the bottom', async () => {
    useAppStore.setState({ client: fakeClient() })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-2')
    const scroller = container.querySelector('.message-scroll')!
    stubGeometry(scroller, { scrollTop: 100 })

    fireEvent.scroll(scroller)
    const jump = await screen.findByRole('button', { name: /latest/i })

    stubGeometry(scroller, { scrollTop: 1200 })
    await userEvent.click(jump)
    expect(screen.queryByRole('button', { name: /latest/i })).toBeNull()
    expect(scroller.scrollTop).toBe(2000)
  })

  test('a message arriving while scrolled up is announced, not jumped to', async () => {
    useAppStore.setState({ client: fakeClient() })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-2')
    const scroller = container.querySelector('.message-scroll')!
    stubGeometry(scroller, { scrollTop: 100 })
    fireEvent.scroll(scroller)
    await screen.findByRole('button', { name: /latest/i })

    act(() => {
      useAppStore.getState().applyEvents([
        { id: 1, type: 'message', message: makeMsg(3) } as never,
      ])
    })

    expect(await screen.findByRole('button', { name: /new messages/i })).toBeInTheDocument()
    // The viewport was left exactly where the reader put it.
    expect(scroller.scrollTop).toBe(100)
  })
})

describe('message grouping and layout', () => {
  test('a run from one sender drops the repeated avatar and name', async () => {
    const base = 1755100000
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { timestamp: base, sender_full_name: 'Agent' }),
        makeMsg(2, { timestamp: base + 30, sender_full_name: 'Agent' }),
        makeMsg(3, { timestamp: base + 60, sender_email: 'me@b.c', sender_full_name: 'Human' }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-3')
    const rows = container.querySelectorAll('.msg-row')
    expect(rows[0].className).not.toContain('msg-grouped')
    expect(rows[1].className).toContain('msg-grouped')
    // Different sender: a new turn, so the avatar and label come back.
    expect(rows[2].className).not.toContain('msg-grouped')
    expect(container.querySelectorAll('.sender-label')).toHaveLength(1)
    expect(container.querySelectorAll('.avatar-hidden')).toHaveLength(1)
    // Every row still carries an avatar element, hidden or not.
    expect(container.querySelectorAll('.avatar')).toHaveLength(3)
  })

  test('a time gap starts a new turn even for the same sender', async () => {
    const base = 1755100000
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { timestamp: base }),
        makeMsg(2, { timestamp: base + 600 }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-2')
    expect(container.querySelectorAll('.msg-grouped')).toHaveLength(0)
  })

  test.each([
    ['code fence', '```\nx = 1\n```'],
    ['table', '| a | b |\n| - | - |'],
    ['display math', '$$x^2$$'],
  ])('a message containing a %s gets the full-width treatment', async (_name, content) => {
    const client = fakeClient({ getMessages: vi.fn(async () => [makeMsg(1, { content })]) })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await waitFor(() => expect(container.querySelector('.msg-row')).not.toBeNull())
    expect(container.querySelector('.msg-row')!.className).toContain('msg-rich')
  })

  test.each([
    ['inline image', '![plot](/api/chambers/cham-a/files/a.png)'],
    ['heading', '## Report'],
    ['blockquote', '> quoted'],
  ])('a message containing a %s stays an ordinary bubble with avatar', async (_name, content) => {
    const client = fakeClient({ getMessages: vi.fn(async () => [makeMsg(1, { content })]) })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await waitFor(() => expect(container.querySelector('.msg-row')).not.toBeNull())
    expect(container.querySelector('.msg-row')!.className).not.toContain('msg-rich')
    expect(container.querySelector('.avatar')).not.toBeNull()
  })

  test('a plain paragraph stays a chat bubble', async () => {
    useAppStore.setState({ client: fakeClient() })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    expect(container.querySelector('.msg-row')!.className).not.toContain('msg-rich')
  })
})

describe('message loading', () => {
  test('renders message bodies as markdown', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [makeMsg(1, { content: '**bold** $x^2$' })]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await waitFor(() =>
      expect(container.querySelector('.message-body strong')?.textContent).toBe('bold'),
    )
    expect(container.querySelector('.message-body .katex')).not.toBeNull()
  })

  test('a server 404 on a revoked chamber leaves for the projects list and drops the project', async () => {
    // A real client that did register: the chamber resolves, and the hub itself
    // answers 404 for it.
    const fetchFn = vi.fn(async (url: string) =>
      String(url).includes('/messages')
        ? new Response('{}', { status: 404 })
        : new Response(JSON.stringify([{ id: 'cham-a', name: 'alpha' }]), { status: 200 }),
    ) as unknown as typeof fetch
    const hubCreds = { kind: 'hub' as const, prefix: '', email: 'Alice', apiKey: 'tok', sendTopic: '' }
    const client = new HubClient(hubCreds, fetchFn)
    await client.register()
    useAppStore.setState({
      client,
      creds: hubCreds,
      view: { name: 'conversation', streamId: 1 },
    })
    render(<ConversationView streamId={1} />)
    await waitFor(() => expect(useAppStore.getState().view).toEqual({ name: 'projects' }))
    // Merely navigating away left it tappable in the list, failing every time.
    expect(useAppStore.getState().streams).toEqual([])
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('an unregistered client never prunes the project it could not resolve', async () => {
    // Offline cold boot: cached projects render, but register() has not run, so
    // every name lookup misses and throws a client-side 404. Pruning on that
    // would delete the cached project for good.
    const fetchFn = vi.fn(async () => {
      throw new TypeError('Failed to fetch')
    }) as unknown as typeof fetch
    const hubCreds = { kind: 'hub' as const, prefix: '', email: 'Alice', apiKey: 'tok', sendTopic: '' }
    useAppStore.setState({
      client: new HubClient(hubCreds, fetchFn),
      creds: hubCreds,
      view: { name: 'conversation', streamId: 1 },
    })
    render(<ConversationView streamId={1} />)
    expect(await screen.findByRole('alert')).toHaveTextContent(/couldn’t load this conversation/i)
    expect(screen.getByRole('button', { name: /try again/i })).toBeInTheDocument()
    expect(useAppStore.getState().streams).toHaveLength(1)
    expect(useAppStore.getState().view).toEqual({ name: 'conversation', streamId: 1 })
  })

  test('a project that disappears from the store navigates out instead of going blank', async () => {
    useAppStore.setState({
      client: fakeClient(),
      view: { name: 'conversation', streamId: 1 },
    })
    const { container } = render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    // A re-register that no longer lists this chamber: the view used to render
    // null forever, stranding the user on an empty screen with no controls.
    act(() => {
      useAppStore.setState({ streams: [] })
    })
    await waitFor(() => expect(useAppStore.getState().view).toEqual({ name: 'projects' }))
    expect(container.querySelector('.conversation')).toBeNull()
  })

  test('a non-404 failure shows the retryable error panel', async () => {
    const client = fakeClient({
      getMessages: vi.fn().mockRejectedValue(new ApiError('HTTP 500', 500)),
    })
    useAppStore.setState({ client, view: { name: 'conversation', streamId: 1 } })
    render(<ConversationView streamId={1} />)
    expect(await screen.findByRole('alert')).toHaveTextContent(/couldn’t load this conversation/i)
    expect(useAppStore.getState().view).toEqual({ name: 'conversation', streamId: 1 })
  })
})

describe('isRichMessage', () => {
  test.each([
    ['fenced code', 'run this:\n```py\nx = 1\n```'],
    ['table row', '| a | b |\n| - | - |'],
    ['display math', 'so\n$$x^2$$'],
  ])('%s is wide content', (_name, content) => {
    expect(isRichMessage(content)).toBe(true)
  })

  test.each([
    ['prose', '**bold** and a $x$ inline'],
    ['inline code', 'use `npm run build` here'],
    ['a pipe mid-line', 'pipe a | b in prose'],
    ['single dollars', 'costs $5 and $10'],
    // The detectors read markdown source, not HTML.
    ['a mention of pre', 'the <pre> tag is html'],
  ])('%s stays an ordinary bubble', (_name, content) => {
    expect(isRichMessage(content)).toBe(false)
  })

  test('code fences get the full-width treatment end to end', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [makeMsg(1, { content: '```\nx = 1\n```' })]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView streamId={1} />)
    await waitFor(() => expect(container.querySelector('.msg-row')).not.toBeNull())
    expect(container.querySelector('.msg-row')!.className).toContain('msg-rich')
  })
})

test('a mailbox returns its whole history, so nothing offers to load earlier', async () => {
  useAppStore.setState({ client: fakeClient() })
  render(<ConversationView streamId={1} />)
  await screen.findByText('msg-1')
  expect(screen.queryByRole('button', { name: /load earlier/i })).toBeNull()
})

describe('the asleep banner', () => {
  function setStream(extra: Record<string, unknown>) {
    useAppStore.setState({
      client: fakeClient(),
      streams: [{ stream_id: 1, name: 'alpha', description: 'A', ...extra }],
    })
  }

  test('a sleeping agent in a running chamber says so, composer stays usable', async () => {
    setStream({ running: true, agentRunning: false })
    render(<ConversationView streamId={1} />)
    const note = await screen.findByRole('status')
    expect(note).toHaveTextContent('Agent is asleep — messages will be read at the next wake')
    expect(note).not.toHaveTextContent('·')
    expect(screen.getByRole('textbox')).toBeEnabled()
  })

  test('a scheduled wake is named', async () => {
    setStream({ running: true, agentRunning: false, nextWake: 'in 2 h' })
    render(<ConversationView streamId={1} />)
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Agent is asleep — messages will be read at the next wake · in 2 h',
    )
  })

  test('a stopped chamber says so — and never shows a (stale) next wake', async () => {
    // A dead chamber still carries whatever wake was pending when it died;
    // naming it would promise a read that is not scheduled.
    setStream({ running: false, agentRunning: false, nextWake: 'in 2 h' })
    render(<ConversationView streamId={1} />)
    const note = await screen.findByRole('status')
    expect(note).toHaveTextContent(
      'Chamber is not running — messages will wait in its inbox until it is started',
    )
    expect(note).not.toHaveTextContent('in 2 h')
    expect(screen.getByRole('textbox')).toBeEnabled()
  })

  test.each([
    ['a working agent', { running: true, agentRunning: true }],
    ['a project whose liveness the hub never reported', {}],
  ])('%s shows no banner', async (_name, extra) => {
    setStream(extra)
    render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    expect(screen.queryByRole('status')).toBeNull()
  })
})

describe('outbox bubbles', () => {
  test('a sending item renders a pending self bubble', async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    act(() => {
      useAppStore.getState().enqueueOutbox(1, 'on its way')
    })
    expect(await screen.findByText('on its way')).toBeInTheDocument()
    expect(screen.getByText(/sending/i)).toBeInTheDocument()
    expect(document.querySelector('.msg-pending')).not.toBeNull()
  })

  test('a failed item offers retry, and retrying re-sends it', async () => {
    const client = fakeClient()
    useAppStore.setState({ client })
    render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    act(() => {
      const id = useAppStore.getState().enqueueOutbox(1, 'try again')
      useAppStore.getState().failOutbox(1, id)
    })
    await userEvent.click(await screen.findByRole('button', { name: /failed — tap to retry/i }))
    await waitFor(() => expect(client.sendMessage).toHaveBeenCalledWith('alpha', 'try again'))
    // Retry is manual only: nothing re-sends it on its own.
    await waitFor(() => expect(screen.getByText('Sent')).toBeInTheDocument())
    expect(client.sendMessage).toHaveBeenCalledTimes(1)
  })

  test('a sent item says Sent and clears when the thread echoes it back', async () => {
    const client = fakeClient()
    useAppStore.setState({ client })
    render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    await userEvent.type(screen.getByRole('textbox'), 'run the sweep')
    await userEvent.click(screen.getByRole('button', { name: /^send$/i }))

    // Accepted by the server, but not yet part of the thread: the bubble stays
    // rather than blinking out and back in when the event lands.
    expect(await screen.findByText('Sent')).toBeInTheDocument()
    expect(useAppStore.getState().outboxByStream[1]).toHaveLength(1)

    act(() => {
      useAppStore.getState().applyEvents([
        {
          id: 1,
          type: 'message',
          message: makeMsg(9, { sender_email: 'me@b.c', content: 'run the sweep' }),
        } as never,
      ])
    })
    await waitFor(() => expect(useAppStore.getState().outboxByStream[1]).toEqual([]))
    expect(screen.queryByText('Sent')).toBeNull()
  })

  test('a sent item gives up waiting after the fallback timeout', async () => {
    vi.useFakeTimers()
    try {
      const client = fakeClient()
      useAppStore.setState({ client })
      act(() => {
        sendViaOutbox(1, 'alpha', 'no echo will come')
      })
      await vi.waitFor(() =>
        expect(useAppStore.getState().outboxByStream[1][0].state).toBe('sent'),
      )
      await vi.advanceTimersByTimeAsync(ECHO_TIMEOUT_MS)
      expect(useAppStore.getState().outboxByStream[1]).toEqual([])
    } finally {
      vi.useRealTimers()
    }
  })

  test('outbox items of other projects stay out of this thread', async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    act(() => {
      useAppStore.getState().enqueueOutbox(2, 'elsewhere')
    })
    expect(screen.queryByText('elsewhere')).toBeNull()
  })
})

describe('owner header actions', () => {
  test('an owner gets an Invite button that opens the sheet for this chamber', async () => {
    useAppStore.setState({
      client: fakeClient({ chamberIdFor: vi.fn(() => 'cham-a'), listInvites: vi.fn(async () => []) }),
      hubRole: 'owner',
    })
    render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    await userEvent.click(screen.getByRole('button', { name: 'Invite' }))
    expect(await screen.findByRole('dialog', { name: 'Invite' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Invite to alpha' })).toBeInTheDocument()
  })

  test('a guest is never shown the Invite button at all', async () => {
    useAppStore.setState({ client: fakeClient(), hubRole: 'invite' })
    render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    expect(screen.queryByRole('button', { name: 'Invite' })).toBeNull()
  })

  test('a session whose role is unknown shows no owner actions', async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<ConversationView streamId={1} />)
    await screen.findByText('msg-1')
    expect(screen.queryByRole('button', { name: 'Invite' })).toBeNull()
  })
})
