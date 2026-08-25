import { HubClient, toChamber, toChamberMessage } from './hubClient'
import { ApiError } from './types'

const OPTS = { token: 'tok123' }

function mockFetch(handler: (url: string, init?: RequestInit) => object | Response) {
  return vi.fn(async (url: string, init?: RequestInit) => {
    const out = handler(url, init)
    return out instanceof Response ? out : new Response(JSON.stringify(out), { status: 200 })
  }) as unknown as typeof fetch
}

beforeEach(() => localStorage.clear())

test('listChambers maps the hub index; absent flags stay unknown; stopped chambers show no wake', async () => {
  const fetchFn = mockFetch(() => [
    { id: 'cham-b', name: 'beta', running: false, next_wake_display: 'in 2 h' },
    { id: 'cham-a', name: 'alpha', running: true, agent_running: true, next_wake_display: 'in 1 h', completed: true, archived: false, has_open_question: true },
  ])
  const list = await new HubClient({ ...OPTS, fetch: fetchFn }).listChambers()
  expect(list).toEqual([
    { id: 'cham-b', name: 'beta', running: false, agentRunning: undefined, nextWakeDisplay: null, completed: false, archived: false, hasOpenQuestion: false },
    { id: 'cham-a', name: 'alpha', running: true, agentRunning: true, nextWakeDisplay: 'in 1 h', completed: true, archived: false, hasOpenQuestion: true },
  ])
  expect(vi.mocked(fetchFn).mock.calls[0][1]?.headers).toMatchObject({ Authorization: 'Bearer tok123' })
})

test('getMessages maps and sorts by messageKey (timestamp, then id)', async () => {
  const fetchFn = mockFetch(() => [
    { id: 'outbox/2.md', direction: 'outbox', from: 'agent', subject: '', body: 'later', timestamp: '2026-08-15T10:05:00', session: 2, is_question: true },
    { id: 'inbox/1.md', direction: 'inbox', from: 'human', subject: 's', body: 'first', timestamp: '2026-08-15T10:00:00', is_question: false },
  ])
  const msgs = await new HubClient({ ...OPTS, fetch: fetchFn }).getMessages('cham-a')
  expect(msgs.map((m) => m.id)).toEqual(['inbox/1.md', 'outbox/2.md'])
  expect(msgs[1]).toEqual({ id: 'outbox/2.md', chamberId: 'cham-a', direction: 'outbox', sender: 'agent', subject: '', body: 'later', timestamp: '2026-08-15T10:05:00', session: 2, isQuestion: true })
  expect(msgs[0].session).toBeNull()
  expect(String(vi.mocked(fetchFn).mock.calls[0][0])).toBe('/api/chambers/cham-a/messages')
})

test('toEventMessage maps an SSE payload and rejects one without chamber_id', () => {
  const c = new HubClient(OPTS)
  expect(c.toEventMessage({ id: 'inbox/x.md', chamber_id: 'cham-a', direction: 'inbox', from: 'me', subject: '', body: 'b', timestamp: '2026-08-15T10:00:00', is_question: false }))
    .toEqual({ id: 'inbox/x.md', chamberId: 'cham-a', direction: 'inbox', sender: 'me', subject: '', body: 'b', timestamp: '2026-08-15T10:00:00', session: null, isQuestion: false })
  expect(c.toEventMessage({ from: 'me' })).toBeNull()
})

test('sendMessage returns the id the hub minted; uploadFile posts to the chamber', async () => {
  const fetchFn = mockFetch((url) =>
    url.endsWith('/send') ? { ok: true, id: 'inbox/new.md' } : { name: 'a.png', markdown: '[a.png](/api/chambers/cham-a/files/a.png)' },
  )
  const c = new HubClient({ ...OPTS, fetch: fetchFn })
  await expect(c.sendMessage('cham-a', 'hi')).resolves.toEqual({ id: 'inbox/new.md' })
  await expect(c.uploadFile(new File(['x'], 'a.png'), 'cham-a')).resolves.toBe('/api/chambers/cham-a/files/a.png')
  const [uploadUrl, uploadInit] = vi.mocked(fetchFn).mock.calls[1]
  expect(String(uploadUrl)).toBe('/api/chambers/cham-a/uploads')
  // No manual Content-Type: only the browser knows the multipart boundary.
  expect(uploadInit?.body).toBeInstanceOf(FormData)
  expect(uploadInit?.headers).not.toHaveProperty('Content-Type')
})

test('a send the hub answered without an id yields an empty id, not undefined', async () => {
  // The outbox waits on this string; `undefined` would silently match nothing.
  const c = new HubClient({ ...OPTS, fetch: mockFetch(() => ({ ok: true })) })
  await expect(c.sendMessage('cham-a', 'hi')).resolves.toEqual({ id: '' })
})

test('uploadFile falls back to the files route when the markdown is unreadable', async () => {
  const c = new HubClient({ ...OPTS, fetch: mockFetch(() => ({ name: 'a.png' })) })
  await expect(c.uploadFile(new File(['x'], 'a.png'), 'cham-a')).resolves.toBe(
    '/api/chambers/cham-a/files/a.png',
  )
})

test('toChamber/toChamberMessage tolerate junk fields', () => {
  expect(toChamber({ id: 'x', name: 'X', running: 'yes' as unknown as boolean })).toMatchObject({
    running: undefined,
    agentRunning: undefined,
  })
  expect(toChamberMessage({ id: 'inbox/1.md', from: 1 as unknown as string, timestamp: 't' }, 'c').sender).toBe('')
})

test('invite management wrappers hit the token routes', async () => {
  const calls: string[] = []
  const fetchFn = mockFetch((url, init) => {
    calls.push(`${init?.method ?? 'GET'} ${url}`)
    if (url.endsWith('/api/tokens') && init?.method === 'POST') return { ok: true, token: 'ff'.repeat(32) }
    if (url.endsWith('/api/tokens')) return { invites: [{ name: 'Bob', chambers: [], created_at: 't', revoked_at: null }] }
    return { ok: true }
  })
  const c = new HubClient({ ...OPTS, fetch: fetchFn })
  expect((await c.listInvites())[0].name).toBe('Bob')
  expect((await c.createInvite('Cara', ['cham-a'])).token).toHaveLength(64)
  await c.revokeInvite('Cara')
  expect(calls).toContain('POST /api/tokens/Cara/revoke')
})

test('host agent config uses the owner-only config route', async () => {
  const calls: Array<[string, RequestInit | undefined]> = []
  const fetchFn = mockFetch((url, init) => {
    calls.push([String(url), init])
    return { default_agent: init?.method === 'POST' ? 'pi --thinking high' : 'pi' }
  })
  const client = new HubClient({ ...OPTS, fetch: fetchFn })

  await expect(client.hostConfig()).resolves.toEqual({ default_agent: 'pi' })
  await expect(client.updateHostConfig('pi --thinking high')).resolves.toEqual({
    default_agent: 'pi --thinking high',
  })
  expect(calls[0][0]).toBe('/api/config')
  expect(calls[0][1]?.method).toBeUndefined()
  expect(calls[1][0]).toBe('/api/config')
  expect(calls[1][1]?.method).toBe('POST')
  expect(calls[1][1]?.body).toBe(JSON.stringify({ default_agent: 'pi --thinking high' }))
})

test('setChamberAgent posts to the chamber agent route and fills in absent flags', async () => {
  const calls: Array<[string, RequestInit | undefined]> = []
  const fetchFn = mockFetch((url, init) => {
    calls.push([String(url), init])
    return { agent: 'claude', restart_required: true, override_active: true }
  })
  const client = new HubClient({ ...OPTS, fetch: fetchFn })

  await expect(client.setChamberAgent('cham a/b', 'claude')).resolves.toEqual({
    agent: 'claude',
    restart_required: true,
    override_active: true,
  })
  // Ids can carry a path separator, so they are encoded or they address a
  // different route.
  expect(calls[0][0]).toBe('/api/chambers/cham%20a%2Fb/agent')
  expect(calls[0][1]?.method).toBe('POST')
  expect(calls[0][1]?.body).toBe(JSON.stringify({ agent: 'claude' }))

  // A hub that answers with nothing to say means "no, and no": the caller must
  // never read an absent flag as a warning it then shows the operator.
  const quiet = new HubClient({ ...OPTS, fetch: mockFetch(() => ({})) })
  await expect(quiet.setChamberAgent('cham-a', 'pi')).resolves.toEqual({
    agent: 'pi',
    restart_required: false,
    override_active: false,
  })
})

test('setChamberPlan posts the raw markdown to the chamber plan route', async () => {
  const calls: Array<[string, RequestInit | undefined]> = []
  const fetchFn = mockFetch((url, init) => {
    calls.push([String(url), init])
    return { bytes: 8 }
  })
  const client = new HubClient({ ...OPTS, fetch: fetchFn })

  await client.setChamberPlan('cham-a', '# Brief\n')

  expect(calls[0][0]).toBe('/api/chambers/cham-a/plan')
  expect(calls[0][1]?.method).toBe('POST')
  expect(calls[0][1]?.body).toBe(JSON.stringify({ content: '# Brief\n' }))
})

test('createInvite surfaces the hub\'s own words on a rejected name', async () => {
  // A duplicate name is a considered answer, not a broken connection, and the
  // hub sometimes says why — so the caller gets those words verbatim.
  const explained = new HubClient({
    ...OPTS,
    fetch: mockFetch(
      () =>
        new Response(JSON.stringify({ error: "an active invite named 'Bob' already exists" }), {
          status: 400,
        }),
    ),
  })
  await expect(explained.createInvite('Bob', ['cham-a'])).rejects.toThrow(
    "an active invite named 'Bob' already exists",
  )

  // The hub's token routes actually answer a bare 400: then the status is all
  // there is to report, and the caller decides how to phrase it.
  const bare = new HubClient({ ...OPTS, fetch: mockFetch(() => new Response('', { status: 400 })) })
  await expect(bare.createInvite('Bob', ['cham-a'])).rejects.toThrow('HTTP 400')
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
    const c = new HubClient({ ...OPTS, fetch: fetchFn })
    const status = await c.chamberStatus('cham-a')
    expect(status.session).toBe(4)
    expect(status.plan_html).toBe('<p>plan</p>')
    const [url, init] = vi.mocked(fetchFn).mock.calls[0]
    expect(String(url)).toBe('/api/chambers/cham-a/status')
    expect(init?.method).toBeUndefined()
    expect(init?.headers).toMatchObject({ Authorization: 'Bearer tok123' })
    expect(init?.headers).not.toHaveProperty('X-Cryo-CSRF')
  })

  test('chamberTodos GETs its route', async () => {
    const fetchFn = mockFetch(() => [
      { id: 1, text: 'check the runner', done: false, claimed: false, at: '2026-08-15T18:00', created: '2026-08-14T09:00' },
    ])
    const c = new HubClient({ ...OPTS, fetch: fetchFn })
    expect((await c.chamberTodos('cham-a'))[0].text).toBe('check the runner')
    expect(vi.mocked(fetchFn).mock.calls.map(([u]) => String(u))).toEqual([
      '/api/chambers/cham-a/todos',
    ])
  })

  test('lifecycle POSTs the action and returns the hub ok/message verbatim', async () => {
    const fetchFn = mockFetch(() => ({ ok: true, message: 'Chamber started' }))
    const c = new HubClient({ ...OPTS, fetch: fetchFn })
    expect(await c.lifecycle('cham-a', 'start')).toEqual({ ok: true, message: 'Chamber started' })
    const [url, init] = vi.mocked(fetchFn).mock.calls[0]
    expect(String(url)).toBe('/api/chambers/cham-a/start')
    expect(init?.method).toBe('POST')
    expect(init?.headers).toMatchObject({ Authorization: 'Bearer tok123', 'X-Cryo-CSRF': '1' })
  })

  test('chamber ids are percent-encoded into every path', async () => {
    const fetchFn = mockFetch(() => STATUS)
    await new HubClient({ ...OPTS, fetch: fetchFn }).chamberStatus('work/alpha')
    expect(String(vi.mocked(fetchFn).mock.calls[0][0])).toBe('/api/chambers/work%2Falpha/status')
  })

  test('createChamber returns the new id and surfaces the hub error text on 400', async () => {
    const ok = mockFetch(() => new Response(JSON.stringify({
      id: 'cham-new', started: true, start_error: null,
    }), { status: 201 }))
    const c = new HubClient({ ...OPTS, fetch: ok })
    expect(await c.createChamber({ name: 'alpha', start: true })).toEqual({
      id: 'cham-new', started: true, start_error: null,
    })
    const [url, init] = vi.mocked(ok).mock.calls[0]
    expect(String(url)).toBe('/api/chambers/new')
    expect(init?.method).toBe('POST')
    expect(init?.headers).toMatchObject({ 'Content-Type': 'application/json', 'X-Cryo-CSRF': '1' })
    expect(JSON.parse(String(init?.body))).toEqual({ name: 'alpha', start: true })

    const warned = new HubClient({
      ...OPTS,
      fetch: mockFetch(() => new Response(JSON.stringify({
        id: 'cham-new', started: false, start_error: 'service install failed',
      }), { status: 201 })),
    })
    await expect(warned.createChamber({ name: 'alpha', start: true })).resolves.toEqual({
      id: 'cham-new', started: false, start_error: 'service install failed',
    })

    const bad = new HubClient({ ...OPTS, fetch: mockFetch(() => new Response(JSON.stringify({ error: 'chamber already exists' }), { status: 400 })) })
    await expect(bad.createChamber({ name: 'alpha' })).rejects.toThrow('chamber already exists')

    // 201 with an empty id is a real server path (index refresh missed the
    // new chamber); it must not read as success.
    const blank = new HubClient({ ...OPTS, fetch: mockFetch(() => new Response(JSON.stringify({ id: '' }), { status: 201 })) })
    await expect(blank.createChamber({ name: 'alpha' })).rejects.toThrow(/did not report its id/)
  })

  test('createInvite rejects a 200 that carries no token', async () => {
    const c = new HubClient({ ...OPTS, fetch: mockFetch(() => new Response('{}', { status: 200 })) })
    await expect(c.createInvite('Bob', ['cham-a'])).rejects.toThrow(/did not return an invite token/)
  })

  test('refreshIndex POSTs the refresh route', async () => {
    const fetchFn = mockFetch(() => [])
    await new HubClient({ ...OPTS, fetch: fetchFn }).refreshIndex()
    const [url, init] = vi.mocked(fetchFn).mock.calls[0]
    expect(String(url)).toBe('/api/chambers/refresh')
    expect(init?.method).toBe('POST')
  })

  test('listChambers carries completion, archive and open-question flags', async () => {
    const fetchFn = mockFetch(() => [
      { id: 'cham-a', name: 'alpha', running: true, agent_running: false, completed: true, archived: false, has_open_question: true },
    ])
    const c = new HubClient({ ...OPTS, fetch: fetchFn })
    expect((await c.listChambers())[0]).toMatchObject({
      id: 'cham-a', completed: true, archived: false, hasOpenQuestion: true,
    })
  })

  test('a row without an id is dropped rather than rendered as a nameless card', async () => {
    const c = new HubClient({ ...OPTS, fetch: mockFetch(() => [{ name: 'nameless' }, { id: 'cham-a', name: 'alpha' }]) })
    expect((await c.listChambers()).map((x) => x.id)).toEqual(['cham-a'])
  })
})

describe('request<T> error funnel', () => {
  test('non-2xx with {error} surfaces the hub text', async () => {
    const fetchFn = mockFetch(() => new Response(JSON.stringify({ error: 'no such chamber' }), { status: 404 }))
    const c = new HubClient({ ...OPTS, fetch: fetchFn })
    await expect(c.chamberStatus('x')).rejects.toMatchObject({ status: 404, message: 'no such chamber' })
  })

  test('non-2xx without a body falls back to HTTP N', async () => {
    const fetchFn = mockFetch(() => new Response('', { status: 502 }))
    const c = new HubClient({ ...OPTS, fetch: fetchFn })
    await expect(c.chamberStatus('x')).rejects.toMatchObject({ status: 502, message: 'HTTP 502' })
  })

  test('200 with ok:false throws ApiError carrying message', async () => {
    const fetchFn = mockFetch(() => ({ ok: false, message: 'already running' }))
    const c = new HubClient({ ...OPTS, fetch: fetchFn })
    await expect(c.lifecycle('x', 'start')).rejects.toBeInstanceOf(ApiError)
    await expect(c.lifecycle('x', 'start')).rejects.toMatchObject({ status: 200, message: 'already running' })
  })

  test('createChamber and createInvite go through the same funnel', async () => {
    const fetchFn = mockFetch(() => new Response(JSON.stringify({ error: 'bad name' }), { status: 400 }))
    const c = new HubClient({ ...OPTS, fetch: fetchFn })
    await expect(c.createChamber({ name: 'x' })).rejects.toMatchObject({ status: 400, message: 'bad name' })
    await expect(c.createInvite('x', ['a'])).rejects.toMatchObject({ status: 400, message: 'bad name' })
  })
})

describe('the 401 hook and authenticated blobs', () => {
  test('401 runs onAuthFailure once, then throws ApiError', async () => {
    const onAuthFailure = vi.fn()
    const fetchFn = mockFetch(() => new Response('', { status: 401 }))
    const c = new HubClient({ token: 't', onAuthFailure, fetch: fetchFn })
    await expect(c.whoami()).rejects.toMatchObject({ status: 401 })
    expect(onAuthFailure).toHaveBeenCalledTimes(1)
  })

  test('fetchBlob sends the bearer header and funnels 401', async () => {
    const onAuthFailure = vi.fn()
    const fetchFn = mockFetch(() => new Response('', { status: 401 }))
    const c = new HubClient({ token: 't', onAuthFailure, fetch: fetchFn })
    await expect(c.fetchBlob('/api/chambers/a/files/x.png')).rejects.toMatchObject({ status: 401 })
    expect(onAuthFailure).toHaveBeenCalledTimes(1)
    expect(vi.mocked(fetchFn).mock.calls[0][1]?.headers).toMatchObject({ Authorization: 'Bearer t' })
  })

  test('sendMessage posts only the body, with CSRF — the hub stamps the sender', async () => {
    const fetchFn = mockFetch(() => ({ ok: true, id: 'inbox/x.md' }))
    const c = new HubClient({ token: 't', fetch: fetchFn })
    await c.sendMessage('cham-a', 'hi')
    const [url, init] = vi.mocked(fetchFn).mock.calls[0]
    expect(String(url)).toBe('/api/chambers/cham-a/send')
    expect(JSON.parse(String(init?.body))).toEqual({ body: 'hi' })
    expect(init?.headers).toMatchObject({ 'X-Cryo-CSRF': '1' })
  })
})

describe('events()', () => {
  test('honours the injected fetch and runs onAuthFailure on a 401 connect', async () => {
    const onAuthFailure = vi.fn()
    const fetchFn = vi.fn(async () => new Response('', { status: 401 })) as unknown as typeof fetch
    const c = new HubClient({ token: 't', onAuthFailure, fetch: fetchFn })
    await expect(
      c.events(() => {}, new AbortController().signal),
    ).rejects.toMatchObject({ status: 401 })
    expect(onAuthFailure).toHaveBeenCalledTimes(1)
    expect(vi.mocked(fetchFn)).toHaveBeenCalledTimes(1)
    expect(String(vi.mocked(fetchFn).mock.calls[0][0])).toBe('/api/events')
    expect(vi.mocked(fetchFn).mock.calls[0][1]?.headers).toMatchObject({
      Authorization: 'Bearer t',
    })
  })
})

describe('baseUrl', () => {
  it('prefixes every request path', async () => {
    const calls: string[] = []
    const fakeFetch = (async (input: RequestInfo | URL) => {
      calls.push(String(input))
      return new Response(JSON.stringify({ role: 'owner' }), { status: 200 })
    }) as typeof fetch
    const c = new HubClient({ token: 't', baseUrl: 'http://hub.local:8765', fetch: fakeFetch })
    await c.whoami()
    expect(calls).toEqual(['http://hub.local:8765/api/whoami'])
  })

  it('fetchBlob prefixes hub-relative urls and leaves absolute ones alone', async () => {
    const calls: string[] = []
    const fakeFetch = (async (input: RequestInfo | URL) => {
      calls.push(String(input))
      return new Response(new Blob(['x']), { status: 200 })
    }) as typeof fetch
    const c = new HubClient({ token: 't', baseUrl: 'http://hub.local:8765', fetch: fakeFetch })
    await c.fetchBlob('/api/chambers/a/files/pic.png')
    await c.fetchBlob('http://elsewhere.example/x')
    expect(calls).toEqual([
      'http://hub.local:8765/api/chambers/a/files/pic.png',
      'http://elsewhere.example/x',
    ])
  })

  it('defaults to same-origin relative paths (browser mode unchanged)', async () => {
    const calls: string[] = []
    const fakeFetch = (async (input: RequestInfo | URL) => {
      calls.push(String(input))
      return new Response(JSON.stringify({ role: 'owner' }), { status: 200 })
    }) as typeof fetch
    const c = new HubClient({ token: 't', fetch: fakeFetch })
    await c.whoami()
    expect(calls).toEqual(['/api/whoami'])
  })
})
