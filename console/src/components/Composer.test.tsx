import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Composer } from './Composer'
import { useAppStore, resetAppStore } from '../store/appStore'
import { draftKey } from '../lib/outbox'
import { attachmentMarkdown } from '../lib/attachments'
import { ApiError } from '../api/types'
import type { HubClient } from '../api/hubClient'

function fakeClient(overrides: Partial<Record<keyof HubClient, unknown>> = {}) {
  return {
    sendMessage: vi.fn(async () => ({ id: 'inbox/42.md' })),
    ...overrides,
  } as unknown as HubClient
}

beforeEach(() => {
  resetAppStore()
  // Drafts outlive the store reset by design; clear them so each test starts
  // from an empty composer.
  localStorage.clear()
  useAppStore.setState({
    creds: { token: 'k', name: 'me@b.c', role: 'owner' },
  })
})

test('a 401 send is not offered as a retry — the client already signed out', async () => {
  // Marking it failed would invite the user to tap retry against a token the
  // hub has already refused.
  const client = fakeClient({
    sendMessage: vi.fn().mockRejectedValue(new ApiError(401, 'HTTP 401')),
  })
  useAppStore.setState({ client })
  render(<Composer chamberId="cham-a" />)
  const box = screen.getByRole('textbox')
  await userEvent.type(box, 'do it')
  await userEvent.click(screen.getByRole('button', { name: /send/i }))
  await waitFor(() => expect(client.sendMessage).toHaveBeenCalled())
  expect(useAppStore.getState().outboxByChamber['cham-a']).toMatchObject([{ state: 'sending' }])
})

test('a non-auth send failure leaves a failed outbox item, not restored text', async () => {
  const client = fakeClient({
    sendMessage: vi.fn().mockRejectedValue(new Error('boom')),
  })
  useAppStore.setState({ client })
  render(<Composer chamberId="cham-a" />)
  const box = screen.getByRole('textbox')
  await userEvent.type(box, 'keep me')
  await userEvent.click(screen.getByRole('button', { name: /send/i }))
  await waitFor(() =>
    expect(useAppStore.getState().outboxByChamber['cham-a']).toMatchObject([
      { body: 'keep me', state: 'failed' },
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
  render(<Composer chamberId="cham-a" />)
  await userEvent.type(screen.getByRole('textbox'), 'ship it')
  await userEvent.click(screen.getByRole('button', { name: /send/i }))
  // Queued optimistically; it stays as a `sent` bubble until the thread itself
  // shows the message, so nothing disappears into that gap.
  await waitFor(() =>
    expect(useAppStore.getState().outboxByChamber['cham-a']).toMatchObject([
      { body: 'ship it', state: 'sent' },
    ]),
  )
  expect(client.sendMessage).toHaveBeenCalledWith('cham-a', 'ship it')
})

describe('per-chamber drafts', () => {
  // Spelled out rather than built with draftKey(), so the stored key shape is
  // actually pinned by a test. The account segment is the fingerprint of the
  // token (token 'k'), never the display name — names are reusable; the last
  // segment is the hub's own chamber id.
  const KEY_1 = 'agent-console.draft.hub|ee0c38ea156277d1.cham-a'
  const KEY_2 = 'agent-console.draft.hub|ee0c38ea156277d1.cham-b'
  const OTHER_CREDS = { token: 'tok', name: 'Alice', role: 'owner' as const }

  test('typing persists a draft under the chamber key', async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<Composer chamberId="cham-a" />)
    await userEvent.type(screen.getByRole('textbox'), 'half a thought')
    await waitFor(() => expect(localStorage.getItem(KEY_1)).toBe('half a thought'))
  })

  test('remounting restores this chamber draft and not another chamber one', () => {
    localStorage.setItem(KEY_1, 'alpha draft')
    localStorage.setItem(KEY_2, 'beta draft')
    useAppStore.setState({ client: fakeClient() })
    const { unmount } = render(<Composer chamberId="cham-a" />)
    expect(screen.getByRole('textbox')).toHaveValue('alpha draft')
    unmount()
    render(<Composer chamberId="cham-b" />)
    expect(screen.getByRole('textbox')).toHaveValue('beta draft')
  })

  test('sending clears the draft', async () => {
    useAppStore.setState({ client: fakeClient() })
    render(<Composer chamberId="cham-a" />)
    await userEvent.type(screen.getByRole('textbox'), 'go')
    await userEvent.click(screen.getByRole('button', { name: /send/i }))
    await waitFor(() => expect(localStorage.getItem(KEY_1)).toBeNull())
  })

  test('emptying the box drops the draft', async () => {
    localStorage.setItem(KEY_1, 'stale')
    useAppStore.setState({ client: fakeClient() })
    render(<Composer chamberId="cham-a" />)
    await userEvent.clear(screen.getByRole('textbox'))
    await waitFor(() => expect(localStorage.getItem(KEY_1)).toBeNull())
  })

  test('another account never picks up the draft of the same chamber', async () => {
    localStorage.setItem(KEY_1, 'work in progress')
    useAppStore.setState({ client: fakeClient(), creds: OTHER_CREDS })
    render(<Composer chamberId="cham-a" />)
    // A chamber id is shared across tokens, but a draft is not: it is the
    // other account's unsent work and must stay theirs.
    expect(screen.getByRole('textbox')).toHaveValue('')
    await userEvent.type(screen.getByRole('textbox'), 'other work')
    await waitFor(() =>
      expect(localStorage.getItem('agent-console.draft.hub|a210d45de0363526.cham-a')).toBe(
        'other work',
      ),
    )
    expect(localStorage.getItem(KEY_1)).toBe('work in progress')
  })

  test('app mode has no session token, so its drafts share one namespace', async () => {
    // The chamber key already carries the hub, so `app` cannot collide across
    // hubs the way a bare chamber id would.
    useAppStore.setState({ mode: 'app', creds: null, client: fakeClient() })
    render(<Composer chamberId="aaaaaaaa:cham-a" />)
    await userEvent.type(screen.getByRole('textbox'), 'from the app')
    await waitFor(() =>
      expect(localStorage.getItem(draftKey('app', 'aaaaaaaa:cham-a'))).toBe('from the app'),
    )
    expect(localStorage.getItem('agent-console.draft..aaaaaaaa:cham-a')).toBeNull()
  })
})

describe('file upload', () => {
  const attach = (): HTMLInputElement =>
    screen.getByLabelText('Attach file', { selector: 'input' }) as HTMLInputElement

  test('picked files upload in order and stay out of the textarea', async () => {
    const order: string[] = []
    const client = fakeClient({
      uploadFile: vi.fn(async (file: File) => {
        order.push(file.name)
        return `/api/chambers/cham-a/files/${file.name}`
      }),
    })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    const box = screen.getByRole('textbox')
    const first = new File(['a'], 'a.txt', { type: 'text/plain' })
    const second = new File(['b'], 'b.pdf', { type: 'application/pdf' })
    await userEvent.upload(attach(), [first, second])
    await waitFor(() => expect(client.uploadFile).toHaveBeenCalledTimes(2))
    expect(box).toHaveValue('')
    const attachments = screen.getByRole('list', { name: 'Attachments' })
    expect(attachments).toHaveTextContent('a.txt')
    expect(attachments).toHaveTextContent('b.pdf')
    expect(order).toEqual(['a.txt', 'b.pdf'])
    expect(client.uploadFile).toHaveBeenNthCalledWith(1, first, 'cham-a')
    expect(client.uploadFile).toHaveBeenNthCalledWith(2, second, 'cham-a')
  })

  test('drop and paste use the staged upload path', async () => {
    const client = fakeClient({
      uploadFile: vi.fn(async (file: File) => `/api/chambers/cham-a/files/${file.name}`),
    })
    useAppStore.setState({ client })
    const { container } = render(<Composer chamberId="cham-a" />)
    const dock = container.querySelector('.composer-dock')!
    const dropped = new File(['a'], 'drop.txt', { type: 'text/plain' })
    const pasted = new File(['b'], 'paste.pdf', { type: 'application/pdf' })
    const dataTransfer = { files: [dropped], types: ['Files'] }
    fireEvent.dragOver(dock, { dataTransfer })
    expect(dock).toHaveClass('is-drop')
    fireEvent.drop(dock, { dataTransfer })
    fireEvent.paste(screen.getByRole('textbox'), { clipboardData: { files: [pasted] } })
    await waitFor(() => expect(client.uploadFile).toHaveBeenCalledTimes(2))
    expect(dock).not.toHaveClass('is-drop')
    expect(screen.getByRole('list', { name: 'Attachments' })).toHaveTextContent('drop.txt')
    expect(screen.getByRole('list', { name: 'Attachments' })).toHaveTextContent('paste.pdf')
    expect(screen.getByRole('textbox')).toHaveValue('')
  })

  test('an image has a local preview which is revoked when removed', async () => {
    const createObjectURL = vi.fn(() => 'blob:preview')
    const revokeObjectURL = vi.fn()
    vi.stubGlobal('URL', { ...URL, createObjectURL, revokeObjectURL })
    const client = fakeClient({
      uploadFile: vi.fn(async () => '/api/chambers/cham-a/files/ab_photo.png'),
    })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    const file = new File(['png'], 'photo.png', { type: 'image/png' })
    await userEvent.upload(attach(), file)
    expect(await screen.findByRole('img', { name: 'photo.png' })).toHaveAttribute('src', 'blob:preview')
    expect(createObjectURL).toHaveBeenCalledWith(file)
    await userEvent.click(screen.getByRole('button', { name: 'Remove photo.png' }))
    expect(screen.queryByRole('img', { name: 'photo.png' })).toBeNull()
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:preview')
    vi.unstubAllGlobals()
  })

  test('one upload may fail, later files continue, and retry repairs only the failed file', async () => {
    const client = fakeClient({
      uploadFile: vi.fn()
        .mockResolvedValueOnce('/api/chambers/cham-a/files/a.txt')
        .mockRejectedValueOnce(new ApiError(400, 'disk full'))
        .mockResolvedValueOnce('/api/chambers/cham-a/files/c.txt')
        .mockResolvedValueOnce('/api/chambers/cham-a/files/b.txt'),
    })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    await userEvent.upload(attach(), [
      new File(['a'], 'a.txt'),
      new File(['b'], 'b.txt'),
      new File(['c'], 'c.txt'),
    ])
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not upload b.txt. disk full')
    await waitFor(() => expect(client.uploadFile).toHaveBeenCalledTimes(3))
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled()
    await userEvent.click(screen.getByRole('button', { name: 'Retry b.txt' }))
    await waitFor(() => expect(client.uploadFile).toHaveBeenCalledTimes(4))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled())
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('click and Enter cannot send while an upload is pending', async () => {
    vi.stubGlobal('matchMedia', () => ({ matches: true }))
    let resolveUpload!: (uri: string) => void
    const client = fakeClient({
      uploadFile: vi.fn(() => new Promise<string>((r) => { resolveUpload = r })),
    })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'hello')
    const sendBtn = screen.getByRole('button', { name: /send/i })
    expect(sendBtn).toBeEnabled()
    await userEvent.upload(attach(), new File(['x'], 'a.txt', { type: 'text/plain' }))
    expect(sendBtn).toBeDisabled()
    expect(screen.getByRole('status')).toHaveTextContent(/uploading a\.txt/i)
    await userEvent.click(sendBtn)
    await userEvent.type(box, '{Enter}')
    expect(client.sendMessage).not.toHaveBeenCalled()
    expect(box).toHaveValue('hello')
    resolveUpload('/api/chambers/cham-a/files/a.txt')
    await waitFor(() => expect(sendBtn).toBeEnabled())
    expect(screen.queryByRole('status')).toBeNull()
    vi.unstubAllGlobals()
  })

  test('a ready attachment can send without text', async () => {
    const client = fakeClient({ uploadFile: vi.fn(async () => '/api/chambers/cham-a/files/a.txt') })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    await userEvent.upload(attach(), new File(['x'], 'a.txt', { type: 'text/plain' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled())
    await userEvent.click(screen.getByRole('button', { name: 'Send' }))
    await waitFor(() =>
      expect(client.sendMessage).toHaveBeenCalledWith(
        'cham-a',
        '[a.txt](</api/chambers/cham-a/files/a.txt> "attachment:1")',
      ),
    )
    expect(screen.queryByRole('list', { name: 'Attachments' })).toBeNull()
  })
})

test('attachment markdown escapes labels and unsafe target characters', () => {
  expect(attachmentMarkdown('a[b]\\c\n.png', '/files/a b>".png', 42)).toBe(
    '![a\\[b\\]\\\\c .png](</files/a%20b%3E%22.png> "attachment:42")',
  )
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
    render(<Composer chamberId="cham-a" />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'ship it{Enter}')
    await waitFor(() => expect(client.sendMessage).toHaveBeenCalledWith('cham-a', 'ship it'))
    expect(box).toHaveValue('')
  })

  test('Shift+Enter inserts a newline instead of sending', async () => {
    stubPointer(true)
    const client = fakeClient()
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'line one{Shift>}{Enter}{/Shift}line two')
    expect(box).toHaveValue('line one\nline two')
    expect(client.sendMessage).not.toHaveBeenCalled()
  })

  test('on a touch keyboard Enter is a newline, since there is no modifier', async () => {
    stubPointer(false)
    const client = fakeClient()
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'line one{Enter}line two')
    expect(box).toHaveValue('line one\nline two')
    expect(client.sendMessage).not.toHaveBeenCalled()
  })
})

test('text typed while an upload is pending stays unchanged', async () => {
  let resolveUpload!: (uri: string) => void
  const client = {
    uploadFile: vi.fn(() => new Promise<string>((r) => { resolveUpload = r })),
    sendMessage: vi.fn(),
  } as unknown as HubClient
  useAppStore.setState({ client })
  render(<Composer chamberId="cham-a" />)
  const box = screen.getByRole('textbox', { name: /message/i })
  await userEvent.type(box, 'first ')
  const file = new File(['x'], 'notes.txt', { type: 'text/plain' })
  await userEvent.upload(screen.getByLabelText(/attach file/i, { selector: 'input' }), file)
  // keep typing while the upload is in flight
  await userEvent.type(box, 'second ')
  resolveUpload('/api/chambers/cham-a/files/aa_notes.txt')
  await waitFor(() => expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled())
  expect(box).toHaveValue('first second ')
})

test('ready attachments survive a remount and send with the restored text', async () => {
  const client = fakeClient({ uploadFile: vi.fn(async () => '/api/chambers/cham-a/files/a.txt') })
  useAppStore.setState({ client })
  const { unmount } = render(<Composer chamberId="cham-a" />)
  await userEvent.type(screen.getByRole('textbox'), 'context')
  await userEvent.upload(
    screen.getByLabelText('Attach file', { selector: 'input' }) as HTMLInputElement,
    new File(['x'], 'a.txt', { type: 'text/plain' }),
  )
  await waitFor(() => expect(localStorage.getItem(
    'agent-console.draft.hub|ee0c38ea156277d1.cham-a.files',
  )).toContain('/api/chambers/cham-a/files/a.txt'))
  unmount()
  render(<Composer chamberId="cham-a" />)
  expect(screen.getByRole('textbox')).toHaveValue('context')
  expect(screen.getByRole('list', { name: 'Attachments' })).toHaveTextContent('a.txt')
  await userEvent.click(screen.getByRole('button', { name: 'Send' }))
  await waitFor(() => expect(client.sendMessage).toHaveBeenCalledWith(
    'cham-a',
    'context\n\n[a.txt](</api/chambers/cham-a/files/a.txt> "attachment:1")',
  ))
})

test('thread drafts and outbox sends keep their thread id', async () => {
  const client = fakeClient()
  useAppStore.setState({ client })
  const { unmount } = render(<Composer chamberId="cham-a" threadId="outbox/7.md" />)
  await userEvent.type(screen.getByRole('textbox', { name: 'Thread reply' }), 'reply later')
  await waitFor(() => expect(localStorage.getItem(
    'agent-console.draft.hub|ee0c38ea156277d1.cham-a.thread.outbox/7.md',
  )).toBe('reply later'))
  unmount()
  render(<Composer chamberId="cham-a" threadId="outbox/7.md" />)
  expect(screen.getByRole('textbox', { name: 'Thread reply' })).toHaveValue('reply later')
  await userEvent.click(screen.getByRole('button', { name: 'Send reply' }))
  await waitFor(() => expect(client.sendMessage).toHaveBeenCalledWith(
    'cham-a', 'reply later', 'outbox/7.md',
  ))
  expect(useAppStore.getState().outboxByChamber['cham-a']).toMatchObject([
    { body: 'reply later', threadId: 'outbox/7.md' },
  ])
})

test('upload posts to this chamber so hub uploads reach the right mailbox', async () => {
  const client = fakeClient({ uploadFile: vi.fn(async () => '/api/chambers/c1/files/a_a.txt') })
  useAppStore.setState({ client })
  render(<Composer chamberId="cham-a" />)
  await userEvent.upload(
    screen.getByLabelText('Attach file', { selector: 'input' }) as HTMLInputElement,
    new File(['x'], 'a.txt', { type: 'text/plain' }),
  )
  await waitFor(() =>
    expect(client.uploadFile).toHaveBeenCalledWith(expect.any(File), 'cham-a'),
  )
})
