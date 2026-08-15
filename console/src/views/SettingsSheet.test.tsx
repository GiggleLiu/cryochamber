import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SettingsSheet } from './SettingsSheet'
import { HubClient } from '../api/hubClient'
import { ApiError } from '../api/errors'
import { useAppStore, resetAppStore } from '../store/appStore'

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    creds: { kind: 'hub', prefix: '', email: 'me@b.c', apiKey: 'k', sendTopic: '' },
    settingsOpen: true,
    streams: [
      { stream_id: 1, name: 'alpha', description: 'A' },
      { stream_id: 2, name: 'beta', description: 'B' },
    ],
    hiddenStreams: [2],
  })
})

test('shows identity and stream checkboxes reflecting hidden state', () => {
  render(<SettingsSheet />)
  expect(screen.getByText(/me@b\.c/)).toBeInTheDocument()
  expect(screen.getByRole('checkbox', { name: 'alpha' })).toBeChecked()
  expect(screen.getByRole('checkbox', { name: 'beta' })).not.toBeChecked()
})

test('toggling a checkbox flips hidden state', async () => {
  render(<SettingsSheet />)
  await userEvent.click(screen.getByRole('checkbox', { name: 'beta' }))
  expect(useAppStore.getState().hiddenStreams).toEqual([])
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
    const client = new HubClient(
      { kind: 'hub', prefix: '', email: 'me@b.c', apiKey: 'k', sendTopic: '' },
      vi.fn(),
    )
    vi.spyOn(client, 'refreshIndex').mockResolvedValue(undefined)
    vi.spyOn(client, 'register').mockResolvedValue({
      subscriptions: [{ stream_id: 3, name: 'gamma', description: '' }],
      unread: [],
    })
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

  test('refresh chambers re-scans the hub and re-registers', async () => {
    const client = ownerHub()
    useAppStore.setState({ hubRole: 'owner', client })
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('button', { name: 'Refresh chambers' }))
    expect(client.refreshIndex).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(useAppStore.getState().streams.map((s) => s.name)).toEqual(['gamma']))
  })

  test('a 401 while refreshing signs out', async () => {
    const client = ownerHub()
    vi.mocked(client.refreshIndex).mockRejectedValue(new ApiError('HTTP 401', 401))
    useAppStore.setState({ hubRole: 'owner', client })
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('button', { name: 'Refresh chambers' }))
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
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
