import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ConversationView, isRichMessage } from './ConversationView'
import { ACCESS_REVOKED_NOTICE, unreadCount, useAppStore, resetAppStore } from '../store/appStore'
import { ApiError } from '../api/types'
import { HubClient } from '../api/hubClient'
import { makeHubAccount, MemoryHubsBackend } from '../store/hubs'
import { chamberKey } from '../lib/hubKeys'
import { ECHO_TIMEOUT_MS, sendViaOutbox } from '../lib/outbox'
import type { Chamber, ChamberMessage, Credentials } from '../api/types'

const creds: Credentials = { token: 'k', name: 'me@b.c', role: 'owner' }

/** A message at `2026-08-15T10:00:00 + offsetSeconds`, from the agent unless
 * said otherwise. `n` orders the mailbox ids the store dedupes on. */
function makeMsg(n: number, overrides: Partial<ChamberMessage> = {}): ChamberMessage {
  return {
    id: `outbox/${n}.md`,
    chamberId: 'cham-a',
    direction: 'outbox',
    sender: 'Agent',
    subject: '',
    body: `msg-${n}`,
    timestamp: stamp(n),
    session: null,
    isQuestion: false,
    ...overrides,
  }
}

/** `%Y-%m-%dT%H:%M:%S` `offset` seconds after a fixed wall clock. */
function stamp(offsetSeconds: number): string {
  const d = new Date(Date.parse('2026-08-15T10:00:00') + offsetSeconds * 1000)
  const pad = (x: number) => String(x).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

function chamber(extra: Partial<Chamber> = {}): Chamber {
  return {
    id: 'cham-a',
    name: 'alpha',
    running: true,
    agentRunning: true,
    nextWakeDisplay: null,
    completed: false,
    archived: false,
    hasOpenQuestion: false,
    ...extra,
  }
}

function fakeClient(overrides: Partial<Record<keyof HubClient, unknown>> = {}) {
  const blob = vi.fn(async (_url: string) => new Blob(['x']))
  return {
    getMessages: vi.fn(async () => [makeMsg(1), makeMsg(2)]),
    sendMessage: vi.fn(async () => ({ id: 'inbox/99.md' })),
    fetchBlob: blob,
    // The ConsoleClient spelling the view actually calls; browser mode ignores
    // the chamber key exactly as HubClient does.
    fetchBlobFor: vi.fn(async (_key: string, url: string) => blob(url)),
    // The controls sheet mounts over this view and asks for a status; a fake
    // without one would throw where the real client answers.
    chamberStatus: vi.fn(async () => {
      throw new ApiError(500, 'HTTP 500')
    }),
    ...overrides,
  } as unknown as HubClient
}

beforeEach(() => {
  localStorage.clear()
  resetAppStore()
  useAppStore.setState({
    creds,
    // What the hub stamps on this token's messages: the name that makes a
    // bubble "mine".
    selfName: creds.name,
    chambers: [chamber()],
  })
})

afterEach(() => vi.unstubAllGlobals())

describe('WeChat-style chat bubbles', () => {
  test('each message bubble exposes its exact local timestamp', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [makeMsg(1, { timestamp: '2026-08-15T14:32:00' })]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    expect(container.querySelector('.bubble')).toHaveAttribute('title', '2026-08-15 14:32')
  })

  test('a fine-pointer bubble copies the whole message body', async () => {
    vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: true })))
    const writeText = vi.fn(async () => {})
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    })
    const client = fakeClient({
      getMessages: vi.fn(async () => [makeMsg(1, { body: 'copy the whole message' })]),
    })
    useAppStore.setState({ client })
    render(<ConversationView chamberId="cham-a" />)
    await userEvent.click(await screen.findByRole('button', { name: 'Copy' }))
    expect(writeText).toHaveBeenCalledWith('copy the whole message')
    expect(await screen.findByRole('button', { name: 'Copied' })).toBeInTheDocument()
  })

  test('touch devices do not render message copy controls', async () => {
    vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false })))
    useAppStore.setState({ client: fakeClient() })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    expect(screen.queryByRole('button', { name: 'Copy' })).toBeNull()
  })

  test('marks own messages msg-self and other messages msg-other', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { sender: 'Agent' }),
        makeMsg(2, { sender: 'me@b.c' }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    const rows = container.querySelectorAll('.msg-row')
    expect(rows).toHaveLength(2)
    expect(rows[0].className).toContain('msg-other')
    expect(rows[1].className).toContain('msg-self')
  })

  test('a bubble is mine when the sender is what the hub calls me', async () => {
    // The hub stamps its own sender on what we send — `alice (invite)` for an
    // invite named Alice — so "mine" is `sender === selfName`, never a guess
    // at a direction or a name like `human`.
    useAppStore.setState({ selfName: 'alice (invite)' })
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { sender: 'autoresearch-agent' }),
        makeMsg(2, { sender: 'alice (invite)', direction: 'inbox', id: 'inbox/2.md' }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-2')
    const rows = container.querySelectorAll('.msg-row')
    expect(rows[0].className).toContain('msg-other')
    expect(rows[1].className).toContain('msg-self')
  })

  test('in app mode "mine" is what THIS chamber\'s hub calls me', async () => {
    // Two hubs can name the same token differently; the session-wide name is
    // browser mode's answer and means nothing here.
    useAppStore.setState({
      mode: 'app',
      creds: null,
      selfName: 'me@b.c',
      selfNameByHub: { aaaaaaaa: 'alice (invite)' },
      chambers: [chamber({ id: 'aaaaaaaa:cham-a', hubId: 'aaaaaaaa' })],
    })
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { chamberId: 'aaaaaaaa:cham-a', sender: 'me@b.c' }),
        makeMsg(2, { chamberId: 'aaaaaaaa:cham-a', sender: 'alice (invite)' }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="aaaaaaaa:cham-a" />)
    await screen.findByText('msg-2')
    const rows = container.querySelectorAll('.msg-row')
    expect(rows[0].className).toContain('msg-other')
    expect(rows[1].className).toContain('msg-self')
  })

  test('shows a sender label only above other people\'s bubbles', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { sender: 'Agent' }),
        makeMsg(2, { sender: 'me@b.c' }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    const labels = container.querySelectorAll('.sender-label')
    expect(labels).toHaveLength(1)
    expect(labels[0].textContent).toBe('Agent')
  })

  test('avatar shows the sender\'s first character uppercased', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { sender: 'ada lovelace' }),
        makeMsg(2, { sender: 'me@b.c' }),
        makeMsg(3, { sender: 'Another' }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    const avatars = container.querySelectorAll('.avatar')
    expect(avatars).toHaveLength(3)
    expect(avatars[0].textContent).toBe('A')
    // The hub's sender string is the whole identity now: 'me@b.c' initials 'M'.
    expect(avatars[1].textContent).toBe('M')
    // deterministic per-sender colour: msg-1 and msg-3 share the sender 'Agent'
    expect((avatars[2] as HTMLElement).style.backgroundColor).toBe(
      (avatars[0] as HTMLElement).style.backgroundColor,
    )
    expect((avatars[1] as HTMLElement).style.backgroundColor).toMatch(/^rgb\(/)
  })

  test('renders a time pill before the first message and before gaps of 300s+', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { timestamp: stamp(0) }),
        makeMsg(2, { timestamp: stamp(60) }), // 1 minute later: no new pill
        makeMsg(3, { timestamp: stamp(660) }), // 10 minutes later: new pill
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
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
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { timestamp: stamp(0) }),
        makeMsg(2, { timestamp: stamp(30) }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-2')
    expect(container.querySelectorAll('.time-pill')).toHaveLength(1)
  })
})

test('loads the history on mount and renders it sanitized', async () => {
  const client = fakeClient()
  useAppStore.setState({ client })
  render(<ConversationView chamberId="cham-a" />)
  expect(await screen.findByText('msg-1')).toBeInTheDocument()
  // Addressed by the hub's own chamber id, not by a display name.
  expect(client.getMessages).toHaveBeenCalledWith('cham-a')
})

test('opening the conversation marks it read', async () => {
  useAppStore.setState({ client: fakeClient(), selfName: 'someone else' })
  render(<ConversationView chamberId="cham-a" />)
  await screen.findByText('msg-2')
  await waitFor(() => expect(unreadCount(useAppStore.getState(), 'cham-a')).toBe(0))
  // The watermark is the newest message on screen, so a later arrival counts.
  expect(useAppStore.getState().lastReadByChamber['cham-a']).toBe(
    `${stamp(2)} outbox/2.md`,
  )
})

test('re-fetches history when the chamber is not in loadedChambers even if messages exist', async () => {
  const client = fakeClient({ getMessages: vi.fn(async () => [makeMsg(1), makeMsg(2)]) })
  useAppStore.setState({
    client,
    messagesByChamber: { 'cham-a': [makeMsg(2)] },
    loadedChambers: [],
  })
  render(<ConversationView chamberId="cham-a" />)
  await waitFor(() => expect(client.getMessages).toHaveBeenCalledWith('cham-a'))
  await waitFor(() =>
    expect(useAppStore.getState().messagesByChamber['cham-a'].map((m) => m.id)).toEqual([
      'outbox/1.md',
      'outbox/2.md',
    ]),
  )
  expect(useAppStore.getState().loadedChambers).toEqual(['cham-a'])
})

test('send clears composer on success', async () => {
  const client = fakeClient()
  useAppStore.setState({ client })
  render(<ConversationView chamberId="cham-a" />)
  await screen.findByText('msg-1')
  const box = screen.getByRole('textbox')
  await userEvent.type(box, 'do the thing')
  await userEvent.click(screen.getByRole('button', { name: /^send$/i }))
  await waitFor(() => expect(box).toHaveValue(''))
  expect(client.sendMessage).toHaveBeenCalledWith('cham-a', 'do the thing')
})

test('a failed send becomes a retryable bubble and empties the composer', async () => {
  const client = fakeClient({
    sendMessage: vi
      .fn()
      .mockRejectedValueOnce(new Error('boom'))
      .mockResolvedValueOnce({ id: 'inbox/99.md' }),
  })
  useAppStore.setState({ client })
  render(<ConversationView chamberId="cham-a" />)
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
  useAppStore.setState({ client: fakeClient(), view: { name: 'conversation', chamberId: 'cham-a' } })
  render(<ConversationView chamberId="cham-a" />)
  await userEvent.click(screen.getByRole('button', { name: /back/i }))
  expect(useAppStore.getState().view).toEqual({ name: 'projects' })
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
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-2')
    const scroller = container.querySelector('.message-scroll')!
    // Bottom-parked: no jump affordance is offered.
    expect(container.querySelector('.jump-latest')).toBeNull()
    expect(scroller.scrollTop).toBe(scroller.scrollHeight)
  })

  test('scrolling away offers a jump chip; using it returns to the bottom', async () => {
    useAppStore.setState({ client: fakeClient() })
    const { container } = render(<ConversationView chamberId="cham-a" />)
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
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-2')
    const scroller = container.querySelector('.message-scroll')!
    stubGeometry(scroller, { scrollTop: 100 })
    fireEvent.scroll(scroller)
    await screen.findByRole('button', { name: /latest/i })

    act(() => {
      useAppStore.getState().applyMessage(makeMsg(3))
    })

    expect(await screen.findByRole('button', { name: /new messages/i })).toBeInTheDocument()
    // The viewport was left exactly where the reader put it.
    expect(scroller.scrollTop).toBe(100)
  })
})

describe('message grouping and layout', () => {
  test('a run from one sender drops the repeated avatar and name', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { timestamp: stamp(0), sender: 'Agent' }),
        makeMsg(2, { timestamp: stamp(30), sender: 'Agent' }),
        makeMsg(3, { timestamp: stamp(60), sender: 'me@b.c' }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
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
    const client = fakeClient({
      getMessages: vi.fn(async () => [
        makeMsg(1, { timestamp: stamp(0) }),
        makeMsg(2, { timestamp: stamp(600) }),
      ]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-2')
    expect(container.querySelectorAll('.msg-grouped')).toHaveLength(0)
  })

  test.each([
    ['code fence', '```\nx = 1\n```'],
    ['table', '| a | b |\n| - | - |'],
    ['display math', '$$x^2$$'],
  ])('a message containing a %s gets the full-width treatment', async (_name, content) => {
    const client = fakeClient({ getMessages: vi.fn(async () => [makeMsg(1, { body: content })]) })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await waitFor(() => expect(container.querySelector('.msg-row')).not.toBeNull())
    expect(container.querySelector('.msg-row')!.className).toContain('msg-rich')
  })

  test.each([
    ['inline image', '![plot](/api/chambers/cham-a/files/a.png)'],
    ['heading', '## Report'],
    ['blockquote', '> quoted'],
  ])('a message containing a %s stays an ordinary bubble with avatar', async (_name, content) => {
    const client = fakeClient({ getMessages: vi.fn(async () => [makeMsg(1, { body: content })]) })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await waitFor(() => expect(container.querySelector('.msg-row')).not.toBeNull())
    expect(container.querySelector('.msg-row')!.className).not.toContain('msg-rich')
    expect(container.querySelector('.avatar')).not.toBeNull()
  })

  test('a plain paragraph stays a chat bubble', async () => {
    useAppStore.setState({ client: fakeClient() })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    expect(container.querySelector('.msg-row')!.className).not.toContain('msg-rich')
  })
})

describe('message loading', () => {
  test('renders message bodies as markdown', async () => {
    const client = fakeClient({
      getMessages: vi.fn(async () => [makeMsg(1, { body: '**bold** $x^2$' })]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await waitFor(() =>
      expect(container.querySelector('.message-body strong')?.textContent).toBe('bold'),
    )
    expect(container.querySelector('.message-body .katex')).not.toBeNull()
  })

  test.each([403, 404])(
    'a %i on the history drops the chamber and says why',
    async (status) => {
      // Scope was revoked while we were looking at it. Merely navigating away
      // left the chamber tappable in the list, failing again on every tap.
      const client = fakeClient({
        getMessages: vi.fn().mockRejectedValue(new ApiError(status, `HTTP ${status}`)),
      })
      useAppStore.setState({ client, view: { name: 'conversation', chamberId: 'cham-a' } })
      render(<ConversationView chamberId="cham-a" />)
      await waitFor(() => expect(useAppStore.getState().view).toEqual({ name: 'projects' }))
      expect(useAppStore.getState().chambers).toEqual([])
      expect(useAppStore.getState().accessNotice).toBe(ACCESS_REVOKED_NOTICE)
      expect(screen.queryByRole('alert')).toBeNull()
    },
  )

  test('an offline history fetch keeps the chamber and offers a retry', async () => {
    // A transport failure says nothing about whether the chamber still exists;
    // pruning on it would delete a cached chamber for good.
    const client = fakeClient({
      getMessages: vi.fn().mockRejectedValue(new TypeError('Failed to fetch')),
    })
    useAppStore.setState({ client, view: { name: 'conversation', chamberId: 'cham-a' } })
    render(<ConversationView chamberId="cham-a" />)
    expect(await screen.findByRole('alert')).toHaveTextContent(/couldn’t load this conversation/i)
    expect(screen.getByRole('button', { name: /try again/i })).toBeInTheDocument()
    expect(useAppStore.getState().chambers).toHaveLength(1)
    expect(useAppStore.getState().view).toEqual({ name: 'conversation', chamberId: 'cham-a' })
  })

  test('a chamber that disappears from the store navigates out instead of going blank', async () => {
    useAppStore.setState({
      client: fakeClient(),
      view: { name: 'conversation', chamberId: 'cham-a' },
    })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    // A re-register that no longer lists this chamber: the view used to render
    // null forever, stranding the user on an empty screen with no controls.
    act(() => {
      useAppStore.setState({ chambers: [] })
    })
    await waitFor(() => expect(useAppStore.getState().view).toEqual({ name: 'projects' }))
    expect(container.querySelector('.conversation')).toBeNull()
  })

  test('a non-404 failure shows the retryable error panel', async () => {
    const client = fakeClient({
      getMessages: vi.fn().mockRejectedValue(new ApiError(500, 'HTTP 500')),
    })
    useAppStore.setState({ client, view: { name: 'conversation', chamberId: 'cham-a' } })
    render(<ConversationView chamberId="cham-a" />)
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent(/couldn’t load this conversation/i)
    expect(alert).toHaveTextContent('Check your connection and try again.')
    expect(alert).not.toHaveTextContent('HTTP 500')
    expect(useAppStore.getState().view).toEqual({ name: 'conversation', chamberId: 'cham-a' })
  })

  test('a hub-authored history error is shown verbatim', async () => {
    const client = fakeClient({
      getMessages: vi.fn().mockRejectedValue(new ApiError(500, 'Mailbox is unavailable.', true)),
    })
    useAppStore.setState({ client })
    render(<ConversationView chamberId="cham-a" />)
    expect(await screen.findByRole('alert')).toHaveTextContent('Mailbox is unavailable.')
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
      getMessages: vi.fn(async () => [makeMsg(1, { body: '```\nx = 1\n```' })]),
    })
    useAppStore.setState({ client })
    const { container } = render(<ConversationView chamberId="cham-a" />)
    await waitFor(() => expect(container.querySelector('.msg-row')).not.toBeNull())
    expect(container.querySelector('.msg-row')!.className).toContain('msg-rich')
  })
})

test('long histories mount 100 messages and reveal the previous page per tap', async () => {
  const history = Array.from({ length: 250 }, (_, i) => makeMsg(i + 1))
  useAppStore.setState({ client: fakeClient({ getMessages: vi.fn(async () => history) }) })
  const { container } = render(<ConversationView chamberId="cham-a" />)
  await screen.findByText('msg-250')
  expect(container.querySelectorAll('.msg-row')).toHaveLength(100)
  expect(screen.queryByText('msg-150')).toBeNull()
  expect(screen.getByRole('button', { name: 'Earlier messages (150)' })).toBeInTheDocument()

  await userEvent.click(screen.getByRole('button', { name: 'Earlier messages (150)' }))
  expect(container.querySelectorAll('.msg-row')).toHaveLength(200)
  expect(screen.getByText('msg-51')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Earlier messages (50)' })).toBeInTheDocument()
})

test('prepending an earlier page preserves the reader’s viewport', async () => {
  const history = Array.from({ length: 250 }, (_, i) => makeMsg(i + 1))
  useAppStore.setState({ client: fakeClient({ getMessages: vi.fn(async () => history) }) })
  const { container } = render(<ConversationView chamberId="cham-a" />)
  await screen.findByText('msg-250')
  const scroller = container.querySelector('.message-scroll')!
  Object.defineProperty(scroller, 'scrollHeight', {
    configurable: true,
    get: () => container.querySelectorAll('.msg-row').length * 10,
  })
  Object.defineProperty(scroller, 'scrollTop', { configurable: true, writable: true, value: 100 })
  await userEvent.click(screen.getByRole('button', { name: 'Earlier messages (150)' }))
  expect(scroller.scrollTop).toBe(1100)
})

test('the visible cut starts a group and keeps grouping correct across revealed pages', async () => {
  const history = Array.from({ length: 250 }, (_, i) =>
    makeMsg(i + 1, { timestamp: stamp(i), sender: 'Agent' }),
  )
  useAppStore.setState({ client: fakeClient({ getMessages: vi.fn(async () => history) }) })
  const { container } = render(<ConversationView chamberId="cham-a" />)
  await screen.findByText('msg-250')
  let rows = container.querySelectorAll('.msg-row')
  expect(rows[0]).not.toHaveClass('msg-grouped')
  expect(rows[1]).toHaveClass('msg-grouped')
  expect(container.querySelectorAll('.time-pill')).toHaveLength(1)

  await userEvent.click(screen.getByRole('button', { name: 'Earlier messages (150)' }))
  rows = container.querySelectorAll('.msg-row')
  expect(rows[0]).not.toHaveClass('msg-grouped')
  expect(rows[100]).toHaveClass('msg-grouped')
  expect(container.querySelectorAll('.time-pill')).toHaveLength(1)
})

describe('the asleep banner', () => {
  function setChamber(extra: Partial<Chamber>) {
    useAppStore.setState({ client: fakeClient(), chambers: [chamber(extra)] })
  }

  test('a sleeping agent in a running chamber says so, composer stays usable', async () => {
    setChamber({ running: true, agentRunning: false })
    render(<ConversationView chamberId="cham-a" />)
    const note = await screen.findByRole('status')
    expect(note).toHaveTextContent('Agent is asleep — messages will be read at the next wake')
    expect(note).not.toHaveTextContent('·')
    expect(screen.getByRole('textbox')).toBeEnabled()
  })

  test('a scheduled wake is named', async () => {
    setChamber({ running: true, agentRunning: false, nextWakeDisplay: 'in 2 h' })
    render(<ConversationView chamberId="cham-a" />)
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Agent is asleep — messages will be read at the next wake · in 2 h',
    )
  })

  test('a stopped chamber says so — and never shows a (stale) next wake', async () => {
    // A dead chamber still carries whatever wake was pending when it died, so
    // `toChamber` nulls it: naming it would promise a read that is not
    // scheduled. Passed here as the store would ever hold it.
    setChamber({ running: false, agentRunning: false, nextWakeDisplay: null })
    render(<ConversationView chamberId="cham-a" />)
    const note = await screen.findByRole('status')
    expect(note).toHaveTextContent(
      'Chamber is not running — messages will wait in its inbox until it is started',
    )
    expect(note).not.toHaveTextContent('in 2 h')
    expect(screen.getByRole('textbox')).toBeEnabled()
  })

  test('a working agent shows no banner', async () => {
    setChamber({ running: true, agentRunning: true })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    expect(screen.queryByRole('status')).toBeNull()
  })
})

describe('liveness in the header', () => {
  function setChamber(extra: Partial<Chamber>) {
    useAppStore.setState({ client: fakeClient(), chambers: [chamber(extra)] })
  }

  test.each([
    ['a working agent', { running: true, agentRunning: true }, 'agent working', 'is-awake'],
    [
      'a chamber between wakes',
      { running: true, agentRunning: false },
      'chamber running, agent asleep',
      'is-running',
    ],
  ])('%s is dotted beside the title', async (_n, extra, label, cls) => {
    setChamber(extra)
    render(<ConversationView chamberId="cham-a" />)
    const dot = await screen.findByLabelText(label)
    expect(dot).toHaveClass(cls)
    // Beside the name, in the same heading — the glance that replaces opening
    // the controls sheet to read a status.
    const heading = screen.getByRole('heading', { name: /alpha/ })
    expect(heading).toContainElement(dot)
    expect(heading.firstElementChild).toBe(dot)
  })

  test('a stopped chamber is dotted too, in its own state', async () => {
    setChamber({ running: false, agentRunning: false })
    render(<ConversationView chamberId="cham-a" />)
    const dot = await screen.findByLabelText('chamber stopped')
    expect(dot).not.toHaveClass('is-awake')
    expect(dot).not.toHaveClass('is-running')
  })

})

describe('outbox bubbles', () => {
  test('a sending item renders a pending self bubble', async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    act(() => {
      useAppStore.getState().enqueueOutbox('cham-a', 'on its way')
    })
    expect(await screen.findByText('on its way')).toBeInTheDocument()
    expect(screen.getByText(/sending/i)).toBeInTheDocument()
    expect(document.querySelector('.msg-pending')).not.toBeNull()
  })

  test('a failed item offers retry, and retrying re-sends it', async () => {
    const client = fakeClient()
    useAppStore.setState({ client })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    act(() => {
      const id = useAppStore.getState().enqueueOutbox('cham-a', 'try again')
      useAppStore.getState().failOutbox('cham-a', id)
    })
    await userEvent.click(await screen.findByRole('button', { name: /failed — tap to retry/i }))
    await waitFor(() => expect(client.sendMessage).toHaveBeenCalledWith('cham-a', 'try again'))
    // Retry is manual only: nothing re-sends it on its own.
    await waitFor(() => expect(screen.getByText('Sent')).toBeInTheDocument())
    expect(client.sendMessage).toHaveBeenCalledTimes(1)
  })

  test("a failed item shows the hub's reason next to the retry prompt", async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    act(() => {
      const id = useAppStore.getState().enqueueOutbox('cham-a', 'too fast')
      useAppStore.getState().failOutbox('cham-a', id, 'rate limited')
    })
    expect(
      await screen.findByRole('button', { name: /failed — tap to retry · rate limited/i }),
    ).toBeInTheDocument()
  })

  test('a sent item says Sent and clears when the thread echoes it back', async () => {
    const client = fakeClient()
    useAppStore.setState({ client })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    await userEvent.type(screen.getByRole('textbox'), 'run the sweep')
    await userEvent.click(screen.getByRole('button', { name: /^send$/i }))

    // Accepted by the server, but not yet part of the thread: the bubble stays
    // rather than blinking out and back in when the event lands.
    expect(await screen.findByText('Sent')).toBeInTheDocument()
    expect(useAppStore.getState().outboxByChamber['cham-a']).toHaveLength(1)

    act(() => {
      // The hub's own id for the message we sent is what retires the bubble.
      useAppStore
        .getState()
        .applyMessage(makeMsg(9, { id: 'inbox/99.md', sender: 'me@b.c', body: 'run the sweep' }))
    })
    await waitFor(() => expect(useAppStore.getState().outboxByChamber['cham-a']).toEqual([]))
    expect(screen.queryByText('Sent')).toBeNull()
  })

  test('a sent item gives up waiting after the fallback timeout', async () => {
    vi.useFakeTimers()
    try {
      const client = fakeClient()
      useAppStore.setState({ client })
      act(() => {
        sendViaOutbox('cham-a', 'no echo will come')
      })
      await vi.waitFor(() =>
        expect(useAppStore.getState().outboxByChamber['cham-a'][0].state).toBe('sent'),
      )
      await vi.advanceTimersByTimeAsync(ECHO_TIMEOUT_MS)
      expect(useAppStore.getState().outboxByChamber['cham-a']).toEqual([])
    } finally {
      vi.useRealTimers()
    }
  })

  test('outbox items of other projects stay out of this thread', async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    act(() => {
      useAppStore.getState().enqueueOutbox('cham-b', 'elsewhere')
    })
    expect(screen.queryByText('elsewhere')).toBeNull()
  })
})

describe('owner header actions', () => {
  test('an owner gets an Invite button that opens the sheet for this chamber', async () => {
    useAppStore.setState({
      client: fakeClient({ listInvites: vi.fn(async () => []) }),
      hubRole: 'owner',
    })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    await userEvent.click(screen.getByRole('button', { name: 'Invite' }))
    expect(await screen.findByRole('dialog', { name: 'Invite' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Invite to alpha' })).toBeInTheDocument()
  })

  test('a guest is never shown the Invite button at all', async () => {
    useAppStore.setState({ client: fakeClient(), hubRole: 'invite' })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    expect(screen.queryByRole('button', { name: 'Invite' })).toBeNull()
  })

  test('a session whose role is unknown shows no owner actions', async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    expect(screen.queryByRole('button', { name: 'Invite' })).toBeNull()
  })

  test('an owner gets a Controls button that opens the controls sheet', async () => {
    // What the sheet reads is covered in ControlsSheet.test.tsx; this fake
    // answers its status call with nothing worth reading, so only the button
    // mounting the sheet is under test here.
    useAppStore.setState({
      client: fakeClient(),
      hubRole: 'owner',
    })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    await userEvent.click(screen.getByRole('button', { name: 'Chamber controls' }))
    expect(await screen.findByRole('dialog', { name: 'Chamber controls' })).toBeInTheDocument()
  })

  test('a guest is never shown the Controls button', async () => {
    useAppStore.setState({ client: fakeClient(), hubRole: 'invite' })
    render(<ConversationView chamberId="cham-a" />)
    await screen.findByText('msg-1')
    expect(screen.queryByRole('button', { name: 'Chamber controls' })).toBeNull()
  })
})

/**
 * The view under a real `HubRouter`. An attachment is the sharpest case: the
 * fetch has to reach the hub the chamber key names, *and* the URL it asks for
 * has to carry that hub's own chamber id — the `{hubId}:` prefix is the
 * router's business and means nothing on the wire.
 */
describe('app mode', () => {
  const hubA = makeHubAccount({
    url: 'http://a.local:1', token: 'ta', trust: { kind: 'plain-http' },
  })
  const hubB = makeHubAccount({
    url: 'http://b.local:2', token: 'tb', trust: { kind: 'plain-http' },
  })
  const keyA = chamberKey(hubA.id, 'cham-a')

  const originalCreateObjectURL = URL.createObjectURL
  beforeEach(() => {
    URL.createObjectURL = (() => 'blob:mock-1') as typeof URL.createObjectURL
  })
  afterEach(() => {
    URL.createObjectURL = originalCreateObjectURL
  })

  /** Both hubs behind one router over a fetch that records every URL asked
   * for. Hub A's mailbox holds one message carrying `body`. */
  function enterAppMode(body: string): string[] {
    const calls: string[] = []
    const fetchFn = (async (url: RequestInfo | URL) => {
      const target = String(url)
      calls.push(target)
      if (target.endsWith('/messages')) {
        return new Response(
          JSON.stringify([
            {
              id: 'outbox/1.md', chamber_id: 'cham-a', direction: 'outbox', from: 'Agent',
              subject: '', body, timestamp: stamp(0), is_question: false,
            },
          ]),
          { status: 200 },
        )
      }
      return new Response(new Blob(['bytes']), {
        status: 200, headers: { 'Content-Type': 'image/png' },
      })
    }) as typeof fetch
    useAppStore
      .getState()
      .initApp(
        [hubA, hubB],
        new MemoryHubsBackend(),
        (h) => new HubClient({ token: h.token, baseUrl: h.url, fetch: fetchFn }),
      )
    useAppStore.setState({ chambers: [chamber({ id: keyA, hubId: hubA.id })] })
    return calls
  }

  test('a chamber-relative image is fetched from that hub under its plain id', async () => {
    const calls = enterAppMode('![plot.png](artwork/plot.png)')
    const { container } = render(<ConversationView chamberId={keyA} />)
    await waitFor(() => expect(container.querySelector('img')).not.toBeNull())
    const img = container.querySelector('img')!
    await waitFor(() => expect(img.getAttribute('src')).toBe('blob:mock-1'))
    expect(calls).toContain('http://a.local:1/api/chambers/cham-a/messages')
    expect(calls).toContain(
      'http://a.local:1/api/chambers/cham-a/file?path=artwork%2Fplot.png',
    )
    expect(calls.some((c) => c.includes('b.local'))).toBe(false)
  })

  test('a hub-minted attachment URL is fetched from the same hub', async () => {
    const calls = enterAppMode('![plot.png](/api/chambers/cham-a/files/plot.png)')
    const { container } = render(<ConversationView chamberId={keyA} />)
    await waitFor(() => expect(container.querySelector('img')).not.toBeNull())
    await waitFor(() =>
      expect(container.querySelector('img')!.getAttribute('src')).toBe('blob:mock-1'),
    )
    expect(calls).toContain('http://a.local:1/api/chambers/cham-a/files/plot.png')
  })
})
