import { ApiError } from './types'

/** Minimal SSE reader over fetch streaming — EventSource cannot send an
 * Authorization header, and the token must never ride in a query string. */
export async function readSse(
  url: string,
  headers: Record<string, string>,
  onEvent: (event: string, data: string) => void,
  signal: AbortSignal,
): Promise<void> {
  const res = await fetch(url, { headers, signal })
  if (!res.ok || !res.body) throw new ApiError(res.status, `HTTP ${res.status}`)
  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let eventName = 'message'
  let data: string[] = []
  const dispatch = () => {
    if (data.length > 0) onEvent(eventName, data.join('\n'))
    eventName = 'message'
    data = []
  }
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })
      let nl: number
      while ((nl = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, nl).replace(/\r$/, '')
        buffer = buffer.slice(nl + 1)
        if (line === '') dispatch()
        else if (line.startsWith('event:')) eventName = line.slice(6).trim()
        else if (line.startsWith('data:')) data.push(line.slice(5).trimStart())
        // comments (`:keepalive`) and other fields are ignored
      }
    }
  } finally {
    // Every exit path — stream end, abort, or a throw out of onEvent — must
    // release the connection, or a re-registering loop stacks up open streams.
    void reader.cancel().catch(() => {})
  }
  dispatch()
}
