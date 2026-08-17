/** The whole identity on a hub: the bearer token, the name the hub labels
 * this token's messages with (`whoami.name`), and the role behind it. Origin
 * is implicit — the console is served by the hub it talks to. */
export interface Credentials {
  token: string
  name: string
  role: 'owner' | 'invite'
}

/** A chamber as the projects list needs it. Absent liveness flags from an
 * older hub are mapped to `false` at the client boundary — see `toChamber` —
 * so views never reason about `undefined`. */
export interface Chamber {
  id: string
  name: string
  /** The chamber daemon holds its lock (the chamber is started). */
  running: boolean
  /** A session is executing right now; implies `running`. */
  agentRunning: boolean
  /** Display form of the next scheduled wake; null for a stopped chamber. */
  nextWakeDisplay: string | null
  completed: boolean
  archived: boolean
  hasOpenQuestion: boolean
}

export interface ChamberMessage {
  /** The hub's mailbox id, `"{inbox|outbox}/{filename}"`. Unique per chamber,
   * NOT time-ordered across directions — order by `messageKey`. */
  id: string
  chamberId: string
  direction: 'inbox' | 'outbox'
  sender: string
  subject: string
  /** Markdown source, rendered client-side. */
  body: string
  /** `%Y-%m-%dT%H:%M:%S`, local time as the hub formats it. */
  timestamp: string
  session: number | null
  isQuestion: boolean
}

/** The sort/watermark key: timestamp first, id as the tie-breaker. String
 * comparison on it is time order. */
export function messageKey(m: Pick<ChamberMessage, 'id' | 'timestamp'>): string {
  return `${m.timestamp} ${m.id}`
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
