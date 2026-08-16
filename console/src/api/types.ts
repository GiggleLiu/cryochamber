/** The whole identity on a hub: the bearer token, the name the hub labels
 * this token's messages with (`whoami.name`), and the role behind it. Origin
 * is implicit — the console is served by the hub it talks to. */
export interface Credentials {
  token: string
  name: string
  role: 'owner' | 'invite'
}

export interface Message {
  id: number
  sender_full_name: string
  sender_email: string
  timestamp: number
  content: string // markdown source, rendered client-side
  stream_id: number
  subject: string
}

export interface User {
  user_id: number
  full_name: string
  email: string
  is_bot: boolean
}

export interface StreamSub {
  stream_id: number
  name: string
  description: string
  /** Whether the chamber daemon is started at all. Absent until the hub says. */
  running?: boolean
  /** Whether a session is executing right now; implies `running`. */
  agentRunning?: boolean
  /** Display form of the next scheduled wake. Only a started chamber has
   *  one — a stopped chamber's schedule is a stale leftover and never shown. */
  nextWake?: string | null
  /** The latest session reported a completed plan. */
  completed?: boolean
  /** Put away by the operator; it cannot run until unarchived. */
  archived?: boolean
  /** The agent asked something and is waiting on a reply. */
  hasOpenQuestion?: boolean
}

export interface UnreadStreamEntry {
  stream_id: number
  topic: string
  unread_message_ids: number[]
}

export interface InitialState {
  subscriptions: StreamSub[]
  unread: UnreadStreamEntry[]
}

export interface MessageEvent {
  id: number
  type: 'message'
  message: Message
}

export interface FlagsEvent {
  id: number
  type: 'update_message_flags'
  flag: string
  op?: 'add' | 'remove'
  messages: number[]
}

export type AppEvent = MessageEvent | FlagsEvent | { id: number; type: string }

export function isMessageEvent(ev: AppEvent): ev is MessageEvent {
  return (
    ev.type === 'message' &&
    'message' in ev &&
    typeof (ev as MessageEvent).message === 'object' &&
    (ev as MessageEvent).message !== null
  )
}

export function isReadFlagsEvent(ev: AppEvent): ev is FlagsEvent {
  return (
    ev.type === 'update_message_flags' &&
    (ev as FlagsEvent).flag === 'read' &&
    'messages' in ev &&
    Array.isArray((ev as FlagsEvent).messages)
  )
}

/** Every failed hub call, whatever the transport said: `status` is the HTTP
 * status (200 when the hub answered `{ok:false}`), `message` is the hub's own
 * sentence when it gave one — and `hubSaid` is how a caller knows which of the
 * two it holds. A synthesized `HTTP 502` is not something to show a user; the
 * hub's own sentence is the most useful thing on the screen. */
export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
    /** True only when the hub's response body carried the words in `message`. */
    public readonly hubSaid: boolean = false,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

/** A revoked or foreign token. The `HubClient` that raised it has already run
 * its `onAuthFailure` hook — the app's one logout path — so callers use this
 * only to skip their own inline error path. */
export function isUnauthorized(e: unknown): boolean {
  return e instanceof ApiError && e.status === 401
}
