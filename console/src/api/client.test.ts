import { ZulipClient, ZulipApiError } from './client'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('fetchApiKey', () => {
  test('posts form credentials and returns the api key', async () => {
    const fetchFn = vi.fn(async () =>
      jsonResponse({ result: 'success', api_key: 'k123', email: 'a@b.c' }),
    )
    const key = await ZulipClient.fetchApiKey('/zulip/qec', 'a@b.c', 'pw', fetchFn as unknown as typeof fetch)
    expect(key).toBe('k123')
    const [url, init] = fetchFn.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/zulip/qec/api/v1/fetch_api_key')
    expect(init.method).toBe('POST')
    expect(String(init.body)).toBe('username=a%40b.c&password=pw')
  })

  test('maps Zulip error payloads to ZulipApiError', async () => {
    const fetchFn = vi.fn(async () =>
      jsonResponse({ result: 'error', msg: 'Your username or password is incorrect', code: 'AUTHENTICATION_FAILED' }, 403),
    )
    const err = await ZulipClient.fetchApiKey('/zulip/qec', 'a@b.c', 'bad', fetchFn as unknown as typeof fetch).catch((e) => e)
    expect(err).toBeInstanceOf(ZulipApiError)
    expect(err.code).toBe('AUTHENTICATION_FAILED')
    expect(err.httpStatus).toBe(403)
  })

  test('maps non-JSON HTTP failures to ZulipApiError', async () => {
    const fetchFn = vi.fn(async () => new Response('Bad Gateway', { status: 502 }))
    const err = await ZulipClient.fetchApiKey('/zulip/qec', 'a@b.c', 'pw', fetchFn as unknown as typeof fetch).catch((e) => e)
    expect(err).toBeInstanceOf(ZulipApiError)
    expect(err.httpStatus).toBe(502)
  })
})
