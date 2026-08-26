import { create } from 'zustand'
import { HubClient } from '../api/hubClient'
import { HubRouter, type ConsoleClient } from '../api/hubRouter'
import { messageKey, type Chamber, type ChamberMessage, type Credentials } from '../api/types'
import { chamberKey, splitChamberKey } from '../lib/hubKeys'
import type { HubAccount, HubsBackend } from './hubs'
import { saveCredentials, clearCredentials } from './auth'
import {
  loadCachedState,
  saveCachedStateDebounced,
  cancelPendingCachedState,
  clearCachedState,
  cacheKey,
  CACHE_PREFIX,
  type CachedState,
} from './cache'
import { accountKey } from '../lib/account'
import { resetChamberEvents } from './chamberEvents'
import { writeViewHash } from '../lib/hashRoute'

/** Per account: whether the projects list shows the completed and archived
 * folds. A name is reusable and a token is not, so the preference is keyed on
 * the token like every other per-account store. */
const SHOW_COMPLETED_PREFIX = 'agent-console.show-archived.'
const APP_SHOW_COMPLETED_KEY = 'agent-console.app.show-archived'

/** The fields a status refresh is allowed to carry: liveness only. Name and
 * ordering belong to the index read, not to a status event. */
const LIVENESS_FIELDS = [
  'running',
  'agentRunning',
  'nextWakeDisplay',
  'completed',
  'archived',
  'hasOpenQuestion',
] as const satisfies ReadonlyArray<keyof Chamber>

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
  /** Why the last attempt failed, in the hub's own words — `null` when it
   * never failed, or failed with nothing worth showing (a synthesized
   * `HTTP 502` tells the user less than "Failed" already does). */
  error: string | null
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

/** The name that makes a bubble "mine" in this chamber: the hub the chamber
 * lives on names our token, and two hubs can name the same person differently.
 * Browser mode splits to `''`, which no hub ever fills, so the answer is the
 * one `selfName` the session signed in with. */
export function selfNameFor(
  s: Pick<AppState, 'selfName' | 'selfNameByHub'>,
  chamberKey: string,
): string {
  return s.selfNameByHub[splitChamberKey(chamberKey).hubId] ?? s.selfName
}

/** Unread = messages above this chamber's read watermark from anyone but us.
 * No watermark means the chamber was never opened on this device: only what
 * arrived live since (the cached list) counts, which is what a cold boot showed
 * before too. */
export function unreadCount(
  s: Pick<AppState, 'messagesByChamber' | 'lastReadByChamber' | 'selfName' | 'selfNameByHub'>,
  chamberId: string,
): number {
  const mark = s.lastReadByChamber[chamberId] ?? ''
  const me = selfNameFor(s, chamberId)
  let n = 0
  for (const m of s.messagesByChamber[chamberId] ?? []) {
    if (m.sender !== me && messageKey(m) > mark) n += 1
  }
  return n
}

/** Which hub a row belongs to, and the only place that answer is spelled out:
 * `c.hubId ?? splitChamberKey(c.id).hubId`. A row the router stamped says so
 * itself; a browser row (and every row a pre-multi-hub cache holds) says
 * nothing, and its key carries no hub prefix either — both answer `''`.
 * Anything grouping or filtering chambers by hub must come through here rather
 * than read `hubId` directly, which is what keeps the field safely optional. */
export function hubIdOf(c: Chamber): string {
  return c.hubId ?? splitChamberKey(c.id).hubId
}

/** The app is as connected as its best hub: one live hub still shows live
 * chambers, and only every hub being down is an offline app. */
function aggregateConnection(byHub: Record<string, Connection>): Connection {
  const values = Object.values(byHub)
  if (values.includes('live')) return 'live'
  if (values.includes('connecting')) return 'connecting'
  return values.length === 0 ? 'connecting' : 'offline'
}

/** Drop every entry whose chamber key belongs to `hubId`. */
function withoutHub<T>(map: Record<string, T>, hubId: string): Record<string, T> {
  return Object.fromEntries(
    Object.entries(map).filter(([key]) => splitChamberKey(key).hubId !== hubId),
  )
}

/** Keep only the entries whose chamber key belongs to one of `hubIds`. */
function onlyHubs<T>(map: Record<string, T>, hubIds: ReadonlySet<string>): Record<string, T> {
  return Object.fromEntries(
    Object.entries(map).filter(([key]) => hubIds.has(splitChamberKey(key).hubId)),
  )
}

/** Cache records are keyed by token, so they survive the access-id migration
 * from URL-only to URL+token. Re-scope every embedded chamber key to the id the
 * current account now uses; otherwise the first fresh index cannot replace the
 * legacy rows and they look like chambers from a hub that no longer exists. */
function scopeCachedState(hub: HubAccount, cached: CachedState): CachedState {
  const scoped = (key: string) => chamberKey(hub.id, splitChamberKey(key).chamberId)
  return {
    chambers: cached.chambers.map((c) => ({ ...c, id: scoped(c.id), hubId: hub.id })),
    messagesByChamber: Object.fromEntries(
      Object.entries(cached.messagesByChamber).map(([key, messages]) => {
        const id = scoped(key)
        return [id, messages.map((m) => ({ ...m, chamberId: id }))]
      }),
    ),
    lastReadByChamber: Object.fromEntries(
      Object.entries(cached.lastReadByChamber).map(([key, mark]) => [scoped(key), mark]),
    ),
  }
}

let nextClientId = 1

/** How app mode builds a client for a hub, kept from `initApp` so adding or
 * removing a hub can rebuild the router. Process-lifetime configuration, not
 * state: nothing renders from it. */
let hubClientFactory: ((hub: HubAccount) => HubClient) | null = null

export interface AppState {
  /** `browser`: one hub, the one that served this page, signed in with `creds`.
   * `app`: the desktop app's own list of hubs, every one of them at once. */
  mode: 'browser' | 'app'
  creds: Credentials | null
  /** A `HubClient` in browser mode, the `HubRouter` over every hub in app mode. */
  client: ConsoleClient | null
  /** App mode's remembered hubs, in the order the user added them. */
  hubs: HubAccount[]
  /** Where that list is persisted; null until `initApp`. */
  hubsBackend: HubsBackend | null
  /** Per hub, what `/api/whoami` said: the role, the name our token wears, and
   * the hub's version. Browser mode keeps using `hubRole`/`selfName`/`hubVersion`. */
  roleByHub: Record<string, HubRole>
  selfNameByHub: Record<string, string>
  versionByHub: Record<string, string | null>
  /** Per-hub liveness; `connection` is the aggregate the chrome shows. */
  connectionByHub: Record<string, Connection>
  /** Hubs whose token the hub refused: one hub signing out must not sign the
   * app out, so the failure is a note on that hub's row instead. */
  authFailedHubs: string[]
  view: View
  settingsOpen: boolean
  /** A newer console build is installed and waiting; the UpdateBar offers a
   *  reload. Transient — never cached, never persisted. */
  updateAvailable: boolean
  chambers: Chamber[]
  /** The hub index has answered at least once for this session. */
  chambersLoaded: boolean
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
  /** Why the sign-in screen is what the window shows: browser mode's
   * auth-forced logout, or app mode's unreadable hub store. Cleared on the next
   * setCreds. */
  loginReason: string | null
  /** One-line banner after a chamber was pruned from under the user; cleared on navigate. */
  accessNotice: string | null
  /** Unconfirmed sends per chamber. Session-local: never cached, cleared on logout. */
  outboxByChamber: Record<string, OutboxItem[]>
  setCreds(c: Credentials): void
  logout(reason?: string): void
  /** Enter app mode over a remembered hub list. `makeClient` is the caller's
   * so tests inject a fake transport and the desktop app a trust-aware one. */
  initApp(
    hubs: HubAccount[],
    backend: HubsBackend,
    makeClient: (hub: HubAccount) => HubClient,
  ): void
  /** Add an access link, or refresh the metadata for that exact URL+token.
   * Different tokens on one hub remain separate because their scopes can expose
   * different chambers. Persists the list and rebuilds the router. */
  addHub(hub: HubAccount): Promise<void>
  /** Forget a hub: its chambers, conversations, watermarks, unsent messages
   * and local cache go with it. */
  removeHub(hubId: string): Promise<void>
  setChambersForHub(hubId: string, list: Chamber[]): void
  setConnectionForHub(hubId: string, c: Connection): void
  setHubIdentity(hubId: string, who: { role: HubRole; name?: string; version?: string | null }): void
  markHubAuthFailed(hubId: string): void
  navigate(v: View, options?: { replace?: boolean }): void
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
  /** `error` is the hub's own sentence when it gave one, so the failed bubble
   *  can say *why* (a 429 reads "rate limited"). */
  failOutbox(chamberId: string, clientId: number, error?: string | null): void
  retryOutbox(chamberId: string, clientId: number): void
  resolveOutbox(chamberId: string, clientId: number): void
}

const initialData = {
  mode: 'browser' as 'browser' | 'app',
  creds: null as Credentials | null,
  client: null as ConsoleClient | null,
  hubs: [] as HubAccount[],
  hubsBackend: null as HubsBackend | null,
  roleByHub: {} as Record<string, HubRole>,
  selfNameByHub: {} as Record<string, string>,
  versionByHub: {} as Record<string, string | null>,
  connectionByHub: {} as Record<string, Connection>,
  authFailedHubs: [] as string[],
  view: { name: 'projects' } as View,
  settingsOpen: false,
  updateAvailable: false,
  chambers: [] as Chamber[],
  chambersLoaded: false,
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

/** The router over exactly these hubs. A hub added or removed rebuilds it
 * rather than patching it, so the event loop's `[client]` effect restarts on
 * the new set. Only reachable in app mode, where `initApp` set the factory. */
function routerOver(hubs: HubAccount[]): HubRouter {
  const make = hubClientFactory
  if (!make) throw new Error('initApp must enter app mode before hubs are added or removed')
  return new HubRouter(hubs.map((hub) => ({ hub, client: make(hub) })))
}

/** Shared by failOutbox/retryOutbox: the target state and the reason to carry
 * with it (a retry is a fresh attempt, so it clears the old reason). */
function setOutboxState(
  chamberId: string,
  clientId: number,
  next: OutboxItem['state'],
  error: string | null,
) {
  return (state: AppState) => ({
    outboxByChamber: {
      ...state.outboxByChamber,
      [chamberId]: (state.outboxByChamber[chamberId] ?? []).map((o) =>
        o.clientId === clientId ? { ...o, state: next, error } : o,
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

function loadAppShowCompleted(): boolean {
  try {
    return localStorage.getItem(APP_SHOW_COMPLETED_KEY) === 'true'
  } catch {
    return false
  }
}

export const useAppStore = create<AppState>()((set, get) => {
  /** Mirror the list, messages and watermarks to the per-account cache so the
   * next boot paints instantly and unread counts survive a reload. */
  const persist = () => {
    const s = get()
    // App mode has no session-wide `creds`: each hub keeps its own record under
    // its own token, holding only its own rows. Anything else would leak one
    // hub's chambers into another's cache — and survive that hub being removed.
    if (s.mode === 'app') {
      // Grouped by token because the record is keyed on it: two entries for
      // one hub reached two ways (a tunnel and its LAN name) share a record,
      // and writing it once per hub would keep only the last alias's rows.
      const idsByToken = new Map<string, Set<string>>()
      for (const hub of s.hubs) {
        const ids = idsByToken.get(hub.token) ?? new Set<string>()
        ids.add(hub.id)
        idsByToken.set(hub.token, ids)
      }
      for (const [token, ids] of idsByToken) {
        saveCachedStateDebounced(
          { token },
          {
            chambers: s.chambers.filter((c) => ids.has(hubIdOf(c))),
            messagesByChamber: onlyHubs(s.messagesByChamber, ids),
            lastReadByChamber: onlyHubs(s.lastReadByChamber, ids),
          },
        )
      }
      return
    }
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
        chambersLoaded: false,
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

    initApp: (hubs, backend, makeClient) => {
      hubClientFactory = makeClient
      // Hydrate from every hub's cache before a single round-trip, as browser
      // mode does at sign-in: the list and recent messages paint immediately,
      // and each hub's index read merges its own rows on top. The keys are
      // composite, so the hubs' records cannot collide.
      const chambers: Chamber[] = []
      const messagesByChamber: Record<string, ChamberMessage[]> = {}
      const lastReadByChamber: Record<string, string> = {}
      // The cache is keyed on the token, not on the hub: two entries for one
      // hub reached two ways (a tunnel and its LAN name) share a record, and
      // reading it once per hub pushed the same rows in twice.
      const hydrated = new Set<string>()
      for (const hub of hubs) {
        if (hydrated.has(cacheKey({ token: hub.token }))) continue
        hydrated.add(cacheKey({ token: hub.token }))
        const loaded = loadCachedState({ token: hub.token })
        if (!loaded) continue
        const cached = scopeCachedState(hub, loaded)
        chambers.push(...cached.chambers)
        Object.assign(messagesByChamber, cached.messagesByChamber)
        Object.assign(lastReadByChamber, cached.lastReadByChamber)
      }
      set({
        mode: 'app',
        chambers: chambers.sort((a, b) => a.name.localeCompare(b.name)),
        messagesByChamber,
        lastReadByChamber,
        // A cached tail is not a fetched history: every conversation opened
        // this session still refetches its own.
        loadedChambers: [],
        hubs,
        hubsBackend: backend,
        client: routerOver(hubs),
        // What the stored accounts last knew; `bootApp`'s whoami refreshes it.
        roleByHub: Object.fromEntries(hubs.map((h) => [h.id, h.role])),
        selfNameByHub: Object.fromEntries(hubs.map((h) => [h.id, h.name])),
        versionByHub: {},
        connectionByHub: Object.fromEntries(hubs.map((h) => [h.id, 'connecting' as Connection])),
        // With no hubs there is no index to wait for — the app shows Add Hub.
        chambersLoaded: hubs.length === 0,
        authFailedHubs: [],
        view: { name: 'projects' },
        showCompletedArchived: loadAppShowCompleted(),
      })
    },

    addHub: async (hub) => {
      const state = get()
      const known = state.hubs.find((h) => h.id === hub.id)
      const hubs = known ? state.hubs.map((h) => (h.id === hub.id ? hub : h)) : [...state.hubs, hub]
      set({
        hubs,
        client: routerOver(hubs),
        roleByHub: { ...state.roleByHub, [hub.id]: hub.role },
        selfNameByHub: { ...state.selfNameByHub, [hub.id]: hub.name },
        connectionByHub: {
          ...state.connectionByHub,
          [hub.id]: state.connectionByHub[hub.id] ?? 'connecting',
        },
        // Every caller has just authenticated this token, so whatever the old
        // one failed with is history.
        authFailedHubs: state.authFailedHubs.filter((id) => id !== hub.id),
      })
      await state.hubsBackend?.save(hubs)
    },

    removeHub: async (hubId) => {
      const state = get()
      const hub = state.hubs.find((h) => h.id === hubId)
      if (!hub) return
      const hubs = state.hubs.filter((h) => h.id !== hubId)
      // Order matters, as in logout: a pending debounced write would otherwise
      // land after the clear and put the forgotten hub's cache back.
      cancelPendingCachedState()
      // The cache is keyed on the token, like every other per-account store —
      // so an alias of the same hub that stays still owns the shared record.
      if (!hubs.some((h) => h.token === hub.token)) clearCachedState({ token: hub.token })
      const connectionByHub = { ...state.connectionByHub }
      const roleByHub = { ...state.roleByHub }
      const selfNameByHub = { ...state.selfNameByHub }
      const versionByHub = { ...state.versionByHub }
      delete connectionByHub[hubId]
      delete roleByHub[hubId]
      delete selfNameByHub[hubId]
      delete versionByHub[hubId]
      const onThisHub = (key: string) => splitChamberKey(key).hubId === hubId
      const leavingConversation =
        state.view.name === 'conversation' && onThisHub(state.view.chamberId)
      set({
        hubs,
        client: routerOver(hubs),
        chambers: state.chambers.filter((c) => hubIdOf(c) !== hubId),
        messagesByChamber: withoutHub(state.messagesByChamber, hubId),
        lastReadByChamber: withoutHub(state.lastReadByChamber, hubId),
        outboxByChamber: withoutHub(state.outboxByChamber, hubId),
        loadedChambers: state.loadedChambers.filter((id) => !onThisHub(id)),
        roleByHub,
        selfNameByHub,
        versionByHub,
        connectionByHub,
        connection: aggregateConnection(connectionByHub),
        authFailedHubs: state.authFailedHubs.filter((id) => id !== hubId),
        // A conversation on a forgotten hub has nowhere left to talk to.
        view: leavingConversation ? { name: 'projects' } : state.view,
      })
      // The cancel above took the surviving hubs' pending writes down with the
      // forgotten hub's; without this their records stay at whatever the last
      // flush left, and a boot after that hydrates a stale list.
      persist()
      await state.hubsBackend?.save(hubs)
    },

    setHubIdentity: (hubId, who) =>
      set((state) => ({
        roleByHub: { ...state.roleByHub, [hubId]: who.role },
        selfNameByHub:
          who.name === undefined
            ? state.selfNameByHub
            : { ...state.selfNameByHub, [hubId]: who.name },
        versionByHub:
          who.version === undefined
            ? state.versionByHub
            : { ...state.versionByHub, [hubId]: who.version },
      })),

    markHubAuthFailed: (hubId) =>
      set((state) =>
        state.authFailedHubs.includes(hubId)
          ? state
          : { authFailedHubs: [...state.authFailedHubs, hubId] },
      ),

    navigate: (v, options) => {
      if (get().mode !== 'app') writeViewHash(v, options?.replace)
      set({ view: v, accessNotice: null })
    },
    setSettingsOpen: (open) => set({ settingsOpen: open }),
    setUpdateAvailable: (v) => set({ updateAvailable: v }),

    /** Browser mode's one hub is the anonymous `''`, so this is the whole list. */
    setChambers: (list) => get().setChambersForHub('', list),

    setChambersForHub: (hubId, list) => {
      set((state) => ({
        // Only this hub's rows are replaced; the other hubs answered their own
        // index reads and their rows are still current.
        chambers: [...state.chambers.filter((c) => hubIdOf(c) !== hubId), ...list].sort((a, b) =>
          a.name.localeCompare(b.name),
        ),
        chambersLoaded: true,
        // Clearing loadedChambers on an index read is what makes a re-register
        // re-fetch histories over whatever the stream left behind — but only
        // for the hub that re-registered, since no other hub was interrupted.
        loadedChambers: state.loadedChambers.filter((id) => splitChamberKey(id).hubId !== hubId),
      }))
      persist()
    },

    updateChamberStatus: (list) => {
      const byId = new Map(list.map((c) => [c.id, c]))
      set((state) => {
        let touched = false
        const chambers = state.chambers.map((c) => {
          const fresh = byId.get(c.id)
          if (!fresh) return c
          // Only liveness: a status refresh must not reorder or replace rows.
          // A field the refresh left undefined leaves what we knew in place.
          const patch: Partial<Chamber> = {}
          let changed = false
          for (const k of LIVENESS_FIELDS) {
            const v = fresh[k]
            if (v === undefined || Object.is(v, c[k])) continue
            Object.assign(patch, { [k]: v })
            changed = true
          }
          if (!changed) return c
          touched = true
          return { ...c, ...patch }
        })
        // Returning the state object itself is Zustand's "nothing happened":
        // no listener fires and every selector keeps its reference. Status
        // events arrive several times a session and mostly say the same thing;
        // a fresh array would re-render every consumer for nothing — and, with
        // the sheet's focus effect behind it, yank focus from whatever the
        // owner is typing.
        return touched ? { chambers } : state
      })
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
      let redirected = false
      set((state) => {
        const messagesByChamber = { ...state.messagesByChamber }
        const lastReadByChamber = { ...state.lastReadByChamber }
        const outboxByChamber = { ...state.outboxByChamber }
        delete messagesByChamber[chamberId]
        delete lastReadByChamber[chamberId]
        delete outboxByChamber[chamberId]
        redirected = state.view.name === 'conversation' && state.view.chamberId === chamberId
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
      if (redirected && get().mode !== 'app') writeViewHash({ name: 'projects' }, true)
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
        } else if (state.mode === 'app') {
          try {
            localStorage.setItem(APP_SHOW_COMPLETED_KEY, String(on))
          } catch {
            /* storage unavailable: the choice still applies for this session */
          }
        }
        return { showCompletedArchived: on }
      }),

    setConnection: (c) => get().setConnectionForHub('', c),

    setConnectionForHub: (hubId, c) =>
      set((state) => {
        const connectionByHub = { ...state.connectionByHub, [hubId]: c }
        return { connectionByHub, connection: aggregateConnection(connectionByHub) }
      }),

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
            { clientId, chamberId, body, state: 'sending' as const, serverId: null, error: null },
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

    failOutbox: (chamberId, clientId, error = null) =>
      set(setOutboxState(chamberId, clientId, 'failed', error)),
    retryOutbox: (chamberId, clientId) =>
      set(setOutboxState(chamberId, clientId, 'sending', null)),

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
export function useIsOwner(scope?: string): boolean {
  return useAppStore((s) => isOwnerFor(s, scope))
}

/** The check itself. Without a scope it answers for the session (browser
 * mode's one hub); with a chamber key it answers for the hub that chamber is
 * on, because a token can own one hub and be a guest on the next. */
export function isOwnerFor(
  s: Pick<AppState, 'hubRole' | 'roleByHub'>,
  scope?: string,
): boolean {
  if (scope === undefined) return s.hubRole === 'owner'
  return (s.roleByHub[splitChamberKey(scope).hubId] ?? s.hubRole) === 'owner'
}

export function resetAppStore(): void {
  resetChamberEvents()
  cancelPendingCachedState()
  nextClientId = 1
  hubClientFactory = null
  try {
    for (const key of Object.keys(localStorage)) {
      if (key.startsWith(CACHE_PREFIX)) localStorage.removeItem(key)
    }
  } catch {
    /* storage unavailable */
  }
  useAppStore.setState({ ...initialData, showCompletedArchived: false })
}
