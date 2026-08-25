import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { AddHubView } from './AddHubView'
import { bootApp, setAppRuntime, type AppRuntime } from '../lib/appBoot'
import { MemoryHubsBackend } from '../store/hubs'
import { useAppStore, resetAppStore } from '../store/appStore'

const TOKEN = 'ef'.repeat(16)

/** The app has already booted (empty hub list) — which is exactly when the
 * Add Hub screen is what the window shows. */
async function boot(fetchMock: typeof fetch): Promise<MemoryHubsBackend> {
  const backend = new MemoryHubsBackend()
  const rt: AppRuntime = { backend, transportFor: () => fetchMock }
  setAppRuntime(rt)
  await bootApp(rt)
  return backend
}

function whoamiMock(who: unknown, status = 200) {
  return vi.fn(async () => new Response(JSON.stringify(who), { status })) as unknown as typeof fetch
}

beforeEach(() => {
  resetAppStore()
  localStorage.clear()
})

afterEach(() => vi.unstubAllGlobals())

test('a hub URL and a token are enough to add a hub', async () => {
  const fetchMock = whoamiMock({ role: 'owner', name: 'Jin', hub_version: '0.4.0' })
  const backend = await boot(fetchMock)
  render(<AddHubView />)
  await userEvent.type(await screen.findByLabelText(/hub address/i), 'https://hub.example')
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  await userEvent.click(screen.getByRole('button', { name: /add hub/i }))

  await waitFor(() => expect(useAppStore.getState().hubs).toHaveLength(1))
  const [hub] = useAppStore.getState().hubs
  expect(hub.url).toBe('https://hub.example')
  expect(hub.token).toBe(TOKEN)
  expect(hub.name).toBe('Jin')
  expect(hub.role).toBe('owner')
  expect(hub.trust).toEqual({ kind: 'https' })
  // Persisted, not just in memory: the list is the app's account file.
  expect(await backend.load()).toHaveLength(1)
  expect(fetchMock).toHaveBeenCalledWith(
    'https://hub.example/api/whoami',
    expect.objectContaining({
      headers: expect.objectContaining({ Authorization: `Bearer ${TOKEN}` }),
    }),
  )
})

test('an optional label names the hub in place of its host', async () => {
  await boot(whoamiMock({ role: 'invite', name: 'guest' }))
  render(<AddHubView />)
  await userEvent.type(await screen.findByLabelText(/hub address/i), 'https://hub.example')
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  await userEvent.type(screen.getByLabelText(/label/i), 'Lab box')
  await userEvent.click(screen.getByRole('button', { name: /add hub/i }))
  await waitFor(() => expect(useAppStore.getState().hubs[0]?.label).toBe('Lab box'))
})

test('a plain-http hub is added only after the risk is acknowledged', async () => {
  await boot(whoamiMock({ role: 'owner', name: 'Jin' }))
  render(<AddHubView />)
  await userEvent.type(await screen.findByLabelText(/hub address/i), 'http://hub.local:8765')
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  expect(screen.getByRole('button', { name: /add hub/i })).toBeDisabled()

  await userEvent.click(
    screen.getByRole('checkbox', { name: /traffic to this hub is unencrypted/i }),
  )
  const submit = screen.getByRole('button', { name: /add hub/i })
  expect(submit).toBeEnabled()
  await userEvent.click(submit)

  await waitFor(() => expect(useAppStore.getState().hubs).toHaveLength(1))
  expect(useAppStore.getState().hubs[0].trust).toEqual({ kind: 'plain-http' })
})

test('changing the address asks for the acknowledgement again', async () => {
  await boot(whoamiMock({ role: 'owner', name: 'Jin' }))
  render(<AddHubView />)
  await userEvent.type(await screen.findByLabelText(/hub address/i), 'http://hub.local:8765')
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  await userEvent.click(
    screen.getByRole('checkbox', { name: /traffic to this hub is unencrypted/i }),
  )
  // The box was ticked for one host; it cannot carry over to another.
  await userEvent.type(screen.getByLabelText(/hub address/i), '9')
  expect(screen.getByRole('button', { name: /add hub/i })).toBeDisabled()
})

test('a rejected token is said in the hub’s own terms and adds nothing', async () => {
  await boot(whoamiMock('', 401))
  render(<AddHubView />)
  await userEvent.type(await screen.findByLabelText(/hub address/i), 'https://hub.example')
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  await userEvent.click(screen.getByRole('button', { name: /add hub/i }))

  expect(await screen.findByRole('alert')).toHaveTextContent('The hub rejected this token')
  expect(useAppStore.getState().hubs).toEqual([])
})

test('an unreachable hub shows what went wrong and adds nothing', async () => {
  const failing = vi.fn(async () => {
    throw new TypeError('Failed to fetch')
  }) as unknown as typeof fetch
  await boot(failing)
  render(<AddHubView />)
  await userEvent.type(await screen.findByLabelText(/hub address/i), 'https://hub.example')
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  await userEvent.click(screen.getByRole('button', { name: /add hub/i }))

  expect(await screen.findByRole('alert')).toHaveTextContent(/Failed to fetch/)
  expect(useAppStore.getState().hubs).toEqual([])
})

test('a pasted invite link fills in the address and the token', async () => {
  await boot(whoamiMock({ role: 'invite', name: 'guest' }))
  render(<AddHubView />)
  const link = 'http://hub.local:8765/#invite=' + 'a'.repeat(32)
  await userEvent.type(await screen.findByLabelText(/invite link/i), link)
  expect(screen.getByLabelText(/hub address/i)).toHaveValue('http://hub.local:8765')
  expect(screen.getByLabelText(/access token/i)).toHaveValue('a'.repeat(32))
})

test('a hub address that is not a URL is refused before anything is asked of it', async () => {
  const fetchMock = whoamiMock({ role: 'owner' })
  await boot(fetchMock)
  render(<AddHubView />)
  await userEvent.type(await screen.findByLabelText(/hub address/i), 'hub.example')
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  await userEvent.click(screen.getByRole('button', { name: /add hub/i }))
  expect(await screen.findByRole('alert')).toBeInTheDocument()
  expect(useAppStore.getState().hubs).toEqual([])
  expect(fetchMock).not.toHaveBeenCalled()
})
