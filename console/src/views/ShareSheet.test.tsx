import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ShareSheet } from './ShareSheet'
import { HubClient, type Invite } from '../api/hubClient'
import { useAppStore, resetAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'
import { ZulipApiError } from '../api/client'

const ALICE: Invite = {
  name: 'Alice',
  chambers: ['cham-a'],
  created_at: '2026-08-01T10:00:00Z',
  revoked_at: null,
}

let hub: HubClient
let writeText: ReturnType<typeof vi.fn>

function makeHub(): HubClient {
  const client = new HubClient(
    { kind: 'hub', prefix: '', email: 'Owner', apiKey: 'k', sendTopic: '' },
    vi.fn(),
  )
  vi.spyOn(client, 'listInvites').mockResolvedValue([ALICE])
  vi.spyOn(client, 'createInvite').mockResolvedValue({ token: 'ff'.repeat(16) })
  vi.spyOn(client, 'revokeInvite').mockResolvedValue(undefined)
  vi.spyOn(client, 'chamberIdFor').mockImplementation((sid) =>
    sid === 1 ? 'cham-a' : undefined,
  )
  return client
}

beforeEach(() => {
  resetAppStore()
  hub = makeHub()
  useAppStore.setState({
    creds: { kind: 'hub', prefix: '', email: 'Owner', apiKey: 'k', sendTopic: '' },
    client: hub,
    hubRole: 'owner',
    shareOpen: true,
    streams: [
      { stream_id: 1, name: 'alpha', description: '' },
      { stream_id: 2, name: 'beta', description: '' },
    ],
  })
  writeText = vi.fn(async () => {})
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
})

test('lists invites with the project names they can see', async () => {
  render(<ShareSheet />)
  const row = await screen.findByRole('listitem')
  expect(row).toHaveTextContent('Alice')
  expect(row).toHaveTextContent('alpha')
  expect(row).not.toHaveTextContent('beta')
})

test('a revoked invite is badged and cannot be revoked again', async () => {
  vi.mocked(hub.listInvites).mockResolvedValue([{ ...ALICE, revoked_at: '2026-08-02T10:00:00Z' }])
  render(<ShareSheet />)
  expect(await screen.findByText(/revoked/i)).toBeInTheDocument()
  expect(screen.queryByRole('button', { name: /^revoke$/i })).toBeNull()
})

test('create shows the invite link exactly once, with a copy button', async () => {
  render(<ShareSheet />)
  await screen.findByRole('listitem')
  await userEvent.type(screen.getByLabelText(/^name$/i), 'Bob')
  await userEvent.click(screen.getByRole('checkbox', { name: 'alpha' }))
  await userEvent.click(screen.getByRole('button', { name: /create invite link/i }))

  const link = `${window.location.origin}/#invite=${'ff'.repeat(16)}`
  const field = (await screen.findByLabelText(/invite link/i)) as HTMLInputElement
  expect(field).toHaveValue(link)
  expect(field.readOnly).toBe(true)
  expect(hub.createInvite).toHaveBeenCalledWith('Bob', ['cham-a'])

  await userEvent.click(screen.getByRole('button', { name: /copy/i }))
  expect(writeText).toHaveBeenCalledWith(link)
  await waitFor(() => expect(screen.getByRole('button', { name: 'Copied' })).toBeInTheDocument())
  // The list is re-read so the new invite shows up alongside the link.
  expect(hub.listInvites).toHaveBeenCalledTimes(2)
})

describe('copying the invite link is only claimed once it happened', () => {
  async function createLink() {
    render(<ShareSheet />)
    await screen.findByRole('listitem')
    await userEvent.type(screen.getByLabelText(/^name$/i), 'Bob')
    await userEvent.click(screen.getByRole('button', { name: /create invite link/i }))
    await screen.findByLabelText(/invite link/i)
  }

  test('a rejected clipboard write keeps "Copy" and says so', async () => {
    writeText.mockRejectedValue(new Error('permission denied'))
    await createLink()
    await userEvent.click(screen.getByRole('button', { name: 'Copy' }))
    expect(await screen.findByText(/could not copy/i)).toBeInTheDocument()
    // The link is not on the clipboard, so the button must not say it is.
    expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Copied' })).toBeNull()
  })

  test('a browser without the clipboard API is handled the same way', async () => {
    Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true })
    await createLink()
    await userEvent.click(screen.getByRole('button', { name: 'Copy' }))
    expect(await screen.findByText(/could not copy/i)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument()
  })
})

describe('a revoked owner token signs out instead of showing an inline error', () => {
  const unauthorized = () => new ZulipApiError('HTTP 401', 401)

  test('listing invites', async () => {
    vi.mocked(hub.listInvites).mockRejectedValue(unauthorized())
    render(<ShareSheet />)
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
  })

  test('creating an invite', async () => {
    vi.mocked(hub.createInvite).mockRejectedValue(unauthorized())
    render(<ShareSheet />)
    await screen.findByRole('listitem')
    await userEvent.type(screen.getByLabelText(/^name$/i), 'Bob')
    await userEvent.click(screen.getByRole('button', { name: /create invite link/i }))
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
  })

  test('revoking an invite', async () => {
    vi.mocked(hub.revokeInvite).mockRejectedValue(unauthorized())
    render(<ShareSheet />)
    await userEvent.click(await screen.findByRole('button', { name: /^revoke$/i }))
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
  })
})

test('creating without a name does not call the API', async () => {
  render(<ShareSheet />)
  await screen.findByRole('listitem')
  await userEvent.click(screen.getByRole('button', { name: /create invite link/i }))
  expect(hub.createInvite).not.toHaveBeenCalled()
})

test('a failed create is reported instead of silently doing nothing', async () => {
  vi.mocked(hub.createInvite).mockRejectedValue(new Error('name in use'))
  render(<ShareSheet />)
  await screen.findByRole('listitem')
  await userEvent.type(screen.getByLabelText(/^name$/i), 'Alice')
  await userEvent.click(screen.getByRole('button', { name: /create invite link/i }))
  expect(await screen.findByRole('alert')).toHaveTextContent(/could not create/i)
})

test('revoke calls the API and refreshes the list', async () => {
  render(<ShareSheet />)
  await userEvent.click(await screen.findByRole('button', { name: /^revoke$/i }))
  await waitFor(() => expect(hub.revokeInvite).toHaveBeenCalledWith('Alice'))
  expect(hub.listInvites).toHaveBeenCalledTimes(2)
})

test('close dismisses the sheet', async () => {
  render(<ShareSheet />)
  await userEvent.click(screen.getByRole('button', { name: /close/i }))
  expect(useAppStore.getState().shareOpen).toBe(false)
})

test('a failed list load is reported', async () => {
  vi.mocked(hub.listInvites).mockRejectedValue(new Error('boom'))
  render(<ShareSheet />)
  expect(await screen.findByRole('alert')).toHaveTextContent(/could not load invites/i)
})
