import { renderHook } from '@testing-library/react'
import {
  useAppStore,
  resetAppStore,
  unreadCount,
  useIsOwner,
  showCompletedKey,
  selfNameFor,
  isOwnerFor,
} from './appStore'
import type { Chamber, ChamberMessage, Credentials } from '../api/types'
import { cacheKey, loadCachedState, flushCachedState } from './cache'
import { MemoryHubsBackend, makeHubAccount, type HubAccount } from './hubs'
import { HubClient } from '../api/hubClient'
import { chamberKey } from '../lib/hubKeys'

const creds: Credentials = { token: 'k', name: 'me', role: 'owner' }

/** A reload: an empty store over a full cache. `resetAppStore` also wipes the
 * cache (test hygiene between files), so the record is put back to stand for
 * what a real reload would find on disk. */
function reload(): void {
  // A real reload is preceded by `pagehide`, which flushes the debounced write.
  flushCachedState()
  const record = localStorage.getItem(cacheKey(creds))
  resetAppStore()
  if (record !== null) localStorage.setItem(cacheKey(creds), record)
  useAppStore.getState().setCreds(creds)
}

const chamber = (id: string, name = id): Chamber => ({
  id,
  name,
  running: true,
  agentRunning: false,
  nextWakeDisplay: null,
  completed: false,
  archived: false,
  hasOpenQuestion: false,
})

function msg(n: number, sender = 'agent', dir: 'inbox' | 'outbox' = 'outbox'): ChamberMessage {
  const ts = `2026-08-15T10:${String(n).padStart(2, '0')}:00`
  return {
    id: `${dir}/${n}.md`,
    chamberId: 'a',
    direction: dir,
    sender,
    subject: '',
    body: `m${n}`,
    timestamp: ts,
    session: null,
    isQuestion: false,
  }
}

beforeEach(() => {
  localStorage.clear()
  resetAppStore()
})

test('setCreds stores creds, builds a client, sets selfName and role, navigates to projects', () => {
  useAppStore.getState().setCreds(creds)
  const s = useAppStore.getState()
  expect(s.creds).toEqual(creds)
  expect(s.client).not.toBeNull()
  expect(s.selfName).toBe('me')
  expect(s.hubRole).toBe('owner')
  expect(s.view).toEqual({ name: 'projects' })
})

test('setChambers sorts by name and clears loadedChambers', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setMessages('a', [msg(1)])
  expect(useAppStore.getState().loadedChambers).toEqual(['a'])
  useAppStore.getState().setChambers([chamber('b', 'beta'), chamber('a', 'alpha')])
  expect(useAppStore.getState().chambers.map((c) => c.name)).toEqual(['alpha', 'beta'])
  expect(useAppStore.getState().loadedChambers).toEqual([])
})

test('setMessages replaces history but keeps live messages newer than the fetch', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().applyMessage(msg(5))
  useAppStore.getState().setMessages('a', [msg(1), msg(2)])
  expect(useAppStore.getState().messagesByChamber.a.map((m) => m.id)).toEqual([
    'outbox/1.md',
    'outbox/2.md',
    'outbox/5.md',
  ])
  // an older cached-only message is NOT kept: the fetch is the whole history
  useAppStore.getState().setMessages('a', [msg(2)])
  expect(useAppStore.getState().messagesByChamber.a.map((m) => m.id)).toEqual([
    'outbox/2.md',
    'outbox/5.md',
  ])
})

test('applyMessage dedupes by id and orders by messageKey across directions', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().applyMessage(msg(2, 'agent', 'outbox'))
  useAppStore.getState().applyMessage(msg(1, 'human', 'inbox'))
  useAppStore.getState().applyMessage(msg(2, 'agent', 'outbox'))
  expect(useAppStore.getState().messagesByChamber.a.map((m) => m.id)).toEqual([
    'inbox/1.md',
    'outbox/2.md',
  ])
})

test('updateChamberStatus refreshes liveness without disturbing the list', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setChambers([chamber('a'), chamber('b')])
  useAppStore
    .getState()
    .updateChamberStatus([
      { ...chamber('a'), agentRunning: true, nextWakeDisplay: 'in 2 h', hasOpenQuestion: true },
    ])
  const s = useAppStore.getState()
  expect(s.chambers.map((c) => c.id)).toEqual(['a', 'b'])
  expect(s.chambers[0]).toMatchObject({
    agentRunning: true,
    nextWakeDisplay: 'in 2 h',
    hasOpenQuestion: true,
  })
  // A chamber the refresh did not mention keeps what we knew.
  expect(s.chambers[1].agentRunning).toBe(false)
})

describe('unread watermark', () => {
  test('counts messages above the watermark from others only; markRead moves it to the newest key', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().setChambers([chamber('a')])
    useAppStore.getState().applyMessage(msg(1))
    useAppStore.getState().applyMessage(msg(2, 'me'))
    useAppStore.getState().applyMessage(msg(3))
    expect(unreadCount(useAppStore.getState(), 'a')).toBe(2)
    useAppStore.getState().markRead('a')
    expect(unreadCount(useAppStore.getState(), 'a')).toBe(0)
    useAppStore.getState().applyMessage(msg(4))
    expect(unreadCount(useAppStore.getState(), 'a')).toBe(1)
  })

  test('an inbox message newer in time than an outbox one counts as unread (ids do not sort by time)', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().applyMessage(msg(1, 'agent', 'outbox'))
    useAppStore.getState().markRead('a')
    useAppStore.getState().applyMessage(msg(2, 'other', 'inbox'))
    expect(unreadCount(useAppStore.getState(), 'a')).toBe(1)
  })

  test('survives setChambers (re-register) and reload from cache', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().setChambers([chamber('a')])
    useAppStore.getState().applyMessage(msg(1))
    useAppStore.getState().setChambers([chamber('a')])
    expect(unreadCount(useAppStore.getState(), 'a')).toBe(1)
    useAppStore.getState().markRead('a')
    useAppStore.getState().applyMessage(msg(2))
    flushCachedState()
    const cached = loadCachedState(creds)!
    expect(cached.lastReadByChamber.a).toBe('2026-08-15T10:01:00 outbox/1.md')
    reload()
    expect(unreadCount(useAppStore.getState(), 'a')).toBe(1)
  })

  test('markRead on an empty conversation writes no watermark', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().markRead('a')
    expect(useAppStore.getState().lastReadByChamber.a).toBeUndefined()
  })
})

describe('outbox', () => {
  test('sent item resolves when the message with its server id arrives', () => {
    useAppStore.getState().setCreds(creds)
    const clientId = useAppStore.getState().enqueueOutbox('a', 'hello')
    useAppStore.getState().markOutboxSent('a', clientId, 'inbox/9.md')
    expect(useAppStore.getState().outboxByChamber.a[0].state).toBe('sent')
    useAppStore.getState().applyMessage({ ...msg(9, 'me', 'inbox'), body: 'different text' })
    expect(useAppStore.getState().outboxByChamber.a).toEqual([])
  })

  test('identical text from someone else does not retire a pending item', () => {
    useAppStore.getState().setCreds(creds)
    const clientId = useAppStore.getState().enqueueOutbox('a', 'hello')
    useAppStore.getState().markOutboxSent('a', clientId, 'inbox/9.md')
    useAppStore.getState().applyMessage({ ...msg(3, 'other', 'inbox'), body: 'hello' })
    expect(useAppStore.getState().outboxByChamber.a).toHaveLength(1)
  })

  test('markOutboxSent resolves at once if the message is already in the thread', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().applyMessage(msg(9, 'me', 'inbox'))
    const clientId = useAppStore.getState().enqueueOutbox('a', 'hello')
    useAppStore.getState().markOutboxSent('a', clientId, 'inbox/9.md')
    expect(useAppStore.getState().outboxByChamber.a).toEqual([])
  })

  test('failOutbox / retryOutbox toggle state', () => {
    useAppStore.getState().setCreds(creds)
    const id = useAppStore.getState().enqueueOutbox('a', 'x')
    useAppStore.getState().failOutbox('a', id)
    expect(useAppStore.getState().outboxByChamber.a[0].state).toBe('failed')
    useAppStore.getState().retryOutbox('a', id)
    expect(useAppStore.getState().outboxByChamber.a[0].state).toBe('sending')
  })

  test('failOutbox keeps the reason, and a retry drops it', () => {
    useAppStore.getState().setCreds(creds)
    const id = useAppStore.getState().enqueueOutbox('a', 'x')
    useAppStore.getState().failOutbox('a', id, 'rate limited')
    expect(useAppStore.getState().outboxByChamber.a[0].error).toBe('rate limited')
    useAppStore.getState().retryOutbox('a', id)
    expect(useAppStore.getState().outboxByChamber.a[0].error).toBeNull()
  })

  test('resolveOutbox drops the item the fallback timer gave up on', () => {
    useAppStore.getState().setCreds(creds)
    const id = useAppStore.getState().enqueueOutbox('a', 'x')
    useAppStore.getState().resolveOutbox('a', id)
    expect(useAppStore.getState().outboxByChamber.a).toEqual([])
  })

  test('the outbox is session-local: it is never written to the cache', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().applyMessage(msg(1))
    useAppStore.getState().enqueueOutbox('a', 'unsent')
    flushCachedState()
    expect(JSON.stringify(loadCachedState(creds))).not.toContain('unsent')
  })
})

test('pruneChamber drops list, messages, watermark, and leaves the conversation with a notice', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setChambers([chamber('a'), chamber('b')])
  useAppStore.getState().applyMessage(msg(1))
  useAppStore.getState().markRead('a')
  useAppStore.getState().navigate({ name: 'conversation', chamberId: 'a' })
  useAppStore.getState().pruneChamber('a', 'gone')
  const s = useAppStore.getState()
  expect(s.chambers.map((c) => c.id)).toEqual(['b'])
  expect(s.messagesByChamber.a).toBeUndefined()
  expect(s.lastReadByChamber.a).toBeUndefined()
  expect(s.view).toEqual({ name: 'projects' })
  expect(s.accessNotice).toBe('gone')
})

test('navigating clears the access notice', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setAccessNotice('gone')
  useAppStore.getState().navigate({ name: 'conversation', chamberId: 'b' })
  expect(useAppStore.getState().accessNotice).toBeNull()
})

test('logout clears cache and returns to initial state', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().applyMessage(msg(1))
  useAppStore.getState().logout('bye')
  expect(useAppStore.getState().creds).toBeNull()
  expect(useAppStore.getState().loginReason).toBe('bye')
  expect(loadCachedState(creds)).toBeNull()
})

test('logout leaves nothing behind even with a debounced write still pending', () => {
  vi.useFakeTimers()
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().applyMessage(msg(1))
  useAppStore.getState().logout('bye')
  vi.advanceTimersByTime(300)
  expect(loadCachedState(creds)).toBeNull()
  vi.useRealTimers()
})

test('setCreds hydrates chambers, messages and watermarks from the cache', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setChambers([chamber('a', 'alpha')])
  useAppStore.getState().applyMessage(msg(1))
  useAppStore.getState().markRead('a')
  reload()
  const s = useAppStore.getState()
  expect(s.chambers.map((c) => c.name)).toEqual(['alpha'])
  expect(s.messagesByChamber.a.map((m) => m.id)).toEqual(['outbox/1.md'])
  expect(s.lastReadByChamber.a).toBe('2026-08-15T10:01:00 outbox/1.md')
  // The cache is a first paint, not a fetch: every open conversation re-fetches.
  expect(s.loadedChambers).toEqual([])
})

test('the update banner flag is transient and never persisted', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setUpdateAvailable(true)
  expect(useAppStore.getState().updateAvailable).toBe(true)
  flushCachedState()
  expect(JSON.stringify(loadCachedState(creds))).not.toContain('updateAvailable')
})

test('useIsOwner reflects role; showCompletedKey is per account', () => {
  useAppStore.getState().setCreds(creds)
  expect(renderHook(() => useIsOwner()).result.current).toBe(true)
  expect(showCompletedKey(creds)).toContain('agent-console.show-archived.')
})

describe('updateChamberStatus identity', () => {
  test('a refresh that changes nothing leaves the chambers array untouched and wakes no subscriber', () => {
    useAppStore.setState({ chambers: [chamber('a'), chamber('b')] })
    const before = useAppStore.getState().chambers
    const listener = vi.fn()
    const unsubscribe = useAppStore.subscribe(listener)
    useAppStore.getState().updateChamberStatus([chamber('a'), chamber('b')])
    unsubscribe()
    // Status events arrive several times a session and mostly say the same
    // thing; a new array would re-render every consumer for nothing.
    expect(Object.is(useAppStore.getState().chambers, before)).toBe(true)
    expect(listener).not.toHaveBeenCalled()
  })

  test('a differing refresh replaces only the chamber that changed', () => {
    useAppStore.setState({ chambers: [chamber('a'), chamber('b')] })
    const [, beforeB] = useAppStore.getState().chambers
    useAppStore.getState().updateChamberStatus([{ ...chamber('a'), agentRunning: true }])
    const [afterA, afterB] = useAppStore.getState().chambers
    expect(afterA.agentRunning).toBe(true)
    expect(Object.is(afterB, beforeB)).toBe(true)
  })

  test('a refresh naming only chambers we do not have changes nothing', () => {
    useAppStore.setState({ chambers: [chamber('a')] })
    const before = useAppStore.getState().chambers
    useAppStore.getState().updateChamberStatus([{ ...chamber('nope'), running: false }])
    expect(Object.is(useAppStore.getState().chambers, before)).toBe(true)
  })

  test('an undefined field in the refresh does not erase what was known', () => {
    useAppStore.setState({ chambers: [{ ...chamber('a'), nextWakeDisplay: 'in 2 h' }] })
    useAppStore.getState().updateChamberStatus([
      { ...chamber('a'), nextWakeDisplay: undefined as unknown as string | null, running: false },
    ])
    expect(useAppStore.getState().chambers[0]).toMatchObject({
      nextWakeDisplay: 'in 2 h',
      running: false,
    })
  })
})

describe('app mode (multi-hub)', () => {
  /** No app-mode test makes a request: the clients exist to be routed, not called. */
  const okFetch = (async () => new Response(JSON.stringify({}), { status: 200 })) as typeof fetch

  function twoHubs() {
    const a = makeHubAccount({ url: 'http://a.local:1', token: 'ta', trust: { kind: 'plain-http' } })
    const b = makeHubAccount({ url: 'http://b.local:2', token: 'tb', trust: { kind: 'plain-http' } })
    return { a, b }
  }

  function enterAppMode(...hubs: HubAccount[]): MemoryHubsBackend {
    const backend = new MemoryHubsBackend()
    useAppStore
      .getState()
      .initApp(
        hubs,
        backend,
        (h) => new HubClient({ token: h.token, baseUrl: h.url, fetch: okFetch }),
      )
    return backend
  }

  const hubChamber = (hubId: string, id: string, name = id): Chamber => ({
    ...chamber(id, name),
    id: chamberKey(hubId, id),
    hubId,
  })

  test('initApp builds a router over every hub and enters app mode', () => {
    const { a, b } = twoHubs()
    enterAppMode(a, b)
    const s = useAppStore.getState()
    expect(s.mode).toBe('app')
    expect(s.hubs.map((h) => h.id)).toEqual([a.id, b.id])
    expect(s.client).not.toBeNull()
    expect(s.creds).toBeNull()
    expect(s.roleByHub).toEqual({ [a.id]: a.role, [b.id]: b.role })
    expect(s.connectionByHub).toEqual({ [a.id]: 'connecting', [b.id]: 'connecting' })
  })

  test('the app show-completed choice survives another init', () => {
    const { a } = twoHubs()
    enterAppMode(a)
    useAppStore.getState().setShowCompletedArchived(true)
    resetAppStore()
    enterAppMode(a)
    expect(useAppStore.getState().showCompletedArchived).toBe(true)
  })

  test('setChambersForHub merges per hub without clobbering the other hub', () => {
    const { a, b } = twoHubs()
    enterAppMode(a, b)
    const s = useAppStore.getState()
    s.setChambersForHub(a.id, [hubChamber(a.id, 'x')])
    s.setChambersForHub(b.id, [hubChamber(b.id, 'y')])
    expect(useAppStore.getState().chambers.map((c) => c.name)).toEqual(['x', 'y'])
    // refreshing hub a must not drop hub b's rows
    s.setChambersForHub(a.id, [])
    expect(useAppStore.getState().chambers.map((c) => c.name)).toEqual(['y'])
  })

  test('a hub index read re-fetches only its own conversations', () => {
    const { a, b } = twoHubs()
    enterAppMode(a, b)
    const s = useAppStore.getState()
    s.setMessages(chamberKey(a.id, 'x'), [])
    s.setMessages(chamberKey(b.id, 'y'), [])
    expect(useAppStore.getState().loadedChambers).toHaveLength(2)
    s.setChambersForHub(a.id, [hubChamber(a.id, 'x')])
    // b's stream was never interrupted, so its histories are still current.
    expect(useAppStore.getState().loadedChambers).toEqual([chamberKey(b.id, 'y')])
  })

  test('aggregate connection: one live hub keeps the app live', () => {
    const { a, b } = twoHubs()
    enterAppMode(a, b)
    const s = useAppStore.getState()
    s.setConnectionForHub(a.id, 'live')
    s.setConnectionForHub(b.id, 'offline')
    expect(useAppStore.getState().connection).toBe('live')
    s.setConnectionForHub(a.id, 'connecting')
    expect(useAppStore.getState().connection).toBe('connecting')
    s.setConnectionForHub(a.id, 'offline')
    expect(useAppStore.getState().connection).toBe('offline')
  })

  test('setHubIdentity feeds selfNameFor and the per-hub owner check', () => {
    const { a, b } = twoHubs()
    enterAppMode(a, b)
    useAppStore.getState().setHubIdentity(a.id, { role: 'owner', name: 'liu', version: '9.9.9' })
    useAppStore.getState().setHubIdentity(b.id, { role: 'invite', name: 'guest' })
    const s = useAppStore.getState()
    expect(selfNameFor(s, chamberKey(a.id, 'x'))).toBe('liu')
    expect(selfNameFor(s, chamberKey(b.id, 'y'))).toBe('guest')
    expect(s.versionByHub).toEqual({ [a.id]: '9.9.9' })
    expect(isOwnerFor(s, chamberKey(a.id, 'x'))).toBe(true)
    expect(isOwnerFor(s, chamberKey(b.id, 'y'))).toBe(false)
  })

  test('removeHub prunes that hub completely and persists the shorter list', async () => {
    const { a, b } = twoHubs()
    const backend = enterAppMode(a, b)
    const key = chamberKey(a.id, 'x')
    const s = useAppStore.getState()
    s.setChambersForHub(a.id, [hubChamber(a.id, 'x')])
    s.setChambersForHub(b.id, [hubChamber(b.id, 'y')])
    s.setConnectionForHub(a.id, 'live')
    s.applyMessage({ ...msg(1), chamberId: key })
    s.markRead(key)
    s.enqueueOutbox(key, 'pending')
    localStorage.setItem(
      cacheKey({ token: a.token }),
      JSON.stringify({ chambers: [], messagesByChamber: {}, lastReadByChamber: {} }),
    )

    await useAppStore.getState().removeHub(a.id)

    const after = useAppStore.getState()
    expect(after.hubs.map((h) => h.id)).toEqual([b.id])
    expect(after.chambers.map((c) => c.id)).toEqual([chamberKey(b.id, 'y')])
    expect(after.messagesByChamber[key]).toBeUndefined()
    expect(after.lastReadByChamber[key]).toBeUndefined()
    expect(after.outboxByChamber[key]).toBeUndefined()
    expect(after.roleByHub[a.id]).toBeUndefined()
    expect(after.connectionByHub[a.id]).toBeUndefined()
    // The hub that was live is gone, so the app is no longer live through it.
    expect(after.connection).toBe('connecting')
    expect(loadCachedState({ token: a.token })).toBeNull()
    expect((await backend.load()).map((h) => h.id)).toEqual([b.id])
  })

  test('removeHub re-persists the hubs that stay', async () => {
    const { a, b } = twoHubs()
    enterAppMode(a, b)
    const s = useAppStore.getState()
    s.setChambersForHub(a.id, [hubChamber(a.id, 'x')])
    s.setChambersForHub(b.id, [hubChamber(b.id, 'y')])

    // Forgetting a hub cancels every pending cache write, including the ones
    // that belong to the hubs that stay — their record must be written again.
    await useAppStore.getState().removeHub(a.id)
    flushCachedState()

    expect(loadCachedState({ token: b.token })?.chambers.map((c) => c.id)).toEqual([
      chamberKey(b.id, 'y'),
    ])
  })

  test('two hubs sharing one token hydrate that cache once', () => {
    // Same token, two addresses (a tunnel and the LAN name): one cache record,
    // and reading it per hub used to push its rows twice.
    const a = makeHubAccount({ url: 'http://a.local:1', token: 't', trust: { kind: 'plain-http' } })
    const b = makeHubAccount({ url: 'http://b.local:2', token: 't', trust: { kind: 'plain-http' } })
    localStorage.setItem(
      cacheKey({ token: 't' }),
      JSON.stringify({
        chambers: [hubChamber(a.id, 'x')],
        messagesByChamber: {},
        lastReadByChamber: {},
      }),
    )
    enterAppMode(a, b)
    expect(useAppStore.getState().chambers.map((c) => c.id)).toEqual([chamberKey(a.id, 'x')])
  })

  test('removeHub leaves a conversation on that hub for the projects list', async () => {
    const { a } = twoHubs()
    enterAppMode(a)
    useAppStore.getState().navigate({ name: 'conversation', chamberId: chamberKey(a.id, 'x') })
    await useAppStore.getState().removeHub(a.id)
    expect(useAppStore.getState().view).toEqual({ name: 'projects' })
  })

  test('app navigation does not create browser history entries', () => {
    const { a } = twoHubs()
    window.history.replaceState(null, '', '#/')
    enterAppMode(a)
    useAppStore.getState().navigate({ name: 'conversation', chamberId: chamberKey(a.id, 'x') })
    expect(window.location.hash).toBe('#/')
  })

  test('addHub with a known id replaces that hub rather than adding a second row', async () => {
    const { a, b } = twoHubs()
    const backend = enterAppMode(a, b)
    useAppStore.getState().markHubAuthFailed(a.id)
    const fresh = makeHubAccount({
      url: 'http://a.local:1/',
      token: 'ta2',
      name: 'liu',
      role: 'owner',
      trust: { kind: 'plain-http' },
    })
    expect(fresh.id).toBe(a.id)

    await useAppStore.getState().addHub(fresh)

    const s = useAppStore.getState()
    expect(s.hubs.map((h) => h.id)).toEqual([a.id, b.id])
    expect(s.hubs[0].token).toBe('ta2')
    expect(s.roleByHub[a.id]).toBe('owner')
    expect(s.selfNameByHub[a.id]).toBe('liu')
    // The token that failed is gone, so the failure note goes with it.
    expect(s.authFailedHubs).toEqual([])
    expect((await backend.load()).map((h) => h.token)).toEqual(['ta2', 'tb'])
  })

  test('addHub appends an unknown hub and markHubAuthFailed notes it once', async () => {
    const { a, b } = twoHubs()
    enterAppMode(a)
    await useAppStore.getState().addHub(b)
    expect(useAppStore.getState().hubs.map((h) => h.id)).toEqual([a.id, b.id])
    useAppStore.getState().markHubAuthFailed(b.id)
    useAppStore.getState().markHubAuthFailed(b.id)
    expect(useAppStore.getState().authFailedHubs).toEqual([b.id])
  })

  /** A relaunch, the app-mode twin of `reload`: an empty store over every
   * hub's cache record, which `resetAppStore` wipes for test hygiene. */
  function relaunch(...hubs: HubAccount[]): void {
    flushCachedState()
    const records = hubs.map((h) => {
      const key = cacheKey({ token: h.token })
      return [key, localStorage.getItem(key)] as const
    })
    resetAppStore()
    for (const [key, record] of records) if (record !== null) localStorage.setItem(key, record)
    enterAppMode(...hubs)
  }

  test('each hub is cached under its own token, holding only its own rows', () => {
    const { a, b } = twoHubs()
    enterAppMode(a, b)
    const s = useAppStore.getState()
    const ka = chamberKey(a.id, 'x')
    s.setChambersForHub(a.id, [hubChamber(a.id, 'x')])
    s.setChambersForHub(b.id, [hubChamber(b.id, 'y')])
    s.applyMessage({ ...msg(1), chamberId: ka })
    s.markRead(ka)
    flushCachedState()

    const cachedA = loadCachedState({ token: a.token })!
    expect(cachedA.chambers.map((c) => c.id)).toEqual([ka])
    expect(Object.keys(cachedA.messagesByChamber)).toEqual([ka])
    expect(cachedA.lastReadByChamber[ka]).toBeDefined()

    // Hub b's record must not carry a single one of hub a's rows.
    const cachedB = loadCachedState({ token: b.token })!
    expect(cachedB.chambers.map((c) => c.id)).toEqual([chamberKey(b.id, 'y')])
    expect(cachedB.messagesByChamber).toEqual({})
    expect(cachedB.lastReadByChamber).toEqual({})
  })

  test('app mode rehydrates every hub from its own cache on relaunch', () => {
    const { a, b } = twoHubs()
    enterAppMode(a, b)
    const s = useAppStore.getState()
    const ka = chamberKey(a.id, 'x')
    s.setChambersForHub(a.id, [hubChamber(a.id, 'x')])
    s.setChambersForHub(b.id, [hubChamber(b.id, 'y')])
    s.applyMessage({ ...msg(1), chamberId: ka })
    s.markRead(ka)

    relaunch(a, b)

    const after = useAppStore.getState()
    expect(after.chambers.map((c) => c.name)).toEqual(['x', 'y'])
    expect(after.messagesByChamber[ka]).toHaveLength(1)
    expect(after.lastReadByChamber[ka]).toBeDefined()
    // A cached tail is not a fetched history: every opened conversation still refetches.
    expect(after.loadedChambers).toEqual([])
    // And the cache is not an index answer.
    expect(after.chambersLoaded).toBe(false)
  })

  test('a hub with no cache contributes nothing and does not break the others', () => {
    const { a, b } = twoHubs()
    enterAppMode(a, b)
    useAppStore.getState().setChambersForHub(a.id, [hubChamber(a.id, 'x')])
    flushCachedState()
    localStorage.removeItem(cacheKey({ token: b.token }))

    relaunch(a, b)

    expect(useAppStore.getState().chambers.map((c) => c.name)).toEqual(['x'])
  })
})
