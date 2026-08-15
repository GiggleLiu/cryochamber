import { render, screen, waitFor } from '@testing-library/react'
import App from './App'
import { useAppStore, resetAppStore } from './store/appStore'
import { saveCredentials } from './store/auth'

vi.mock('./api/servers', () => ({
  loadServers: vi.fn(async () => [
    { name: 'Chamber Hub', prefix: '', kind: 'hub', sendTopic: '' },
  ]),
}))

const creds = { kind: 'hub' as const, prefix: '', email: 'Alice', apiKey: 'tok', sendTopic: '' }

beforeEach(() => {
  resetAppStore()
  // Stored credentials outlive resetAppStore; a leftover set would short-circuit
  // the boot effects each test is exercising.
  localStorage.clear()
})

afterEach(() => {
  window.history.replaceState(null, '', '/')
  vi.unstubAllGlobals()
})

test('shows login when no credentials are stored', async () => {
  render(<App />)
  expect(await screen.findByRole('heading', { name: 'Agent Console' })).toBeInTheDocument()
  expect(screen.getByLabelText(/access token/i)).toBeInTheDocument()
})

describe('chamber-file anchors', () => {
  /** Object URLs and a blob response for the download under test. */
  function stubDownload(name: string) {
    const blob = new Blob(['pdf'], { type: 'application/pdf' })
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) =>
        String(url).includes('/files/')
          ? new Response(blob, { status: 200 })
          : new Response(JSON.stringify(String(url).endsWith('/api/whoami') ? { role: 'invite', name: 'Alice' } : []), { status: 200 }),
      ),
    )
    const originalCreate = URL.createObjectURL
    const originalRevoke = URL.revokeObjectURL
    URL.createObjectURL = (() => `blob:${name}`) as typeof URL.createObjectURL
    URL.revokeObjectURL = vi.fn() as typeof URL.revokeObjectURL
    const anchor = document.createElement('a')
    anchor.href = '/api/chambers/c1/files/x_y.pdf'
    document.body.appendChild(anchor)
    return () => {
      anchor.remove()
      URL.createObjectURL = originalCreate
      URL.revokeObjectURL = originalRevoke
    }
  }

  // Outside the React tree on purpose: the interceptor must catch chamber file
  // anchors from ANY render path, not just MessageBody's delegated handler.
  test('a click downloads in place with the bearer token', async () => {
    saveCredentials(creds)
    const cleanup = stubDownload('mock-hub')
    try {
      render(<App />)
      await screen.findByRole('heading', { name: 'Projects' })
      const event = new MouseEvent('click', { bubbles: true, cancelable: true })
      document.querySelector('a[href^="/api/chambers"]')!.dispatchEvent(event)
      expect(event.defaultPrevented).toBe(true)
      await waitFor(() =>
        // Chamber file paths are already absolute app paths: never re-prefixed.
        expect(vi.mocked(fetch)).toHaveBeenCalledWith('/api/chambers/c1/files/x_y.pdf', {
          headers: { Authorization: 'Bearer tok' },
        }),
      )
    } finally {
      cleanup()
    }
  })

  test('an ordinary link is left to the browser', async () => {
    saveCredentials(creds)
    const cleanup = stubDownload('mock-plain')
    const anchor = document.createElement('a')
    anchor.href = 'https://arxiv.org/abs/1'
    document.body.appendChild(anchor)
    try {
      render(<App />)
      await screen.findByRole('heading', { name: 'Projects' })
      const event = new MouseEvent('click', { bubbles: true, cancelable: true })
      anchor.dispatchEvent(event)
      expect(event.defaultPrevented).toBe(false)
    } finally {
      anchor.remove()
      cleanup()
    }
  })
})

describe('invite-link onboarding', () => {
  const TOKEN = 'ab'.repeat(16)

  test('an #invite fragment signs in via whoami and lands on projects', async () => {
    window.location.hash = `#invite=${TOKEN}`
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) =>
        String(url).endsWith('/api/whoami')
          ? new Response(JSON.stringify({ role: 'invite', name: 'Alice' }), { status: 200 })
          : new Response(JSON.stringify([]), { status: 200 }),
      ),
    )
    render(<App />)
    expect(await screen.findByRole('heading', { name: 'Projects' })).toBeInTheDocument()
    expect(window.location.hash).toBe('')
    const saved = JSON.parse(localStorage.getItem('agent-console.credentials')!)
    expect(saved).toMatchObject({ kind: 'hub', email: 'Alice', apiKey: TOKEN })
    expect(useAppStore.getState().hubRole).toBe('invite')
    // The token rides in the Authorization header, never in a query string.
    for (const [url] of vi.mocked(fetch).mock.calls) expect(String(url)).not.toContain(TOKEN)
  })

  test('a revoked invite link shows login with a reason', async () => {
    window.location.hash = `#invite=${'cd'.repeat(16)}`
    vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
    render(<App />)
    expect(await screen.findByText(/no longer valid/i)).toBeInTheDocument()
  })

  test('a malformed #invite fragment is still stripped and explained', async () => {
    window.location.hash = '#invite=not-a-token'
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })))
    render(<App />)
    // Stripped whether or not it parsed: half a token is still a secret.
    await waitFor(() => expect(window.location.hash).toBe(''))
    expect(await screen.findByText(/this invite link is not valid/i)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /sign in/i })).toBeInTheDocument()
    // Never offered to the hub — a value that cannot be a token is not tried.
    for (const [url] of vi.mocked(fetch).mock.calls) {
      expect(String(url)).not.toContain('/api/whoami')
    }
  })

  test('a stored token the hub now rejects lands on login with a reason', async () => {
    saveCredentials({ ...creds, apiKey: 'revoked' })
    vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
    render(<App />)
    // Previously absorbed silently, leaving cached projects on screen behind a
    // token the hub had already revoked.
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(await screen.findByText(/no longer valid/i)).toBeInTheDocument()
    expect(localStorage.getItem('agent-console.credentials')).toBeNull()
  })

  test('a 401 on an intercepted attachment click signs the user out', async () => {
    saveCredentials(creds)
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) =>
        String(url).includes('/files/')
          ? new Response('', { status: 401 })
          : new Response(
              JSON.stringify(String(url).endsWith('/api/whoami') ? { role: 'invite' } : []),
              { status: 200 },
            ),
      ),
    )
    const anchor = document.createElement('a')
    anchor.href = '/api/chambers/c1/files/x_y.pdf'
    document.body.appendChild(anchor)
    try {
      render(<App />)
      await screen.findByRole('heading', { name: 'Projects' })
      anchor.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
      await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
      expect(await screen.findByText(/no longer valid|sign in again/i)).toBeInTheDocument()
    } finally {
      anchor.remove()
    }
  })

  test('stored hub credentials repopulate the role at boot', async () => {
    saveCredentials({ ...creds, email: 'Owner' })
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) =>
        String(url).endsWith('/api/whoami')
          ? new Response(JSON.stringify({ role: 'owner', name: 'Owner' }), { status: 200 })
          : new Response(JSON.stringify([]), { status: 200 }),
      ),
    )
    render(<App />)
    await waitFor(() => expect(useAppStore.getState().hubRole).toBe('owner'))
  })
})
