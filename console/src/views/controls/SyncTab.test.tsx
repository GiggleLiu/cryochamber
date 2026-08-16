import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SyncTab } from './SyncTab'
import { HubClient, type SyncSummary } from '../../api/hubClient'
import { useAppStore, resetAppStore } from '../../store/appStore'
import { emitChamberEvent } from '../../store/chamberEvents'
import { ApiError } from '../../api/types'
import type { Credentials } from '../../api/types'

const creds: Credentials = { token: 'k', name: 'Owner', role: 'owner' }

function summary(overrides: Partial<SyncSummary> = {}): SyncSummary {
  return {
    backend: 'zulip', configured: true, installed: true, running: false,
    target: '#research > decoders', last_pushed_session: 3, log_tail_path: '/tmp/z.log',
    ...overrides,
  }
}

function makeHub(list: SyncSummary[]): HubClient {
  const client = new HubClient({ token: creds.token, fetch: vi.fn() })
  vi.spyOn(client, 'chamberSync').mockResolvedValue(list)
  vi.spyOn(client, 'syncAction').mockResolvedValue({ ok: true, message: 'zulip start' })
  return client
}

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({ creds, hubRole: 'owner' })
})

test('one card per backend, with its state, target and configuration', async () => {
  useAppStore.setState({ client: makeHub([summary()]) })
  render(<SyncTab chamberId="cham-a" />)
  expect(await screen.findByText('zulip')).toBeInTheDocument()
  expect(screen.getByText('off')).toBeInTheDocument()
  expect(screen.getByText('configured')).toBeInTheDocument()
  expect(screen.getByText('#research > decoders')).toBeInTheDocument()
})

test('an unconfigured backend says so', async () => {
  useAppStore.setState({ client: makeHub([summary({ configured: false, target: '' })]) })
  render(<SyncTab chamberId="cham-a" />)
  expect(await screen.findByText('not configured')).toBeInTheDocument()
})

test('the empty state names what is missing', async () => {
  useAppStore.setState({ client: makeHub([]) })
  render(<SyncTab chamberId="cham-a" />)
  expect(
    await screen.findByText('No message sync is configured for this chamber.'),
  ).toBeInTheDocument()
})

test('a stopped backend offers Start, and starting re-reads the list', async () => {
  const hub = makeHub([summary({ running: false })])
  useAppStore.setState({ client: hub })
  render(<SyncTab chamberId="cham-a" />)
  await userEvent.click(await screen.findByRole('button', { name: 'Start zulip sync' }))
  expect(hub.syncAction).toHaveBeenCalledWith('cham-a', 'zulip', 'start')
  await waitFor(() => expect(hub.chamberSync).toHaveBeenCalledTimes(2))
})

test('a running backend offers Stop', async () => {
  const hub = makeHub([summary({ running: true })])
  useAppStore.setState({ client: hub })
  render(<SyncTab chamberId="cham-a" />)
  expect(await screen.findByText('running')).toBeInTheDocument()
  await userEvent.click(screen.getByRole('button', { name: 'Stop zulip sync' }))
  expect(hub.syncAction).toHaveBeenCalledWith('cham-a', 'zulip', 'stop')
})

test('an ok:false action is reported inline and the button comes back', async () => {
  const hub = makeHub([summary()])
  vi.mocked(hub.syncAction).mockRejectedValue(new ApiError(200, 'cryo-zulip not found', true))
  useAppStore.setState({ client: hub })
  render(<SyncTab chamberId="cham-a" />)
  await userEvent.click(await screen.findByRole('button', { name: 'Start zulip sync' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('cryo-zulip not found')
  expect(screen.getByRole('button', { name: 'Start zulip sync' })).toBeEnabled()
})

test('a 401 stays silent — the client already signed out', async () => {
  const hub = makeHub([])
  vi.mocked(hub.chamberSync).mockRejectedValue(new ApiError(401, 'HTTP 401'))
  useAppStore.setState({ client: hub })
  render(<SyncTab chamberId="cham-a" />)
  await waitFor(() => expect(hub.chamberSync).toHaveBeenCalled())
  expect(screen.queryByRole('alert')).toBeNull()
})

test('a refusal outlives the status event that follows it', async () => {
  const hub = makeHub([summary()])
  vi.mocked(hub.syncAction).mockRejectedValue(new ApiError(200, 'cryo-zulip not found', true))
  useAppStore.setState({ client: hub })
  render(<SyncTab chamberId="cham-a" />)
  await userEvent.click(await screen.findByRole('button', { name: 'Start zulip sync' }))
  await screen.findByRole('alert')
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  await waitFor(() => expect(hub.chamberSync).toHaveBeenCalledTimes(3))
  expect(screen.getByRole('alert')).toHaveTextContent('cryo-zulip not found')
})

test('a failed refresh keeps the loaded cards on screen beside the error', async () => {
  const hub = makeHub([summary()])
  useAppStore.setState({ client: hub })
  render(<SyncTab chamberId="cham-a" />)
  await screen.findByText('zulip')
  vi.mocked(hub.chamberSync).mockRejectedValueOnce(new ApiError(500, 'HTTP 500'))
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  expect(await screen.findByRole('alert')).toHaveTextContent(/could not load message sync/i)
  expect(screen.getByText('zulip')).toBeInTheDocument()
})
