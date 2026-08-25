import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Composer } from './Composer'
import { useAppStore, resetAppStore } from '../store/appStore'
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
})

describe('file upload', () => {
  const attach = (): HTMLInputElement =>
    screen.getByLabelText('Attach file', { selector: 'input' }) as HTMLInputElement

  test('successful upload inserts the markdown link at the caret', async () => {
    const client = fakeClient({
      uploadFile: vi.fn(async () => '/user_uploads/2/ab/report.pdf'),
    })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'see ')
    await userEvent.upload(attach(), new File(['pdf'], 'report.pdf', { type: 'application/pdf' }))
    await waitFor(() =>
      expect(box).toHaveValue('see [report.pdf](/user_uploads/2/ab/report.pdf)'),
    )
    expect(client.uploadFile).toHaveBeenCalledTimes(1)
  })

  test('picking two files uploads them sequentially and inserts both links in order', async () => {
    const order: string[] = []
    const client = fakeClient({
      uploadFile: vi.fn(async (file: File) => {
        order.push(file.name)
        return `/files/${file.name}`
      }),
    })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    const first = new File(['a'], 'a.txt', { type: 'text/plain' })
    const second = new File(['b'], 'b.pdf', { type: 'application/pdf' })
    await userEvent.upload(attach(), [first, second])
    await waitFor(() =>
      expect(screen.getByRole('textbox')).toHaveValue(
        '[a.txt](/files/a.txt) [b.pdf](/files/b.pdf)',
      ),
    )
    expect(order).toEqual(['a.txt', 'b.pdf'])
    expect(client.uploadFile).toHaveBeenNthCalledWith(1, first, 'cham-a')
    expect(client.uploadFile).toHaveBeenNthCalledWith(2, second, 'cham-a')
  })

  test('dropping files on the dock uses the same sequential upload path', async () => {
    const client = fakeClient({
      uploadFile: vi.fn(async (file: File) => `/files/${file.name}`),
    })
    useAppStore.setState({ client })
    const { container } = render(<Composer chamberId="cham-a" />)
    const dock = container.querySelector('.composer-dock')!
    const first = new File(['a'], 'a.txt', { type: 'text/plain' })
    const second = new File(['b'], 'b.pdf', { type: 'application/pdf' })
    const dataTransfer = { files: [first, second], types: ['Files'] }
    fireEvent.dragOver(dock, { dataTransfer })
    expect(dock).toHaveClass('is-drop')
    fireEvent.drop(dock, { dataTransfer })
    await waitFor(() =>
      expect(screen.getByRole('textbox')).toHaveValue(
        '[a.txt](/files/a.txt) [b.pdf](/files/b.pdf)',
      ),
    )
    expect(dock).not.toHaveClass('is-drop')
    expect(client.uploadFile).toHaveBeenCalledTimes(2)
  })

  test('an uploaded image is inserted as an embed so it previews inline', async () => {
    const client = fakeClient({
      uploadFile: vi.fn(async () => '/api/chambers/cham-a/files/ab_photo.png'),
    })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    const box = screen.getByRole('textbox')
    await userEvent.upload(attach(), new File(['png'], 'photo.png', { type: 'image/png' }))
    await waitFor(() =>
      expect(box).toHaveValue('![photo.png](/api/chambers/cham-a/files/ab_photo.png)'),
    )
  })

  test('failed upload shows the server message and leaves text unchanged', async () => {
    const client = fakeClient({
      uploadFile: vi.fn().mockRejectedValue(new ApiError(400, 'File too large')),
    })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    const box = screen.getByRole('textbox')
    await userEvent.type(box, 'keep this')
    await userEvent.upload(attach(), new File(['x'], 'big.pdf', { type: 'application/pdf' }))
    expect(await screen.findByText(/Could not upload big\.pdf\. File too large/)).toBeInTheDocument()
    expect(box).toHaveValue('keep this')
  })

  test('a failed file aborts the remaining uploads', async () => {
    const client = fakeClient({
      uploadFile: vi
        .fn()
        .mockResolvedValueOnce('/files/a.txt')
        .mockRejectedValueOnce(new Error('disk full')),
    })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
    await userEvent.upload(attach(), [
      new File(['a'], 'a.txt'),
      new File(['b'], 'b.txt'),
      new File(['c'], 'c.txt'),
    ])
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not upload b.txt. disk full',
    )
    expect(client.uploadFile).toHaveBeenCalledTimes(2)
    expect(screen.getByRole('textbox')).toHaveValue('[a.txt](/files/a.txt)')
  })

  test('send is disabled while uploading and re-enabled after', async () => {
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
    resolveUpload('/user_uploads/1/aa/a.txt')
    await waitFor(() => expect(sendBtn).toBeEnabled())
    expect(screen.queryByRole('status')).toBeNull()
    expect(box).toHaveValue('hello [a.txt](/user_uploads/1/aa/a.txt)')
  })

  test('re-picking the same file works (input is reset)', async () => {
    const client = fakeClient({ uploadFile: vi.fn(async () => '/user_uploads/1/aa/a.txt') })
    useAppStore.setState({ client })
    render(<Composer chamberId="cham-a" />)
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

test('text typed while an upload is pending survives link insertion', async () => {
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
  await waitFor(() =>
    expect(box).toHaveValue(
      'first second [notes.txt](/api/chambers/cham-a/files/aa_notes.txt)',
    ),
  )
})

test('a draft is kept per chamber, keyed by the hub id', async () => {
  useAppStore.setState({ client: fakeClient() })
  const { unmount } = render(<Composer chamberId="cham-a" />)
  await userEvent.type(screen.getByRole('textbox'), 'half a sentence')
  await waitFor(() =>
    expect(Object.keys(localStorage).some((k) => k.endsWith('.cham-a'))).toBe(true),
  )
  unmount()
  render(<Composer chamberId="cham-b" />)
  expect(screen.getByRole('textbox')).toHaveValue('')
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
