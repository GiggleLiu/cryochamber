import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { LoginView } from './LoginView'
import { loadServers } from '../api/servers'
import { useAppStore, resetAppStore } from '../store/appStore'
import type { ServerConfig } from '../api/types'

const HUB_SERVER: ServerConfig = { name: 'Chamber Hub', prefix: '', kind: 'hub', sendTopic: '' }
const TOKEN = 'ef'.repeat(16)

vi.mock('../api/servers', () => ({ loadServers: vi.fn() }))

beforeEach(() => {
  resetAppStore()
  localStorage.clear()
  vi.mocked(loadServers).mockResolvedValue([HUB_SERVER])
})

afterEach(() => vi.unstubAllGlobals())

test('sign-in asks for an access token and nothing else', async () => {
  render(<LoginView />)
  expect(await screen.findByLabelText(/access token/i)).toHaveAttribute('type', 'password')
  expect(screen.queryByLabelText(/email/i)).toBeNull()
  expect(screen.queryByLabelText(/^password$/i)).toBeNull()
})

test('single server: no server picker rendered', async () => {
  render(<LoginView />)
  await screen.findByLabelText(/access token/i)
  expect(screen.queryByLabelText(/server/i)).toBeNull()
})

test('submitting the token signs in via whoami and stores credentials', async () => {
  const fetchMock = vi.fn(
    async () => new Response(JSON.stringify({ role: 'owner', name: 'Jin' }), { status: 200 }),
  )
  vi.stubGlobal('fetch', fetchMock)
  render(<LoginView />)
  await userEvent.type(await screen.findByLabelText(/access token/i), TOKEN)
  await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
  await waitFor(() => expect(useAppStore.getState().creds).not.toBeNull())
  expect(useAppStore.getState().creds).toEqual({
    kind: 'hub', prefix: '', email: 'Jin', apiKey: TOKEN, sendTopic: '',
  })
  expect(useAppStore.getState().hubRole).toBe('owner')
  // Bearer header, never a query string.
  expect(fetchMock).toHaveBeenCalledWith(
    '/api/whoami',
    expect.objectContaining({
      headers: expect.objectContaining({ Authorization: `Bearer ${TOKEN}` }),
    }),
  )
})

test('a rejected token shows an error and keeps the form', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
  render(<LoginView />)
  await userEvent.type(await screen.findByLabelText(/access token/i), TOKEN)
  await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
  expect(await screen.findByRole('alert')).toHaveTextContent(/token/i)
  expect(useAppStore.getState().creds).toBeNull()
})

test('renders a stored login reason above the form', async () => {
  useAppStore.setState({
    loginReason: 'Your session is no longer valid — please sign in again.',
  })
  render(<LoginView />)
  expect(await screen.findByRole('alert')).toHaveTextContent(/session is no longer valid/i)
})
