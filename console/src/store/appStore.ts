import { create } from 'zustand'
import { HubClient, HISTORY_FETCH_COUNT } from '../api/hubClient'
import {
  isMessageEvent,
  isReadFlagsEvent,
  type AppEvent,
  type Credentials,
  type InitialState,
  type Message,
  type StreamSub,
  type User,
} from '../api/types'
import { saveCredentials, clearCredentials } from './auth'
import { loadCachedState, saveCachedState, clearCachedState, CACHE_PREFIX } from './cache'
import { accountKey } from '../lib/account'
import { resetChamberEvents } from './chamberEvents'

/** Per account — every token numbers its own chambers from 1, so one global
 * key would apply the wrong token's preference: whether the projects list
 * shows the completed and archived folds. */
const SHOW_COMPLETED_PREFIX = 'agent-console.show-archived.'

export const AUTH_LOGOUT_REASON =
  'Your session is no longer valid — please sign in again.'

function dedupeById(msgs: Message[]): Message[] {
  const seen = new Set<number>()
  const out: Message[] = []
  for (const m of msgs) {
    if (!seen.has(m.id)) {
      seen.add(m.id)
      out.push(m)
    }
  }
  out.sort((a, b) => a.id - b.id)
  return out
}

export type View = { name: 'projects' } | { name: 'conversation'; streamId: number }
export type Connection = 'live' | 'connecting' | 'offline'
export type HubRole = 'owner' | 'invite'

/**
 * A message the user has sent that the thread has not shown back yet. Rendered
 * as a pending self-bubble so a send is never silently swallowed.
 *
 * `sending` → the request is in flight. `sent` → the server accepted it, and we
 * are waiting for the echo (SSE event or the next history fetch) that turns it
 * into a real message; it is removed when that arrives, or by a short fallback
 * timer if it never does. `failed` → the request failed and retry is the user's
 * to trigger, never automatic: without a server-side idempotency key a retry
 * that failed *after* the server committed would send the command twice.
 */
export interface OutboxItem {
  /** Negative and monotonically decreasing, so it can key a React list next to
   *  real message ids without ever colliding with one. */
  localId: number
  streamId: number
  content: string
  state: 'sending' | 'sent' | 'failed'
}

/** Drop outbox items the server has echoed back into the thread: same project,
 * identical text. Content matching, because the hub has no client-message-id to
 * correlate on yet; only `sent` items are eligible, so a message still in flight
 * is never mistaken for its own confirmation.
 *
 * Deliberately sender-agnostic: the hub stamps its own sender on what we send
 * (`alice (invite)` for `Alice`), so requiring our own address left every hub
 * bubble pending forever. The cost is that someone else posting the identical
 * text into the same project also retires it — a duplicate bubble disappearing
 * is far cheaper than one that never resolves. */
function reconcileOutbox(
  outboxByStream: Record<number, OutboxItem[]>,
  arrived: Record<number, Message[]>,
): Record<number, OutboxItem[]> {
  let next = outboxByStream
  for (const [key, msgs] of Object.entries(arrived)) {
    const streamId = Number(key)
    const pending = next[streamId]
    if (!pending || pending.length === 0) continue
    const echoed = new Set(msgs.map((m) => m.content.trim()))
    if (echoed.size === 0) continue
    const kept = pending.filter((o) => !(o.state === 'sent' && echoed.has(o.content.trim())))
    if (kept.length !== pending.length) next = { ...next, [streamId]: kept }
  }
  return next
}

let nextLocalId = -1

export interface AppState {
  creds: Credentials | null
  client: HubClient | null
  view: View
  settingsOpen: boolean
  streams: StreamSub[]
  unreadByStream: Record<number, number[]>
  messagesByStream: Record<number, Message[]>
  /** Id of the signed-in user; null until fetched once per session. */
  ownUserId: number | null
  /** Realm members for @-mention autocomplete; null until lazily fetched. */
  users: User[] | null
  /** Streams whose full history has been fetched; cleared on every register so a
   *  re-register (e.g. expired event queue) re-fetches gaps over cached messages. */
  loadedStreams: number[]
  /** Owner preference: show the Completed and Archived groups in the list. */
  showCompletedArchived: boolean
  setShowCompletedArchived(on: boolean): void
  connection: Connection
  /** Shown on the login screen after an auth-forced logout; cleared on next setCreds. */
  loginReason: string | null
  /** Hub role behind the current token; null until whoami answers. Owner-only
   *  UI — the per-chamber Invite sheet — keys off this. */
  hubRole: HubRole | null
  /** Unconfirmed sends per stream. Session-local: never cached, cleared on logout. */
  outboxByStream: Record<number, OutboxItem[]>
  setCreds(c: Credentials): void
  logout(reason?: string): void
  navigate(v: View): void
  setSettingsOpen(open: boolean): void
  applyInitialState(s: InitialState): void
  /** Merge fresh liveness into the projects already on screen. */
  updateStreamStatus(
    list: Array<{
      stream_id: number
      running?: boolean
      agentRunning?: boolean
      nextWake: string | null
      completed: boolean
      archived: boolean
      hasOpenQuestion: boolean
    }>,
  ): void
  setMessages(streamId: number, msgs: Message[]): void
  applyEvents(events: AppEvent[]): void
  clearUnread(streamId: number): void
  /** Forget a project we no longer have access to: it leaves the list, its
   *  messages and unreads go, and an open conversation on it returns to the
   *  projects list. */
  pruneStream(streamId: number): void
  setConnection(c: Connection): void
  setOwnUserId(id: number): void
  setUsers(users: User[]): void
  setHubRole(role: HubRole | null): void
  /** Queue a send and return its local id; the caller drives the request. */
  enqueueOutbox(streamId: number, content: string): number
  resolveOutbox(streamId: number, localId: number): void
  /** The server took it; the bubble now waits for its echo. */
  markOutboxSent(streamId: number, localId: number): void
  failOutbox(streamId: number, localId: number): void
  retryOutbox(streamId: number, localId: number): void
}

/** Shared by failOutbox/retryOutbox: the only difference is the target state. */
function setOutboxState(streamId: number, localId: number, next: OutboxItem['state']) {
  return (state: AppState) => ({
    outboxByStream: {
      ...state.outboxByStream,
      [streamId]: (state.outboxByStream[streamId] ?? []).map((o) =>
        o.localId === localId ? { ...o, state: next } : o,
      ),
    },
  })
}

export function showCompletedKey(creds: Credentials): string {
  return SHOW_COMPLETED_PREFIX + accountKey(creds)
}

function loadShowCompleted(creds: Credentials): boolean {
  try {
    return localStorage.getItem(showCompletedKey(creds)) === 'true'
  } catch {
    return false
  }
}

const initialData = {
  creds: null as Credentials | null,
  client: null as HubClient | null,
  view: { name: 'projects' } as View,
  settingsOpen: false,
  streams: [] as StreamSub[],
  unreadByStream: {} as Record<number, number[]>,
  messagesByStream: {} as Record<number, Message[]>,
  ownUserId: null as number | null,
  users: null as User[] | null,
  loadedStreams: [] as number[],
  showCompletedArchived: false,
  connection: 'connecting' as Connection,
  loginReason: null as string | null,
  hubRole: null as HubRole | null,
  outboxByStream: {} as Record<number, OutboxItem[]>,
}

export const useAppStore = create<AppState>()((set, get) => {
  /** Mirror streams + messages to the per-account local cache so the next
   * boot paints instantly. Called after every mutation of either. */
  const persist = () => {
    const s = get()
    if (s.creds) saveCachedState(s.creds, s.streams, s.messagesByStream)
  }

  return {
  ...initialData,

  setCreds: (c) => {
    saveCredentials(c)
    // Hydrate from the local cache before any network round-trip: the projects
    // list and recent messages render immediately; register + history fetches
    // then merge fresh data on top (loadedStreams stays empty so every opened
    // conversation still re-fetches).
    const cached = loadCachedState(c)
    set({
      creds: c,
      client: new HubClient(c),
      view: { name: 'projects' },
      loginReason: null,
      // The list preference is this account's own, so it is re-read here rather
      // than carried over from whoever was signed in before. The role is left
      // alone: whoami sets it just before this call.
      showCompletedArchived: loadShowCompleted(c),
      ...(cached ? { streams: cached.streams, messagesByStream: cached.messagesByStream } : {}),
    })
  },

  logout: (reason) => {
    const creds = get().creds
    if (creds) clearCachedState(creds)
    clearCredentials()
    set({ ...initialData, loginReason: reason ?? null })
  },

  navigate: (v) => set({ view: v }),
  setSettingsOpen: (open) => set({ settingsOpen: open }),

  applyInitialState: (s) => {
    const unreadByStream: Record<number, number[]> = {}
    for (const entry of s.unread) {
      const list = unreadByStream[entry.stream_id] ?? []
      for (const id of entry.unread_message_ids) {
        if (!list.includes(id)) list.push(id)
      }
      unreadByStream[entry.stream_id] = list
    }
    set({
      streams: [...s.subscriptions].sort((a, b) => a.name.localeCompare(b.name)),
      unreadByStream,
      // Runs on every register, including re-register after an expired queue:
      // clear the loaded marker so open conversations re-fetch gap messages
      // while the cached lists keep rendering instantly.
      loadedStreams: [],
    })
    persist()
  },

  updateStreamStatus: (list) => {
    const byId = new Map(list.map((s) => [s.stream_id, s]))
    set((state) => ({
      streams: state.streams.map((s) => {
        const status = byId.get(s.stream_id)
        return status
          ? {
              ...s,
              // An update that omits a flag leaves what we knew in place.
              running: status.running ?? s.running,
              agentRunning: status.agentRunning ?? s.agentRunning,
              nextWake: status.nextWake,
              completed: status.completed,
              archived: status.archived,
              hasOpenQuestion: status.hasOpenQuestion,
            }
          : s
      }),
    }))
  },

  setMessages: (streamId, msgs) => {
    set((state) => {
      let existing = state.messagesByStream[streamId] ?? []
      // A fetch that fills its whole history window may not reach back to
      // older locally-cached messages — merging would render a silent hole in
      // the middle of the thread. Drop anything older than the window; "Load
      // earlier" re-fetches it contiguously.
      if (msgs.length >= HISTORY_FETCH_COUNT && existing.length > 0) {
        const oldest = Math.min(...msgs.map((m) => m.id))
        existing = existing.filter((m) => m.id >= oldest)
      }
      return {
        messagesByStream: {
          ...state.messagesByStream,
          [streamId]: dedupeById([...existing, ...msgs]),
        },
        loadedStreams: state.loadedStreams.includes(streamId)
          ? state.loadedStreams
          : [...state.loadedStreams, streamId],
      }
    })
    persist()
  },

  applyEvents: (events) => {
    set((state) => {
      const messagesByStream = { ...state.messagesByStream }
      const unreadByStream = { ...state.unreadByStream }
      const self = state.creds?.email
      // Only what this batch delivered, so an outbox item is retired by its own
      // echo rather than by an identical message from last week.
      const arrived: Record<number, Message[]> = {}
      for (const ev of events) {
        if (isMessageEvent(ev)) {
          const m = ev.message
          arrived[m.stream_id] = [...(arrived[m.stream_id] ?? []), m]
          const list = messagesByStream[m.stream_id]
          // Always append message events — even for streams whose history was
          // never fetched — so live messages can never be lost to a later
          // setMessages overwrite. setMessages merges with whatever exists.
          if (!list || !list.some((x) => x.id === m.id)) {
            messagesByStream[m.stream_id] = [...(list ?? []), m]
          }
          if (m.sender_email !== self) {
            const prev = unreadByStream[m.stream_id] ?? []
            if (!prev.includes(m.id)) unreadByStream[m.stream_id] = [...prev, m.id]
          }
        } else if (isReadFlagsEvent(ev) && ev.op === 'add') {
          const read = new Set(ev.messages)
          for (const key of Object.keys(unreadByStream)) {
            const sid = Number(key)
            unreadByStream[sid] = unreadByStream[sid].filter((id) => !read.has(id))
          }
        }
        // 'subscription' and unknown event types are intentionally ignored in v1;
        // stream list refreshes on the next register().
      }
      return {
        messagesByStream,
        unreadByStream,
        outboxByStream: reconcileOutbox(state.outboxByStream, arrived),
      }
    })
    persist()
  },

  clearUnread: (streamId) =>
    set((state) => ({ unreadByStream: { ...state.unreadByStream, [streamId]: [] } })),

  // Access was revoked: navigating away is not enough, because the project
  // stays in the list, stays tappable, and fails again on every tap.
  pruneStream: (streamId) => {
    set((state) => {
      const messagesByStream = { ...state.messagesByStream }
      const unreadByStream = { ...state.unreadByStream }
      delete messagesByStream[streamId]
      delete unreadByStream[streamId]
      return {
        streams: state.streams.filter((s) => s.stream_id !== streamId),
        messagesByStream,
        unreadByStream,
        loadedStreams: state.loadedStreams.filter((id) => id !== streamId),
        view:
          state.view.name === 'conversation' && state.view.streamId === streamId
            ? { name: 'projects' as const }
            : state.view,
      }
    })
    persist()
  },

  setShowCompletedArchived: (on) =>
    set((state) => {
      if (state.creds) {
        try {
          localStorage.setItem(showCompletedKey(state.creds), String(on))
        } catch {
          /* storage unavailable: the choice still applies for this session */
        }
      }
      return { showCompletedArchived: on }
    }),

  setConnection: (c) => set({ connection: c }),
  setOwnUserId: (id) => set({ ownUserId: id }),
  setUsers: (users) => set({ users }),
  setHubRole: (role) => set({ hubRole: role }),

  enqueueOutbox: (streamId, content) => {
    const localId = nextLocalId
    nextLocalId -= 1
    set((state) => ({
      outboxByStream: {
        ...state.outboxByStream,
        [streamId]: [
          ...(state.outboxByStream[streamId] ?? []),
          { localId, streamId, content, state: 'sending' as const },
        ],
      },
    }))
    return localId
  },

  // Resolving drops the pending bubble; by then the confirmed message has
  // arrived through the event stream (or the fallback timer gave up waiting).
  resolveOutbox: (streamId, localId) =>
    set((state) => ({
      outboxByStream: {
        ...state.outboxByStream,
        [streamId]: (state.outboxByStream[streamId] ?? []).filter((o) => o.localId !== localId),
      },
    })),

  markOutboxSent: (streamId, localId) => set(setOutboxState(streamId, localId, 'sent')),
  failOutbox: (streamId, localId) => set(setOutboxState(streamId, localId, 'failed')),
  retryOutbox: (streamId, localId) => set(setOutboxState(streamId, localId, 'sending')),
  }
})

/**
 * The single owner check. Owner-only UI is never rendered for anyone else —
 * not disabled, absent — and a session whose role has not come back from
 * `/api/whoami` yet is not an owner either.
 *
 * This is chrome only. Security is the hub's default-deny classifier; the app
 * never trusts its own flag for anything but what it draws.
 */
export function useIsOwner(): boolean {
  return useAppStore((s) => s.hubRole === 'owner')
}

export function resetAppStore(): void {
  resetChamberEvents()
  try {
    for (const key of Object.keys(localStorage)) {
      if (key.startsWith(CACHE_PREFIX)) localStorage.removeItem(key)
    }
  } catch {
    /* storage unavailable */
  }
  useAppStore.setState({ ...initialData, showCompletedArchived: false })
}
