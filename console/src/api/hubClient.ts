import { ApiError, isUnauthorized } from './types'
import { readSse } from './sse'
import { accountKey, fnv1a } from '../lib/account'
import type { InitialState, Message, StreamSub, User } from './types'

/** Stream ids are allocated from ONE map for every account on this hub, keyed
 * by account *and* chamber. The app shows the chambers of every token it
 * remembers in a single list, so two tokens numbering their own chambers from
 * 1 would collide — same number, different chamber, one message cache. */
const IDS_KEY = 'agent-console.hub-ids.v2'

/** The pre-merge, per-account id maps. Only read now, to carry a draft over to
 * the number its chamber was renumbered to. */
const LEGACY_IDS_PREFIX = 'agent-console.hub-ids.'

/** Message ids stay per account: they are local to one token's conversation
 * and never share a list. */
const MSG_IDS_PREFIX = 'agent-console.hub-msgids.'

/** History window size. The store's cache-merge logic keys off this: a fetch
 * that fills the whole window may not reach back to older cached messages. */
export const HISTORY_FETCH_COUNT = 50

/** Thrown by the client itself when a project name is not in its map yet
 * (register() has not run — offline cold boot). Not the hub's 404: only a 404
 * the hub actually answered with means the chamber is gone. */
export class UnresolvedProjectError extends ApiError {
  constructor(name: string) {
    super(404, `unknown project ${name}`)
    this.name = 'UnresolvedProjectError'
  }
}

interface IdMap {
  next: number
  /** `<accountKey>\u0000<chamberId>` -> stream id. */
  byKey: Record<string, number>
}

/** Shape of the per-account maps written before ids went global. */
interface LegacyIdMap {
  next: number
  byChamber: Record<string, number>
}

/** `{ last }` is the highest number handed out so far, which is what makes a
 * fresh id after a same-millisecond arrival still land above its predecessor. */
interface MessageIdMap {
  last: number
  byId: Record<string, number>
}

function load<T>(key: string, empty: T): T {
  try {
    const raw = localStorage.getItem(key)
    if (raw) return JSON.parse(raw) as T
  } catch {
    /* storage unavailable or corrupt: start fresh */
  }
  return empty
}

function save(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    /* quota: ids stay session-local, which only costs a cache miss */
  }
}

/** Numeric stream id for a chamber. The store keys streams, the unread map and
 * the local message cache by number, so the mapping is persisted and ids are
 * handed out from 1 upward on first sight — stable across reloads, and unique
 * across every token this app remembers.
 */
export function numericStreamId(chamberId: string, account: string): number {
  const map = load<IdMap>(IDS_KEY, { next: 1, byKey: {} })
  const key = `${account}\u0000${chamberId}`
  const existing = map.byKey[key]
  if (existing !== undefined) return existing
  const id = map.next
  map.byKey[key] = id
  map.next = id + 1
  save(IDS_KEY, map)
  carryLegacyDraft(account, chamberId, id)
  return id
}

/** Move an unsent draft onto the chamber's new number, once, when this build
 * first renumbers it. An unsent message is the one thing here a user would
 * miss; caches simply refetch. */
function carryLegacyDraft(account: string, chamberId: string, newId: number): void {
  try {
    const legacy = localStorage.getItem(LEGACY_IDS_PREFIX + account)
    if (!legacy) return
    const oldId = (JSON.parse(legacy) as LegacyIdMap).byChamber?.[chamberId]
    if (oldId === undefined || oldId === newId) return
    const from = `agent-console.draft.${account}.${oldId}`
    const draft = localStorage.getItem(from)
    if (draft === null) return
    localStorage.setItem(`agent-console.draft.${account}.${newId}`, draft)
    localStorage.removeItem(from)
  } catch {
    /* storage unavailable: the draft stays where it was */
  }
}




/**
 * Numeric surrogate for a mailbox message id, for a store that sorts and
 * dedupes by number.
 *
 * A hash of the id folded into the timestamp is not good enough: with 997
 * buckets two messages sharing a millisecond collide often (msg-55 and msg-108
 * do), and the store silently drops one of them. So the assignment is persisted
 * instead — an unseen id takes `max(timestamp, last + 1)`, which keeps ordinary
 * arrivals in timestamp order, guarantees distinctness, and stays stable across
 * reloads and client instances because it is written down.
 */
export function numericMessageId(id: string, timestampMs: number, account: string): number {
  const key = MSG_IDS_PREFIX + account
  const map = load<MessageIdMap>(key, { last: 0, byId: {} })
  const existing = map.byId[id]
  if (existing !== undefined) return existing
  const assigned = Math.max(timestampMs, map.last + 1)
  map.byId[id] = assigned
  map.last = assigned
  save(key, map)
  return assigned
}

interface ChamberMessage {
  id: string
  direction: string
  from: string
  subject: string
  body: string
  timestamp: string
  session?: number | null
  is_question: boolean
}

interface Chamber {
  id: string
  name: string
  /** The chamber daemon holds its lock (the chamber is started). */
  running?: boolean
  /** A session is executing right now; implies `running`. */
  agent_running?: boolean
  next_wake_display?: string | null
  completed?: boolean
  archived?: boolean
  has_open_question?: boolean
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

export interface SyncSummary {
  backend: string
  configured: boolean
  installed: boolean
  running: boolean
  target: string
  last_pushed_session: number | null
  log_tail_path: string
}

/** `GET /api/chambers/{id}/status`. The raw `cryo.toml` is deliberately absent
 * from the hub's payload (it can hold an API key); `has_config` plus the masked
 * `settings_rows` are what the UI gets. */
export interface ChamberStatus {
  running: boolean
  agent_running: boolean
  session: number
  agent: string
  log_tail: string
  daily_digests: DailyDigest[]
  next_wake: string | null
  notes_html: string
  plan_html: string
  has_config: boolean
  settings_rows: SettingsRow[]
  task: string | null
  session_summary: string | null
  completed: boolean
  completion_summary: string | null
}

/** Every lifecycle route the hub actually serves. There is no `wake` route. */
export type LifecycleAction = 'start' | 'stop' | 'restart' | 'reset' | 'archive' | 'unarchive'

/** The `{ok, message}` shape every lifecycle and sync action answers with. */
export interface ActionResult {
  ok: boolean
  message: string
}

export interface NewChamberPayload {
  name: string
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

/** A 200 the hub used to say no: `{ok:false, message}` on lifecycle/sync. */
function isRefusal(body: unknown): boolean {
  return !!body && typeof body === 'object' && (body as { ok?: unknown }).ok === false
}

export interface HubClientOptions {
  token: string
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
 * The app's only client: register/getMessages/sendMessage/… over the chamber
 * hub's REST API. Bearer token auth; every mutating call also carries the
 * `X-Cryo-CSRF` header the hub requires.
 */
export class HubClient {
  private readonly token: string
  private readonly onAuthFailure: (() => void) | undefined
  private readonly fetchFn: typeof fetch
  private authFailed = false

  constructor(opts: HubClientOptions) {
    this.token = opts.token
    this.onAuthFailure = opts.onAuthFailure
    // Native window.fetch throws "Illegal invocation" when called as a member
    // (this.fetchFn(...) binds `this` to the client). Bind to undefined so
    // every call is the browser-legal bare invocation.
    this.fetchFn = (opts.fetch ?? fetch).bind(undefined)
  }

  /** stream name -> chamber id, refreshed by every register(). */
  private byName = new Map<string, string>()
  private byStreamId = new Map<number, string>()

  /** Namespace for this token's persisted id maps. */
  private get account(): string {
    return accountKey({ token: this.token })
  }

  authHeaderValue(): string {
    return `Bearer ${this.token}`
  }

  /** Every hub request goes through here: bearer header always, CSRF header on
   * anything that is not a GET, and the one place a 401 is noticed. Nothing
   * may build its own fetch call. */
  private async send(path: string, init: RequestInit = {}): Promise<Response> {
    const headers: Record<string, string> = {
      Authorization: this.authHeaderValue(),
      ...((init.headers as Record<string, string>) ?? {}),
    }
    // The hub rejects state-changing requests without this header.
    if (init.method && init.method !== 'GET') headers['X-Cryo-CSRF'] = '1'
    const res = await this.fetchFn(path, { ...init, headers })
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

  /** The one `/api/events` stream. A 401 on connect takes the same hook. */
  async events(
    onEvent: (event: string, data: string) => void,
    signal: AbortSignal,
  ): Promise<void> {
    try {
      await readSse(
        '/api/events',
        { Authorization: this.authHeaderValue() },
        onEvent,
        signal,
        this.fetchFn,
      )
    } catch (e) {
      if (isUnauthorized(e)) this.noteAuthFailure()
      throw e
    }
  }

  async register(): Promise<InitialState> {
    const chambers = await this.chambers()
    this.byName.clear()
    this.byStreamId.clear()
    const subscriptions: StreamSub[] = chambers.map((c) => {
      const sid = numericStreamId(c.id, this.account)
      this.byName.set(c.name, c.id)
      this.byStreamId.set(sid, c.id)
      return {
        stream_id: sid,
        name: c.name,
        description: '',
        // Absent flags stay undefined (a hub that says nothing about
        // liveness must not paint every chamber as stopped).
        running: typeof c.running === 'boolean' ? c.running : undefined,
        agentRunning: typeof c.agent_running === 'boolean' ? c.agent_running : undefined,
        // Only a started chamber has a real schedule; a stopped one reports
        // whatever was pending when it died, which reads as nonsense.
        nextWake: c.running === false ? null : (c.next_wake_display ?? null),
        completed: c.completed === true,
        archived: c.archived === true,
        hasOpenQuestion: c.has_open_question === true,
      }
    })
    // No server-side unread state on the hub: it is tracked client-side.
    return { subscriptions, unread: [] }
  }

  private async chambers(): Promise<Chamber[]> {
    return this.request<Chamber[]>('/api/chambers')
  }

  /** Liveness only, for the `status` events the stream fires when a chamber
   * wakes or falls asleep: the same index, re-read, without disturbing the
   * projects the store already has. */
  async chamberStatuses(): Promise<
    Array<{
      stream_id: number
      running?: boolean
      agentRunning?: boolean
      nextWake: string | null
      completed: boolean
      archived: boolean
      hasOpenQuestion: boolean
    }>
  > {
    const chambers = await this.chambers()
    // Same rule as register(): absent flags stay undefined — a hub that says
    // nothing about liveness must not flip every project to "stopped".
    return chambers.map((c) => ({
      stream_id: numericStreamId(c.id, this.account),
      running: typeof c.running === 'boolean' ? c.running : undefined,
      agentRunning: typeof c.agent_running === 'boolean' ? c.agent_running : undefined,
      nextWake: c.running === false ? null : (c.next_wake_display ?? null),
      completed: c.completed === true,
      archived: c.archived === true,
      hasOpenQuestion: c.has_open_question === true,
    }))
  }

  chamberIdFor(streamId: number): string | undefined {
    return this.byStreamId.get(streamId)
  }

  /** Stream id for a chamber id — the inverse of `chamberIdFor`, for the paths
   * that start from a hub id (a freshly created chamber) rather than a row. */
  streamIdFor(chamberId: string): number | undefined {
    for (const [streamId, id] of this.byStreamId) {
      if (id === chamberId) return streamId
    }
    return undefined
  }

  private chamberByName(streamName: string): string {
    const id = this.byName.get(streamName)
    // 404 so a chamber that vanished from our scope is handled like any other
    // "gone" resource rather than as a crash — but its own subclass, because
    // the map is also empty before the first register() (offline cold boot),
    // and that says nothing about whether the chamber still exists.
    if (!id) throw new UnresolvedProjectError(streamName)
    return id
  }

  toMessage(m: ChamberMessage, chamberId: string): Message {
    const tsMs = Date.parse(m.timestamp) || 0
    return {
      id: numericMessageId(m.id, tsMs, this.account),
      sender_full_name: m.from,
      sender_email: m.from,
      timestamp: Math.floor(tsMs / 1000),
      content: m.body,
      stream_id: numericStreamId(chamberId, this.account),
      subject: m.subject,
    }
  }

  /** Map an SSE message payload to a store message, or null if the chamber is
   * unknown (e.g. scope changed since register). The hub sends the real mailbox
   * id, which is what makes a live event and the same message re-fetched later
   * collapse to one numeric id. Older hubs omit it; then the key is synthesized
   * deterministically, so at least a redelivered event still dedupes. */
  toChamberEventMessage(m: {
    id?: string
    chamber_id: string
    from: string
    subject: string
    body: string
    timestamp: string
    is_question: boolean
  }): Message | null {
    if (!Array.from(this.byStreamId.values()).includes(m.chamber_id)) return null
    return this.toMessage(
      {
        id: m.id ?? `${m.chamber_id}:${m.timestamp}:${m.from}:${fnv1a(m.body)}`,
        direction: 'event',
        from: m.from,
        subject: m.subject,
        body: m.body,
        timestamp: m.timestamp,
        is_question: m.is_question,
      },
      m.chamber_id,
    )
  }

  async getMessages(streamName: string): Promise<Message[]> {
    const chamberId = this.chamberByName(streamName)
    const msgs = await this.request<ChamberMessage[]>(
      `/api/chambers/${encodeURIComponent(chamberId)}/messages`,
    )
    // The mailbox returns the full history in one fetch, so there is never an
    // earlier window to ask for.
    return msgs.map((m) => this.toMessage(m, chamberId))
  }

  async sendMessage(streamName: string, content: string): Promise<number> {
    const chamberId = this.chamberByName(streamName)
    await this.request(`/api/chambers/${encodeURIComponent(chamberId)}/send`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ body: content }),
    })
    return Date.now()
  }

  async markStreamRead(_streamId: number): Promise<void> {
    // Unread state is client-local on hub; nothing to sync.
  }

  async getOwnUser(): Promise<{ user_id: number }> {
    // Hub identities are names, not numeric ids; 0 never matches a mention's
    // data-user-id, so own-mention highlighting simply stays off.
    return { user_id: 0 }
  }

  async getUsers(): Promise<User[]> {
    return [] // mention autocomplete falls back to senders seen in messages
  }

  async uploadFile(file: File, streamName?: string): Promise<string> {
    if (!streamName) throw new ApiError(400, 'upload needs a project')
    const chamberId = this.chamberByName(streamName)
    const form = new FormData()
    form.append('file', file)
    // No manual Content-Type: the browser must set the multipart boundary.
    const body = await this.request<{ name: string; markdown: string }>(
      `/api/chambers/${encodeURIComponent(chamberId)}/uploads`,
      { method: 'POST', body: form },
    )
    const match = /\(([^)]+)\)$/.exec(body.markdown)
    return match ? match[1] : `/api/chambers/${chamberId}/files/${body.name}`
  }

  /** Owner-only chamber detail. Every id is encoded: a chamber id can carry a
   * path separator, and an unencoded one would address a different route. */
  async chamberStatus(chamberId: string): Promise<ChamberStatus> {
    return this.request<ChamberStatus>(`/api/chambers/${encodeURIComponent(chamberId)}/status`)
  }

  async chamberTodos(chamberId: string): Promise<TodoItem[]> {
    return this.request<TodoItem[]>(`/api/chambers/${encodeURIComponent(chamberId)}/todos`)
  }

  async chamberSync(chamberId: string): Promise<SyncSummary[]> {
    return this.request<SyncSummary[]>(`/api/chambers/${encodeURIComponent(chamberId)}/sync`)
  }

  async syncAction(
    chamberId: string,
    backend: string,
    verb: 'start' | 'stop',
  ): Promise<ActionResult> {
    return this.request<ActionResult>(
      `/api/chambers/${encodeURIComponent(chamberId)}/sync/${encodeURIComponent(backend)}/${verb}`,
      { method: 'POST' },
    )
  }

  /** The hub answers 200 with `{ok:false, message}` for a refused action;
   * `request` raises that as an `ApiError` carrying `message`, so a refusal and
   * a transport failure reach the caller's catch by the same door. */
  async lifecycle(chamberId: string, action: LifecycleAction): Promise<ActionResult> {
    return this.request<ActionResult>(`/api/chambers/${encodeURIComponent(chamberId)}/${action}`, {
      method: 'POST',
    })
  }

  /** 201 → the new chamber id. A rejected name answers 400 with `{error}`,
   * which `request` already turns into that sentence. */
  async createChamber(payload: NewChamberPayload): Promise<{ id: string }> {
    const body = await this.request<{ id?: string }>('/api/chambers/new', {
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
    return { id: body.id }
  }

  /** Re-scan the workspace. The hub also emits an `index` SSE event, which is
   * what makes the app re-register; the returned list is not needed here. */
  async refreshIndex(): Promise<void> {
    await this.request('/api/chambers/refresh', { method: 'POST' })
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
