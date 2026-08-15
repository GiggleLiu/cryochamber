import { useAppStore, resetAppStore } from './appStore'
import { loadCredentials } from './auth'
import { loadCachedState, saveCachedState } from './cache'
import type { Credentials, InitialState, Message } from '../api/types'

const creds: Credentials = { kind: 'hub', prefix: '', email: 'me@b.c', apiKey: 'k', sendTopic: '' }
const otherCreds: Credentials = { ...creds, prefix: '/other', email: 'Bob' }

const initial: InitialState = {
  subscriptions: [
    { stream_id: 2, name: 'beta', description: 'B' },
    { stream_id: 1, name: 'alpha', description: 'A' },
  ],
  unread: [
    { stream_id: 1, topic: '', unread_message_ids: [10] },
    { stream_id: 1, topic: 'chat', unread_message_ids: [11] },
  ],
}

function makeMsg(id: number, sender = 'bot@b.c'): Message {
  return {
    id, sender_full_name: 'Bot', sender_email: sender,
    timestamp: 1755100000 + id, content: `m${id}`, stream_id: 1, subject: '',
  }
}

beforeEach(() => {
  // Per-account keys (hidden projects, drafts) outlive resetAppStore by design.
  localStorage.clear()
  resetAppStore()
})

test('setCreds stores creds, builds a client, navigates to projects', () => {
  useAppStore.getState().setCreds(creds)
  const s = useAppStore.getState()
  expect(s.creds).toEqual(creds)
  expect(s.client).not.toBeNull()
  expect(s.view).toEqual({ name: 'projects' })
})

test('applyInitialState sorts streams and merges per-topic unreads', () => {
  useAppStore.getState().applyInitialState(initial)
  const s = useAppStore.getState()
  expect(s.streams.map((x) => x.name)).toEqual(['alpha', 'beta'])
  expect(s.unreadByStream[1]).toEqual([10, 11])
})

test('message events append to loaded streams and count unread for others only', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setMessages(1, [makeMsg(1)])
  useAppStore.getState().applyEvents([
    { id: 1, type: 'message', message: makeMsg(2) },
    { id: 2, type: 'message', message: makeMsg(3, 'me@b.c') },
  ])
  const s = useAppStore.getState()
  expect(s.messagesByStream[1].map((m) => m.id)).toEqual([1, 2, 3])
  expect(s.unreadByStream[1]).toEqual([2]) // own message never unread
})

test('read-flag events remove unreads', () => {
  useAppStore.getState().applyInitialState(initial)
  useAppStore.getState().applyEvents([
    { id: 3, type: 'update_message_flags', flag: 'read', op: 'add', messages: [10] },
  ])
  expect(useAppStore.getState().unreadByStream[1]).toEqual([11])
})

test('duplicate message event ids do not double-count unread', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().applyEvents([
    { id: 1, type: 'message', message: makeMsg(9) },
    { id: 2, type: 'message', message: makeMsg(9) },
  ])
  const s = useAppStore.getState()
  expect(s.unreadByStream[1]).toEqual([9])
  expect(s.messagesByStream[1].map((m) => m.id)).toEqual([9])
})

test('applyInitialState dedupes unread ids across topics', () => {
  useAppStore.getState().applyInitialState({
    ...initial,
    unread: [
      { stream_id: 1, topic: '', unread_message_ids: [10] },
      { stream_id: 1, topic: 'chat', unread_message_ids: [10, 11] },
    ],
  })
  expect(useAppStore.getState().unreadByStream[1]).toEqual([10, 11])
})

describe('hidden projects are per account', () => {
  test('toggleHidden persists under this account key', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().toggleHidden(1)
    expect(useAppStore.getState().hiddenStreams).toEqual([1])
    expect(JSON.parse(localStorage.getItem('agent-console.hidden.hub||me@b.c')!)).toEqual([1])
    useAppStore.getState().toggleHidden(1)
    expect(useAppStore.getState().hiddenStreams).toEqual([])
  })

  test('signing into another account re-reads its own list', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().toggleHidden(1)
    // Another token numbers its own chambers from 1, so its project 1 is a
    // different project and must not arrive pre-hidden.
    useAppStore.getState().setCreds(otherCreds)
    expect(useAppStore.getState().hiddenStreams).toEqual([])
    useAppStore.getState().toggleHidden(1)
    expect(JSON.parse(localStorage.getItem('agent-console.hidden.hub|/other|Bob')!)).toEqual([1])
    // …and going back finds the first account's list intact.
    useAppStore.getState().setCreds(creds)
    expect(useAppStore.getState().hiddenStreams).toEqual([1])
  })

  test('logout clears the in-memory list; the next sign-in restores it', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().toggleHidden(2)
    useAppStore.getState().logout()
    expect(useAppStore.getState().hiddenStreams).toEqual([])
    useAppStore.getState().setCreds(creds)
    expect(useAppStore.getState().hiddenStreams).toEqual([2])
  })
})

test('message event for an un-fetched stream creates the list', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().applyEvents([
    { id: 1, type: 'message', message: makeMsg(5) },
  ])
  expect(useAppStore.getState().messagesByStream[1].map((m) => m.id)).toEqual([5])
})

test('setMessages merges, dedupes and sorts with pre-existing event messages, and marks the stream loaded', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().applyEvents([
    { id: 1, type: 'message', message: makeMsg(3) },
  ])
  useAppStore.getState().setMessages(1, [makeMsg(3), makeMsg(2), makeMsg(1)])
  const s = useAppStore.getState()
  expect(s.messagesByStream[1].map((m) => m.id)).toEqual([1, 2, 3])
  expect(s.loadedStreams).toEqual([1])
})

test('applyInitialState clears loadedStreams but keeps cached messages', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setMessages(1, [makeMsg(1)])
  useAppStore.getState().applyInitialState(initial)
  const s = useAppStore.getState()
  expect(s.loadedStreams).toEqual([])
  expect(s.messagesByStream[1].map((m) => m.id)).toEqual([1])
})

test('logout clears everything including stored credentials', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().applyInitialState(initial)
  useAppStore.getState().logout()
  const s = useAppStore.getState()
  expect(s.creds).toBeNull()
  expect(s.client).toBeNull()
  expect(s.streams).toEqual([])
  expect(loadCredentials()).toBeNull()
})

test('setOwnUserId stores the id and logout resets it', () => {
  useAppStore.getState().setOwnUserId(7)
  expect(useAppStore.getState().ownUserId).toBe(7)
  useAppStore.getState().logout()
  expect(useAppStore.getState().ownUserId).toBeNull()
})

test('setUsers stores users and logout resets them to null', () => {
  const list = [{ user_id: 1, full_name: 'Alice', email: 'a@b.c', is_bot: false }]
  useAppStore.getState().setUsers(list)
  expect(useAppStore.getState().users).toEqual(list)
  useAppStore.getState().logout()
  expect(useAppStore.getState().users).toBeNull()
})

test('setCreds hydrates streams and messages from the local cache', () => {
  saveCachedState(creds, [{ stream_id: 1, name: 'alpha', description: 'A' }], { 1: [makeMsg(4)] })
  useAppStore.getState().setCreds(creds)
  const s = useAppStore.getState()
  expect(s.streams.map((x) => x.name)).toEqual(['alpha'])
  expect(s.messagesByStream[1].map((m) => m.id)).toEqual([4])
  // Still not marked loaded: opening the conversation re-fetches fresh history.
  expect(s.loadedStreams).toEqual([])
})

test('store mutations persist to the cache and logout clears it', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().applyInitialState(initial)
  useAppStore.getState().setMessages(1, [makeMsg(1)])
  const cached = loadCachedState(creds)
  expect(cached?.streams.map((x) => x.name)).toEqual(['alpha', 'beta'])
  expect(cached?.messagesByStream[1].map((m) => m.id)).toEqual([1])
  useAppStore.getState().logout()
  expect(loadCachedState(creds)).toBeNull()
})

test('a full history window replaces cached messages older than the window', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setMessages(1, [makeMsg(1), makeMsg(2)])
  // 50 fresh messages, none overlapping the cache: merging would fake
  // contiguity across the (1,2)…(100…) gap.
  const window50 = Array.from({ length: 50 }, (_, i) => makeMsg(100 + i))
  useAppStore.getState().setMessages(1, window50)
  const ids = useAppStore.getState().messagesByStream[1].map((m) => m.id)
  expect(ids[0]).toBe(100)
  expect(ids).toHaveLength(50)
})

test('a short history window merges with cached messages (no gap possible)', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().setMessages(1, [makeMsg(1)])
  useAppStore.getState().setMessages(1, [makeMsg(2), makeMsg(3)])
  expect(useAppStore.getState().messagesByStream[1].map((m) => m.id)).toEqual([1, 2, 3])
})

test('logout stores a reason and setCreds clears it', () => {
  useAppStore.getState().setCreds(creds)
  useAppStore.getState().logout('session expired')
  expect(useAppStore.getState().loginReason).toBe('session expired')
  useAppStore.getState().setCreds(creds)
  expect(useAppStore.getState().loginReason).toBeNull()
})

describe('updateStreamStatus', () => {
  test('merges liveness into the matching projects and touches nothing else', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().applyInitialState(initial)
    useAppStore.getState().updateStreamStatus([
      { stream_id: 1, agentRunning: false, nextWake: 'in 2 h' },
    ])
    const [alpha, beta] = useAppStore.getState().streams
    expect(alpha).toEqual({
      stream_id: 1, name: 'alpha', description: 'A', agentRunning: false, nextWake: 'in 2 h',
    })
    // Not in the update: left exactly as it was, not reset to "unknown".
    expect(beta).toEqual({ stream_id: 2, name: 'beta', description: 'B' })
  })

  test('a status for a project we do not have is ignored', () => {
    useAppStore.getState().applyInitialState(initial)
    useAppStore.getState().updateStreamStatus([
      { stream_id: 99, agentRunning: true, nextWake: null },
    ])
    expect(useAppStore.getState().streams.map((s) => s.stream_id)).toEqual([1, 2])
  })
})

describe('pruneStream', () => {
  test('removes the project everywhere and leaves its open conversation', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().applyInitialState(initial)
    useAppStore.getState().setMessages(1, [makeMsg(1)])
    useAppStore.getState().navigate({ name: 'conversation', streamId: 1 })

    useAppStore.getState().pruneStream(1)

    const s = useAppStore.getState()
    expect(s.streams.map((x) => x.name)).toEqual(['beta'])
    expect(s.messagesByStream[1]).toBeUndefined()
    expect(s.unreadByStream[1]).toBeUndefined()
    expect(s.loadedStreams).toEqual([])
    expect(s.view).toEqual({ name: 'projects' })
    // The cache follows, so a reload does not resurrect it.
    expect(loadCachedState(creds)?.streams.map((x) => x.name)).toEqual(['beta'])
  })

  test('pruning some other project leaves the current view alone', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().applyInitialState(initial)
    useAppStore.getState().navigate({ name: 'conversation', streamId: 1 })
    useAppStore.getState().pruneStream(2)
    expect(useAppStore.getState().view).toEqual({ name: 'conversation', streamId: 1 })
    expect(useAppStore.getState().streams.map((x) => x.name)).toEqual(['alpha'])
  })
})

describe('outbox', () => {
  test('lifecycle: enqueue → fail → retry → resolve', () => {
    const id = useAppStore.getState().enqueueOutbox(1, 'hello')
    expect(id).toBeLessThan(0)
    expect(useAppStore.getState().outboxByStream[1][0]).toMatchObject({
      localId: id, streamId: 1, content: 'hello', state: 'sending',
    })
    useAppStore.getState().failOutbox(1, id)
    expect(useAppStore.getState().outboxByStream[1][0].state).toBe('failed')
    useAppStore.getState().retryOutbox(1, id)
    expect(useAppStore.getState().outboxByStream[1][0].state).toBe('sending')
    useAppStore.getState().resolveOutbox(1, id)
    expect(useAppStore.getState().outboxByStream[1]).toEqual([])
  })

  test('local ids are unique and negative, so they never collide with real ids', () => {
    const a = useAppStore.getState().enqueueOutbox(1, 'a')
    const b = useAppStore.getState().enqueueOutbox(1, 'b')
    const c = useAppStore.getState().enqueueOutbox(2, 'c')
    expect(new Set([a, b, c]).size).toBe(3)
    for (const id of [a, b, c]) expect(id).toBeLessThan(0)
    expect(useAppStore.getState().outboxByStream[1]).toHaveLength(2)
    expect(useAppStore.getState().outboxByStream[2]).toHaveLength(1)
  })

  test('failing or resolving an unknown id is a no-op', () => {
    const id = useAppStore.getState().enqueueOutbox(1, 'a')
    useAppStore.getState().failOutbox(1, -9999)
    useAppStore.getState().resolveOutbox(2, id)
    expect(useAppStore.getState().outboxByStream[1][0].state).toBe('sending')
  })

  test('a sent item is retired by its own echo', () => {
    useAppStore.getState().setCreds(creds)
    const id = useAppStore.getState().enqueueOutbox(1, '  do the thing  ')
    useAppStore.getState().markOutboxSent(1, id)
    useAppStore.getState().applyEvents([
      { id: 1, type: 'message', message: makeMsg(20, 'me@b.c') },
    ])
    // Not this one: different text.
    expect(useAppStore.getState().outboxByStream[1]).toHaveLength(1)
    useAppStore.getState().applyEvents([
      {
        id: 2,
        type: 'message',
        message: { ...makeMsg(21, 'me@b.c'), content: 'do the thing' },
      },
    ])
    expect(useAppStore.getState().outboxByStream[1]).toEqual([])
  })

  test('an echo never retires an item that is still in flight or failed', () => {
    useAppStore.getState().setCreds(creds)
    const sending = useAppStore.getState().enqueueOutbox(1, 'again')
    const failed = useAppStore.getState().enqueueOutbox(1, 'again')
    useAppStore.getState().failOutbox(1, failed)
    useAppStore.getState().applyEvents([
      { id: 1, type: 'message', message: { ...makeMsg(22, 'me@b.c'), content: 'again' } },
    ])
    expect(useAppStore.getState().outboxByStream[1].map((o) => o.localId)).toEqual([
      sending,
      failed,
    ])
  })

  test('an echo retires our bubble even when the sender name differs', () => {
    // The hub stamps its own sender on what we send (`alice (invite)` for
    // `Alice`), so matching on the address left every bubble stuck.
    useAppStore.getState().setCreds({ ...creds, email: 'Alice' })
    const id = useAppStore.getState().enqueueOutbox(1, 'ping')
    useAppStore.getState().markOutboxSent(1, id)
    useAppStore.getState().applyEvents([
      { id: 1, type: 'message', message: { ...makeMsg(23, 'alice (invite)'), content: 'ping' } },
    ])
    expect(useAppStore.getState().outboxByStream[1]).toEqual([])
  })

  test('an unrelated new message leaves the bubble pending', () => {
    useAppStore.getState().setCreds({ ...creds, email: 'Alice' })
    const id = useAppStore.getState().enqueueOutbox(1, 'ping')
    useAppStore.getState().markOutboxSent(1, id)
    useAppStore.getState().applyEvents([
      { id: 1, type: 'message', message: { ...makeMsg(24, 'alice (invite)'), content: 'pong' } },
      // Same text, different project: not our echo either.
      {
        id: 2,
        type: 'message',
        message: { ...makeMsg(25, 'alice (invite)'), stream_id: 2, content: 'ping' },
      },
    ])
    expect(useAppStore.getState().outboxByStream[1]).toHaveLength(1)
  })

  test('the outbox is session-local: never cached, cleared on logout', () => {
    useAppStore.getState().setCreds(creds)
    useAppStore.getState().setMessages(1, [makeMsg(1)])
    useAppStore.getState().enqueueOutbox(1, 'pending')
    expect(JSON.stringify(loadCachedState(creds))).not.toContain('pending')
    useAppStore.getState().logout()
    expect(useAppStore.getState().outboxByStream).toEqual({})
  })
})
