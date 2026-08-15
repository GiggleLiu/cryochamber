import { ZulipClient } from './client'
import type { Credentials } from './types'

const creds: Credentials = { prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'k', sendTopic: '' }

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

test('getOwnUser hits /users/me with auth and returns the user id', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse({ result: 'success', user_id: 7, email: 'a@b.c' }),
  )
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const out = await client.getOwnUser()
  expect(out).toEqual({ user_id: 7 })
  const [url, init] = fetchFn.mock.calls[0] as unknown as [string, RequestInit]
  expect(url).toBe('/zulip/qec/api/v1/users/me')
  expect((init.headers as Record<string, string>).Authorization).toBe(
    'Basic ' + btoa('a@b.c:k'),
  )
})

test('getUsers hits /users with auth, maps members, drops inactive ones', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse({
      result: 'success',
      members: [
        { user_id: 1, full_name: 'Alice Doe', email: 'alice@b.c', is_bot: false, is_active: true },
        { user_id: 2, full_name: 'Research Bot', email: 'bot@b.c', is_bot: true },
        { user_id: 3, full_name: 'Gone', email: 'gone@b.c', is_bot: false, is_active: false },
      ],
    }),
  )
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const out = await client.getUsers()
  expect(out).toEqual([
    { user_id: 1, full_name: 'Alice Doe', email: 'alice@b.c', is_bot: false },
    { user_id: 2, full_name: 'Research Bot', email: 'bot@b.c', is_bot: true },
  ])
  const [url, init] = fetchFn.mock.calls[0] as unknown as [string, RequestInit]
  expect(url).toBe('/zulip/qec/api/v1/users')
  expect((init.headers as Record<string, string>).Authorization).toBe(
    'Basic ' + btoa('a@b.c:k'),
  )
})
