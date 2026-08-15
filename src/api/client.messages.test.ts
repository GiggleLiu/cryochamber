import { ZulipClient } from './client'
import type { Credentials } from './types'

const creds: Credentials = { prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'k', sendTopic: '' }

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

const msg = {
  id: 7, sender_full_name: 'Agent', sender_email: 'bot@b.c',
  timestamp: 1755100000, content: '<p>hi</p>', stream_id: 1, subject: '',
}

test('getMessages narrows by stream and returns messages', async () => {
  const fetchFn = vi.fn(async () => jsonResponse({ result: 'success', messages: [msg] }))
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const out = await client.getMessages('qec', 'newest')
  expect(out).toEqual([msg])
  const url = new URL(String((fetchFn.mock.calls[0] as unknown as [string, RequestInit])[0]), 'http://x')
  expect(url.pathname).toBe('/zulip/qec/api/v1/messages')
  expect(url.searchParams.get('anchor')).toBe('newest')
  expect(url.searchParams.get('num_before')).toBe('50')
  expect(url.searchParams.get('num_after')).toBe('0')
  expect(JSON.parse(url.searchParams.get('narrow')!)).toEqual([{ operator: 'stream', operand: 'qec' }])
  const init = (fetchFn.mock.calls[0] as unknown as [string, RequestInit])[1]
  expect((init.headers as Record<string, string>).Authorization).toBe('Basic ' + btoa('a@b.c:k'))
})

test('sendMessage posts to the configured sendTopic and returns id', async () => {
  const fetchFn = vi.fn(async () => jsonResponse({ result: 'success', id: 42 }))
  const client = new ZulipClient({ ...creds, sendTopic: 'chat' }, fetchFn as unknown as typeof fetch)
  const id = await client.sendMessage('qec', 'run the scan')
  expect(id).toBe(42)
  const init = (fetchFn.mock.calls[0] as unknown as [string, RequestInit])[1]
  const body = new URLSearchParams(String(init.body))
  expect(body.get('type')).toBe('stream')
  expect(body.get('to')).toBe('qec')
  expect(body.get('topic')).toBe('chat')
  expect(body.get('content')).toBe('run the scan')
})

test('markStreamRead posts the stream id', async () => {
  const fetchFn = vi.fn(async () => jsonResponse({ result: 'success' }))
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  await client.markStreamRead(1)
  expect(String((fetchFn.mock.calls[0] as unknown as [string, RequestInit])[0])).toBe('/zulip/qec/api/v1/mark_stream_as_read')
  expect(new URLSearchParams(String(((fetchFn.mock.calls[0] as unknown as [string, RequestInit])[1]).body)).get('stream_id')).toBe('1')
})

test('invokes fetchFn as a bare call so browser fetch accepts it', async () => {
  // Native window.fetch throws "Illegal invocation" when called as a member
  // (this bound to the client). Mirrors that contract: any this !== undefined
  // is rejected, so a regression here fails even under Node's lenient fetch.
  const fetchFn = vi.fn(function (this: unknown) {
    if (this !== undefined) throw new TypeError('Illegal invocation')
    return Promise.resolve(jsonResponse({ result: 'success', messages: [] }))
  })
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  await client.getMessages('alpha', 'newest')
  expect(fetchFn).toHaveBeenCalled()
})
