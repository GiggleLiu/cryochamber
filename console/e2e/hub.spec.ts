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
const HUB_SERVERS = [{ name: 'Chamber Hub', prefix: '', kind: 'hub', sendTopic: '' }]

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

/** What the hub stamps on the message the client sends below. Deliberately
 * different from the `from` the client puts in the POST body: the thread must
 * show the server's word on who spoke, never the client's claim. */
const SERVER_STAMPED_SENDER = 'alice (invite)'

interface HubMock {
  /** Bodies POSTed to /send, in order. */
  sent: unknown[]
  /** Bodies POSTed to /api/tokens, in order. */
  created: unknown[]
  /** How many times the chamber index was fetched (i.e. registers). */
  registers: number
}

interface HubOptions {
  role?: 'owner' | 'invite'
  whoamiStatus?: number
  /** Deliver one SSE message (then EOF) once the client has sent something. */
  echoSends?: boolean
  /** After the first SSE stream ends, answer the next one with 401 — the token
   *  being revoked while the session is live. */
  revokeAfterFirstStream?: boolean
}

async function mockHub(page: Page, opts: HubOptions = {}): Promise<HubMock> {
  const mock: HubMock = { sent: [], created: [], registers: 0 }

  await page.route('**/servers.json', (r) => r.fulfill({ json: HUB_SERVERS }))

  await page.route('**/api/whoami', (r) =>
    opts.whoamiStatus
      ? r.fulfill({ status: opts.whoamiStatus, body: '' })
      : r.fulfill({ json: { role: opts.role ?? 'invite', name: 'Alice' } }),
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
    return r.fulfill({ json: { ok: true } })
  })

  await page.route('**/api/tokens', (r) => {
    if (r.request().method() === 'POST') {
      mock.created.push(JSON.parse(r.request().postData() ?? 'null'))
      return r.fulfill({ json: { ok: true, token: 'ff'.repeat(16) } })
    }
    return r.fulfill({ json: { invites: [] } })
  })

  let streams = 0
  await page.route('**/api/events', async (r) => {
    streams += 1
    if (streams === 1 && opts.echoSends) {
      await sendHappened
      const echo = {
        id: 'msg-2',
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
  // out of the address bar on the way through.
  await expect(page.getByRole('button', { name: /autoresearch/ })).toBeVisible()
  expect(page.url()).not.toContain(TOKEN)

  // Scope is exactly what the hub handed this token — the other chamber exists
  // on the server and must not be listed here.
  await expect(page.locator('.stream-list li')).toHaveCount(1)
  await expect(page.getByRole('button', { name: /private-lab/ })).toHaveCount(0)

  await page.getByRole('button', { name: /autoresearch/ }).click()

  // Markdown and math are rendered client-side for hub messages.
  await expect(page.locator('.message-body strong')).toHaveText('done')
  await expect(page.locator('.message-body .katex').first()).toBeVisible()
  // History carries the server's sender, and that is what the thread shows.
  await expect(page.locator('.sender-label').first()).toHaveText('autoresearch-agent')

  await page.getByRole('textbox').fill('continue')
  await page.keyboard.press('Enter')
  await expect.poll(() => hub.sent).toEqual([{ body: 'continue', from: 'Alice' }])
})

test('a message pushed over SSE appears without a reload, as the server named it', async ({
  page,
}) => {
  const hub = await mockHub(page, { echoSends: true })
  await page.goto(`/#invite=${TOKEN}`)
  await page.getByRole('button', { name: /autoresearch/ }).click()
  await expect(page.locator('.message-body strong')).toHaveText('done')

  await page.getByRole('textbox').fill('continue')
  await page.keyboard.press('Enter')
  await expect.poll(() => hub.sent).toEqual([{ body: 'continue', from: 'Alice' }])

  // Delivered by the event stream into the open conversation — no navigation,
  // no re-fetch.
  const echoed = page.locator('.msg-row.msg-other', { hasText: 'continue' })
  await expect(echoed).toBeVisible()
  // The client claimed to be "Alice" in the POST; the hub stamped its own
  // answer, and the thread shows the hub's.
  await expect(echoed.locator('.sender-label')).toHaveText(SERVER_STAMPED_SENDER)
  expect(await echoed.locator('.sender-label').textContent()).not.toBe('Alice')
})

test('a token revoked mid-session ends up back on login with a reason', async ({ page }) => {
  await mockHub(page, { echoSends: true, revokeAfterFirstStream: true })
  await page.goto(`/#invite=${TOKEN}`)
  await page.getByRole('button', { name: /autoresearch/ }).click()

  await page.getByRole('textbox').fill('continue')
  await page.keyboard.press('Enter')

  // The stream ends, the app reconnects — and the hub now refuses the token.
  // Staying signed in on cached messages would be the wrong answer.
  await expect(page.getByRole('alert')).toContainText(/no longer valid|sign in again/i)
  await expect(page.getByRole('button', { name: /sign in/i })).toBeVisible()
  await expect(page.getByRole('button', { name: /autoresearch/ })).toHaveCount(0)
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

test('an owner mints a chamber-scoped invite from that chamber\'s header', async ({ page }) => {
  const hub = await mockHub(page, { role: 'owner' })
  await page.goto(`/#invite=${OWNER_TOKEN}`)

  await expect(page.locator('.stream-list li')).toHaveCount(2)

  // Sharing lives where the thing being shared is: the conversation itself.
  await page.getByRole('button', { name: /autoresearch/ }).click()
  await page.getByRole('button', { name: 'Invite' }).click()

  await expect(page.getByRole('dialog', { name: 'Invite' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Invite to autoresearch' })).toBeVisible()

  await page.getByLabel('Who is this for?').fill('Bob')
  await page.getByRole('button', { name: 'Copy invite link' }).click()

  await expect(page.getByLabel('Invite link')).toHaveValue(
    `${new URL(page.url()).origin}/#invite=${'ff'.repeat(16)}`,
  )
  // Scoped to exactly the chamber whose header minted it, never the whole index.
  expect(hub.created).toEqual([{ name: 'Bob', chambers: ['cham-a'] }])
})

test('an invited user has no Invite button in the header the owner mints from', async ({
  page,
}) => {
  await mockHub(page, { role: 'invite' })
  await page.goto(`/#invite=${TOKEN}`)
  await page.getByRole('button', { name: /autoresearch/ }).click()

  // The header rendered — this is the same conversation header that carries the
  // owner's Invite button in the test above, so its absence here is the point.
  await expect(page.getByRole('button', { name: 'Back' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Invite' })).toHaveCount(0)
})
