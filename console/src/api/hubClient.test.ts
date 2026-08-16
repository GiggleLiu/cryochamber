import { HubClient, numericMessageId, numericStreamId, UnresolvedProjectError } from './hubClient'
import { ApiError } from './types'
import type { Credentials } from './types'

const creds: Credentials = { kind: 'hub', prefix: '', email: 'Alice', apiKey: 'tok123', sendTopic: '' }
const ACCOUNT = 'hub||Alice'

function mockFetch(handler: (url: string, init?: RequestInit) => object | Response) {
  return vi.fn(async (url: string, init?: RequestInit) => {
    const out = handler(url, init)
    return out instanceof Response ? out : new Response(JSON.stringify(out), { status: 200 })
  }) as unknown as typeof fetch
}

beforeEach(() => localStorage.clear())

test('register maps chambers to subscriptions with stable numeric ids', async () => {
  const fetchFn = mockFetch(() => [
    { id: 'cham-b', name: 'beta', task: null },
    { id: 'cham-a', name: 'alpha', task: null },
  ])
  const c = new HubClient(creds, fetchFn)
  const init = await c.register()
  expect(init.subscriptions.map((s) => s.name).sort()).toEqual(['alpha', 'beta'])
  const idA = init.subscriptions.find((s) => s.name === 'alpha')!.stream_id
  // stable across a second client instance (persisted map)
  const again = await new HubClient(creds, fetchFn).register()
  expect(again.subscriptions.find((s) => s.name === 'alpha')!.stream_id).toBe(idA)
  expect(vi.mocked(fetchFn).mock.calls[0][1]?.headers).toMatchObject({ Authorization: 'Bearer tok123' })
})

test('getMessages maps a chamber message to a store message with markdown content', async () => {
  const fetchFn = mockFetch((url) =>
    url.includes('/messages')
      ? [{ id: 'm1', direction: 'outbox', from: 'agent', subject: 's', body: '**hi**',
           timestamp: '2026-08-15T10:00:00', session: 1, is_question: false }]
      : [{ id: 'cham-a', name: 'alpha' }],
  )
  const c = new HubClient(creds, fetchFn)
  await c.register()
  const msgs = await c.getMessages('alpha')
  expect(msgs).toHaveLength(1)
  expect(msgs[0].sender_email).toBe('agent')
  expect(msgs[0].content).toBe('**hi**')
  expect(msgs[0].timestamp).toBe(Math.floor(Date.parse('2026-08-15T10:00:00') / 1000))
})

describe('chamber liveness', () => {
  const chambers = [
    { id: 'cham-a', name: 'alpha', running: true, agent_running: true, next_wake_display: null },
    { id: 'cham-b', name: 'beta', running: true, agent_running: false, next_wake_display: 'in 2 h' },
  ]

  test('register carries the hub liveness onto each subscription', async () => {
    const c = new HubClient(creds, mockFetch(() => chambers))
    const subs = (await c.register()).subscriptions
    expect(subs.find((s) => s.name === 'alpha')).toMatchObject({
      running: true, agentRunning: true, nextWake: null,
    })
    expect(subs.find((s) => s.name === 'beta')).toMatchObject({
      running: true, agentRunning: false, nextWake: 'in 2 h',
    })
  })

  test('a hub that reports no liveness leaves it undefined rather than asleep', async () => {
    const c = new HubClient(creds, mockFetch(() => [{ id: 'cham-a', name: 'alpha' }]))
    const sub = (await c.register()).subscriptions[0]
    expect(sub.running).toBeUndefined()
    expect(sub.agentRunning).toBeUndefined()
  })

  test('chamberStatuses re-reads the index and keys it by stream id', async () => {
    const c = new HubClient(creds, mockFetch(() => chambers))
    const subs = (await c.register()).subscriptions
    expect(await c.chamberStatuses()).toEqual([
      { stream_id: subs.find((s) => s.name === 'alpha')!.stream_id, running: true, agentRunning: true, nextWake: null,
        completed: false, archived: false, hasOpenQuestion: false },
      { stream_id: subs.find((s) => s.name === 'beta')!.stream_id, running: true, agentRunning: false, nextWake: 'in 2 h',
        completed: false, archived: false, hasOpenQuestion: false },
    ])
  })

  test('chamberStatuses keeps missing flags unknown instead of claiming stopped', async () => {
    // Against a hub that predates the liveness fields, a status event must
    // not repaint every project as a stopped chamber.
    const c = new HubClient(creds, mockFetch(() => [{ id: 'cham-a', name: 'alpha' }]))
    const [status] = await c.chamberStatuses()
    expect(status.stream_id).toBe((await c.register()).subscriptions[0].stream_id)
    expect(status.running).toBeUndefined()
    expect(status.agentRunning).toBeUndefined()
    expect(status.nextWake).toBeNull()
  })
})

describe('numericMessageId', () => {
  test('is stable for one id and ordered for messages seen in order', () => {
    expect(numericMessageId('m1', 1000_000, ACCOUNT)).toBe(numericMessageId('m1', 1000_000, ACCOUNT))
    const older = numericMessageId('older', 1000_000, ACCOUNT)
    const newer = numericMessageId('newer', 2000_000, ACCOUNT)
    expect(newer).toBeGreaterThan(older)
    // A late arrival with an older timestamp is placed after what it followed,
    // not merged into it: distinctness is the property the store depends on.
    expect(numericMessageId('backfilled', 500_000, ACCOUNT)).toBe(newer + 1)
  })

  test('distinct ids sharing a timestamp stay distinct', () => {
    // The old timestamp+hash%997 scheme mapped exactly these two to the same
    // number, and the store's dedupe-by-id silently dropped one of them.
    const a = numericMessageId('msg-55', 1_700_000_000_000, ACCOUNT)
    const b = numericMessageId('msg-108', 1_700_000_000_000, ACCOUNT)
    expect(a).not.toBe(b)

    // Not just those two: a whole second's worth of arrivals must all survive.
    const ids = Array.from({ length: 200 }, (_, i) =>
      numericMessageId(`same-ms-${i}`, 1_700_000_001_000, ACCOUNT),
    )
    expect(new Set(ids).size).toBe(200)
  })

  test('assignments survive a reload and are not shared between accounts', () => {
    const first = numericMessageId('msg-7', 1_700_000_000_000, ACCOUNT)
    // Same persisted map, read fresh: the number a cached message already
    // carries must keep resolving to the same message.
    expect(numericMessageId('msg-7', 1_700_000_000_000, ACCOUNT)).toBe(first)
    const other = 'hub||Bob'
    numericMessageId('msg-1', 1_700_000_000_000, other)
    expect(
      JSON.parse(localStorage.getItem('agent-console.hub-msgids.hub||Bob')!).byId,
    ).toEqual({ 'msg-1': 1_700_000_000_000 })
    expect(Object.keys(JSON.parse(localStorage.getItem(`agent-console.hub-msgids.${ACCOUNT}`)!).byId))
      .toEqual(['msg-7'])
  })
})

test('a second client instance agrees on the number for the same mailbox id', async () => {
  const history = [
    { id: 'msg-7', direction: 'outbox', from: 'agent', subject: '', body: 'hi',
      timestamp: '2026-08-15T10:00:00', session: 1, is_question: false },
    { id: 'msg-8', direction: 'outbox', from: 'agent', subject: '', body: 'again',
      timestamp: '2026-08-15T10:00:00', session: 1, is_question: false },
  ]
  const fetchFn = mockFetch((url) =>
    url.includes('/messages') ? history : [{ id: 'cham-a', name: 'alpha' }],
  )
  const a = new HubClient(creds, fetchFn)
  await a.register()
  const first = (await a.getMessages('alpha')).map((m) => m.id)
  // Same second, distinct mailbox ids: both must survive the store's dedupe.
  expect(new Set(first).size).toBe(2)
  const b = new HubClient(creds, fetchFn)
  await b.register()
  expect((await b.getMessages('alpha')).map((m) => m.id)).toEqual(first)
})

test('stream ids are unique across the accounts the app remembers', () => {
  // The app lists the chambers of every token it remembers in ONE list, so a
  // number must mean one chamber of one token. Per-token numbering — which
  // this replaced — gave two different chambers the same id, and with it one
  // message cache and one draft.
  expect(numericStreamId('cham-a', ACCOUNT)).toBe(1)
  expect(numericStreamId('cham-z', 'hub||Bob')).toBe(2)
  // Even the same chamber seen through another token is its own row: which
  // token can open it is part of what the number identifies.
  expect(numericStreamId('cham-a', 'hub||Bob')).toBe(3)
  // …and every one of them is stable on re-ask.
  expect(numericStreamId('cham-a', ACCOUNT)).toBe(1)
  expect(numericStreamId('cham-z', 'hub||Bob')).toBe(2)
})

test('an unsent draft follows its chamber to the new numbering', () => {
  // The pre-merge build numbered per token; a draft keyed by the old number
  // would otherwise be stranded on a row that no longer exists.
  localStorage.setItem(
    'agent-console.hub-ids.hub||Carol',
    JSON.stringify({ next: 2, byChamber: { 'cham-q': 1 } }),
  )
  localStorage.setItem('agent-console.draft.hub||Carol.1', 'half a sentence')
  // Another token got number 1 first, which is exactly the collision the new
  // numbering exists to prevent.
  numericStreamId('cham-other', 'hub||Dave')
  const id = numericStreamId('cham-q', 'hub||Carol')
  expect(id).not.toBe(1)
  expect(localStorage.getItem(`agent-console.draft.hub||Carol.${id}`)).toBe('half a sentence')
  expect(localStorage.getItem('agent-console.draft.hub||Carol.1')).toBeNull()
})

test('sendMessage posts body with CSRF header and 401 throws an auth error', async () => {
  const fetchFn = mockFetch((url, init) => {
    if (url.endsWith('/send')) {
      expect((init?.headers as Record<string, string>)['X-Cryo-CSRF']).toBe('1')
      expect(JSON.parse(String(init?.body)).body).toBe('do it')
      return { ok: true }
    }
    return [{ id: 'cham-a', name: 'alpha' }]
  })
  const c = new HubClient(creds, fetchFn)
  await c.register()
  await c.sendMessage('alpha', 'do it')

  const denied = new HubClient(creds, mockFetch(() => new Response('', { status: 401 })))
  await expect(denied.register()).rejects.toMatchObject({ status: 401 })
})

test('an unresolvable project name is marked as a client-side 404, not a server one', async () => {
  // Before register() (offline cold boot, cached projects on screen) the name
  // map is empty. The marker is what stops callers treating this like a chamber
  // the hub says is gone.
  const c = new HubClient(creds, mockFetch(() => new Response('', { status: 500 })))
  await expect(c.getMessages('alpha')).rejects.toBeInstanceOf(UnresolvedProjectError)
  await expect(c.getMessages('alpha')).rejects.toMatchObject({ status: 404 })

  const registered = new HubClient(
    creds,
    mockFetch((url) =>
      url.includes('/messages')
        ? new Response('', { status: 404 })
        : [{ id: 'cham-a', name: 'alpha' }],
    ),
  )
  await registered.register()
  // A real server 404 is the plain ApiError, not the client-side marker.
  const err = await registered.getMessages('alpha').catch((e: unknown) => e)
  expect(err).toBeInstanceOf(ApiError)
  expect(err).not.toBeInstanceOf(UnresolvedProjectError)
  expect(err).toMatchObject({ status: 404 })
})

test('uploadFile posts multipart and returns the files URL', async () => {
  const fetchFn = mockFetch((url, init) => {
    if (url.endsWith('/uploads')) {
      expect(init?.body).toBeInstanceOf(FormData)
      return { ok: true, name: 'ab_report.pdf', markdown: '[report.pdf](/api/chambers/cham-a/files/ab_report.pdf)' }
    }
    return [{ id: 'cham-a', name: 'alpha' }]
  })
  const c = new HubClient(creds, fetchFn)
  await c.register()
  const url = await c.uploadFile(new File(['x'], 'report.pdf'), 'alpha')
  expect(url).toBe('/api/chambers/cham-a/files/ab_report.pdf')
})

test('invite management wrappers hit the token routes', async () => {
  const calls: string[] = []
  const fetchFn = mockFetch((url, init) => {
    calls.push(`${init?.method ?? 'GET'} ${url}`)
    if (url.endsWith('/api/tokens') && init?.method === 'POST') return { ok: true, token: 'ff'.repeat(32) }
    if (url.endsWith('/api/tokens')) return { invites: [{ name: 'Bob', chambers: [], created_at: 't', revoked_at: null }] }
    return { ok: true }
  })
  const c = new HubClient(creds, fetchFn)
  expect((await c.listInvites())[0].name).toBe('Bob')
  expect((await c.createInvite('Cara', ['cham-a'])).token).toHaveLength(64)
  await c.revokeInvite('Cara')
  expect(calls).toContain('POST /api/tokens/Cara/revoke')
})

test('createInvite surfaces the hub\'s own words on a rejected name', async () => {
  // A duplicate name is a considered answer, not a broken connection, and the
  // hub sometimes says why — so the caller gets those words verbatim.
  const explained = new HubClient(
    creds,
    mockFetch(
      () =>
        new Response(JSON.stringify({ error: "an active invite named 'Bob' already exists" }), {
          status: 400,
        }),
    ),
  )
  await expect(explained.createInvite('Bob', ['cham-a'])).rejects.toThrow(
    "an active invite named 'Bob' already exists",
  )

  // The hub's token routes actually answer a bare 400: then the status is all
  // there is to report, and the caller decides how to phrase it.
  const bare = new HubClient(creds, mockFetch(() => new Response('', { status: 400 })))
  await expect(bare.createInvite('Bob', ['cham-a'])).rejects.toThrow('HTTP 400')
})

describe('SSE message mapping', () => {
  const chambers = [{ id: 'cham-a', name: 'alpha' }]

  test('the same mailbox id maps to the same numeric id via SSE and via getMessages', async () => {
    const fetchFn = mockFetch((url) =>
      url.includes('/messages')
        ? [{ id: 'msg-7', direction: 'outbox', from: 'agent', subject: 's', body: 'hi',
             timestamp: '2026-08-15T10:00:00', session: 1, is_question: false }]
        : chambers,
    )
    const c = new HubClient(creds, fetchFn)
    await c.register()
    const [fetched] = await c.getMessages('alpha')
    const live = c.toChamberEventMessage({
      id: 'msg-7',
      chamber_id: 'cham-a',
      from: 'agent',
      subject: 's',
      body: 'hi',
      timestamp: '2026-08-15T10:00:00',
      is_question: false,
    })
    // Redelivery of a message we already fetched must dedupe, not double up.
    expect(live!.id).toBe(fetched.id)
  })

  test('a payload without an id still gets a deterministic synthesized one', async () => {
    const c = new HubClient(creds, mockFetch(() => chambers))
    await c.register()
    const payload = {
      chamber_id: 'cham-a',
      from: 'agent',
      subject: 's',
      body: 'hi',
      timestamp: '2026-08-15T10:00:00',
      is_question: false,
    }
    expect(c.toChamberEventMessage(payload)!.id).toBe(c.toChamberEventMessage(payload)!.id)
  })

  test('a payload for a chamber outside our scope is dropped', async () => {
    const c = new HubClient(creds, mockFetch(() => chambers))
    await c.register()
    expect(
      c.toChamberEventMessage({
        chamber_id: 'cham-z', from: 'a', subject: '', body: 'x',
        timestamp: '2026-08-15T10:00:00', is_question: false,
      }),
    ).toBeNull()
  })
})

describe('owner chamber routes', () => {
  const STATUS = {
    running: true, agent_running: false, session: 4, agent: 'opencode',
    log_tail: 'line one\nline two', daily_digests: [], next_wake: '2026-08-15T18:00',
    notes_content: '', notes_html: '<p>notes</p>', plan_content: '', plan_html: '<p>plan</p>',
    has_config: true, settings_rows: [{ key: 'agent', value: '"opencode"', kind: 'scalar' }],
    task: null, session_summary: 'swept the decoders', completed: false, completion_summary: null,
  }

  test('chamberStatus GETs the status route with the bearer header and no CSRF', async () => {
    const fetchFn = mockFetch(() => STATUS)
    const c = new HubClient(creds, fetchFn)
    const status = await c.chamberStatus('cham-a')
    expect(status.session).toBe(4)
    expect(status.plan_html).toBe('<p>plan</p>')
    const [url, init] = vi.mocked(fetchFn).mock.calls[0]
    expect(String(url)).toBe('/api/chambers/cham-a/status')
    expect(init?.method).toBeUndefined()
    expect(init?.headers).toMatchObject({ Authorization: 'Bearer tok123' })
    expect(init?.headers).not.toHaveProperty('X-Cryo-CSRF')
  })

  test('chamberTodos and chamberSync GET their routes', async () => {
    const fetchFn = mockFetch((url) =>
      String(url).endsWith('/todos')
        ? [{ id: 1, text: 'check the runner', done: false, claimed: false, at: '2026-08-15T18:00', created: '2026-08-14T09:00' }]
        : [{ backend: 'zulip', configured: true, installed: true, running: false, target: '#research', last_pushed_session: 3, log_tail_path: '/tmp/z.log' }],
    )
    const c = new HubClient(creds, fetchFn)
    expect((await c.chamberTodos('cham-a'))[0].text).toBe('check the runner')
    expect((await c.chamberSync('cham-a'))[0].backend).toBe('zulip')
    expect(vi.mocked(fetchFn).mock.calls.map(([u]) => String(u))).toEqual([
      '/api/chambers/cham-a/todos',
      '/api/chambers/cham-a/sync',
    ])
  })

  test('lifecycle POSTs the action and returns the hub ok/message verbatim', async () => {
    const fetchFn = mockFetch(() => ({ ok: true, message: 'Chamber started' }))
    const c = new HubClient(creds, fetchFn)
    expect(await c.lifecycle('cham-a', 'start')).toEqual({ ok: true, message: 'Chamber started' })
    const [url, init] = vi.mocked(fetchFn).mock.calls[0]
    expect(String(url)).toBe('/api/chambers/cham-a/start')
    expect(init?.method).toBe('POST')
    expect(init?.headers).toMatchObject({ Authorization: 'Bearer tok123', 'X-Cryo-CSRF': '1' })
  })

  test('syncAction POSTs backend and verb into the path', async () => {
    const fetchFn = mockFetch(() => ({ ok: true, message: 'zulip start' }))
    const c = new HubClient(creds, fetchFn)
    expect(await c.syncAction('cham-a', 'zulip', 'stop')).toEqual({ ok: true, message: 'zulip start' })
    const [url, init] = vi.mocked(fetchFn).mock.calls[0]
    expect(String(url)).toBe('/api/chambers/cham-a/sync/zulip/stop')
    expect(init?.headers).toMatchObject({ 'X-Cryo-CSRF': '1' })
  })

  test('chamber ids are percent-encoded into every path', async () => {
    const fetchFn = mockFetch(() => STATUS)
    await new HubClient(creds, fetchFn).chamberStatus('work/alpha')
    expect(String(vi.mocked(fetchFn).mock.calls[0][0])).toBe('/api/chambers/work%2Falpha/status')
  })

  test('createChamber returns the new id and surfaces the hub error text on 400', async () => {
    const ok = mockFetch(() => new Response(JSON.stringify({ id: 'cham-new' }), { status: 201 }))
    const c = new HubClient(creds, ok)
    expect(await c.createChamber({ name: 'alpha' })).toEqual({ id: 'cham-new' })
    const [url, init] = vi.mocked(ok).mock.calls[0]
    expect(String(url)).toBe('/api/chambers/new')
    expect(init?.method).toBe('POST')
    expect(init?.headers).toMatchObject({ 'Content-Type': 'application/json', 'X-Cryo-CSRF': '1' })
    expect(JSON.parse(String(init?.body))).toEqual({ name: 'alpha' })

    const bad = new HubClient(
      creds,
      mockFetch(() => new Response(JSON.stringify({ error: 'chamber already exists' }), { status: 400 })),
    )
    await expect(bad.createChamber({ name: 'alpha' })).rejects.toThrow('chamber already exists')

    // 201 with an empty id is a real server path (index refresh missed the
    // new chamber); it must not read as success.
    const blank = new HubClient(creds, mockFetch(() => new Response(JSON.stringify({ id: '' }), { status: 201 })))
    await expect(blank.createChamber({ name: 'alpha' })).rejects.toThrow(/did not report its id/)
  })

  test('createInvite rejects a 200 that carries no token', async () => {
    const c = new HubClient(creds, mockFetch(() => new Response('{}', { status: 200 })))
    await expect(c.createInvite('Bob', ['cham-a'])).rejects.toThrow(/did not return an invite token/)
  })

  test('refreshIndex POSTs the refresh route', async () => {
    const fetchFn = mockFetch(() => [])
    await new HubClient(creds, fetchFn).refreshIndex()
    const [url, init] = vi.mocked(fetchFn).mock.calls[0]
    expect(String(url)).toBe('/api/chambers/refresh')
    expect(init?.method).toBe('POST')
  })

  test('register carries completion, archive and open-question flags, and maps ids both ways', async () => {
    const fetchFn = mockFetch(() => [
      { id: 'cham-a', name: 'alpha', running: true, agent_running: false, completed: true, archived: false, has_open_question: true },
    ])
    const c = new HubClient(creds, fetchFn)
    const sub = (await c.register()).subscriptions[0]
    expect(sub).toMatchObject({ completed: true, archived: false, hasOpenQuestion: true })
    expect(c.streamIdFor('cham-a')).toBe(sub.stream_id)
    expect(c.chamberIdFor(sub.stream_id)).toBe('cham-a')
    expect(c.streamIdFor('cham-zzz')).toBeUndefined()
  })

  test('chamberStatuses re-reads the same three flags for a status event', async () => {
    const c = new HubClient(
      creds,
      mockFetch(() => [{ id: 'cham-a', name: 'alpha', completed: false, archived: true, has_open_question: false }]),
    )
    await c.register()
    expect((await c.chamberStatuses())[0]).toMatchObject({
      completed: false, archived: true, hasOpenQuestion: false,
    })
  })
})

describe('request<T> error funnel', () => {
  test('non-2xx with {error} surfaces the hub text', async () => {
    const fetchFn = mockFetch(() => new Response(JSON.stringify({ error: 'no such chamber' }), { status: 404 }))
    const c = new HubClient(creds, fetchFn)
    await expect(c.chamberStatus('x')).rejects.toMatchObject({ status: 404, message: 'no such chamber' })
  })

  test('non-2xx without a body falls back to HTTP N', async () => {
    const fetchFn = mockFetch(() => new Response('', { status: 502 }))
    const c = new HubClient(creds, fetchFn)
    await expect(c.chamberStatus('x')).rejects.toMatchObject({ status: 502, message: 'HTTP 502' })
  })

  test('200 with ok:false throws ApiError carrying message', async () => {
    const fetchFn = mockFetch(() => ({ ok: false, message: 'already running' }))
    const c = new HubClient(creds, fetchFn)
    await expect(c.lifecycle('x', 'start')).rejects.toBeInstanceOf(ApiError)
    await expect(c.lifecycle('x', 'start')).rejects.toMatchObject({ status: 200, message: 'already running' })
  })

  test('createChamber and createInvite go through the same funnel', async () => {
    const fetchFn = mockFetch(() => new Response(JSON.stringify({ error: 'bad name' }), { status: 400 }))
    const c = new HubClient(creds, fetchFn)
    await expect(c.createChamber({ name: 'x' })).rejects.toMatchObject({ status: 400, message: 'bad name' })
    await expect(c.createInvite('x', ['a'])).rejects.toMatchObject({ status: 400, message: 'bad name' })
  })
})
