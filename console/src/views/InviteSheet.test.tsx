import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { InviteSheet, defaultInviteLabel } from './InviteSheet'
import { HubClient, type Invite } from '../api/hubClient'
import { useAppStore, resetAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'
import { ApiError } from '../api/errors'
import type { Credentials } from '../api/types'

const creds: Credentials = { kind: 'hub', prefix: '', email: 'Owner', apiKey: 'k', sendTopic: '' }
const NEW_TOKEN = 'ff'.repeat(16)

const ALICE: Invite = {
  name: 'Alice', chambers: ['cham-a'], created_at: '2026-08-15T10:00:00Z', revoked_at: null,
}
const BOTH: Invite = {
  name: 'Bob', chambers: ['cham-a', 'cham-b'], created_at: '2026-08-15T09:00:00Z', revoked_at: null,
}
const GONE: Invite = {
  name: 'Carol', chambers: ['cham-a'], created_at: '2026-08-01T09:00:00Z',
  revoked_at: '2026-08-10T09:00:00Z',
}

/** URLs of every request the client made, so a test can prove the token never
 *  rode in one. */
let urls: string[]

function makeHub(invites: Invite[] = [ALICE, BOTH, GONE]): HubClient {
  urls = []
  const fetchFn = vi.fn(async (url: string, init?: RequestInit) => {
    urls.push(String(url))
    if (String(url).endsWith('/api/tokens') && init?.method === 'POST') {
      return new Response(JSON.stringify({ ok: true, name: 'x', token: NEW_TOKEN }), { status: 200 })
    }
    if (String(url).includes('/revoke')) {
      return new Response(JSON.stringify({ ok: true }), { status: 200 })
    }
    return new Response(JSON.stringify({ invites }), { status: 200 })
  })
  const client = new HubClient(creds, fetchFn as unknown as typeof fetch)
  vi.spyOn(client, 'chamberIdFor').mockImplementation((sid) =>
    sid === 1 ? 'cham-a' : sid === 2 ? 'cham-b' : undefined,
  )
  return client
}

let writeText: ReturnType<typeof vi.fn>

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    creds,
    client: makeHub(),
    hubRole: 'owner',
    streams: [
      { stream_id: 1, name: 'alpha', description: '' },
      { stream_id: 2, name: 'beta', description: '' },
    ],
  })
  writeText = vi.fn(async () => {})
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
})

function renderSheet() {
  return render(<InviteSheet chamberId="cham-a" chamberName="alpha" onClose={() => {}} />)
}

test('titles the sheet for this chamber and lists only its active invites', async () => {
  renderSheet()
  expect(screen.getByRole('heading', { name: 'Invite to alpha' })).toBeInTheDocument()
  const rows = await screen.findAllByRole('listitem')
  expect(rows).toHaveLength(2)
  expect(rows[0]).toHaveTextContent('Alice')
  expect(rows[0]).toHaveTextContent('added')
  // A revoked invite is not "people with access".
  expect(screen.queryByText('Carol')).toBeNull()
})

test('a multi-chamber invite says where else it reaches', async () => {
  renderSheet()
  const rows = await screen.findAllByRole('listitem')
  const bob = rows.find((r) => r.textContent?.includes('Bob'))!
  expect(bob).toHaveTextContent('also: beta')
  const alice = rows.find((r) => r.textContent?.includes('Alice'))!
  expect(alice).not.toHaveTextContent('also:')
})

test('the empty state invites the owner to copy a link', async () => {
  useAppStore.setState({ client: makeHub([]) })
  renderSheet()
  expect(
    await screen.findByText('Nobody else has access. Copy a link to invite someone.'),
  ).toBeInTheDocument()
})

test('copy mints a chamber-scoped link, copies it, and never puts it in a URL', async () => {
  const hub = useAppStore.getState().client as HubClient
  const create = vi.spyOn(hub, 'createInvite')
  renderSheet()
  await screen.findAllByRole('listitem')
  await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))

  const link = `${window.location.origin}/#invite=${NEW_TOKEN}`
  const field = (await screen.findByLabelText('Invite link')) as HTMLInputElement
  expect(field).toHaveValue(link)
  expect(field.readOnly).toBe(true)
  expect(create).toHaveBeenCalledWith('guest-1', ['cham-a'])
  expect(writeText).toHaveBeenCalledWith(link)
  expect(await screen.findByText('Copied')).toBeInTheDocument()
  // The token is a credential: it may never appear in a request line.
  expect(urls.some((u) => u.includes(NEW_TOKEN))).toBe(false)
})

test('a typed label names the invite instead of guest-N', async () => {
  const hub = useAppStore.getState().client as HubClient
  const create = vi.spyOn(hub, 'createInvite')
  renderSheet()
  await screen.findAllByRole('listitem')
  await userEvent.type(screen.getByLabelText('Who is this for?'), '  Mei  ')
  await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
  expect(create).toHaveBeenCalledWith('Mei', ['cham-a'])
})

test('guest-N skips names already taken in this chamber', () => {
  expect(defaultInviteLabel([])).toBe('guest-1')
  expect(defaultInviteLabel([{ ...ALICE, name: 'guest-1' }])).toBe('guest-2')
  expect(
    defaultInviteLabel([{ ...ALICE, name: 'guest-1' }, { ...ALICE, name: 'guest-2' }]),
  ).toBe('guest-3')
})

test('a clipboard that refuses says so and keeps the link selectable', async () => {
  writeText.mockRejectedValue(new Error('denied'))
  renderSheet()
  await screen.findAllByRole('listitem')
  await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
  expect(await screen.findByText('Copy failed — select and copy')).toBeInTheDocument()
  expect(screen.queryByText('Copied')).toBeNull()
  expect(await screen.findByLabelText('Invite link')).toHaveValue(
    `${window.location.origin}/#invite=${NEW_TOKEN}`,
  )
})

test('remove asks first, then revokes and re-reads the list', async () => {
  const hub = useAppStore.getState().client as HubClient
  const revoke = vi.spyOn(hub, 'revokeInvite')
  const list = vi.spyOn(hub, 'listInvites')
  renderSheet()
  const rows = await screen.findAllByRole('listitem')
  const alice = rows.find((r) => r.textContent?.includes('Alice'))!
  await userEvent.click(within(alice).getByRole('button', { name: 'Remove' }))
  expect(revoke).not.toHaveBeenCalled()
  expect(
    screen.getByText('Remove Alice? Their link stops working immediately.'),
  ).toBeInTheDocument()

  await userEvent.click(screen.getByRole('button', { name: 'Remove Alice' }))
  expect(revoke).toHaveBeenCalledWith('Alice')
  await waitFor(() => expect(list).toHaveBeenCalledTimes(2))
})

test('cancelling the confirm leaves the invite alone', async () => {
  const hub = useAppStore.getState().client as HubClient
  const revoke = vi.spyOn(hub, 'revokeInvite')
  renderSheet()
  const rows = await screen.findAllByRole('listitem')
  const alice = rows.find((r) => r.textContent?.includes('Alice'))!
  await userEvent.click(within(alice).getByRole('button', { name: 'Remove' }))
  await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
  expect(revoke).not.toHaveBeenCalled()
  expect(screen.queryByText(/stops working immediately/)).toBeNull()
})

describe('errors', () => {
  test('a 401 signs out instead of showing an inline error', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.spyOn(hub, 'listInvites').mockRejectedValue(new ApiError('HTTP 401', 401))
    renderSheet()
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
  })

  test('a failed mint stays in the sheet and re-enables the button', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.spyOn(hub, 'createInvite').mockRejectedValue(new ApiError('HTTP 500', 500))
    renderSheet()
    await screen.findAllByRole('listitem')
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not create the invite link. Check your connection and try again.',
    )
    expect(screen.getByRole('button', { name: 'Copy invite link' })).toBeEnabled()
    expect(screen.getByRole('dialog', { name: 'Invite' })).toBeInTheDocument()
  })
})
