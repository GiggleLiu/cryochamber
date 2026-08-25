import { ApiError, isUnauthorized, messageKey } from './types'
import { readSse } from './sse'
import type { Chamber, ChamberMessage } from './types'

/** Raw hub index row → `Chamber`. Absent liveness flags stay absent because
 * the hub has not said the chamber is stopped. A stopped chamber's schedule
 * is a stale leftover and never shown. */
export function toChamber(raw: Record<string, unknown>): Chamber {
  const running = typeof raw.running === 'boolean' ? raw.running : undefined
  return {
    id: String(raw.id ?? ''),
    name: String(raw.name ?? raw.id ?? ''),
    running,
    agentRunning: typeof raw.agent_running === 'boolean' ? raw.agent_running : undefined,
    nextWakeDisplay:
      running === true && typeof raw.next_wake_display === 'string'
        ? raw.next_wake_display
        : null,
    completed: raw.completed === true,
    archived: raw.archived === true,
    hasOpenQuestion: raw.has_open_question === true,
  }
}

/** Raw mailbox message (REST or SSE payload) → `ChamberMessage`. */
export function toChamberMessage(
  raw: Record<string, unknown>,
  chamberId: string,
): ChamberMessage {
  return {
    id: String(raw.id ?? ''),
    chamberId,
    direction: raw.direction === 'outbox' ? 'outbox' : 'inbox',
    sender: typeof raw.from === 'string' ? raw.from : '',
    subject: typeof raw.subject === 'string' ? raw.subject : '',
    body: typeof raw.body === 'string' ? raw.body : '',
    timestamp: typeof raw.timestamp === 'string' ? raw.timestamp : '',
    session: typeof raw.session === 'number' ? raw.session : null,
    isQuestion: raw.is_question === true,
  }
}

/** Time order. Mailbox ids are `{inbox|outbox}/{filename}`, so sorting on the
 * id alone interleaves the two directions wrongly — the timestamp leads and
 * the id only breaks ties. */
export function sortByKey(msgs: ChamberMessage[]): ChamberMessage[] {
  return [...msgs].sort((a, b) => {
    const ka = messageKey(a)
    const kb = messageKey(b)
    return ka < kb ? -1 : ka > kb ? 1 : 0
  })
}

export interface Invite {
  name: string
  chambers: string[]
  created_at: string
  revoked_at: string | null
}

export interface DailyDigest {
  date: string
  total_sessions: number
  failed_sessions: number
  latest_session: number
}

export interface SettingsRow {
  key: string
  value: string
  kind: string
}

export interface TodoItem {
  id: number
  text: string
  done: boolean
  claimed: boolean
  at: string
  created: string
}

export interface HostConfig {
  default_agent: string
}

/** `POST /api/chambers/{id}/agent`. */
export interface ChamberAgentUpdate {
  agent: string
  /** The chamber is running, so the daemon is still on the old runner. */
  restart_required: boolean
  /** A `cryo start --agent` override in `timer.json` wins over `cryo.toml`,
   * so a restart alone will not put this runner in charge. */
  override_active: boolean
}

export interface NewChamberResult {
  id: string
  started: boolean
  start_error: string | null
}

/** `GET /api/chambers/{id}/status`. The raw `cryo.toml` is deliberately absent
 * from the hub's payload (it can hold an API key); `has_config` plus the masked
 * `settings_rows` are what the UI gets. */
export interface ChamberStatus {
  running: boolean
  agent_running: boolean
  session: number
  /** What will actually run: a `cryo start --agent` override when one is in
   * force, else `cryo.toml`'s `agent`. */
  agent: string
  /** What `cryo.toml` says — the value the agent dropdown edits. Differs from
   * `agent` only while a CLI override is in force. */
  config_agent: string
  log_tail: string
  daily_digests: DailyDigest[]
  next_wake: string | null
  notes_html: string
  plan_html: string
  /** Raw `plan.md`, which the plan editor writes back. Safe to ship (unlike
   * `cryo.toml`): a plan holds no credentials. */
  plan_content: string
  has_config: boolean
  settings_rows: SettingsRow[]
  task: string | null
  session_summary: string | null
  completed: boolean
  completion_summary: string | null
}

/** Every lifecycle route the hub actually serves. There is no `wake` route. */
export type LifecycleAction = 'start' | 'stop' | 'restart' | 'reset' | 'archive' | 'unarchive'

/** The `{ok, message}` shape every lifecycle action answers with. */
export interface ActionResult {
  ok: boolean
  message: string
}

export interface NewChamberPayload {
  name: string
  start?: boolean
  api_key_provider?: string
  api_key?: string
  model?: string
}

/** The hub's own words for a failure, from either error shape. */
function apiMessage(body: unknown): string | undefined {
  if (!body || typeof body !== 'object') return undefined
  const b = body as { error?: unknown; message?: unknown }
  if (typeof b.error === 'string' && b.error) return b.error
  if (typeof b.message === 'string' && b.message) return b.message
  return undefined
}

/** A 200 the hub used to say no: `{ok:false, message}` on lifecycle actions. */
function isRefusal(body: unknown): boolean {
  return !!body && typeof body === 'object' && (body as { ok?: unknown }).ok === false
}

export interface HubClientOptions {
  token: string
  /** Absolute hub origin (`http://hub.local:8765`), no trailing slash. Empty
   * (the default) keeps today's same-origin relative paths: the browser
   * console is served by the hub it talks to. The app sets it per hub. */
  baseUrl?: string
  /** Runs once per 401 before the ApiError propagates: the app's single
   * logout path. Nothing else in the client interprets 401. */
  onAuthFailure?: () => void
  fetch?: typeof fetch
}

/** `GET /api/whoami`. An owner token answers with its owner name; an invite
 * token also lists the chambers it reaches. */
export interface WhoAmI {
  role: 'owner' | 'invite'
  name?: string
  chambers?: string[]
  hub_version?: string
}

/**
 * The app's only client: listChambers/getMessages/sendMessage/… over the
 * chamber hub's REST API. Bearer token auth; every mutating call also carries
 * the `X-Cryo-CSRF` header the hub requires.
 *
 * Stateless with respect to identity: the hub's own ids are what the store
 * keys on, so there is no name→id map to register, go stale, or disagree with
 * the cache after a reload.
 */
export class HubClient {
  private readonly token: string
  private readonly base: string
  private readonly onAuthFailure: (() => void) | undefined
  private readonly fetchFn: typeof fetch
  private authFailed = false

  constructor(opts: HubClientOptions) {
    this.token = opts.token
    this.base = opts.baseUrl ?? ''
    this.onAuthFailure = opts.onAuthFailure
    // Native window.fetch throws "Illegal invocation" when called as a member
    // (this.fetchFn(...) binds `this` to the client). Bind to undefined so
    // every call is the browser-legal bare invocation.
    this.fetchFn = (opts.fetch ?? fetch).bind(undefined)
  }

  authHeaderValue(): string {
    return `Bearer ${this.token}`
  }

  /** Every hub request goes through here: bearer header always, CSRF header on
   * anything that is not a GET, and the one place a 401 is noticed. Nothing
   * may build its own fetch call. A hub-relative path picks up `base`; an
   * already-absolute URL (an attachment link the hub minted) passes through. */
  private async send(path: string, init: RequestInit = {}): Promise<Response> {
    const target = /^https?:\/\//.test(path) ? path : this.base + path
    const headers: Record<string, string> = {
      Authorization: this.authHeaderValue(),
      ...((init.headers as Record<string, string>) ?? {}),
    }
    // The hub rejects state-changing requests without this header.
    if (init.method && init.method !== 'GET') headers['X-Cryo-CSRF'] = '1'
    const res = await this.fetchFn(target, { ...init, headers })
    if (res.status === 401) this.noteAuthFailure()
    return res
  }

  /** Once per client: a revoked token fails every in-flight call at once and
   * logout must not run for each of them. */
  private noteAuthFailure(): void {
    if (this.authFailed) return
    this.authFailed = true
    this.onAuthFailure?.()
  }

  /** Every JSON call goes through here. Two failure shapes exist on the hub —
   * a non-2xx `{error}` and a 200 `{ok:false, message}` — and both become one
   * `ApiError` carrying the hub's own words, so no caller reads a body twice. */
  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const res = await this.send(path, init)
    const body: unknown = await res.json().catch(() => null)
    const said = apiMessage(body)
    if (!res.ok) throw new ApiError(res.status, said ?? `HTTP ${res.status}`, said !== undefined)
    if (isRefusal(body)) {
      throw new ApiError(res.status, said ?? 'Request refused', said !== undefined)
    }
    return body as T
  }

  async whoami(): Promise<WhoAmI> {
    return this.request<WhoAmI>('/api/whoami')
  }

  /** Authenticated fetch of a chamber attachment (or any hub file URL). */
  async fetchBlob(url: string): Promise<Blob> {
    const res = await this.send(url)
    if (!res.ok) throw new ApiError(res.status, `HTTP ${res.status}`)
    return res.blob()
  }

  /** ConsoleClient-shape alias: the browser client has exactly one hub, so
   * the chamber key adds nothing — it is the chamber id. */
  fetchBlobFor(_chamberKey: string, url: string): Promise<Blob> {
    return this.fetchBlob(url)
  }

  /** The one `/api/events` stream. A 401 on connect takes the same hook. */
  async events(
    onEvent: (event: string, data: string) => void,
    signal: AbortSignal,
  ): Promise<void> {
    try {
      await readSse(this.base + '/api/events', {
        signal,
        headers: { Authorization: this.authHeaderValue() },
        onEvent,
        fetch: this.fetchFn,
      })
    } catch (e) {
      if (isUnauthorized(e)) this.noteAuthFailure()
      throw e
    }
  }

  async listChambers(): Promise<Chamber[]> {
    const raw = await this.request<Record<string, unknown>[]>('/api/chambers')
    return (Array.isArray(raw) ? raw : []).map(toChamber).filter((c) => c.id !== '')
  }

  /** The mailbox returns the whole history in one fetch — there is never an
   * earlier window to ask for. */
  async getMessages(chamberId: string): Promise<ChamberMessage[]> {
    const raw = await this.request<Record<string, unknown>[]>(
      `/api/chambers/${encodeURIComponent(chamberId)}/messages`,
    )
    return sortByKey((Array.isArray(raw) ? raw : []).map((m) => toChamberMessage(m, chamberId)))
  }

  /** SSE `message` payload → store message; null when it names no chamber.
   * The store keys by chamber id, so a payload without one has nowhere to go. */
  toEventMessage(payload: unknown): ChamberMessage | null {
    if (!payload || typeof payload !== 'object') return null
    const raw = payload as Record<string, unknown>
    if (typeof raw.chamber_id !== 'string' || raw.chamber_id === '') return null
    return toChamberMessage(raw, raw.chamber_id)
  }

  /** The hub stamps the sender and answers with the mailbox id it minted;
   * that id is what the outbox waits for. */
  async sendMessage(chamberId: string, body: string): Promise<{ id: string }> {
    const res = await this.request<{ id?: string }>(
      `/api/chambers/${encodeURIComponent(chamberId)}/send`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ body }),
      },
    )
    return { id: typeof res.id === 'string' ? res.id : '' }
  }

  async uploadFile(file: File, chamberId: string): Promise<string> {
    const form = new FormData()
    form.append('file', file)
    // No manual Content-Type: the browser must set the multipart boundary.
    const body = await this.request<{ name?: string; markdown?: string }>(
      `/api/chambers/${encodeURIComponent(chamberId)}/uploads`,
      { method: 'POST', body: form },
    )
    const match = /\(([^)]+)\)$/.exec(body.markdown ?? '')
    return match ? match[1] : `/api/chambers/${chamberId}/files/${body.name ?? ''}`
  }

  /** Owner-only chamber detail. Every id is encoded: a chamber id can carry a
   * path separator, and an unencoded one would address a different route. */
  async chamberStatus(chamberId: string): Promise<ChamberStatus> {
    return this.request<ChamberStatus>(`/api/chambers/${encodeURIComponent(chamberId)}/status`)
  }

  /** Set one chamber's `agent` in its own `cryo.toml`. The daemon reads that
   * file when it starts, so `restart_required` says whether the chamber has to
   * be restarted before the new runner is the one that actually wakes. */
  async setChamberAgent(chamberId: string, agent: string): Promise<ChamberAgentUpdate> {
    const body = await this.request<Partial<ChamberAgentUpdate>>(
      `/api/chambers/${encodeURIComponent(chamberId)}/agent`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ agent }),
      },
    )
    return {
      agent: typeof body.agent === 'string' ? body.agent : agent,
      restart_required: body.restart_required === true,
      override_active: body.override_active === true,
    }
  }

  /** Replace a chamber's `plan.md`. No restart: the agent is told to read the
   * plan at the top of every session, so the next wake sees it. */
  async setChamberPlan(chamberId: string, content: string): Promise<void> {
    await this.request(`/api/chambers/${encodeURIComponent(chamberId)}/plan`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ content }),
    })
  }

  async chamberTodos(chamberId: string): Promise<TodoItem[]> {
    return this.request<TodoItem[]>(`/api/chambers/${encodeURIComponent(chamberId)}/todos`)
  }

  /** The hub answers 200 with `{ok:false, message}` for a refused action;
   * `request` raises that as an `ApiError` carrying `message`, so a refusal and
   * a transport failure reach the caller's catch by the same door. */
  async lifecycle(chamberId: string, action: LifecycleAction): Promise<ActionResult> {
    return this.request<ActionResult>(`/api/chambers/${encodeURIComponent(chamberId)}/${action}`, {
      method: 'POST',
    })
  }

  /** 201 → the new chamber id and launch outcome. A rejected name or failed
   * preflight answers 400 with `{error}`, which `request` turns into that
   * sentence. */
  async createChamber(payload: NewChamberPayload): Promise<NewChamberResult> {
    const body = await this.request<{
      id?: string
      started?: boolean
      start_error?: string | null
    }>('/api/chambers/new', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    })
    // The hub answers 201 with an empty id when the new chamber is missing
    // from its refreshed index (`post_new` falls back to default). A 201 with
    // an empty id is a chamber the caller cannot open.
    if (typeof body.id !== 'string' || body.id === '') {
      throw new ApiError(201, 'Chamber was created but the hub did not report its id')
    }
    return {
      id: body.id,
      started: body.started === true,
      start_error: typeof body.start_error === 'string' ? body.start_error : null,
    }
  }

  /** Re-scan the workspace. The hub also emits an `index` SSE event, which is
   * what makes the app re-register; the returned list is not needed here. */
  async refreshIndex(): Promise<void> {
    await this.request('/api/chambers/refresh', { method: 'POST' })
  }

  async hostConfig(): Promise<HostConfig> {
    return this.request<HostConfig>('/api/config')
  }

  async updateHostConfig(defaultAgent: string): Promise<HostConfig> {
    return this.request<HostConfig>('/api/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ default_agent: defaultAgent }),
    })
  }

  async listInvites(): Promise<Invite[]> {
    const body = await this.request<{ invites: Invite[] }>('/api/tokens')
    return body.invites
  }

  /** Mints an invite. A rejected name (the hub refuses a duplicate among active
   * invites) comes back as a 400, sometimes with words of its own and sometimes
   * bare; `request` gives the caller the hub's text when there is any and
   * `HTTP 400` when there is not, so the sheet can tell the two apart. */
  async createInvite(name: string, chambers: string[]): Promise<{ token: string }> {
    const body = await this.request<{ token?: string }>('/api/tokens', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, chambers }),
    })
    // A 200 without a token would become an `#invite=` link the sheet says
    // it copied and can never show again — fail loudly instead.
    if (typeof body.token !== 'string' || body.token === '') {
      throw new ApiError(200, 'The hub did not return an invite token')
    }
    return { token: body.token }
  }

  async revokeInvite(name: string): Promise<void> {
    await this.request(`/api/tokens/${encodeURIComponent(name)}/revoke`, { method: 'POST' })
  }
}
