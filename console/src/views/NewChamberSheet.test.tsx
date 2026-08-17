import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { NewChamberSheet, buildNewChamberPayload } from './NewChamberSheet'
import { HubClient } from '../api/hubClient'
import { useAppStore, resetAppStore } from '../store/appStore'
import { ApiError } from '../api/types'
import type { Chamber, Credentials } from '../api/types'

const creds: Credentials = { token: 'k', name: 'Owner', role: 'owner' }

const INDEX: Chamber[] = [
  {
    id: 'cham-new',
    name: 'gamma',
    running: false,
    agentRunning: false,
    nextWakeDisplay: null,
    completed: false,
    archived: false,
    hasOpenQuestion: false,
  },
]

function makeHub(): HubClient {
  const client = new HubClient({ token: creds.token, fetch: vi.fn() })
  vi.spyOn(client, 'createChamber').mockResolvedValue({ id: 'cham-new' })
  vi.spyOn(client, 'listChambers').mockResolvedValue(INDEX)
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
  await waitFor(() => expect(hub.listChambers).toHaveBeenCalledTimes(1))
  expect(useAppStore.getState().chambers.map((c) => c.name)).toEqual(['gamma'])
  // Straight into the chamber by the id the hub minted — no lookup in between.
  expect(useAppStore.getState().view).toEqual({ name: 'conversation', chamberId: 'cham-new' })
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
  vi.mocked(hub.createChamber).mockRejectedValue(new ApiError(400, 'chamber already exists'))
  const onClose = vi.fn()
  render(<NewChamberSheet onClose={onClose} />)
  await userEvent.type(screen.getByLabelText('Name'), 'gamma')
  await userEvent.click(screen.getByRole('button', { name: 'Create' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('chamber already exists')
  expect(onClose).not.toHaveBeenCalled()
  expect(screen.getByRole('button', { name: 'Create' })).toBeEnabled()
})

test('a 401 shows no inline error — the client already signed out', async () => {
  const hub = useAppStore.getState().client as HubClient
  vi.mocked(hub.createChamber).mockRejectedValue(new ApiError(401, 'HTTP 401'))
  render(<NewChamberSheet onClose={() => {}} />)
  await userEvent.type(screen.getByLabelText('Name'), 'gamma')
  await userEvent.click(screen.getByRole('button', { name: 'Create' }))
  await waitFor(() => expect(hub.createChamber).toHaveBeenCalled())
  expect(screen.queryByRole('alert')).toBeNull()
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
  reject(new ApiError(400, 'chamber already exists'))
  expect(await screen.findByRole('alert')).toHaveTextContent('chamber already exists')
  await userEvent.click(screen.getByRole('button', { name: 'Close' }))
  expect(onClose).toHaveBeenCalledTimes(1)
})
