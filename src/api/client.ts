import type { Credentials, InitialState, StreamSub, ZulipEvent, ZulipMessage, ZulipUser } from './types'

export class ZulipApiError extends Error {
  constructor(
    message: string,
    readonly httpStatus: number,
    readonly code?: string,
  ) {
    super(message)
    this.name = 'ZulipApiError'
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
async function parseOrThrow(res: Response): Promise<any> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let body: any = null
  try {
    body = await res.json()
  } catch {
    // non-JSON body (proxy error page etc.)
  }
  if (!res.ok || body?.result === 'error') {
    throw new ZulipApiError(body?.msg ?? `HTTP ${res.status}`, res.status, body?.code)
  }
  return body
}

export function isAuthError(e: unknown): boolean {
  return e instanceof ZulipApiError && e.httpStatus === 401
}

const FORM = { 'Content-Type': 'application/x-www-form-urlencoded' }

/** History window size. The store's cache-merge logic keys off this: a fetch
 * that fills the whole window may not reach back to older cached messages. */
export const HISTORY_FETCH_COUNT = 50

export class ZulipClient {
  constructor(
    private creds: Credentials,
    fetchFn: typeof fetch = fetch,
  ) {
    // Native window.fetch throws "Illegal invocation" when called as a
    // member (this.fetchFn(...) binds `this` to the client). Bind to
    // undefined so every call is the browser-legal bare invocation.
    this.fetchFn = fetchFn.bind(undefined)
  }
  private fetchFn: typeof fetch

  static async fetchApiKey(
    prefix: string,
    email: string,
    password: string,
    fetchFn: typeof fetch = fetch,
  ): Promise<string> {
    const res = await fetchFn(`${prefix}/api/v1/fetch_api_key`, {
      method: 'POST',
      headers: FORM,
      body: new URLSearchParams({ username: email, password }),
    })
    const body = await parseOrThrow(res)
    return body.api_key as string
  }

  private authHeader(): string {
    return 'Basic ' + btoa(`${this.creds.email}:${this.creds.apiKey}`)
  }

  /**
   * Exposes the Basic auth header for out-of-band fetches that bypass the
   * client (e.g. authenticated user_uploads image/link loads).
   */
  authHeaderValue(): string {
    return this.authHeader()
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  protected async request(path: string, init: RequestInit = {}): Promise<any> {
    const res = await this.fetchFn(`${this.creds.prefix}/api/v1${path}`, {
      ...init,
      headers: { Authorization: this.authHeader(), ...(init.headers ?? {}) },
    })
    return parseOrThrow(res)
  }

  async getMessages(
    streamName: string,
    anchor: number | 'newest',
    numBefore = HISTORY_FETCH_COUNT,
  ): Promise<ZulipMessage[]> {
    const params = new URLSearchParams({
      anchor: String(anchor),
      num_before: String(numBefore),
      num_after: '0',
      narrow: JSON.stringify([{ operator: 'stream', operand: streamName }]),
      apply_markdown: 'true',
    })
    const body = await this.request(`/messages?${params}`)
    return body.messages as ZulipMessage[]
  }

  async sendMessage(streamName: string, content: string): Promise<number> {
    const body = await this.request('/messages', {
      method: 'POST',
      headers: FORM,
      body: new URLSearchParams({
        type: 'stream',
        to: streamName,
        topic: this.creds.sendTopic,
        content,
      }),
    })
    return body.id as number
  }

  async markStreamRead(streamId: number): Promise<void> {
    await this.request('/mark_stream_as_read', {
      method: 'POST',
      headers: FORM,
      body: new URLSearchParams({ stream_id: String(streamId) }),
    })
  }

  async getOwnUser(): Promise<{ user_id: number }> {
    const body = await this.request('/users/me')
    return { user_id: body.user_id as number }
  }

  async getUsers(): Promise<ZulipUser[]> {
    const body = await this.request('/users')
    const members = (body.members ?? []) as Array<{
      user_id?: number
      full_name?: string
      email?: string
      is_bot?: boolean
      is_active?: boolean
    }>
    return members
      .filter((m) => m.is_active !== false)
      .map((m) => ({
        user_id: m.user_id as number,
        full_name: m.full_name ?? '',
        email: m.email ?? '',
        is_bot: m.is_bot ?? false,
      }))
  }

  /** `_streamName` is ignored — Zulip uploads are realm-wide. It exists so the
   * composer can call one signature against either client (HubClient needs the
   * chamber the file belongs to). */
  async uploadFile(file: File, _streamName?: string): Promise<string> {
    const form = new FormData()
    form.append('file', file)
    // No manual Content-Type: the browser must set the multipart boundary.
    // request() only injects the Authorization header, so leave headers out.
    const body = await this.request('/user_uploads', { method: 'POST', body: form })
    // Server version differences: modern Zulip returns `uri`, older ones `url`.
    return (body.uri ?? body.url) as string
  }

  async register(): Promise<InitialState> {
    const body = await this.request('/register', {
      method: 'POST',
      headers: FORM,
      body: new URLSearchParams({
        event_types: JSON.stringify(['message', 'subscription', 'update_message_flags']),
        apply_markdown: 'true',
        client_gravatar: 'true',
      }),
    })
    return {
      queueId: body.queue_id as string,
      lastEventId: body.last_event_id as number,
      subscriptions: ((body.subscriptions ?? []) as StreamSub[]).map((s) => ({
        stream_id: s.stream_id,
        name: s.name,
        description: s.description,
      })),
      unread: body.unread_msgs?.streams ?? [],
    }
  }

  async pollEvents(
    queueId: string,
    lastEventId: number,
    signal?: AbortSignal,
  ): Promise<ZulipEvent[]> {
    const params = new URLSearchParams({
      queue_id: queueId,
      last_event_id: String(lastEventId),
    })
    const res = await this.fetchFn(`${this.creds.prefix}/api/v1/events?${params}`, {
      headers: { Authorization: this.authHeader() },
      signal,
    })
    const body = await parseOrThrow(res)
    return body.events as ZulipEvent[]
  }
}

export { FORM }
