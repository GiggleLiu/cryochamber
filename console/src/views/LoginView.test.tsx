import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { LoginView } from './LoginView'
import { useAppStore, resetAppStore } from '../store/appStore'

const TOKEN = 'ef'.repeat(16)

beforeEach(() => {
  resetAppStore()
  localStorage.clear()
})

afterEach(() => vi.unstubAllGlobals())

test('sign-in asks for an access token and nothing else', async () => {
  render(<LoginView />)
  expect(await screen.findByLabelText(/access token/i)).toHaveAttribute('type', 'password')
  expect(screen.queryByLabelText(/email/i)).toBeNull()
  expect(screen.queryByLabelText(/^password$/i)).toBeNull()
})

test('explains how the hub operator can print a token', () => {
  render(<LoginView />)
  expect(screen.getByText('cryohub token owner')).toBeInTheDocument()
})

test('the access token can be shown and hidden again', async () => {
  render(<LoginView />)
  const input = screen.getByLabelText(/access token/i)
  const toggle = screen.getByRole('button', { name: 'Show' })
  expect(toggle).toHaveAttribute('aria-pressed', 'false')
  await userEvent.click(toggle)
  expect(input).toHaveAttribute('type', 'text')
  expect(screen.getByRole('button', { name: 'Hide' })).toHaveAttribute('aria-pressed', 'true')
  await userEvent.click(screen.getByRole('button', { name: 'Hide' }))
  expect(input).toHaveAttribute('type', 'password')
})

test('there is no server to pick: the console talks to the hub that served it', async () => {
  render(<LoginView />)
  await screen.findByLabelText(/access token/i)
  expect(screen.queryByLabelText(/server/i)).toBeNull()
})

test('submitting the token signs in via whoami and stores credentials', async () => {
  const fetchMock = vi.fn(
    async () =>
      new Response(JSON.stringify({ role: 'owner', name: 'Jin', hub_version: '0.3.0' }), {
        status: 200,
      }),
  )
  vi.stubGlobal('fetch', fetchMock)
  render(<LoginView />)
  await userEvent.type(await screen.findByLabelText(/access token/i), TOKEN)
  await userEvent.click(screen.getByRole('button', { name: /sign in/i }))
  await waitFor(() => expect(useAppStore.getState().creds).not.toBeNull())
  expect(useAppStore.getState().creds).toEqual({ token: TOKEN, name: 'Jin', role: 'owner' })
  expect(useAppStore.getState().hubRole).toBe('owner')
  expect(useAppStore.getState().selfName).toBe('Jin')
  expect(useAppStore.getState().hubVersion).toBe('0.3.0')
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
