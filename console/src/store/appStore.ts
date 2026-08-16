import { create } from 'zustand'
import { HubClient } from '../api/hubClient'
import { messageKey, type Chamber, type ChamberMessage, type Credentials } from '../api/types'
import { saveCredentials, clearCredentials } from './auth'
import {
  loadCachedState,
  saveCachedStateDebounced,
  cancelPendingCachedState,
  clearCachedState,
  CACHE_PREFIX,
} from './cache'
import { accountKey } from '../lib/account'
import { resetChamberEvents } from './chamberEvents'

/** Per account: whether the projects list shows the completed and archived
 * folds. A name is reusable and a token is not, so the preference is keyed on
 * the token like every other per-account store. */
const SHOW_COMPLETED_PREFIX = 'agent-console.show-archived.'

export const AUTH_LOGOUT_REASON = 'Your session is no longer valid — please sign in again.'
export const ACCESS_REVOKED_NOTICE = 'You no longer have access to this chamber.'

export type View = { name: 'projects' } | { name: 'conversation'; chamberId: string }
export type Connection = 'live' | 'connecting' | 'offline'
export type HubRole = 'owner' | 'invite'

/** A message the user sent that the thread has not shown back yet. `sending`
 * → in flight; `sent` → the hub took it and minted `serverId`, waiting for
 * that id to arrive through the stream or the next fetch; `failed` → retry is
 * the user's (no idempotency key on the hub, so never automatic). */
export interface OutboxItem {
  clientId: number
  chamberId: string
  body: string
  state: 'sending' | 'sent' | 'failed'
  serverId: string | null
}

function byKey(a: ChamberMessage, b: ChamberMessage): number {
  const ka = messageKey(a)
  const kb = messageKey(b)
  return ka < kb ? -1 : ka > kb ? 1 : 0
}

/** Dedupe by id, order by messageKey. */
function mergeMessages(...lists: ChamberMessage[][]): ChamberMessage[] {
  const byId = new Map<string, ChamberMessage>()
  for (const list of lists) for (const m of list) if (!byId.has(m.id)) byId.set(m.id, m)
  return [...byId.values()].sort(byKey)
}

/** Unread = messages above this chamber's read watermark from anyone but us.
 * No watermark means the chamber was never opened on this device: only what
 * arrived live since (the cached list) counts, which is what a cold boot showed
 * before too. */
export function unreadCount(
  s: Pick<AppState, 'messagesByChamber' | 'lastReadByChamber' | 'selfName'>,
  chamberId: string,
): number {
  const mark = s.lastReadByChamber[chamberId] ?? ''
  let n = 0
  for (const m of s.messagesByChamber[chamberId] ?? []) {
    if (m.sender !== s.selfName && messageKey(m) > mark) n += 1
  }
  return n
}

let nextClientId = 1

export interface AppState {
  creds: Credentials | null
  client: HubClient | null
  view: View
  settingsOpen: boolean
  /** A newer console build is installed and waiting; the UpdateBar offers a
   *  reload. Transient — never cached, never persisted. */
  updateAvailable: boolean
  chambers: Chamber[]
  messagesByChamber: Record<string, ChamberMessage[]>
  /** `messageKey` of the newest message seen when the chamber was last open. Persisted. */
  lastReadByChamber: Record<string, string>
  /** Chambers whose full history has been fetched this connection; cleared on every setChambers. */
  loadedChambers: string[]
  /** The name the hub stamps on this token's messages: what makes a bubble "mine". */
  selfName: string
  hubRole: HubRole | null
  /** Version of the hub serving this console; null until whoami answers. */
  hubVersion: string | null
  showCompletedArchived: boolean
  connection: Connection
  /** Shown on the login screen after an auth-forced logout; cleared on next setCreds. */
  loginReason: string | null
  /** One-line banner after a chamber was pruned from under the user; cleared on navigate. */
  accessNotice: string | null
  /** Unconfirmed sends per chamber. Session-local: never cached, cleared on logout. */
  outboxByChamber: Record<string, OutboxItem[]>
  setCreds(c: Credentials): void
  logout(reason?: string): void
  navigate(v: View): void
  setSettingsOpen(open: boolean): void
  setUpdateAvailable(v: boolean): void
  setChambers(list: Chamber[]): void
  /** Merge fresh liveness into the chambers already on screen. */
  updateChamberStatus(list: Chamber[]): void
  setMessages(chamberId: string, msgs: ChamberMessage[]): void
  applyMessage(m: ChamberMessage): void
  markRead(chamberId: string): void
  /** Forget a chamber we no longer have access to: it leaves the list, its
   *  messages and watermark go, and an open conversation on it returns to the
   *  projects list with a notice. */
  pruneChamber(chamberId: string, notice?: string): void
  setAccessNotice(n: string | null): void
  setShowCompletedArchived(on: boolean): void
  setConnection(c: Connection): void
  setHubRole(role: HubRole | null): void
  setHubVersion(v: string | null): void
  /** Queue a send and return its client id; the caller drives the request. */
  enqueueOutbox(chamberId: string, body: string): number
  /** The hub took it and named it; the bubble now waits for that id. */
  markOutboxSent(chamberId: string, clientId: number, serverId: string): void
  failOutbox(chamberId: string, clientId: number): void
  retryOutbox(chamberId: string, clientId: number): void
  resolveOutbox(chamberId: string, clientId: number): void
}

const initialData = {
  creds: null as Credentials | null,
  client: null as HubClient | null,
  view: { name: 'projects' } as View,
  settingsOpen: false,
  updateAvailable: false,
  chambers: [] as Chamber[],
  messagesByChamber: {} as Record<string, ChamberMessage[]>,
  lastReadByChamber: {} as Record<string, string>,
  loadedChambers: [] as string[],
  selfName: '',
  hubRole: null as HubRole | null,
  hubVersion: null as string | null,
  showCompletedArchived: false,
  connection: 'connecting' as Connection,
  loginReason: null as string | null,
  accessNotice: null as string | null,
  outboxByChamber: {} as Record<string, OutboxItem[]>,
}

/** Shared by failOutbox/retryOutbox: the only difference is the target state. */
function setOutboxState(chamberId: string, clientId: number, next: OutboxItem['state']) {
  return (state: AppState) => ({
    outboxByChamber: {
      ...state.outboxByChamber,
      [chamberId]: (state.outboxByChamber[chamberId] ?? []).map((o) =>
        o.clientId === clientId ? { ...o, state: next } : o,
      ),
    },
  })
}

export function showCompletedKey(creds: Pick<Credentials, 'token'>): string {
  return SHOW_COMPLETED_PREFIX + accountKey(creds)
}

function loadShowCompleted(creds: Pick<Credentials, 'token'>): boolean {
  try {
    return localStorage.getItem(showCompletedKey(creds)) === 'true'
  } catch {
    return false
  }
}

export const useAppStore = create<AppState>()((set, get) => {
  /** Mirror the list, messages and watermarks to the per-account cache so the
   * next boot paints instantly and unread counts survive a reload. */
  const persist = () => {
    const s = get()
    if (s.creds) {
      saveCachedStateDebounced(s.creds, {
        chambers: s.chambers,
        messagesByChamber: s.messagesByChamber,
        lastReadByChamber: s.lastReadByChamber,
      })
    }
  }

  return {
    ...initialData,

    setCreds: (c) => {
      saveCredentials(c)
      // Hydrate from the local cache before any round-trip: the list and recent
      // messages render immediately, and the network refresh merges on top
      // (loadedChambers stays empty so every opened conversation re-fetches).
      const cached = loadCachedState(c)
      set({
        creds: c,
        // The client owns the only 401 path in the app: whatever call sees the
        // revoked token, the app signs out exactly once.
        client: new HubClient({
          token: c.token,
          onAuthFailure: () => get().logout(AUTH_LOGOUT_REASON),
        }),
        selfName: c.name,
        hubRole: c.role,
        view: { name: 'projects' },
        loginReason: null,
        accessNotice: null,
        showCompletedArchived: loadShowCompleted(c),
        ...(cached
          ? {
              chambers: cached.chambers,
              messagesByChamber: cached.messagesByChamber,
              lastReadByChamber: cached.lastReadByChamber,
            }
          : {}),
      })
    },

    logout: (reason) => {
      const creds = get().creds
      // Order matters: a pending write would otherwise land after the clear.
      cancelPendingCachedState()
      if (creds) clearCachedState(creds)
      clearCredentials()
      set({ ...initialData, loginReason: reason ?? null })
    },

    navigate: (v) => set({ view: v, accessNotice: null }),
    setSettingsOpen: (open) => set({ settingsOpen: open }),
    setUpdateAvailable: (v) => set({ updateAvailable: v }),

    setChambers: (list) => {
      // Clearing loadedChambers on every index read is what makes a re-register
      // re-fetch histories over whatever the stream left behind.
      set({ chambers: [...list].sort((a, b) => a.name.localeCompare(b.name)), loadedChambers: [] })
      persist()
    },

    updateChamberStatus: (list) => {
      const byId = new Map(list.map((c) => [c.id, c]))
      set((state) => ({
        chambers: state.chambers.map((c) => {
          const fresh = byId.get(c.id)
          // Only liveness: a status refresh must not reorder or replace rows.
          return fresh
            ? {
                ...c,
                running: fresh.running,
                agentRunning: fresh.agentRunning,
                nextWakeDisplay: fresh.nextWakeDisplay,
                completed: fresh.completed,
                archived: fresh.archived,
                hasOpenQuestion: fresh.hasOpenQuestion,
              }
            : c
        }),
      }))
    },

    /** The mailbox fetch is the whole history: it replaces what we had, except
     * live messages newer than anything fetched (they raced the fetch). */
    setMessages: (chamberId, msgs) => {
      set((state) => {
        const newest = msgs.length ? messageKey(msgs[msgs.length - 1]) : ''
        const raced = (state.messagesByChamber[chamberId] ?? []).filter(
          (m) => messageKey(m) > newest,
        )
        return {
          messagesByChamber: {
            ...state.messagesByChamber,
            [chamberId]: mergeMessages(msgs, raced),
          },
          loadedChambers: state.loadedChambers.includes(chamberId)
            ? state.loadedChambers
            : [...state.loadedChambers, chamberId],
        }
      })
      persist()
    },

    applyMessage: (m) => {
      set((state) => {
        const list = state.messagesByChamber[m.chamberId] ?? []
        const messagesByChamber = list.some((x) => x.id === m.id)
          ? state.messagesByChamber
          : { ...state.messagesByChamber, [m.chamberId]: mergeMessages(list, [m]) }
        // The id the hub minted is the only correlation there is: someone else
        // posting the same text can no longer retire our pending bubble.
        const pending = state.outboxByChamber[m.chamberId]
        const outboxByChamber = pending?.some((o) => o.serverId === m.id)
          ? {
              ...state.outboxByChamber,
              [m.chamberId]: pending.filter((o) => o.serverId !== m.id),
            }
          : state.outboxByChamber
        return { messagesByChamber, outboxByChamber }
      })
      persist()
    },

    markRead: (chamberId) => {
      const msgs = get().messagesByChamber[chamberId] ?? []
      if (msgs.length === 0) return
      const newest = messageKey(msgs[msgs.length - 1])
      // Monotonic: a stale render must never walk the watermark backwards.
      if ((get().lastReadByChamber[chamberId] ?? '') >= newest) return
      set((state) => ({ lastReadByChamber: { ...state.lastReadByChamber, [chamberId]: newest } }))
      persist()
    },

    // Access was revoked: navigating away is not enough, because the chamber
    // stays in the list, stays tappable, and fails again on every tap.
    pruneChamber: (chamberId, notice) => {
      set((state) => {
        const messagesByChamber = { ...state.messagesByChamber }
        const lastReadByChamber = { ...state.lastReadByChamber }
        const outboxByChamber = { ...state.outboxByChamber }
        delete messagesByChamber[chamberId]
        delete lastReadByChamber[chamberId]
        delete outboxByChamber[chamberId]
        return {
          chambers: state.chambers.filter((c) => c.id !== chamberId),
          messagesByChamber,
          lastReadByChamber,
          outboxByChamber,
          loadedChambers: state.loadedChambers.filter((id) => id !== chamberId),
          view:
            state.view.name === 'conversation' && state.view.chamberId === chamberId
              ? { name: 'projects' as const }
              : state.view,
          accessNotice: notice ?? state.accessNotice,
        }
      })
      persist()
    },

    setAccessNotice: (n) => set({ accessNotice: n }),

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
    setHubRole: (role) => set({ hubRole: role }),
    setHubVersion: (v) => set({ hubVersion: v }),

    enqueueOutbox: (chamberId, body) => {
      const clientId = nextClientId
      nextClientId += 1
      set((state) => ({
        outboxByChamber: {
          ...state.outboxByChamber,
          [chamberId]: [
            ...(state.outboxByChamber[chamberId] ?? []),
            { clientId, chamberId, body, state: 'sending' as const, serverId: null },
          ],
        },
      }))
      return clientId
    },

    /** The hub took it and named it. If that message already sits in the
     * thread (the stream beat the response), the bubble is done now. */
    markOutboxSent: (chamberId, clientId, serverId) =>
      set((state) => {
        const already = (state.messagesByChamber[chamberId] ?? []).some((m) => m.id === serverId)
        const items = state.outboxByChamber[chamberId] ?? []
        return {
          outboxByChamber: {
            ...state.outboxByChamber,
            [chamberId]: already
              ? items.filter((o) => o.clientId !== clientId)
              : items.map((o) =>
                  o.clientId === clientId ? { ...o, state: 'sent' as const, serverId } : o,
                ),
          },
        }
      }),

    failOutbox: (chamberId, clientId) => set(setOutboxState(chamberId, clientId, 'failed')),
    retryOutbox: (chamberId, clientId) => set(setOutboxState(chamberId, clientId, 'sending')),

    resolveOutbox: (chamberId, clientId) =>
      set((state) => ({
        outboxByChamber: {
          ...state.outboxByChamber,
          [chamberId]: (state.outboxByChamber[chamberId] ?? []).filter(
            (o) => o.clientId !== clientId,
          ),
        },
      })),
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
  cancelPendingCachedState()
  nextClientId = 1
  try {
    for (const key of Object.keys(localStorage)) {
      if (key.startsWith(CACHE_PREFIX)) localStorage.removeItem(key)
    }
  } catch {
    /* storage unavailable */
  }
  useAppStore.setState({ ...initialData, showCompletedArchived: false })
}
