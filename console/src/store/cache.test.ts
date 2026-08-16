import {
  loadCachedState,
  saveCachedState,
  clearCachedState,
  purgeLegacyStorage,
  cacheKey,
  CACHE_PREFIX,
  MAX_CACHED_MESSAGES,
} from './cache'
import type { ChamberMessage } from '../api/types'

const creds = { token: 'k' }
const m = (n: number): ChamberMessage => ({
  id: `outbox/${n}.md`,
  chamberId: 'a',
  direction: 'outbox',
  sender: 'x',
  subject: '',
  body: '',
  timestamp: `2026-08-15T10:${String(n).padStart(2, '0')}:00`,
  session: null,
  isQuestion: false,
})

beforeEach(() => localStorage.clear())

test('round-trips and trims to the last MAX_CACHED_MESSAGES', () => {
  const msgs = Array.from({ length: MAX_CACHED_MESSAGES + 5 }, (_, i) => m(i))
  saveCachedState(creds, {
    chambers: [],
    messagesByChamber: { a: msgs },
    lastReadByChamber: { a: 'w' },
  })
  const back = loadCachedState(creds)!
  expect(back.messagesByChamber.a).toHaveLength(MAX_CACHED_MESSAGES)
  expect(back.messagesByChamber.a[0].id).toBe('outbox/5.md')
  expect(back.lastReadByChamber).toEqual({ a: 'w' })
})

test('an empty conversation is not cached at all', () => {
  saveCachedState(creds, { chambers: [], messagesByChamber: { a: [] }, lastReadByChamber: {} })
  expect(loadCachedState(creds)!.messagesByChamber).toEqual({})
})

test('a record written before watermarks existed still loads', () => {
  localStorage.setItem(cacheKey({ token: 'x' }), '{"chambers":[],"messagesByChamber":{}}')
  expect(loadCachedState({ token: 'x' })!.lastReadByChamber).toEqual({})
})

test('rejects a malformed record', () => {
  localStorage.setItem(cacheKey({ token: 'x' }), '{"chambers":1}')
  expect(loadCachedState({ token: 'x' })).toBeNull()
})

test('purgeLegacyStorage removes the pre-cutover keys and nothing else', () => {
  localStorage.setItem('agent-console.cache.|me', '{}')
  localStorage.setItem('agent-console.hub-ids.v2', '{}')
  localStorage.setItem('agent-console.hub-ids.hub||x', '{}')
  localStorage.setItem('agent-console.hub-msgids.hub||x', '{}')
  localStorage.setItem('agent-console.credentials', '{}')
  localStorage.setItem(`${CACHE_PREFIX}hub|k`, '{}')
  purgeLegacyStorage()
  expect(Object.keys(localStorage).sort()).toEqual(
    [`${CACHE_PREFIX}hub|k`, 'agent-console.credentials'].sort(),
  )
})

test('clearCachedState removes the entry', () => {
  saveCachedState(creds, { chambers: [], messagesByChamber: {}, lastReadByChamber: {} })
  clearCachedState(creds)
  expect(loadCachedState(creds)).toBeNull()
})
