import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { AddHubView } from './AddHubView'
import { bootApp, HUB_LOAD_ERROR, setAppRuntime, type AppRuntime } from '../lib/appBoot'
import { MemoryHubsBackend } from '../store/hubs'
import { useAppStore, resetAppStore } from '../store/appStore'

/** The shell's `invoke`, faked at the one seam the console has on it. Every
 * test in this file gets it; only the app-mode ones (which also stub
 * `__TAURI_INTERNALS__`) can reach it, and that is the point of half of them. */
const invoke = vi.hoisted(() => vi.fn())
vi.mock('../lib/tauri', () => ({
  tauriInvoke: invoke,
  tauriFetch: vi.fn(),
  tauriLoadStore: vi.fn(),
}))

const TOKEN = 'ef'.repeat(16)
const FINGERPRINT = '7e3d1274fb15f9bc2c2ac74425a03e1926d8069e9182f2e6efc743ca7705c19d'

/** Inside the shell: `isTauri()` is true, so the https path probes first. */
function enterAppShell() {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
}

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
  invoke.mockReset()
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

test('a single-slash http address is still plain HTTP, warning and all', async () => {
  // `http:/hub.local` is a valid URL that reaches the hub in the clear. Judging
  // the scheme by the raw text would wave it through as HTTPS and store that.
  await boot(whoamiMock({ role: 'owner', name: 'Jin' }))
  render(<AddHubView />)
  await userEvent.type(await screen.findByLabelText(/hub address/i), 'http:/hub.local')
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  expect(screen.getByRole('button', { name: /add hub/i })).toBeDisabled()

  await userEvent.click(
    screen.getByRole('checkbox', { name: /traffic to this hub is unencrypted/i }),
  )
  await userEvent.click(screen.getByRole('button', { name: /add hub/i }))

  await waitFor(() => expect(useAppStore.getState().hubs).toHaveLength(1))
  expect(useAppStore.getState().hubs[0].trust).toEqual({ kind: 'plain-http' })
  expect(useAppStore.getState().hubs[0].url).toBe('http://hub.local')
})

test('an address with a scheme the app cannot speak is refused in plain words', async () => {
  const fetchMock = whoamiMock({ role: 'owner' })
  await boot(fetchMock)
  render(<AddHubView />)
  // Parses fine — as scheme `hub.local:`. The refusal must not be the internal
  // "Hub URLs must be http or https, got …".
  await userEvent.type(await screen.findByLabelText(/hub address/i), 'hub.local:8765')
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  await userEvent.click(screen.getByRole('button', { name: /add hub/i }))

  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Enter an http:// or https:// hub address.',
  )
  expect(useAppStore.getState().hubs).toEqual([])
  expect(fetchMock).not.toHaveBeenCalled()
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

test('a boot that could not read the saved hubs says so before anything is typed', async () => {
  const backend = new MemoryHubsBackend()
  vi.spyOn(backend, 'load').mockRejectedValue(new Error('store unavailable'))
  const rt: AppRuntime = { backend, transportFor: () => whoamiMock({ role: 'owner' }) }
  setAppRuntime(rt)
  await bootApp(rt)
  render(<AddHubView />)
  // Otherwise the screen is indistinguishable from a first run, and adding a
  // hub here would overwrite a list that is still on disk.
  expect(screen.getByRole('alert')).toHaveTextContent(HUB_LOAD_ERROR)
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

/** Fill in the form and press Add hub. */
async function addHub(url: string) {
  render(<AddHubView />)
  await userEvent.type(await screen.findByLabelText(/hub address/i), url)
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  await userEvent.click(screen.getByRole('button', { name: /add hub/i }))
}

test('a certificate the system does not trust is offered for pinning, not silently taken', async () => {
  enterAppShell()
  const fetchMock = whoamiMock({ role: 'owner', name: 'Jin' })
  const backend = await boot(fetchMock)
  invoke.mockResolvedValue({ https_valid: false, fingerprint: FINGERPRINT })
  await addHub('https://hub.example')

  const sheet = await screen.findByRole('dialog', { name: /certificate/i })
  // Grouped as `openssl x509 -fingerprint -sha256` prints it, so the two can be
  // read against each other character by character.
  expect(sheet).toHaveTextContent('7E:3D:12:74:FB:15:F9:BC')
  // Nothing is stored, and the hub has not been spoken to, until the user says
  // this is the certificate the operator read out.
  expect(useAppStore.getState().hubs).toEqual([])
  expect(fetchMock).not.toHaveBeenCalled()

  await userEvent.click(screen.getByRole('button', { name: /add hub anyway/i }))
  await waitFor(() => expect(useAppStore.getState().hubs).toHaveLength(1))
  expect(useAppStore.getState().hubs[0].trust).toEqual({ kind: 'pinned', sha256: FINGERPRINT })
  expect(useAppStore.getState().hubs[0].url).toBe('https://hub.example')
  expect(await backend.load()).toHaveLength(1)
  expect(invoke).toHaveBeenCalledWith('probe_hub', { url: 'https://hub.example' })
})

test('the form behind an open pin sheet cannot be submitted again', async () => {
  enterAppShell()
  await boot(whoamiMock({ role: 'owner', name: 'Jin' }))
  invoke.mockResolvedValue({ https_valid: false, fingerprint: FINGERPRINT })
  await addHub('https://hub.example')
  await screen.findByRole('dialog', { name: /certificate/i })

  // The form is still in the document behind the sheet, and a keyboard user can
  // reach its button: pressing it must not probe again or stack a second
  // question on top of the one already asked.
  await userEvent.click(screen.getByRole('button', { name: 'Add hub' }))
  expect(invoke).toHaveBeenCalledTimes(1)
  expect(screen.getAllByRole('dialog')).toHaveLength(1)
})

test('declining an untrusted certificate adds nothing', async () => {
  enterAppShell()
  const fetchMock = whoamiMock({ role: 'owner', name: 'Jin' })
  const backend = await boot(fetchMock)
  invoke.mockResolvedValue({ https_valid: false, fingerprint: FINGERPRINT })
  await addHub('https://hub.example')

  await userEvent.click(await screen.findByRole('button', { name: /^cancel$/i }))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(useAppStore.getState().hubs).toEqual([])
  expect(await backend.load()).toEqual([])
  expect(fetchMock).not.toHaveBeenCalled()
})

test('a certificate the system trusts is added without a word about fingerprints', async () => {
  enterAppShell()
  const fetchMock = whoamiMock({ role: 'owner', name: 'Jin' })
  await boot(fetchMock)
  invoke.mockResolvedValue({ https_valid: true, fingerprint: FINGERPRINT })
  await addHub('https://hub.example')

  await waitFor(() => expect(useAppStore.getState().hubs).toHaveLength(1))
  expect(useAppStore.getState().hubs[0].trust).toEqual({ kind: 'https' })
  expect(useAppStore.getState().hubs[0].role).toBe('owner')
  expect(screen.queryByRole('dialog')).toBeNull()
  expect(fetchMock).toHaveBeenCalled()
})

test('a plain-http hub is never probed — the checkbox is its whole trust decision', async () => {
  enterAppShell()
  await boot(whoamiMock({ role: 'owner', name: 'Jin' }))
  render(<AddHubView />)
  await userEvent.type(await screen.findByLabelText(/hub address/i), 'http://hub.local:8765')
  await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
  await userEvent.click(
    screen.getByRole('checkbox', { name: /traffic to this hub is unencrypted/i }),
  )
  await userEvent.click(screen.getByRole('button', { name: /add hub/i }))

  await waitFor(() => expect(useAppStore.getState().hubs).toHaveLength(1))
  expect(invoke).not.toHaveBeenCalled()
})

test('in a browser the shell is never asked to probe', async () => {
  // No `__TAURI_INTERNALS__`: there is no shell to ask, and a browser cannot be
  // told which certificate it saw. A hub with a bad certificate keeps failing
  // the way it does today — as a network error on the whoami.
  const failing = vi.fn(async () => {
    throw new TypeError('Failed to fetch')
  }) as unknown as typeof fetch
  await boot(failing)
  await addHub('https://hub.example')

  expect(await screen.findByRole('alert')).toHaveTextContent(/Failed to fetch/)
  expect(invoke).not.toHaveBeenCalled()
  expect(useAppStore.getState().hubs).toEqual([])
})
