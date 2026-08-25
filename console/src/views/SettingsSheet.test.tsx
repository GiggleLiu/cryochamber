import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SettingsSheet } from './SettingsSheet'
import { HubClient } from '../api/hubClient'
import { ApiError } from '../api/types'
import { useAppStore, resetAppStore, type Connection } from '../store/appStore'
import { makeHubAccount, MemoryHubsBackend, type HubAccount } from '../store/hubs'
import type { Chamber, Credentials } from '../api/types'

const chamber = (id: string, name = id) => ({
  id,
  name,
  running: true,
  agentRunning: false,
  nextWakeDisplay: null,
  completed: false,
  archived: false,
  hasOpenQuestion: false,
})

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    creds: { token: 'k', name: 'me@b.c', role: 'owner' },
    settingsOpen: true,
    chambers: [chamber('cham-a', 'alpha'), chamber('cham-b', 'beta')],
  })
})

test('names the token, what it can do, and which hub it opens', () => {
  useAppStore.setState({ hubRole: 'invite' })
  render(<SettingsSheet />)
  expect(screen.getByText(/me@b\.c/)).toBeInTheDocument()
  expect(screen.getByText('Guest')).toBeInTheDocument()
  // The console is served by the hub it talks to, so the origin is the only
  // honest answer to "where am I signed in".
  expect(screen.getByText(window.location.origin)).toBeInTheDocument()
})

test('the version line reports the hub, not the bundle', () => {
  useAppStore.setState({ hubVersion: '1.2.3' })
  render(<SettingsSheet />)
  expect(screen.getByText('cryohub v1.2.3')).toBeInTheDocument()
})

test('before whoami answers, the version line still names the hub', () => {
  render(<SettingsSheet />)
  expect(screen.getByText('cryohub')).toBeInTheDocument()
})

test('an owner is named as one', () => {
  useAppStore.setState({ hubRole: 'owner' })
  render(<SettingsSheet />)
  expect(screen.getByText('Owner')).toBeInTheDocument()
})

test('the per-project hide switches are gone for good', () => {
  // They were a Zulip-subscription idea: a guest could hide their only
  // chamber and be left with an empty list and no way back.
  useAppStore.setState({ hubRole: 'invite' })
  render(<SettingsSheet />)
  expect(screen.queryByRole('checkbox', { name: 'alpha' })).toBeNull()
  expect(screen.queryByRole('checkbox', { name: 'beta' })).toBeNull()
})

test('guest and owner get the same sheet, minus the owner-only fold', () => {
  useAppStore.setState({ hubRole: 'invite' })
  const { unmount } = render(<SettingsSheet />)
  expect(screen.getByText('Appearance')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: /log out/i })).toBeInTheDocument()
  expect(screen.queryByText('Chambers')).toBeNull()
  unmount()
  useAppStore.setState({ hubRole: 'owner' })
  render(<SettingsSheet />)
  expect(screen.getByText('Chambers')).toBeInTheDocument()
})

test('escape closes it, like every other sheet', async () => {
  render(<SettingsSheet />)
  await userEvent.keyboard('{Escape}')
  expect(useAppStore.getState().settingsOpen).toBe(false)
})

test('close button dismisses the sheet', async () => {
  render(<SettingsSheet />)
  await userEvent.click(screen.getByRole('button', { name: /close/i }))
  expect(useAppStore.getState().settingsOpen).toBe(false)
})

test('log out clears credentials', async () => {
  render(<SettingsSheet />)
  await userEvent.click(screen.getByRole('button', { name: /log out/i }))
  expect(useAppStore.getState().creds).toBeNull()
})

describe('appearance', () => {
  afterEach(() => {
    localStorage.removeItem('agent-console.theme')
    delete document.documentElement.dataset.theme
  })

  test('choosing Dark stamps the root and persists the choice', async () => {
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('radio', { name: /dark/i }))
    expect(document.documentElement.dataset.theme).toBe('dark')
    expect(localStorage.getItem('agent-console.theme')).toBe('dark')
  })

  test('choosing System clears both so the OS decides', async () => {
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('radio', { name: /dark/i }))
    await userEvent.click(screen.getByRole('radio', { name: /system/i }))
    expect(document.documentElement.dataset.theme).toBeUndefined()
    expect(localStorage.getItem('agent-console.theme')).toBeNull()
  })

  test('the stored choice is the one shown as selected', () => {
    localStorage.setItem('agent-console.theme', 'light')
    render(<SettingsSheet />)
    expect(screen.getByRole('radio', { name: /light/i })).toBeChecked()
  })
})

describe('owner-only rows', () => {
  function ownerHub() {
    const client = new HubClient({ token: 'k', fetch: vi.fn() })
    vi.spyOn(client, 'hostConfig').mockResolvedValue({ default_agent: 'pi' })
    vi.spyOn(client, 'updateHostConfig').mockImplementation(async (default_agent) => ({
      default_agent,
    }))
    vi.spyOn(client, 'refreshIndex').mockResolvedValue(undefined)
    vi.spyOn(client, 'listChambers').mockResolvedValue([chamber('cham-c', 'gamma')])
    return client
  }

  test('the show-completed toggle flips the persisted preference', async () => {
    useAppStore.setState({ hubRole: 'owner' })
    render(<SettingsSheet />)
    const toggle = screen.getByRole('checkbox', { name: 'Show completed & archived' })
    expect(toggle).not.toBeChecked()
    await userEvent.click(toggle)
    expect(useAppStore.getState().showCompletedArchived).toBe(true)
  })

  test('the default agent dropdown loads the host setting and saves on change', async () => {
    const client = ownerHub()
    useAppStore.setState({ hubRole: 'owner', client })
    render(<SettingsSheet />)

    const select = await screen.findByRole('combobox', { name: 'Default agent' })
    expect(select).toHaveValue('pi')
    await userEvent.selectOptions(select, 'claude')

    expect(client.updateHostConfig).toHaveBeenCalledWith('claude')
    await waitFor(() => expect(select).toHaveValue('claude'))
  })

  test('a host default the dropdown does not know stays selectable', async () => {
    const client = ownerHub()
    vi.mocked(client.hostConfig).mockResolvedValue({ default_agent: 'pi --thinking high' })
    useAppStore.setState({ hubRole: 'owner', client })
    render(<SettingsSheet />)

    const select = await screen.findByRole('combobox', { name: 'Default agent' })
    expect(select).toHaveValue('pi --thinking high')
    expect(Array.from(select.querySelectorAll('option')).map((o) => o.value)).toContain(
      'pi --thinking high',
    )
  })

  test('shows the hub error when the default agent cannot be saved', async () => {
    const client = ownerHub()
    vi.mocked(client.updateHostConfig).mockRejectedValue(new ApiError(400, 'invalid default agent'))
    useAppStore.setState({ hubRole: 'owner', client })
    render(<SettingsSheet />)

    const select = await screen.findByRole('combobox', { name: 'Default agent' })
    await userEvent.selectOptions(select, 'codex')

    expect(await screen.findByRole('alert')).toHaveTextContent('invalid default agent')
    // The dropdown goes back to the runner the hub still holds.
    expect(select).toHaveValue('pi')
  })

  test('refresh chambers re-scans the hub and re-registers', async () => {
    const client = ownerHub()
    useAppStore.setState({ hubRole: 'owner', client })
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('button', { name: 'Refresh chambers' }))
    expect(client.refreshIndex).toHaveBeenCalledTimes(1)
    await waitFor(() =>
      expect(useAppStore.getState().chambers.map((c) => c.name)).toEqual(['gamma']),
    )
  })

  test('a 401 while refreshing shows no inline error — the client already signed out', async () => {
    const client = ownerHub()
    vi.mocked(client.refreshIndex).mockRejectedValue(new ApiError(401, 'HTTP 401'))
    useAppStore.setState({ hubRole: 'owner', client })
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('button', { name: 'Refresh chambers' }))
    await waitFor(() => expect(client.refreshIndex).toHaveBeenCalled())
    expect(screen.queryByText(/Could not refresh/)).toBeNull()
  })

  test('a guest sees neither owner row', () => {
    useAppStore.setState({ hubRole: 'invite' })
    render(<SettingsSheet />)
    expect(screen.queryByRole('checkbox', { name: 'Show completed & archived' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Refresh chambers' })).toBeNull()
  })

  test('a session whose role is unknown sees neither owner row', () => {
    render(<SettingsSheet />)
    expect(screen.queryByRole('checkbox', { name: 'Show completed & archived' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Refresh chambers' })).toBeNull()
  })
})

describe('app mode', () => {
  const alpha = makeHubAccount({
    url: 'https://a.example',
    label: 'Alpha hub',
    token: 'ka',
    role: 'owner',
    trust: { kind: 'https' },
  })
  const beta = makeHubAccount({
    url: 'https://b.example',
    label: 'Beta hub',
    token: 'kb',
    role: 'owner',
    trust: { kind: 'https' },
  })

  /** One HubClient per hub, each answering with its own host config, wired
   * through `initApp` so the store holds a real router over them. */
  function enterAppMode(
    hubs: HubAccount[],
    extra: {
      agents?: Record<string, string>
      versionByHub?: Record<string, string | null>
      connectionByHub?: Record<string, Connection>
      authFailedHubs?: string[]
      roleByHub?: Record<string, 'owner' | 'invite'>
    } = {},
  ) {
    const clients = new Map<string, HubClient>()
    for (const h of hubs) {
      const client = new HubClient({ token: h.token, baseUrl: h.url, fetch: vi.fn() })
      vi.spyOn(client, 'hostConfig').mockResolvedValue({
        default_agent: extra.agents?.[h.id] ?? 'pi',
      })
      vi.spyOn(client, 'updateHostConfig').mockImplementation(async (default_agent) => ({
        default_agent,
      }))
      vi.spyOn(client, 'refreshIndex').mockResolvedValue(undefined)
      vi.spyOn(client, 'listChambers').mockResolvedValue([chamber('cham-c', 'gamma')])
      clients.set(h.id, client)
    }
    useAppStore.getState().initApp(hubs, new MemoryHubsBackend(), (h) => clients.get(h.id)!)
    useAppStore.setState({
      creds: null,
      settingsOpen: true,
      versionByHub: extra.versionByHub ?? {},
      connectionByHub:
        extra.connectionByHub ?? Object.fromEntries(hubs.map((h) => [h.id, 'live' as Connection])),
      authFailedHubs: extra.authFailedHubs ?? [],
      ...(extra.roleByHub ? { roleByHub: extra.roleByHub } : {}),
    })
    return clients
  }

  /** The Hubs row for one hub, found by the only control unique to it. */
  function hubRow(label: string): HTMLElement {
    const row = screen.getByRole('button', { name: `Remove ${label}` }).closest('.row')
    if (!row) throw new Error(`no hub row for ${label}`)
    return row as HTMLElement
  }

  test('the hub list names every remembered hub, its address and its access', () => {
    enterAppMode([alpha, beta], { versionByHub: { [alpha.id]: '1.2.3' } })
    render(<SettingsSheet />)
    expect(hubRow('Alpha hub')).toHaveTextContent('https://a.example')
    expect(hubRow('Alpha hub')).toHaveTextContent('Owner · cryohub v1.2.3')
    expect(hubRow('Beta hub')).toHaveTextContent('https://b.example')
  })

  test('a hub that is down, and one whose token was refused, say which', () => {
    enterAppMode([alpha, beta], {
      connectionByHub: { [alpha.id]: 'live', [beta.id]: 'offline' },
      authFailedHubs: [alpha.id],
    })
    render(<SettingsSheet />)
    expect(screen.getByText('unreachable')).toBeInTheDocument()
    expect(screen.getByText(/token revoked/)).toBeInTheDocument()
  })

  test('a hub older than this console warns that features may be missing', () => {
    enterAppMode([alpha, beta], {
      versionByHub: { [alpha.id]: '0.0.1', [beta.id]: '999.0.0' },
    })
    render(<SettingsSheet />)
    expect(screen.getAllByText(/hub is older/)).toHaveLength(1)
  })

  test('removing a hub asks first, and forgetting it takes its rows with it', async () => {
    enterAppMode([alpha, beta])
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false)
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('button', { name: 'Remove Beta hub' }))
    expect(confirm).toHaveBeenCalled()
    expect(useAppStore.getState().hubs.map((h) => h.id)).toEqual([alpha.id, beta.id])

    confirm.mockReturnValue(true)
    await userEvent.click(screen.getByRole('button', { name: 'Remove Beta hub' }))
    await waitFor(() => expect(useAppStore.getState().hubs.map((h) => h.id)).toEqual([alpha.id]))
    confirm.mockRestore()
  })

  test('Add hub opens the add-hub form on top of the settings sheet', async () => {
    enterAppMode([alpha, beta])
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('button', { name: 'Add hub' }))
    expect(await screen.findByRole('heading', { name: 'Add a hub' })).toBeInTheDocument()
  })

  test('the owner rows act on the hub the select names', async () => {
    enterAppMode([alpha, beta], { agents: { [alpha.id]: 'pi', [beta.id]: 'claude' } })
    render(<SettingsSheet />)
    const agent = await screen.findByRole('combobox', { name: 'Default agent' })
    expect(agent).toHaveValue('pi')
    await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Hub' }), beta.id)
    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'Default agent' })).toHaveValue('claude'),
    )
  })

  test('one owned hub needs no hub select', () => {
    enterAppMode([alpha, beta], { roleByHub: { [alpha.id]: 'owner', [beta.id]: 'invite' } })
    render(<SettingsSheet />)
    expect(screen.queryByRole('combobox', { name: 'Hub' })).toBeNull()
    expect(screen.getByText('Chambers')).toBeInTheDocument()
  })

  test('a token that owns nothing sees no owner section at all', () => {
    enterAppMode([alpha, beta], {
      roleByHub: { [alpha.id]: 'invite', [beta.id]: 'invite' },
    })
    render(<SettingsSheet />)
    expect(screen.queryByText('Chambers')).toBeNull()
    // Hub management is not an owner control: a guest still manages their app.
    expect(screen.getByText('Hubs')).toBeInTheDocument()
  })
})

test('a refresh that finishes after logout does not touch the next session', async () => {
  const creds: Credentials = { token: 'k', name: 'me@b.c', role: 'owner' }
  const hub = new HubClient({ token: creds.token, fetch: vi.fn() })
  vi.spyOn(hub, 'refreshIndex').mockResolvedValue(undefined)
  let resolveIndex!: (v: Chamber[]) => void
  vi.spyOn(hub, 'listChambers').mockReturnValue(new Promise((r) => { resolveIndex = r }))
  useAppStore.setState({ hubRole: 'owner', client: hub, creds })
  render(<SettingsSheet />)
  await userEvent.click(screen.getByRole('button', { name: 'Refresh chambers' }))
  // Sign out and back in as another token while the register is in flight.
  useAppStore.getState().logout()
  const other = new HubClient({ token: 'other-token', fetch: vi.fn() })
  useAppStore.setState({ client: other, creds: { ...creds, token: 'other-token' }, hubRole: 'owner' })
  resolveIndex([chamber('cham-stale', 'stale-list')])
  await new Promise((r) => setTimeout(r, 10))
  expect(useAppStore.getState().chambers).toEqual([])
  expect(useAppStore.getState().creds?.token).toBe('other-token')
})
