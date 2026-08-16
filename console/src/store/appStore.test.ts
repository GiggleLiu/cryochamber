import { renderHook } from '@testing-library/react'
import {
  useAppStore,
  resetAppStore,
  unreadCount,
  useIsOwner,
  showCompletedKey,
} from './appStore'
import type { Chamber, ChamberMessage, Credentials } from '../api/types'
import { cacheKey, loadCachedState, flushCachedState } from './cache'

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
