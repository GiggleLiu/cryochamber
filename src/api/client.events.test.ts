import { ZulipClient, ZulipApiError } from './client'
import type { Credentials } from './types'

const creds: Credentials = { prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'k', sendTopic: '' }

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

test('register returns normalized initial state', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse({
      result: 'success',
      queue_id: 'q9',
      last_event_id: 5,
      subscriptions: [{ stream_id: 1, name: 'qec', description: 'QEC project', color: '#fff' }],
      unread_msgs: { streams: [{ stream_id: 1, topic: '', unread_message_ids: [7, 8] }] },
    }),
  )
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const init = await client.register()
  expect(init).toEqual({
    queueId: 'q9',
    lastEventId: 5,
    subscriptions: [{ stream_id: 1, name: 'qec', description: 'QEC project' }],
    unread: [{ stream_id: 1, topic: '', unread_message_ids: [7, 8] }],
  })
  const body = new URLSearchParams(String(((fetchFn.mock.calls[0] as unknown as [string, RequestInit])[1]).body))
  expect(JSON.parse(body.get('event_types')!)).toEqual(['message', 'subscription', 'update_message_flags'])
  expect(body.get('apply_markdown')).toBe('true')
})

test('pollEvents returns events and passes queue params', async () => {
  const events = [{ id: 6, type: 'heartbeat' }]
  const fetchFn = vi.fn(async () => jsonResponse({ result: 'success', events }))
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const out = await client.pollEvents('q9', 5)
  expect(out).toEqual(events)
  const url = new URL(String((fetchFn.mock.calls[0] as unknown as [string, RequestInit])[0]), 'http://x')
  expect(url.pathname).toBe('/zulip/qec/api/v1/events')
  expect(url.searchParams.get('queue_id')).toBe('q9')
  expect(url.searchParams.get('last_event_id')).toBe('5')
})

test('pollEvents surfaces BAD_EVENT_QUEUE_ID as a typed error', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse({ result: 'error', msg: 'Bad event queue ID', code: 'BAD_EVENT_QUEUE_ID' }, 400),
  )
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const err = await client.pollEvents('dead', 5).catch((e) => e)
  expect(err).toBeInstanceOf(ZulipApiError)
  expect(err.code).toBe('BAD_EVENT_QUEUE_ID')
})
