import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { LoginView } from './LoginView'
import { ZulipClient } from '../api/client'
import { loadServers } from '../api/servers'
import { useAppStore, resetAppStore } from '../store/appStore'
import type { ServerConfig } from '../api/types'

const ZULIP_SERVER: ServerConfig = { name: 'QEC Harness', prefix: '/zulip/qec', sendTopic: '' }
const HUB_SERVER: ServerConfig = { name: 'Chamber Hub', prefix: '', kind: 'hub', sendTopic: '' }

vi.mock('../api/servers', () => ({ loadServers: vi.fn() }))

beforeEach(() => {
  resetAppStore()
  localStorage.clear()
  vi.mocked(loadServers).mockResolvedValue([ZULIP_SERVER])
})

afterEach(() => vi.unstubAllGlobals())

test('password login fetches api key and stores credentials', async () => {
  const spy = vi.spyOn(ZulipClient, 'fetchApiKey').mockResolvedValue('key99')
  render(<LoginView />)
  await userEvent.type(await screen.findByLabelText(/email/i), 'a@b.c')
  await userEvent.type(screen.getByLabelText(/password/i), 'pw')
  await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
  await waitFor(() => expect(useAppStore.getState().creds).not.toBeNull())
  expect(spy).toHaveBeenCalledWith('/zulip/qec', 'a@b.c', 'pw')
  expect(useAppStore.getState().creds).toEqual({
    prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'key99', sendTopic: '',
  })
})

test('paste-API-key path skips fetchApiKey', async () => {
  const spy = vi.spyOn(ZulipClient, 'fetchApiKey')
  render(<LoginView />)
  await userEvent.click(await screen.findByRole('button', { name: /paste an api key/i }))
  await userEvent.type(screen.getByLabelText(/email/i), 'a@b.c')
  await userEvent.type(screen.getByLabelText(/api key/i), 'direct-key')
  await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
  await waitFor(() => expect(useAppStore.getState().creds?.apiKey).toBe('direct-key'))
  expect(spy).not.toHaveBeenCalled()
})

test('shows auth errors and keeps the form', async () => {
  vi.spyOn(ZulipClient, 'fetchApiKey').mockRejectedValue(new Error('Your username or password is incorrect'))
  render(<LoginView />)
  await userEvent.type(await screen.findByLabelText(/email/i), 'a@b.c')
  await userEvent.type(screen.getByLabelText(/password/i), 'bad')
  await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
  expect(await screen.findByRole('alert')).toHaveTextContent(/incorrect/)
  expect(useAppStore.getState().creds).toBeNull()
})

test('single server: no server picker rendered', async () => {
  render(<LoginView />)
  await screen.findByLabelText(/email/i)
  expect(screen.queryByLabelText(/server/i)).toBeNull()
})

test('renders a stored login reason above the form', async () => {
  useAppStore.setState({
    loginReason: 'Your session is no longer valid — please sign in again.',
  })
  render(<LoginView />)
  expect(await screen.findByRole('alert')).toHaveTextContent(/session is no longer valid/i)
})

describe('hub token login', () => {
  const TOKEN = 'ef'.repeat(16)

  beforeEach(() => {
    vi.mocked(loadServers).mockResolvedValue([ZULIP_SERVER, HUB_SERVER])
  })

  async function selectHub() {
    await userEvent.selectOptions(await screen.findByLabelText(/server/i), '1')
  }

  test('a hub server asks for an access token instead of email and password', async () => {
    render(<LoginView />)
    await selectHub()
    expect(screen.getByLabelText(/access token/i)).toHaveAttribute('type', 'password')
    expect(screen.queryByLabelText(/email/i)).toBeNull()
    expect(screen.queryByLabelText(/^password$/i)).toBeNull()
  })

  test('submitting the token signs in via whoami and stores hub credentials', async () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ role: 'owner', name: 'Jin' }), { status: 200 }),
    )
    vi.stubGlobal('fetch', fetchMock)
    render(<LoginView />)
    await selectHub()
    await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
    await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
    await waitFor(() => expect(useAppStore.getState().creds).not.toBeNull())
    expect(useAppStore.getState().creds).toEqual({
      kind: 'hub', prefix: '', email: 'Jin', apiKey: TOKEN, sendTopic: '',
    })
    expect(useAppStore.getState().hubRole).toBe('owner')
    // Bearer header, never a query string.
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/whoami',
      expect.objectContaining({ headers: expect.objectContaining({ Authorization: `Bearer ${TOKEN}` }) }),
    )
  })

  test('a rejected token shows an error and keeps the form', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
    render(<LoginView />)
    await selectHub()
    await userEvent.type(screen.getByLabelText(/access token/i), TOKEN)
    await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
    expect(await screen.findByRole('alert')).toHaveTextContent(/token/i)
    expect(useAppStore.getState().creds).toBeNull()
  })
})
