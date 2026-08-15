import type { ServerConfig } from './types'

export async function loadServers(fetchFn: typeof fetch = fetch): Promise<ServerConfig[]> {
  const res = await fetchFn('/servers.json')
  if (!res.ok) throw new Error(`servers.json: HTTP ${res.status}`)
  const list = (await res.json()) as ServerConfig[]
  if (!Array.isArray(list) || list.length === 0) {
    throw new Error('servers.json: empty or invalid')
  }
  return list.map((s) => ({ sendTopic: '', ...s }))
}
