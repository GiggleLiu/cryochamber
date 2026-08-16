import { render, screen, waitFor } from '@testing-library/react'
import App from './App'
import { useAppStore, resetAppStore } from './store/appStore'
import { saveCredentials } from './store/auth'

const creds = { token: 'tok', name: 'Alice', role: 'owner' as const }

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
    expect(saved).toEqual({ token: TOKEN, name: 'Alice', role: 'invite' })
    expect(useAppStore.getState().hubRole).toBe('invite')
    // The token rides in the Authorization header, never in a query string.
    for (const [url] of vi.mocked(fetch).mock.calls) expect(String(url)).not.toContain(TOKEN)
  })

  /** whoami + a chamber list, the two calls a boot makes. */
  function stubHub(role: 'invite' | 'owner', chambers: Array<{ id: string; name: string }>) {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        const u = String(url)
        if (u.endsWith('/api/whoami')) {
          return new Response(JSON.stringify({ role, name: 'Alice' }), { status: 200 })
        }
        if (u.endsWith('/api/chambers')) {
          return new Response(JSON.stringify(chambers), { status: 200 })
        }
        return new Response(JSON.stringify([]), { status: 200 })
      }),
    )
  }

  test('a guest scoped to one chamber lands in that conversation', async () => {
    // Their link is tied to one chamber; a list of one says nothing and costs
    // them a tap.
    window.location.hash = `#invite=${TOKEN}`
    stubHub('invite', [{ id: '%2Ftmp%2Falpha', name: 'alpha' }])
    render(<App />)
    expect(await screen.findByRole('heading', { name: /alpha/ })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Projects' })).toBeNull()
  })

  test('a guest scoped to several chambers still gets the list', async () => {
    window.location.hash = `#invite=${TOKEN}`
    stubHub('invite', [
      { id: '%2Ftmp%2Falpha', name: 'alpha' },
      { id: '%2Ftmp%2Fbeta', name: 'beta' },
    ])
    render(<App />)
    expect(await screen.findByRole('heading', { name: 'Projects' })).toBeInTheDocument()
  })

  test('an owner with one chamber is left on the list', async () => {
    // The owner's entry point is the hub, not any one chamber — and theirs is
    // where New chamber and the folds live.
    stubHub('owner', [{ id: '%2Ftmp%2Falpha', name: 'alpha' }])
    useAppStore.getState().setCreds({ token: 'ab'.repeat(16), name: 'human', role: 'owner' })
    render(<App />)
    expect(await screen.findByRole('heading', { name: 'Projects' })).toBeInTheDocument()
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
    saveCredentials({ ...creds, token: 'revoked' })
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

  test('boot whoami corrects a stale stored role and name, and reports the hub version', async () => {
    // The stored record is the hub's last answer, not a fact: an invite that
    // has since been promoted (or renamed) must not run the session on it.
    saveCredentials({ token: 'tok', name: 'Old Name', role: 'invite' })
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) =>
        String(url).endsWith('/api/whoami')
          ? new Response(
              JSON.stringify({ role: 'owner', name: 'Owner', hub_version: '0.3.0' }),
              { status: 200 },
            )
          : new Response(JSON.stringify([]), { status: 200 }),
      ),
    )
    render(<App />)
    await waitFor(() => expect(useAppStore.getState().hubRole).toBe('owner'))
    expect(useAppStore.getState().hubVersion).toBe('0.3.0')
    expect(useAppStore.getState().selfName).toBe('Owner')
    expect(JSON.parse(localStorage.getItem('agent-console.credentials')!)).toEqual({
      token: 'tok',
      name: 'Owner',
      role: 'owner',
    })
  })
})
