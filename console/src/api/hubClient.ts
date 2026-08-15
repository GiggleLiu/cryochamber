import { ApiError } from './errors'
import { accountKey } from '../lib/account'
import type { Credentials, InitialState, Message, StreamSub, User } from './types'

/** Both maps are namespaced per account: hub chamber numbering starts at 1 and
 * would otherwise be shared with — and read by — a different token. */
const IDS_PREFIX = 'agent-console.hub-ids.'
const MSG_IDS_PREFIX = 'agent-console.hub-msgids.'

/** History window size. The store's cache-merge logic keys off this: a fetch
 * that fills the whole window may not reach back to older cached messages. */
export const HISTORY_FETCH_COUNT = 50

/** `code` on a 404 this client raised itself because it could not resolve a
 * project name locally — as opposed to a 404 the hub actually answered with.
 * Only the latter means the chamber is gone. */
export const CLIENT_UNRESOLVED = 'CLIENT_UNRESOLVED'

interface IdMap {
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
 * handed out from 1 upward on first sight — stable across reloads. */
export function numericStreamId(chamberId: string, account: string): number {
  const key = IDS_PREFIX + account
  const map = load<IdMap>(key, { next: 1, byChamber: {} })
  const existing = map.byChamber[chamberId]
  if (existing !== undefined) return existing
  const id = map.next
  map.byChamber[chamberId] = id
  map.next = id + 1
  save(key, map)
  return id
}

function fnv1a(s: string): number {
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i += 1) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  return h >>> 0
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
  agent_running?: boolean
  next_wake_display?: string | null
}

export interface Invite {
  name: string
  chambers: string[]
  created_at: string
  revoked_at: string | null
}

/**
 * The app's only client: register/getMessages/sendMessage/… over the chamber
 * hub's REST API. Bearer token auth; every mutating call also carries the
 * `X-Cryo-CSRF` header the hub requires.
 */
export class HubClient {
  constructor(
    private creds: Credentials,
    fetchFn: typeof fetch = fetch,
  ) {
    // Native window.fetch throws "Illegal invocation" when called as a member
    // (this.fetchFn(...) binds `this` to the client). Bind to undefined so
    // every call is the browser-legal bare invocation.
    this.fetchFn = fetchFn.bind(undefined)
  }
  private fetchFn: typeof fetch
  /** stream name -> chamber id, refreshed by every register(). */
  private byName = new Map<string, string>()
  private byStreamId = new Map<number, string>()

  /** Namespace for this token's persisted id maps. */
  private get account(): string {
    return accountKey(this.creds)
  }

  authHeaderValue(): string {
    return `Bearer ${this.creds.apiKey}`
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private async request(path: string, init: RequestInit = {}): Promise<any> {
    const headers: Record<string, string> = {
      Authorization: this.authHeaderValue(),
      ...((init.headers as Record<string, string>) ?? {}),
    }
    // The hub rejects state-changing requests without this header.
    if (init.method && init.method !== 'GET') headers['X-Cryo-CSRF'] = '1'
    const res = await this.fetchFn(`${this.creds.prefix}${path}`, { ...init, headers })
    if (!res.ok) throw new ApiError(`HTTP ${res.status}`, res.status)
    return res.json()
  }

  async whoami(): Promise<{ role: 'owner' | 'invite'; name?: string }> {
    return this.request('/api/whoami')
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
        agentRunning: c.agent_running,
        nextWake: c.next_wake_display ?? null,
      }
    })
    // No server-side unread state on the hub: it is tracked client-side.
    return { subscriptions, unread: [] }
  }

  private async chambers(): Promise<Chamber[]> {
    return (await this.request('/api/chambers')) as Chamber[]
  }

  /** Liveness only, for the `status` events the stream fires when a chamber
   * wakes or falls asleep: the same index, re-read, without disturbing the
   * projects the store already has. */
  async chamberStatuses(): Promise<
    Array<{ stream_id: number; agentRunning: boolean; nextWake: string | null }>
  > {
    const chambers = await this.chambers()
    return chambers.map((c) => ({
      stream_id: numericStreamId(c.id, this.account),
      agentRunning: c.agent_running === true,
      nextWake: c.next_wake_display ?? null,
    }))
  }

  chamberIdFor(streamId: number): string | undefined {
    return this.byStreamId.get(streamId)
  }

  private chamberByName(streamName: string): string {
    const id = this.byName.get(streamName)
    // 404 so a chamber that vanished from our scope is handled like any other
    // "gone" resource rather than as a crash — but marked, because the map is
    // also empty before the first register() (offline cold boot), and that says
    // nothing about whether the chamber still exists.
    if (!id) throw new ApiError(`unknown project ${streamName}`, 404, CLIENT_UNRESOLVED)
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
    const msgs = (await this.request(
      `/api/chambers/${encodeURIComponent(chamberId)}/messages`,
    )) as ChamberMessage[]
    // The mailbox returns the full history in one fetch, so there is never an
    // earlier window to ask for.
    return msgs.map((m) => this.toMessage(m, chamberId))
  }

  async sendMessage(streamName: string, content: string): Promise<number> {
    const chamberId = this.chamberByName(streamName)
    await this.request(`/api/chambers/${encodeURIComponent(chamberId)}/send`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ body: content, from: this.creds.email }),
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
    if (!streamName) throw new ApiError('upload needs a project', 400)
    const chamberId = this.chamberByName(streamName)
    const form = new FormData()
    form.append('file', file)
    // No manual Content-Type: the browser must set the multipart boundary.
    const body = await this.request(`/api/chambers/${encodeURIComponent(chamberId)}/uploads`, {
      method: 'POST',
      body: form,
    })
    const match = /\(([^)]+)\)$/.exec(body.markdown as string)
    return match ? match[1] : `/api/chambers/${chamberId}/files/${body.name}`
  }

  async listInvites(): Promise<Invite[]> {
    const body = await this.request('/api/tokens')
    return body.invites as Invite[]
  }

  async createInvite(name: string, chambers: string[]): Promise<{ token: string }> {
    return this.request('/api/tokens', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, chambers }),
    })
  }

  async revokeInvite(name: string): Promise<void> {
    await this.request(`/api/tokens/${encodeURIComponent(name)}/revoke`, { method: 'POST' })
  }
}
