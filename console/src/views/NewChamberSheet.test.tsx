import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { NewChamberSheet, buildNewChamberPayload } from './NewChamberSheet'
import { HubClient } from '../api/hubClient'
import { useAppStore, resetAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'
import { ApiError } from '../api/errors'
import type { Credentials, InitialState } from '../api/types'

const creds: Credentials = { kind: 'hub', prefix: '', email: 'Owner', apiKey: 'k', sendTopic: '' }

const REGISTERED: InitialState = {
  subscriptions: [{ stream_id: 9, name: 'gamma', description: '' }],
  unread: [],
}

function makeHub(): HubClient {
  const client = new HubClient(creds, vi.fn())
  vi.spyOn(client, 'createChamber').mockResolvedValue({ id: 'cham-new' })
  vi.spyOn(client, 'register').mockResolvedValue(REGISTERED)
  vi.spyOn(client, 'streamIdFor').mockReturnValue(9)
  return client
}

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({ creds, hubRole: 'owner', client: makeHub() })
})

test('a name alone is enough, and success re-registers and opens the chamber', async () => {
  const hub = useAppStore.getState().client as HubClient
  const onClose = vi.fn()
  render(<NewChamberSheet onClose={onClose} />)
  await userEvent.type(screen.getByLabelText('Name'), '  gamma  ')
  await userEvent.click(screen.getByRole('button', { name: 'Create' }))

  expect(hub.createChamber).toHaveBeenCalledWith({ name: 'gamma' })
  await waitFor(() => expect(hub.register).toHaveBeenCalledTimes(1))
  expect(useAppStore.getState().streams.map((s) => s.name)).toEqual(['gamma'])
  expect(useAppStore.getState().view).toEqual({ name: 'conversation', streamId: 9 })
  expect(onClose).toHaveBeenCalledTimes(1)
})

test('an empty name is refused before any request', async () => {
  const hub = useAppStore.getState().client as HubClient
  render(<NewChamberSheet onClose={() => {}} />)
  await userEvent.click(screen.getByRole('button', { name: 'Create' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('name is empty')
  expect(hub.createChamber).not.toHaveBeenCalled()
})

test('the provider section is all-or-nothing', async () => {
  const hub = useAppStore.getState().client as HubClient
  render(<NewChamberSheet onClose={() => {}} />)
  await userEvent.type(screen.getByLabelText('Name'), 'gamma')
  await userEvent.click(screen.getByText('API key provider'))
  await userEvent.type(screen.getByLabelText('Provider'), 'anthropic')
  await userEvent.click(screen.getByRole('button', { name: 'Create' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('api key is empty')
  expect(hub.createChamber).not.toHaveBeenCalled()

  await userEvent.type(screen.getByLabelText('API key'), 'sk-test')
  await userEvent.type(screen.getByLabelText('Model'), 'claude-opus')
  await userEvent.click(screen.getByRole('button', { name: 'Create' }))
  expect(hub.createChamber).toHaveBeenCalledWith({
    name: 'gamma', api_key_provider: 'anthropic', api_key: 'sk-test', model: 'claude-opus',
  })
})

test('buildNewChamberPayload covers every branch', () => {
  const base = { name: 'gamma', provider: '', apiKey: '', model: '', providerOpen: false }
  expect(buildNewChamberPayload({ ...base, name: '   ' })).toBe('name is empty')
  expect(buildNewChamberPayload(base)).toEqual({ name: 'gamma' })
  // Opening the section commits to filling it in.
  expect(buildNewChamberPayload({ ...base, providerOpen: true })).toBe('api key provider is empty')
  // Or typing into any of its fields does.
  expect(buildNewChamberPayload({ ...base, model: 'claude-opus' })).toBe(
    'api key provider is empty',
  )
  expect(buildNewChamberPayload({ ...base, provider: 'anthropic' })).toBe('api key is empty')
  expect(buildNewChamberPayload({ ...base, provider: 'anthropic', apiKey: 'sk' })).toEqual({
    name: 'gamma', api_key_provider: 'anthropic', api_key: 'sk',
  })
})

test('the hub error is shown verbatim and the form stays open', async () => {
  const hub = useAppStore.getState().client as HubClient
  vi.mocked(hub.createChamber).mockRejectedValue(new ApiError('chamber already exists', 400))
  const onClose = vi.fn()
  render(<NewChamberSheet onClose={onClose} />)
  await userEvent.type(screen.getByLabelText('Name'), 'gamma')
  await userEvent.click(screen.getByRole('button', { name: 'Create' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('chamber already exists')
  expect(onClose).not.toHaveBeenCalled()
  expect(screen.getByRole('button', { name: 'Create' })).toBeEnabled()
})

test('a 401 signs out', async () => {
  const hub = useAppStore.getState().client as HubClient
  vi.mocked(hub.createChamber).mockRejectedValue(new ApiError('HTTP 401', 401))
  render(<NewChamberSheet onClose={() => {}} />)
  await userEvent.type(screen.getByLabelText('Name'), 'gamma')
  await userEvent.click(screen.getByRole('button', { name: 'Create' }))
  await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
  expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
})

test('closing while a create is in flight waits for the outcome', async () => {
  const hub = useAppStore.getState().client as HubClient
  let reject!: (e: unknown) => void
  vi.mocked(hub.createChamber).mockReturnValue(new Promise((_, r) => { reject = r }))
  const onClose = vi.fn()
  render(<NewChamberSheet onClose={onClose} />)
  await userEvent.type(screen.getByLabelText('Name'), 'gamma')
  await userEvent.click(screen.getByRole('button', { name: 'Create' }))
  await userEvent.click(screen.getByRole('button', { name: 'Close' }))
  expect(onClose).not.toHaveBeenCalled()
  reject(new ApiError('chamber already exists', 400))
  expect(await screen.findByRole('alert')).toHaveTextContent('chamber already exists')
  await userEvent.click(screen.getByRole('button', { name: 'Close' }))
  expect(onClose).toHaveBeenCalledTimes(1)
})
