import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Composer, filterUsers, mentionQueryAt } from './Composer'
import { useAppStore, resetAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'
import { ZulipApiError, type ZulipClient } from '../api/client'
import type { ZulipUser } from '../api/types'

function fakeClient(overrides: Partial<Record<keyof ZulipClient, unknown>> = {}) {
  return {
    sendMessage: vi.fn(async () => 42),
    getUsers: vi.fn(async () => []),
    ...overrides,
  } as unknown as ZulipClient
}

const users: ZulipUser[] = [
  { user_id: 1, full_name: 'Alice Doe', email: 'alice@b.c', is_bot: false },
  { user_id: 2, full_name: 'Alex Mercer', email: 'alex@b.c', is_bot: false },
  { user_id: 3, full_name: 'Bob', email: 'bob@b.c', is_bot: false },
]

beforeEach(() => {
  resetAppStore()
  // Drafts outlive the store reset by design; clear them so each test starts
  // from an empty composer.
  localStorage.clear()
  useAppStore.setState({
    creds: { prefix: '/zulip/qec', email: 'me@b.c', apiKey: 'k', sendTopic: '' },
  })
})

test('401 send triggers logout with the auth reason', async () => {
  const client = fakeClient({
    sendMessage: vi.fn().mockRejectedValue(new ZulipApiError('Invalid API key', 401)),
  })
  useAppStore.setState({ client })
  render(<Composer streamName="alpha" streamId={1} />)
  const box = screen.getByRole('textbox')
  await userEvent.type(box, 'do it')
  await userEvent.click(screen.getByRole('button', { name: /send/i }))
  await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
  expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
})

test('a non-auth send failure leaves a failed outbox item, not restored text', async () => {
  const client = fakeClient({
    sendMessage: vi.fn().mockRejectedValue(new Error('boom')),
  })
  useAppStore.setState({ client })
  render(<Composer streamName="alpha" streamId={1} />)
  const box = screen.getByRole('textbox')
  await userEvent.type(box, 'keep me')
  await userEvent.click(screen.getByRole('button', { name: /send/i }))
  await waitFor(() =>
    expect(useAppStore.getState().outboxByStream[1]).toMatchObject([
      { content: 'keep me', state: 'failed' },
    ]),
  )
  // The pending bubble owns the text now — putting it back in the composer too
  // would offer the user two ways to send the same message.
  expect(box).toHaveValue('')
  expect(useAppStore.getState().creds).not.toBeNull()
})

test('a successful send moves its outbox item to sent, not away', async () => {
  const client = fakeClient()
  useAppStore.setState({ client })
  render(<Composer streamName="alpha" streamId={1} />)
  await userEvent.type(screen.getByRole('textbox'), 'ship it')
  await userEvent.click(screen.getByRole('button', { name: /send/i }))
  // Queued optimistically; it stays as a `sent` bubble until the thread itself
  // shows the message, so nothing disappears into that gap.
  await waitFor(() =>
    expect(useAppStore.getState().outboxByStream[1]).toMatchObject([
      { content: 'ship it', state: 'sent' },
    ]),
  )
  expect(client.sendMessage).toHaveBeenCalledWith('alpha', 'ship it')
})

describe('per-project drafts', () => {
  // Spelled out rather than built with draftKey(), so the stored key shape is
  // actually pinned by a test.
  const ZULIP_1 = 'zulip-app.draft.zulip|/zulip/qec|me@b.c.1'
  const ZULIP_2 = 'zulip-app.draft.zulip|/zulip/qec|me@b.c.2'
  const HUB_CREDS = { kind: 'hub' as const, prefix: '', email: 'Alice', apiKey: 'tok', sendTopic: '' }

  test('typing persists a draft under the project key', async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<Composer streamName="alpha" streamId={1} />)
    await userEvent.type(screen.getByRole('textbox'), 'half a thought')
    await waitFor(() => expect(localStorage.getItem(ZULIP_1)).toBe('half a thought'))
  })

  test('remounting restores this project draft and not another project one', () => {
    localStorage.setItem(ZULIP_1, 'alpha draft')
    localStorage.setItem(ZULIP_2, 'beta draft')
    useAppStore.setState({ client: fakeClient() })
    const { unmount } = render(<Composer streamName="alpha" streamId={1} />)
    expect(screen.getByRole('textbox')).toHaveValue('alpha draft')
    unmount()
    render(<Composer streamName="beta" streamId={2} />)
    expect(screen.getByRole('textbox')).toHaveValue('beta draft')
  })

  test('sending clears the draft', async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<Composer streamName="alpha" streamId={1} />)
    await userEvent.type(screen.getByRole('textbox'), 'go')
    await userEvent.click(screen.getByRole('button', { name: /send/i }))
    await waitFor(() => expect(localStorage.getItem(ZULIP_1)).toBeNull())
  })

  test('emptying the box drops the draft', async () => {
    localStorage.setItem(ZULIP_1, 'stale')
    useAppStore.setState({ client: fakeClient() })
    render(<Composer streamName="alpha" streamId={1} />)
    await userEvent.clear(screen.getByRole('textbox'))
    await waitFor(() => expect(localStorage.getItem(ZULIP_1)).toBeNull())
  })

  test('a hub chamber never picks up the Zulip draft of the same stream number', async () => {
    localStorage.setItem(ZULIP_1, 'zulip work in progress')
    useAppStore.setState({ client: fakeClient(), creds: HUB_CREDS })
    render(<Composer streamName="alpha" streamId={1} />)
    // Hub chamber 1 and Zulip stream 1 are different projects entirely.
    expect(screen.getByRole('textbox')).toHaveValue('')
    await userEvent.type(screen.getByRole('textbox'), 'hub work')
    await waitFor(() =>
      expect(localStorage.getItem('zulip-app.draft.hub||Alice.1')).toBe('hub work'),
    )
    expect(localStorage.getItem(ZULIP_1)).toBe('zulip work in progress')
  })
})

describe('mention query helpers', () => {
  test('mentionQueryAt only matches right after an @', () => {
    expect(mentionQueryAt('ping @al', 8)).toBe('al')
    expect(mentionQueryAt('ping @', 6)).toBe('')
    expect(mentionQueryAt('plain words', 11)).toBeNull()
    expect(mentionQueryAt('ping @al extra', 14)).toBe('al extra')
    // an @ glued to a word (email/identifier) is not a mention trigger
    expect(mentionQueryAt('foo@', 4)).toBeNull()
    expect(mentionQueryAt('mail me at a@b.c', 13)).toBeNull()
  })

  test('filterUsers lists prefix matches first, then includes, capped at 8', () => {
    const list: ZulipUser[] = [
      { user_id: 1, full_name: 'Alex', email: 'a@b.c', is_bot: false },
      { user_id: 2, full_name: 'Vitaly', email: 'v@b.c', is_bot: false },
      { user_id: 3, full_name: 'Alice', email: 'al@b.c', is_bot: false },
      { user_id: 4, full_name: 'Zed', email: 'z@b.c', is_bot: false },
    ]
    expect(filterUsers(list, 'al').map((u) => u.full_name)).toEqual(['Alex', 'Alice', 'Vitaly'])
    const many: ZulipUser[] = Array.from({ length: 12 }, (_, i) => ({
      user_id: i, full_name: `User ${i}`, email: '', is_bot: false,
    }))
    expect(filterUsers(many, '')).toHaveLength(8)
  })
})

describe('@-mention autocomplete', () => {
  test('typing @al opens a filtered panel; Enter inserts the Zulip mention', async () => {
    const client = fakeClient({ getUsers: vi.fn(async () => users) })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'ping @al')
    const options = await screen.findAllByRole('option')
    expect(options.map((o) => o.textContent)).toEqual(['Alice Doe', 'Alex Mercer'])
    expect(client.getUsers).toHaveBeenCalledTimes(1)
    await userEvent.keyboard('{Enter}')
    expect(box).toHaveValue('ping @**Alice Doe** ')
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  test('ArrowDown moves the active row and Enter confirms it', async () => {
    const client = fakeClient({ getUsers: vi.fn(async () => users) })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'ping @al')
    await screen.findAllByRole('option')
    await userEvent.keyboard('{ArrowDown}')
    const options = screen.getAllByRole('option')
    expect(options[0].className).not.toContain('active')
    expect(options[1].className).toContain('active')
    await userEvent.keyboard('{Enter}')
    expect(box).toHaveValue('ping @**Alex Mercer** ')
  })

  test('Tab confirms the selected user', async () => {
    const client = fakeClient({ getUsers: vi.fn(async () => users) })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, '@al')
    await screen.findAllByRole('option')
    await userEvent.tab()
    expect(box).toHaveValue('@**Alice Doe** ')
  })

  test('click confirms the clicked user', async () => {
    const client = fakeClient({ getUsers: vi.fn(async () => users) })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'ping @al')
    const options = await screen.findAllByRole('option')
    await userEvent.click(options[1])
    expect(box).toHaveValue('ping @**Alex Mercer** ')
  })

  test('Escape closes the panel without inserting', async () => {
    const client = fakeClient({ getUsers: vi.fn(async () => users) })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'ping @al')
    await screen.findAllByRole('option')
    await userEvent.keyboard('{Escape}')
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(box).toHaveValue('ping @al')
  })

  test('no panel opens while typing plain words without @', async () => {
    const client = fakeClient({ getUsers: vi.fn(async () => users) })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'hello world')
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(client.getUsers).not.toHaveBeenCalled()
  })

  test('panel closes once the @ is deleted', async () => {
    const client = fakeClient({ getUsers: vi.fn(async () => users) })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, '@al')
    await screen.findAllByRole('option')
    await userEvent.keyboard('{Backspace}{Backspace}{Backspace}')
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  test('send still sends the mention text after autocomplete', async () => {
    const client = fakeClient({ getUsers: vi.fn(async () => users) })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'ping @al')
    await screen.findAllByRole('option')
    await userEvent.keyboard('{Enter}')
    await userEvent.click(screen.getByRole('button', { name: /send/i }))
    await waitFor(() => expect(box).toHaveValue(''))
    expect(client.sendMessage).toHaveBeenCalledWith('alpha', 'ping @**Alice Doe** ')
  })
})

describe('file upload', () => {
  const attach = (): HTMLInputElement =>
    screen.getByLabelText('Attach file', { selector: 'input' }) as HTMLInputElement

  test('successful upload inserts the markdown link at the caret', async () => {
    const client = fakeClient({
      uploadFile: vi.fn(async () => '/user_uploads/2/ab/report.pdf'),
    })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'see ')
    await userEvent.upload(attach(), new File(['pdf'], 'report.pdf', { type: 'application/pdf' }))
    await waitFor(() =>
      expect(box).toHaveValue('see [report.pdf](/user_uploads/2/ab/report.pdf)'),
    )
    expect(client.uploadFile).toHaveBeenCalledTimes(1)
  })

  test('failed upload shows the server message and leaves text unchanged', async () => {
    const client = fakeClient({
      uploadFile: vi.fn().mockRejectedValue(new ZulipApiError('File too large', 400)),
    })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'keep this')
    await userEvent.upload(attach(), new File(['x'], 'big.pdf', { type: 'application/pdf' }))
    expect(await screen.findByText('File too large')).toBeInTheDocument()
    expect(box).toHaveValue('keep this')
  })

  test('send is disabled while uploading and re-enabled after', async () => {
    let resolveUpload!: (uri: string) => void
    const client = fakeClient({
      uploadFile: vi.fn(() => new Promise<string>((r) => { resolveUpload = r })),
    })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'hello')
    const sendBtn = screen.getByRole('button', { name: /send/i })
    expect(sendBtn).toBeEnabled()
    await userEvent.upload(attach(), new File(['x'], 'a.txt', { type: 'text/plain' }))
    expect(sendBtn).toBeDisabled()
    expect(screen.getByRole('status')).toHaveTextContent(/uploading a\.txt/i)
    resolveUpload('/user_uploads/1/aa/a.txt')
    await waitFor(() => expect(sendBtn).toBeEnabled())
    expect(screen.queryByRole('status')).toBeNull()
    expect(box).toHaveValue('hello [a.txt](/user_uploads/1/aa/a.txt)')
  })

  test('re-picking the same file works (input is reset)', async () => {
    const client = fakeClient({ uploadFile: vi.fn(async () => '/user_uploads/1/aa/a.txt') })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    const input = attach()
    const f = new File(['x'], 'a.txt', { type: 'text/plain' })
    await userEvent.upload(input, f)
    await waitFor(() => expect(box).toHaveValue('[a.txt](/user_uploads/1/aa/a.txt)'))
    await userEvent.upload(input, f)
    await waitFor(() =>
      expect(box).toHaveValue(
        '[a.txt](/user_uploads/1/aa/a.txt) [a.txt](/user_uploads/1/aa/a.txt)',
      ),
    )
  })
})

describe('Enter to send', () => {
  /** Pretend a hardware keyboard is attached (or not). */
  function stubPointer(fine: boolean) {
    vi.stubGlobal('matchMedia', (query: string) => ({
      matches: fine,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }))
  }

  afterEach(() => vi.unstubAllGlobals())

  test('Enter sends on a hardware keyboard', async () => {
    stubPointer(true)
    const client = fakeClient()
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'ship it{Enter}')
    await waitFor(() => expect(client.sendMessage).toHaveBeenCalledWith('alpha', 'ship it'))
    expect(box).toHaveValue('')
  })

  test('Shift+Enter inserts a newline instead of sending', async () => {
    stubPointer(true)
    const client = fakeClient()
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'line one{Shift>}{Enter}{/Shift}line two')
    expect(box).toHaveValue('line one\nline two')
    expect(client.sendMessage).not.toHaveBeenCalled()
  })

  test('on a touch keyboard Enter is a newline, since there is no modifier', async () => {
    stubPointer(false)
    const client = fakeClient()
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'line one{Enter}line two')
    expect(box).toHaveValue('line one\nline two')
    expect(client.sendMessage).not.toHaveBeenCalled()
  })

  test('Enter with the mention panel open confirms the mention, never sends', async () => {
    stubPointer(true)
    const client = fakeClient({ getUsers: vi.fn(async () => users) })
    useAppStore.setState({ client })
    render(<Composer streamName="alpha" streamId={1} />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'ping @al')
    await screen.findAllByRole('option')
    await userEvent.keyboard('{Enter}')
    expect(box).toHaveValue('ping @**Alice Doe** ')
    expect(client.sendMessage).not.toHaveBeenCalled()
  })
})


test('text typed while an upload is pending survives link insertion', async () => {
  let resolveUpload!: (uri: string) => void
  const client = {
    uploadFile: vi.fn(() => new Promise<string>((r) => { resolveUpload = r })),
    sendMessage: vi.fn(),
    getUsers: vi.fn(async () => []),
  } as unknown as ZulipClient
  useAppStore.setState({ client })
  render(<Composer streamName="alpha" streamId={1} />)
  const box = screen.getByRole('textbox', { name: /message/i })
  await userEvent.type(box, 'first ')
  const file = new File(['x'], 'notes.txt', { type: 'text/plain' })
  await userEvent.upload(screen.getByLabelText(/attach file/i, { selector: 'input' }), file)
  // keep typing while the upload is in flight
  await userEvent.type(box, 'second ')
  resolveUpload('/user_uploads/1/aa/notes.txt')
  await waitFor(() => expect(box).toHaveValue('first second [notes.txt](/user_uploads/1/aa/notes.txt)'))
})

describe('mention candidates without a user directory (hub)', () => {
  test('suggests senders seen in the conversation when the user list is empty', async () => {
    const client = fakeClient({ getUsers: vi.fn(async () => []) })
    useAppStore.setState({
      client,
      users: null,
      creds: { kind: 'hub', prefix: '', email: 'Alice', apiKey: 'tok', sendTopic: '' },
      streams: [{ stream_id: 1, name: 'alpha', description: '' }],
      messagesByStream: {
        1: [
          { id: 1, sender_full_name: 'agent', sender_email: 'agent', timestamp: 1,
            content: 'one', stream_id: 1, subject: '' },
          { id: 2, sender_full_name: 'agent', sender_email: 'agent', timestamp: 2,
            content: 'two', stream_id: 1, subject: '' },
        ],
      },
    })
    render(<Composer streamName="alpha" streamId={1} />)
    await userEvent.type(screen.getByRole('textbox'), '@ag')
    const options = await screen.findAllByRole('option')
    expect(options).toHaveLength(1)
    expect(options[0]).toHaveTextContent('agent')
  })
})

test('upload passes the stream name so hub uploads reach the right chamber', async () => {
  const client = fakeClient({ uploadFile: vi.fn(async () => '/api/chambers/c1/files/a_a.txt') })
  useAppStore.setState({ client })
  render(<Composer streamName="alpha" streamId={1} />)
  await userEvent.upload(
    screen.getByLabelText('Attach file', { selector: 'input' }) as HTMLInputElement,
    new File(['x'], 'a.txt', { type: 'text/plain' }),
  )
  await waitFor(() =>
    expect(client.uploadFile).toHaveBeenCalledWith(expect.any(File), 'alpha'),
  )
})
