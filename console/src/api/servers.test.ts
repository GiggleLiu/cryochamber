import { loadServers } from './servers'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

test('loads and normalizes servers.json', async () => {
  const fetchFn = vi.fn(async () =>
    jsonResponse([{ name: 'Chamber Hub', prefix: '', kind: 'hub' }]),
  )
  const servers = await loadServers(fetchFn as unknown as typeof fetch)
  expect(fetchFn).toHaveBeenCalledWith('/servers.json')
  expect(servers).toEqual([{ name: 'Chamber Hub', prefix: '', kind: 'hub', sendTopic: '' }])
})

test('throws on HTTP error', async () => {
  const fetchFn = vi.fn(async () => jsonResponse({}, 500))
  await expect(loadServers(fetchFn as unknown as typeof fetch)).rejects.toThrow('500')
})

test('throws on empty list', async () => {
  const fetchFn = vi.fn(async () => jsonResponse([]))
  await expect(loadServers(fetchFn as unknown as typeof fetch)).rejects.toThrow('empty')
})
