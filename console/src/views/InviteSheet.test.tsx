import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import QRCode from 'qrcode'
import { InviteSheet, defaultInviteLabel, inviteScopeFor } from './InviteSheet'
import { HubClient, type Invite } from '../api/hubClient'
import { useAppStore, resetAppStore } from '../store/appStore'
import { makeHubAccount, MemoryHubsBackend } from '../store/hubs'
import { chamberKey } from '../lib/hubKeys'
import { ApiError } from '../api/types'
import type { Credentials } from '../api/types'

// jsdom has no canvas 2d context, so the QR library can never render there —
// stub it and assert on the call instead of the pixels.
vi.mock('qrcode', () => ({
  default: { toCanvas: vi.fn(async () => {}) },
}))

const creds: Credentials = { token: 'k', name: 'Owner', role: 'owner' }
const NEW_TOKEN = 'ff'.repeat(16)

const ALICE: Invite = {
  name: 'Alice', chambers: ['cham-a'], created_at: '2026-08-15T10:00:00Z', revoked_at: null,
}
const BOTH: Invite = {
  name: 'Bob', chambers: ['cham-a', 'cham-b'], created_at: '2026-08-15T09:00:00Z', revoked_at: null,
}
const GONE: Invite = {
  name: 'Carol', chambers: ['cham-a'], created_at: '2026-08-01T09:00:00Z',
  revoked_at: '2026-08-10T09:00:00Z',
}

/** URLs of every request the client made, so a test can prove the token never
 *  rode in one. */
let urls: string[]

function makeHub(invites: Invite[] = [ALICE, BOTH, GONE]): HubClient {
  urls = []
  const fetchFn = vi.fn(async (url: string, init?: RequestInit) => {
    urls.push(String(url))
    if (String(url).endsWith('/api/tokens') && init?.method === 'POST') {
      return new Response(JSON.stringify({ ok: true, name: 'x', token: NEW_TOKEN }), { status: 200 })
    }
    if (String(url).includes('/revoke')) {
      return new Response(JSON.stringify({ ok: true }), { status: 200 })
    }
    return new Response(JSON.stringify({ invites }), { status: 200 })
  })
  return new HubClient({ token: creds.token, fetch: fetchFn as unknown as typeof fetch })
}

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


let writeText: ReturnType<typeof vi.fn>

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    creds,
    client: makeHub(),
    hubRole: 'owner',
    chambers: [chamber('cham-a', 'alpha'), chamber('cham-b', 'beta')],
  })
  writeText = vi.fn(async () => {})
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
})

/** Browser mode's own answer to "which hub, and where does the link point":
 * the one hub that served the page, at this origin. */
function renderSheet() {
  const scope = inviteScopeFor(useAppStore.getState(), 'cham-a')
  return render(
    <InviteSheet
      chamberId="cham-a"
      chamberName="alpha"
      hub={scope.hub}
      inviteBase={scope.inviteBase}
      onClose={() => {}}
    />,
  )
}

test('titles the sheet for this chamber and lists only its active invites', async () => {
  renderSheet()
  expect(screen.getByRole('heading', { name: 'Invite to alpha' })).toBeInTheDocument()
  expect(screen.getByLabelText('Who is this for?')).toHaveAttribute('placeholder', 'e.g. mei-chen')
  const rows = await screen.findAllByRole('listitem')
  expect(rows).toHaveLength(2)
  expect(rows[0]).toHaveTextContent('Alice')
  expect(rows[0]).toHaveTextContent('added')
  // A revoked invite is not "people with access".
  expect(screen.queryByText('Carol')).toBeNull()
})

test('a multi-chamber invite says where else it reaches', async () => {
  renderSheet()
  const rows = await screen.findAllByRole('listitem')
  const bob = rows.find((r) => r.textContent?.includes('Bob'))!
  expect(bob).toHaveTextContent('also: beta')
  const alice = rows.find((r) => r.textContent?.includes('Alice'))!
  expect(alice).not.toHaveTextContent('also:')
})

test('the empty state invites the owner to copy a link', async () => {
  useAppStore.setState({ client: makeHub([]) })
  renderSheet()
  expect(
    await screen.findByText('Nobody else has access. Copy a link to invite someone.'),
  ).toBeInTheDocument()
})

test('copy mints a chamber-scoped link, copies it, and never puts it in a URL', async () => {
  const hub = useAppStore.getState().client as HubClient
  const create = vi.spyOn(hub, 'createInvite')
  renderSheet()
  await screen.findAllByRole('listitem')
  await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))

  const link = `${window.location.origin}/#invite=${NEW_TOKEN}`
  const field = (await screen.findByLabelText('Invite link')) as HTMLInputElement
  expect(field).toHaveValue(link)
  expect(field.readOnly).toBe(true)
  expect(create).toHaveBeenCalledWith('guest-1', ['cham-a'])
  expect(writeText).toHaveBeenCalledWith(link)
  expect(await screen.findByText('Copied')).toBeInTheDocument()
  // The token is a credential: it may never appear in a request line.
  expect(urls.some((u) => u.includes(NEW_TOKEN))).toBe(false)
})

test('a typed label names the invite instead of guest-N', async () => {
  const hub = useAppStore.getState().client as HubClient
  const create = vi.spyOn(hub, 'createInvite')
  renderSheet()
  await screen.findAllByRole('listitem')
  await userEvent.type(screen.getByLabelText('Who is this for?'), '  Mei  ')
  await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
  expect(create).toHaveBeenCalledWith('Mei', ['cham-a'])
})

test('guest-N skips names already taken in this chamber', () => {
  expect(defaultInviteLabel([])).toBe('guest-1')
  expect(defaultInviteLabel([{ ...ALICE, name: 'guest-1' }])).toBe('guest-2')
  expect(
    defaultInviteLabel([{ ...ALICE, name: 'guest-1' }, { ...ALICE, name: 'guest-2' }]),
  ).toBe('guest-3')
})

test('guest-N counts every active invite, including ones scoped to another chamber', async () => {
  // The hub refuses a duplicate name across ALL active invites, not just the
  // ones this chamber can see, so a name in use elsewhere still costs an N.
  const hub = makeHub([{ ...BOTH, name: 'guest-1', chambers: ['cham-b'] }, ALICE])
  useAppStore.setState({ client: hub })
  const create = vi.spyOn(hub, 'createInvite')
  renderSheet()
  const rows = await screen.findAllByRole('listitem')
  // guest-1 is not one of this chamber's people...
  expect(rows).toHaveLength(1)
  await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
  // ...but its name is still taken.
  expect(create).toHaveBeenCalledWith('guest-2', ['cham-a'])
})

test('copy waits for the list rather than guessing a name that may be taken', async () => {
  const hub = useAppStore.getState().client as HubClient
  vi.spyOn(hub, 'listInvites').mockReturnValue(new Promise<Invite[]>(() => {}))
  renderSheet()
  expect(screen.getByRole('button', { name: 'Copy invite link' })).toBeDisabled()
})

test('a clipboard that refuses says so and keeps the link selectable', async () => {
  writeText.mockRejectedValue(new Error('denied'))
  renderSheet()
  await screen.findAllByRole('listitem')
  await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
  expect(await screen.findByText('Copy failed — select and copy')).toBeInTheDocument()
  expect(screen.queryByText('Copied')).toBeNull()
  expect(await screen.findByLabelText('Invite link')).toHaveValue(
    `${window.location.origin}/#invite=${NEW_TOKEN}`,
  )
})

test('remove asks first, then revokes and re-reads the list', async () => {
  const hub = useAppStore.getState().client as HubClient
  const revoke = vi.spyOn(hub, 'revokeInvite')
  const list = vi.spyOn(hub, 'listInvites')
  renderSheet()
  const rows = await screen.findAllByRole('listitem')
  const alice = rows.find((r) => r.textContent?.includes('Alice'))!
  await userEvent.click(within(alice).getByRole('button', { name: 'Remove' }))
  expect(revoke).not.toHaveBeenCalled()
  expect(
    screen.getByText('Remove Alice? Their link stops working immediately.'),
  ).toBeInTheDocument()

  await userEvent.click(screen.getByRole('button', { name: 'Remove Alice' }))
  expect(revoke).toHaveBeenCalledWith('Alice')
  await waitFor(() => expect(list).toHaveBeenCalledTimes(2))
})

test('cancelling the confirm leaves the invite alone', async () => {
  const hub = useAppStore.getState().client as HubClient
  const revoke = vi.spyOn(hub, 'revokeInvite')
  renderSheet()
  const rows = await screen.findAllByRole('listitem')
  const alice = rows.find((r) => r.textContent?.includes('Alice'))!
  await userEvent.click(within(alice).getByRole('button', { name: 'Remove' }))
  await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
  expect(revoke).not.toHaveBeenCalled()
  expect(screen.queryByText(/stops working immediately/)).toBeNull()
})

describe('errors', () => {
  test('a 401 shows no inline error — signing out is the whole answer', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.spyOn(hub, 'listInvites').mockRejectedValue(new ApiError(401, 'HTTP 401'))
    renderSheet()
    await waitFor(() => expect(hub.listInvites).toHaveBeenCalled())
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.queryByText(/Could not load who has access/)).toBeNull()
  })

  test('a failed list load says so instead of loading for ever', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.spyOn(hub, 'listInvites').mockRejectedValue(new ApiError(500, 'HTTP 500'))
    renderSheet()
    expect(
      await screen.findByText('Could not load who has access. Check your connection and try again.'),
    ).toBeInTheDocument()
    expect(screen.queryByText('Loading…')).toBeNull()
    // Not knowing who is on the list is no reason to refuse a new one.
    expect(screen.getByRole('button', { name: 'Copy invite link' })).toBeEnabled()
  })

  test('a failed remove says so and leaves the person on the list', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.spyOn(hub, 'revokeInvite').mockRejectedValue(new ApiError(500, 'HTTP 500'))
    renderSheet()
    const rows = await screen.findAllByRole('listitem')
    const alice = rows.find((r) => r.textContent?.includes('Alice'))!
    await userEvent.click(within(alice).getByRole('button', { name: 'Remove' }))
    await userEvent.click(screen.getByRole('button', { name: 'Remove Alice' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not remove Alice. Check your connection and try again.',
    )
    expect(screen.getByText('Alice')).toBeInTheDocument()
  })

  test('a name the hub already has is reported as that, not as a network problem', async () => {
    const hub = useAppStore.getState().client as HubClient
    // The hub answers a duplicate with a bare 400 — no words of its own.
    vi.spyOn(hub, 'createInvite').mockRejectedValue(new ApiError(400, 'HTTP 400'))
    renderSheet()
    await screen.findAllByRole('listitem')
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'That label is already in use — pick another.',
    )
  })

  test('a silent 4xx that is not a 400 is not blamed on the label', async () => {
    const hub = useAppStore.getState().client as HubClient
    // Owner rights lost mid-session: the hub answers 403, with no words.
    vi.spyOn(hub, 'createInvite').mockRejectedValue(new ApiError(403, 'HTTP 403'))
    renderSheet()
    await screen.findAllByRole('listitem')
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The hub refused to create this invite link.',
    )
  })

  test('a 4xx the hub explains is quoted in the hub\'s own words', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.spyOn(hub, 'createInvite').mockRejectedValue(
      new ApiError(400, "an active invite named 'Bob' already exists", true),
    )
    renderSheet()
    await screen.findAllByRole('listitem')
    await userEvent.type(screen.getByLabelText('Who is this for?'), 'Bob')
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      "an active invite named 'Bob' already exists",
    )
  })

  test('a failed mint stays in the sheet and re-enables the button', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.spyOn(hub, 'createInvite').mockRejectedValue(new ApiError(500, 'HTTP 500'))
    renderSheet()
    await screen.findAllByRole('listitem')
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not create the invite link. Check your connection and try again.',
    )
    expect(screen.getByRole('button', { name: 'Copy invite link' })).toBeEnabled()
    expect(screen.getByRole('dialog', { name: 'Invite' })).toBeInTheDocument()
  })
})

test('a hub in open mode blames open mode, not "check your connection"', async () => {
  const fetchFn = vi.fn(async () => new Response('', { status: 503 }))
  const client = new HubClient({ token: creds.token, fetch: fetchFn as unknown as typeof fetch })
  useAppStore.setState({ client })
  render(
    <InviteSheet
      chamberId="cham-a"
      chamberName="alpha"
      hub={client}
      inviteBase={window.location.origin}
      onClose={() => {}}
    />,
  )
  const alert = await screen.findByRole('alert')
  expect(alert).toHaveTextContent(/open mode/)
  expect(alert).toHaveTextContent(/cryohub start/)
  expect(alert).not.toHaveTextContent(/connection/)
})

describe('app mode', () => {
  const alpha = makeHubAccount({
    url: 'https://a.example', label: 'Alpha hub', token: 'ka', role: 'owner',
    trust: { kind: 'https' },
  })
  const beta = makeHubAccount({
    url: 'https://b.example', label: 'Beta hub', token: 'kb', role: 'owner',
    trust: { kind: 'https' },
  })
  // Both hubs happen to hold a chamber called `cham-a`: raw ids are only
  // unique per hub, which is what the console-side key exists for.
  const keyA = chamberKey(alpha.id, 'cham-a')
  const keyAOther = chamberKey(alpha.id, 'cham-b')
  const keyB = chamberKey(beta.id, 'cham-a')

  /** Every request either hub was asked for, as `METHOD url`, plus the bodies
   *  of the mints — so a test can prove which hub minted and what it scoped. */
  let calls: string[]
  let minted: Array<{ name: string; chambers: string[] }>

  function enterAppMode(invites: Invite[] = [BOTH]) {
    calls = []
    minted = []
    const fetchFn = (async (url: RequestInfo | URL, init?: RequestInit) => {
      const target = String(url)
      calls.push(`${init?.method ?? 'GET'} ${target}`)
      if (target.endsWith('/api/tokens') && init?.method === 'POST') {
        minted.push(JSON.parse(String(init.body)))
        return new Response(JSON.stringify({ ok: true, name: 'x', token: NEW_TOKEN }), {
          status: 200,
        })
      }
      return new Response(JSON.stringify({ invites }), { status: 200 })
    }) as typeof fetch
    useAppStore
      .getState()
      .initApp(
        [alpha, beta],
        new MemoryHubsBackend(),
        (h) => new HubClient({ token: h.token, baseUrl: h.url, fetch: fetchFn }),
      )
    useAppStore.setState({
      creds: null,
      chambers: [
        { ...chamber(keyA, 'alpha'), hubId: alpha.id },
        { ...chamber(keyAOther, 'beta'), hubId: alpha.id },
        // Another hub's chamber, whose raw id collides with this one's.
        { ...chamber(keyB, 'elsewhere'), hubId: beta.id },
      ],
    })
  }

  function renderFor(key: string, name: string) {
    const scope = inviteScopeFor(useAppStore.getState(), key)
    return render(
      <InviteSheet
        chamberId={key}
        chamberName={name}
        hub={scope.hub}
        inviteBase={scope.inviteBase}
        onClose={() => {}}
      />,
    )
  }

  test('the link points at the hub, never at the app that minted it', async () => {
    enterAppMode()
    renderFor(keyA, 'alpha')
    await screen.findAllByRole('listitem')
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))

    const link = `https://a.example/#invite=${NEW_TOKEN}`
    expect(await screen.findByLabelText('Invite link')).toHaveValue(link)
    expect(writeText).toHaveBeenCalledWith(link)
    // The app's own origin opens nothing — it must never be the base.
    expect(link.startsWith(window.location.origin)).toBe(false)
  })

  test('the mint goes to that chamber\'s own hub, scoped by the id that hub knows', async () => {
    enterAppMode()
    renderFor(keyA, 'alpha')
    await screen.findAllByRole('listitem')
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))

    await waitFor(() => expect(minted).toHaveLength(1))
    expect(calls).toContain('POST https://a.example/api/tokens')
    expect(calls.some((c) => c.includes('b.example'))).toBe(false)
    // `{hubId}:` is the console's own bookkeeping; the hub only knows `cham-a`.
    expect(minted[0]).toEqual({ name: 'guest-1', chambers: ['cham-a'] })
  })

  test('"also" names this hub\'s chambers, not another hub\'s look-alike', async () => {
    enterAppMode()
    renderFor(keyA, 'alpha')
    const rows = await screen.findAllByRole('listitem')
    const bob = rows.find((r) => r.textContent?.includes('Bob'))!
    expect(bob).toHaveTextContent('also: beta')
    expect(bob).not.toHaveTextContent('elsewhere')
  })

  test('the same sheet on the other hub mints there instead', async () => {
    enterAppMode()
    renderFor(keyB, 'elsewhere')
    await screen.findAllByRole('listitem')
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))

    expect(await screen.findByLabelText('Invite link')).toHaveValue(
      `https://b.example/#invite=${NEW_TOKEN}`,
    )
    expect(calls).toContain('POST https://b.example/api/tokens')
    expect(calls.some((c) => c.startsWith('POST https://a.example'))).toBe(false)
  })
})

test('browser mode mints on the hub that served the page, at this origin', () => {
  const client = useAppStore.getState().client as HubClient
  const scope = inviteScopeFor(useAppStore.getState(), 'cham-a')
  expect(scope.hub).toBe(client)
  expect(scope.inviteBase).toBe(window.location.origin)
})

describe('QR code for the invite link', () => {
  beforeEach(() => {
    // The qrcode module mock is file-scoped; its call history accumulates
    // across tests, so each test starts from zero.
    vi.mocked(QRCode.toCanvas).mockClear()
  })

  test('minting a link renders its QR code on a canvas', async () => {
    renderSheet()
    await screen.findAllByRole('listitem')
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))

    const canvas = await screen.findByRole('img', { name: 'QR code for the invite link' })
    expect(canvas.tagName).toBe('CANVAS')
    expect(QRCode.toCanvas).toHaveBeenCalledWith(
      canvas,
      `${window.location.origin}/#invite=${NEW_TOKEN}`,
      expect.objectContaining({ width: 176, errorCorrectionLevel: 'M' }),
    )
    expect(screen.getByText('Scan to open on your phone')).toBeInTheDocument()
  })

  test('a QR render failure keeps the link usable and says so', async () => {
    vi.mocked(QRCode.toCanvas).mockRejectedValueOnce(new Error('no canvas'))
    renderSheet()
    await screen.findAllByRole('listitem')
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))

    expect(
      await screen.findByText('Could not render the QR code — the link above still works.'),
    ).toBeInTheDocument()
    // The link itself is untouched.
    const field = (await screen.findByLabelText('Invite link')) as HTMLInputElement
    expect(field).toHaveValue(`${window.location.origin}/#invite=${NEW_TOKEN}`)
  })

  test('re-minting replaces the QR canvas (a stale draw can never win)', async () => {
    renderSheet()
    await screen.findAllByRole('listitem')
    const hub = useAppStore.getState().client as HubClient
    const create = vi
      .spyOn(hub, 'createInvite')
      .mockResolvedValueOnce({ token: 'aa' } as never)
      .mockResolvedValueOnce({ token: 'bb' } as never)

    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
    const first = await screen.findByRole('img', { name: 'QR code for the invite link' })
    expect(QRCode.toCanvas).toHaveBeenCalledTimes(1)

    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }))
    const second = await screen.findByRole('img', { name: 'QR code for the invite link' })
    // A fresh canvas element for the fresh link, so a slow first render has no
    // canvas left to draw on.
    expect(second).not.toBe(first)
    expect(QRCode.toCanvas).toHaveBeenCalledTimes(2)
    expect(QRCode.toCanvas).toHaveBeenLastCalledWith(
      second,
      `${window.location.origin}/#invite=bb`,
      expect.objectContaining({ width: 176 }),
    )
    create.mockRestore()
  })
})
