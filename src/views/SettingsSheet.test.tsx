import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SettingsSheet } from './SettingsSheet'
import { useAppStore, resetAppStore } from '../store/appStore'

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    creds: { prefix: '/zulip/qec', email: 'me@b.c', apiKey: 'k', sendTopic: '' },
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

describe('hub owner', () => {
  test('the Share access row opens the share sheet and closes settings', async () => {
    useAppStore.setState({ hubRole: 'owner' })
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('button', { name: /share access/i }))
    expect(useAppStore.getState().shareOpen).toBe(true)
    expect(useAppStore.getState().settingsOpen).toBe(false)
  })

  test('invite-token holders never see the Share access row', () => {
    useAppStore.setState({ hubRole: 'invite' })
    render(<SettingsSheet />)
    expect(screen.queryByRole('button', { name: /share access/i })).toBeNull()
  })

  test('Zulip accounts never see the Share access row', () => {
    render(<SettingsSheet />)
    expect(screen.queryByRole('button', { name: /share access/i })).toBeNull()
  })
})

describe('appearance', () => {
  afterEach(() => {
    localStorage.removeItem('zulip-app.theme')
    delete document.documentElement.dataset.theme
  })

  test('choosing Dark stamps the root and persists the choice', async () => {
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('radio', { name: /dark/i }))
    expect(document.documentElement.dataset.theme).toBe('dark')
    expect(localStorage.getItem('zulip-app.theme')).toBe('dark')
  })

  test('choosing System clears both so the OS decides', async () => {
    render(<SettingsSheet />)
    await userEvent.click(screen.getByRole('radio', { name: /dark/i }))
    await userEvent.click(screen.getByRole('radio', { name: /system/i }))
    expect(document.documentElement.dataset.theme).toBeUndefined()
    expect(localStorage.getItem('zulip-app.theme')).toBeNull()
  })

  test('the stored choice is the one shown as selected', () => {
    localStorage.setItem('zulip-app.theme', 'light')
    render(<SettingsSheet />)
    expect(screen.getByRole('radio', { name: /light/i })).toBeChecked()
  })
})
