import { render, screen, waitFor } from '@testing-library/react'
import App from './App'
import { useAppStore, resetAppStore } from './store/appStore'
import { saveCredentials } from './store/auth'

vi.mock('./api/servers', () => ({
  loadServers: vi.fn(async () => [
    { name: 'QEC Harness', prefix: '/zulip/qec', sendTopic: '' },
    { name: 'Chamber Hub', prefix: '', kind: 'hub', sendTopic: '' },
  ]),
}))

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
  expect(screen.getByLabelText(/email/i)).toBeInTheDocument()
})

test('clicking any upload anchor downloads in place instead of navigating', async () => {
  saveCredentials({ prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'k', sendTopic: '' })
  const blob = new Blob(['pdf'], { type: 'application/pdf' })
  vi.stubGlobal('fetch', vi.fn(async () => new Response(blob, { status: 200 })))
  const originalCreate = URL.createObjectURL
  const originalRevoke = URL.revokeObjectURL
  URL.createObjectURL = (() => 'blob:mock-intercept') as typeof URL.createObjectURL
  URL.revokeObjectURL = vi.fn() as typeof URL.revokeObjectURL
  // Outside the React tree on purpose: the interceptor must catch upload
  // anchors from ANY render path, not just MessageBody's delegated handler.
  const anchor = document.createElement('a')
  anchor.href = window.location.origin + '/user_uploads/1/x/notes.pdf'
  document.body.appendChild(anchor)
  try {
    render(<App />)
    await screen.findByRole('heading', { name: 'Projects' })
    const event = new MouseEvent('click', { bubbles: true, cancelable: true })
    anchor.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(true)
    await waitFor(() =>
      expect(vi.mocked(fetch)).toHaveBeenCalledWith(
        '/zulip/qec/user_uploads/1/x/notes.pdf',
        { headers: { Authorization: 'Basic ' + btoa('a@b.c:k') } },
      ),
    )
  } finally {
    anchor.remove()
    URL.createObjectURL = originalCreate
    URL.revokeObjectURL = originalRevoke
  }
})

describe('hub chamber-file anchors', () => {
  /** Object URLs and a blob response, shared by both directions of the test. */
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

  test('in hub mode the click downloads in place with the bearer token', async () => {
    saveCredentials({ kind: 'hub', prefix: '', email: 'Alice', apiKey: 'tok', sendTopic: '' })
    const cleanup = stubDownload('mock-hub')
    try {
      render(<App />)
      await screen.findByRole('heading', { name: 'Projects' })
      const event = new MouseEvent('click', { bubbles: true, cancelable: true })
      document.querySelector('a[href^="/api/chambers"]')!.dispatchEvent(event)
      expect(event.defaultPrevented).toBe(true)
      await waitFor(() =>
        // Hub file paths are already absolute app paths: never re-prefixed.
        expect(vi.mocked(fetch)).toHaveBeenCalledWith('/api/chambers/c1/files/x_y.pdf', {
          headers: { Authorization: 'Bearer tok' },
        }),
      )
    } finally {
      cleanup()
    }
  })

  test('in zulip mode the anchor is not intercepted at all', async () => {
    saveCredentials({ prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'k', sendTopic: '' })
    const cleanup = stubDownload('mock-zulip')
    try {
      render(<App />)
      await screen.findByRole('heading', { name: 'Projects' })
      const event = new MouseEvent('click', { bubbles: true, cancelable: true })
      document.querySelector('a[href^="/api/chambers"]')!.dispatchEvent(event)
      // Left to the browser: a hub route means nothing to a Zulip session, and
      // handling it would put this account's Basic API key on the wire to it.
      expect(event.defaultPrevented).toBe(false)
      await new Promise((resolve) => setTimeout(resolve, 10))
      for (const [url] of vi.mocked(fetch).mock.calls) {
        expect(String(url)).not.toContain('/files/')
      }
    } finally {
      cleanup()
    }
  })
})

test('opening a /user_uploads deep link downloads the file once signed in', async () => {
  window.history.replaceState(null, '', '/user_uploads/90996/abc/review.pdf')
  saveCredentials({ prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'k', sendTopic: '' })
  const blob = new Blob(['pdf'], { type: 'application/pdf' })
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(blob, { status: 200 })),
  )
  const originalCreate = URL.createObjectURL
  const originalRevoke = URL.revokeObjectURL
  URL.createObjectURL = (() => 'blob:mock-deeplink') as typeof URL.createObjectURL
  URL.revokeObjectURL = vi.fn() as typeof URL.revokeObjectURL
  const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, 'click')
  try {
    render(<App />)
    await waitFor(() => expect(clickSpy).toHaveBeenCalledTimes(1))
    const clicked = clickSpy.mock.instances[0] as unknown as HTMLAnchorElement
    expect(clicked.download).toBe('review.pdf')
    expect(vi.mocked(fetch)).toHaveBeenCalledWith(
      '/zulip/qec/user_uploads/90996/abc/review.pdf',
      { headers: { Authorization: 'Basic ' + btoa('a@b.c:k') } },
    )
    expect(window.location.pathname).toBe('/')
  } finally {
    clickSpy.mockRestore()
    URL.createObjectURL = originalCreate
    URL.revokeObjectURL = originalRevoke
    window.history.replaceState(null, '', '/')
  }
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
    const saved = JSON.parse(localStorage.getItem('zulip-app.credentials')!)
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
    saveCredentials({ kind: 'hub', prefix: '', email: 'Alice', apiKey: 'revoked', sendTopic: '' })
    vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 401 })))
    render(<App />)
    // Previously absorbed silently, leaving cached projects on screen behind a
    // token the hub had already revoked.
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(await screen.findByText(/no longer valid/i)).toBeInTheDocument()
    expect(localStorage.getItem('zulip-app.credentials')).toBeNull()
  })

  test('a 401 on an intercepted attachment click signs the user out', async () => {
    saveCredentials({ kind: 'hub', prefix: '', email: 'Alice', apiKey: 'tok', sendTopic: '' })
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
    saveCredentials({ kind: 'hub', prefix: '', email: 'Owner', apiKey: 'k', sendTopic: '' })
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
