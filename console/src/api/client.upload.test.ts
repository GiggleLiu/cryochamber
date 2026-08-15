import { ZulipClient, ZulipApiError } from './client'
import type { Credentials } from './types'

const creds: Credentials = { prefix: '/zulip/qec', email: 'a@b.c', apiKey: 'k', sendTopic: '' }

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function file(name = 'report.pdf'): File {
  return new File(['pdf-bytes'], name, { type: 'application/pdf' })
}

test('uploadFile posts a FormData body with auth and returns the uri', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse({ result: 'success', uri: '/user_uploads/2/ab/report.pdf' }),
  )
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const f = file()
  const uri = await client.uploadFile(f)
  expect(uri).toBe('/user_uploads/2/ab/report.pdf')
  const [url, init] = fetchFn.mock.calls[0] as unknown as [string, RequestInit]
  expect(url).toBe('/zulip/qec/api/v1/user_uploads')
  expect(init.method).toBe('POST')
  expect(init.body).toBeInstanceOf(FormData)
  expect((init.body as FormData).get('file')).toBe(f)
  // No manual Content-Type: the browser must set the multipart boundary.
  const headers = init.headers as Record<string, string>
  expect(headers.Authorization).toBe('Basic ' + btoa('a@b.c:k'))
  expect(headers['Content-Type']).toBeUndefined()
})

test('uploadFile falls back to body.url when uri is absent', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse({ result: 'success', url: '/user_uploads/9/cc/x.txt' }),
  )
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  expect(await client.uploadFile(file('x.txt'))).toBe('/user_uploads/9/cc/x.txt')
})

test('uploadFile maps Zulip errors to ZulipApiError', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse(
      { result: 'error', msg: 'File too large', code: 'REQUEST_VALIDATION_ERROR' },
      400,
    ),
  )
  const client = new ZulipClient(creds, fetchFn as unknown as typeof fetch)
  const err = await client.uploadFile(file('big.pdf')).catch((e) => e)
  expect(err).toBeInstanceOf(ZulipApiError)
  expect((err as ZulipApiError).httpStatus).toBe(400)
  expect((err as ZulipApiError).message).toBe('File too large')
})
