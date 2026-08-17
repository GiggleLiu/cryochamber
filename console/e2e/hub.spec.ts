import { test, expect, type Page } from '@playwright/test'

/**
 * The hub backend end to end, against a mocked cryohub. Covers what no unit
 * test can: the invite link is the entire sign-in, scope is whatever the hub
 * hands this token and nothing more, live messages arrive over SSE, a session
 * revoked mid-flight lands back on login, and the sender shown is the one the
 * server stamped — not the one the client asked for.
 */
const TOKEN = 'ab'.repeat(16) // invite: scoped to one chamber
const OWNER_TOKEN = 'cc'.repeat(16) // owner: sees both

/** The hub holds two chambers; an invite is only ever told about its own. */
const CHAMBERS = [
  { id: 'cham-a', name: 'autoresearch' },
  { id: 'cham-b', name: 'private-lab' },
]

const MESSAGE = {
  id: 'msg-1',
  direction: 'outbox',
  from: 'autoresearch-agent',
  subject: '',
  body: '**done** — the threshold is $x^2$',
  timestamp: '2026-08-15T10:00:00',
  session: 1,
  is_question: false,
}

/** What the hub stamps on the message the client sends below. The client never
 * names a sender, so this is the only word on who spoke — and the thread must
 * show it verbatim. */
const SERVER_STAMPED_SENDER = 'alice (invite)'

/** The mailbox id `POST /send` mints. The SSE echo carries the same id,
 * because it is the same message coming back: that id is the only correlation
 * the client has for retiring the pending bubble. */
const SENT_ID = 'inbox/2026-08-16T10-00-00_human_1.md'

interface HubMock {
  /** Bodies POSTed to /send, in order. */
  sent: unknown[]
  /** Bodies POSTed to /api/tokens, in order. */
  created: unknown[]
  /** How many times the chamber index was fetched (i.e. registers). */
  registers: number
  /** Lifecycle actions taken, as `<chamber>/<action>`. */
  actions: string[]
  /** Invite names revoked, in order. */
  revoked: string[]
}

interface HubOptions {
  role?: 'owner' | 'invite'
  whoamiStatus?: number
  /** Deliver one SSE message (then EOF) once the client has sent something. */
  echoSends?: boolean
  /** After the first SSE stream ends, answer the next one with 401 — the token
   *  being revoked while the session is live. */
  revokeAfterFirstStream?: boolean
  /** Whether cham-a is already running when its status is first read. */
  running?: boolean
  /** Seed for the People-with-access list `GET /api/tokens` answers with. */
  invites?: Array<{
    name: string
    chambers: string[]
    created_at: string
    revoked_at: string | null
  }>
}

async function mockHub(page: Page, opts: HubOptions = {}): Promise<HubMock> {
  const mock: HubMock = { sent: [], created: [], registers: 0, actions: [], revoked: [] }

  await page.route('**/api/whoami', (r) =>
    opts.whoamiStatus
      ? r.fulfill({ status: opts.whoamiStatus, body: '' })
      : r.fulfill({
          json: { role: opts.role ?? 'invite', name: 'Alice', hub_version: '0.3.0' },
        }),
  )

  // Scope is the hub's answer, per token: the owner's own chamber is simply not
  // in the invite's index, so "only the scoped project shows" is not vacuous.
  await page.route('**/api/chambers', (r) => {
    mock.registers += 1
    const bearer = r.request().headers()['authorization'] ?? ''
    return r.fulfill({ json: bearer.includes(OWNER_TOKEN) ? CHAMBERS : [CHAMBERS[0]] })
  })

  await page.route('**/api/chambers/cham-a/messages', (r) => r.fulfill({ json: [MESSAGE] }))
  await page.route('**/api/chambers/cham-b/messages', (r) => r.fulfill({ json: [] }))

  // Released when the client sends, so the SSE event below is genuinely a live
  // delivery into an open conversation rather than seeded history.
  let releaseStream = () => {}
  const sendHappened = new Promise<void>((resolve) => {
    releaseStream = resolve
  })

  await page.route('**/api/chambers/cham-a/send', (r) => {
    mock.sent.push(JSON.parse(r.request().postData() ?? 'null'))
    releaseStream()
    return r.fulfill({ json: { ok: true, id: SENT_ID } })
  })

  await page.route('**/api/tokens', (r) => {
    if (r.request().method() === 'POST') {
      mock.created.push(JSON.parse(r.request().postData() ?? 'null'))
      return r.fulfill({ json: { ok: true, token: 'ff'.repeat(16) } })
    }
    return r.fulfill({ json: { invites: opts.invites ?? [] } })
  })

  // The chamber the owner acts on. `running` flips once `start` is posted, so
  // the pill genuinely follows the server rather than the click.
  let running = opts.running ?? false
  await page.route('**/api/chambers/cham-a/status', (r) =>
    r.fulfill({
      json: {
        running,
        agent_running: running,
        session: 7,
        agent: 'opencode',
        log_tail: 'session 7 started',
        daily_digests: [{ date: '2026-08-15', total_sessions: 2, failed_sessions: 0, latest_session: 7 }],
        next_wake: running ? 'in 2 h' : null,
        notes_html: '<p>agent notes</p>',
        plan_html: '<p>the plan</p>',
        has_config: true,
        settings_rows: [{ key: 'agent', value: '"opencode"', kind: 'scalar' }],
        task: null,
        session_summary: null,
        completed: false,
        completion_summary: null,
      },
    }),
  )
  await page.route('**/api/chambers/cham-a/todos', (r) => r.fulfill({ json: [] }))
  await page.route('**/api/chambers/cham-a/start', (r) => {
    mock.actions.push('cham-a/start')
    running = true
    return r.fulfill({ json: { ok: true, message: 'Started' } })
  })
  await page.route('**/api/tokens/*/revoke', (r) => {
    mock.revoked.push(decodeURIComponent(r.request().url().split('/api/tokens/')[1].split('/')[0]))
    return r.fulfill({ json: { ok: true } })
  })

  let streams = 0
  await page.route('**/api/events', async (r) => {
    streams += 1
    if (streams === 1 && opts.echoSends) {
      await sendHappened
      const echo = {
        id: SENT_ID,
        chamber_id: 'cham-a',
        // Server's word on who spoke — the client asked to be "Alice".
        from: SERVER_STAMPED_SENDER,
        subject: '',
        body: 'continue',
        timestamp: '2026-08-15T10:05:00',
        is_question: false,
      }
      // A finite stream: one event, then EOF, which is also what makes the
      // reconnect path below observable.
      return r.fulfill({
        status: 200,
        headers: { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' },
        body: `event: message\ndata: ${JSON.stringify(echo)}\n\n`,
      })
    }
    if (streams > 1 && opts.revokeAfterFirstStream) {
      return r.fulfill({ status: 401, body: '' })
    }
    // Otherwise hold it open like a real connection would be.
    await new Promise((resolve) => setTimeout(resolve, 30_000))
  })

  return mock
}

test('invite link → scoped project → markdown thread → send', async ({ page }) => {
  const hub = await mockHub(page)

  await page.goto(`/#invite=${TOKEN}`)

  // No form, no account: the link itself is the sign-in, and it takes the token
  // out of the address bar on the way through. A link scoped to one chamber
  // lands in that conversation, not in a list of one.
  await expect(page.getByRole('heading', { name: /autoresearch/ })).toBeVisible()
  expect(page.url()).not.toContain(TOKEN)

  // Markdown and math are rendered client-side for hub messages.
  await expect(page.locator('.message-body strong')).toHaveText('done')
  await expect(page.locator('.message-body .katex').first()).toBeVisible()
  // History carries the server's sender, and that is what the thread shows.
  await expect(page.locator('.sender-label').first()).toHaveText('autoresearch-agent')

  // Scope is exactly what the hub handed this token — the other chamber exists
  // on the server and must not be listed on the way back out.
  await page.getByRole('button', { name: 'Back' }).click()
  await expect(page.locator('.stream-list li')).toHaveCount(1)
  await expect(page.getByRole('button', { name: /private-lab/ })).toHaveCount(0)

  await page.getByRole('button', { name: /autoresearch/ }).click()
  await page.getByRole('textbox').fill('continue')
  await page.keyboard.press('Enter')
  await expect.poll(() => hub.sent).toEqual([{ body: 'continue' }])
})

test('a message pushed over SSE appears without a reload, as the server named it', async ({
  page,
}) => {
  const hub = await mockHub(page, { echoSends: true })
  await page.goto(`/#invite=${TOKEN}`)
  await expect(page.locator('.message-body strong')).toHaveText('done')

  await page.getByRole('textbox').fill('continue')
  await page.keyboard.press('Enter')
  await expect.poll(() => hub.sent).toEqual([{ body: 'continue' }])

  // Delivered by the event stream into the open conversation — no navigation,
  // no re-fetch.
  const echoed = page.locator('.msg-row.msg-other', { hasText: 'continue' })
  await expect(echoed).toBeVisible()
  // The client sent no sender at all; the hub stamped its own, and the thread
  // shows the hub's.
  await expect(echoed.locator('.sender-label')).toHaveText(SERVER_STAMPED_SENDER)
  expect(await echoed.locator('.sender-label').textContent()).not.toBe('Alice')
})

test('a token revoked mid-session ends up back on login with a reason', async ({ page }) => {
  await mockHub(page, { echoSends: true, revokeAfterFirstStream: true })
  await page.goto(`/#invite=${TOKEN}`)
  await expect(page.locator('.message-body strong')).toHaveText('done')

  await page.getByRole('textbox').fill('continue')
  await page.keyboard.press('Enter')

  // The stream ends, the app reconnects — and the hub now refuses the token.
  // Staying signed in on cached messages would be the wrong answer.
  await expect(page.getByRole('alert')).toContainText(/no longer valid|sign in again/i)
  await expect(page.getByRole('button', { name: /sign in/i })).toBeVisible()
  // Cached messages are not a session: the conversation goes with the token.
  await expect(page.getByRole('heading', { name: /autoresearch/ })).toHaveCount(0)
  await expect(page.locator('.message-body')).toHaveCount(0)
})

test('a revoked invite link shows login with a reason', async ({ page }) => {
  await mockHub(page, { whoamiStatus: 401 })
  await page.goto(`/#invite=${'cd'.repeat(16)}`)
  await expect(page.getByRole('alert')).toContainText(/no longer valid/i)
  await expect(page.getByRole('button', { name: /sign in/i })).toBeVisible()
})

test('a malformed invite fragment is stripped and explained', async ({ page }) => {
  await mockHub(page)
  await page.goto('/#invite=not-a-real-token')
  await expect(page.getByRole('alert')).toContainText(/not valid/i)
  expect(page.url()).not.toContain('invite=')
  await expect(page.getByRole('button', { name: /sign in/i })).toBeVisible()
})

test('an owner mints a chamber-scoped invite link from the conversation header', async ({
  page,
}) => {
  const hub = await mockHub(page, { role: 'owner' })
  await page.goto(`/#invite=${OWNER_TOKEN}`)

  // The owner sees the whole index; the invite below is scoped to one chamber.
  await expect(page.locator('.stream-list li')).toHaveCount(2)
  await page.getByRole('button', { name: /autoresearch/ }).click()
  await page.getByRole('button', { name: 'Invite' }).click()

  await expect(page.getByRole('heading', { name: 'Invite to autoresearch' })).toBeVisible()
  await page.getByRole('button', { name: 'Copy invite link' }).click()

  await expect(page.getByLabel('Invite link')).toHaveValue(
    `${new URL(page.url()).origin}/#invite=${'ff'.repeat(16)}`,
  )
  // Named by default and scoped to exactly this chamber — never the index.
  expect(hub.created).toEqual([{ name: 'guest-1', chambers: ['cham-a'] }])
  // A token is a credential: it may not appear in the address bar either.
  expect(page.url()).not.toContain('ff'.repeat(16))
})

test('an owner launches a chamber from Controls and sees it working', async ({ page }) => {
  const hub = await mockHub(page, { role: 'owner' })
  await page.goto(`/#invite=${OWNER_TOKEN}`)
  await page.getByRole('button', { name: /autoresearch/ }).click()
  await page.getByRole('button', { name: 'Chamber controls' }).click()

  await expect(page.getByText('Stopped')).toBeVisible()
  await page.getByRole('button', { name: 'Launch' }).click()

  await expect.poll(() => hub.actions).toEqual(['cham-a/start'])
  // The pill moves because the refetched status says so, not because we clicked.
  await expect(page.getByText('Working')).toBeVisible()

  // The detail sheets read the same status payload. Each opens over the
  // controls list and is closed again, the way the stack is meant to be used.
  await page.getByRole('button', { name: 'Plan' }).click()
  const plan = page.locator('.sheet[role="dialog"]').last()
  await expect(plan).toContainText('the plan')
  await plan.getByRole('button', { name: 'Close' }).click()

  await page.getByRole('button', { name: 'Log' }).click()
  // The session number heads the log now, above the raw tail.
  await expect(page.getByText('Session #7')).toBeVisible()
  await expect(page.getByRole('log')).toContainText('session 7 started')
})

test('an owner sees the owner-only affordances on the projects list and in Settings', async ({
  page,
}) => {
  await mockHub(page, { role: 'owner' })
  await page.goto(`/#invite=${OWNER_TOKEN}`)

  // Deliberately the same three locators the guest test below asserts are
  // absent: a negative only means "hidden from guests" if the identical query
  // finds the control for an owner.
  await expect(page.getByRole('button', { name: 'New chamber' })).toBeVisible()
  await page.getByRole('button', { name: /settings/i }).click()
  await expect(page.getByRole('checkbox', { name: 'Show completed & archived' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Refresh chambers' })).toBeVisible()
})

test('an invited user sees no owner controls anywhere', async ({ page }) => {
  await mockHub(page, { role: 'invite' })
  await page.goto(`/#invite=${TOKEN}`)

  // The invite lands in its one conversation. Header: chat only.
  await expect(page.getByRole('heading', { name: /autoresearch/ })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Invite' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Chamber controls' })).toHaveCount(0)

  // Projects list, one step back: no way to create a chamber, and Settings
  // offers signing out and nothing an owner would use.
  await page.getByRole('button', { name: 'Back' }).click()
  await expect(page.getByRole('button', { name: 'New chamber' })).toHaveCount(0)
  await page.getByRole('button', { name: /settings/i }).click()
  await expect(page.getByRole('button', { name: /log out/i })).toBeVisible()
  await expect(page.getByRole('checkbox', { name: 'Show completed & archived' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Refresh chambers' })).toHaveCount(0)
})

test('removing someone from People with access revokes their link', async ({ page }) => {
  const hub = await mockHub(page, {
    role: 'owner',
    invites: [
      { name: 'Mei', chambers: ['cham-a'], created_at: '2026-08-15T09:00:00Z', revoked_at: null },
    ],
  })
  await page.goto(`/#invite=${OWNER_TOKEN}`)
  await page.getByRole('button', { name: /autoresearch/ }).click()
  await page.getByRole('button', { name: 'Invite' }).click()

  await expect(page.getByText('Mei')).toBeVisible()
  await page.getByRole('button', { name: 'Remove', exact: true }).click()
  // Destructive and immediate, so it asks first.
  await expect(page.getByText('Remove Mei? Their link stops working immediately.')).toBeVisible()
  await page.getByRole('button', { name: 'Remove Mei' }).click()
  await expect.poll(() => hub.revoked).toEqual(['Mei'])
})
