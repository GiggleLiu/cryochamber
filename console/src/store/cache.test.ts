import {
  loadCachedState,
  saveCachedState,
  clearCachedState,
  cacheKey,
  MAX_CACHED_MESSAGES,
} from './cache'
import type { ZulipMessage } from '../api/types'

const account = { prefix: '/zulip/qec', email: 'me@b.c' }
const other = { prefix: '/zulip/qec', email: 'friend@b.c' }
const streams = [{ stream_id: 1, name: 'alpha', description: 'A' }]

function makeMsg(id: number): ZulipMessage {
  return {
    id, sender_full_name: 'Bot', sender_email: 'bot@b.c',
    timestamp: 1755100000 + id, content: `<p>m${id}</p>`, stream_id: 1, subject: '',
  }
}

beforeEach(() => {
  localStorage.removeItem(cacheKey(account))
  localStorage.removeItem(cacheKey(other))
})

test('saved state round-trips', () => {
  saveCachedState(account, streams, { 1: [makeMsg(1), makeMsg(2)] })
  const loaded = loadCachedState(account)
  expect(loaded?.streams).toEqual(streams)
  expect(loaded?.messagesByStream[1].map((m) => m.id)).toEqual([1, 2])
})

test('missing or corrupt entries load as null', () => {
  expect(loadCachedState(account)).toBeNull()
  localStorage.setItem(cacheKey(account), 'not json')
  expect(loadCachedState(account)).toBeNull()
  localStorage.setItem(cacheKey(account), JSON.stringify({ streams: 'nope' }))
  expect(loadCachedState(account)).toBeNull()
})

test('only the newest messages per stream are kept', () => {
  const many = Array.from({ length: MAX_CACHED_MESSAGES + 20 }, (_, i) => makeMsg(i + 1))
  saveCachedState(account, streams, { 1: many, 2: [] })
  const loaded = loadCachedState(account)
  expect(loaded?.messagesByStream[1]).toHaveLength(MAX_CACHED_MESSAGES)
  expect(loaded?.messagesByStream[1].at(-1)?.id).toBe(MAX_CACHED_MESSAGES + 20)
  expect(loaded?.messagesByStream[2]).toBeUndefined()
})

test('accounts are cached independently and cleared independently', () => {
  saveCachedState(account, streams, { 1: [makeMsg(1)] })
  saveCachedState(other, [], { 1: [makeMsg(9)] })
  clearCachedState(account)
  expect(loadCachedState(account)).toBeNull()
  expect(loadCachedState(other)?.messagesByStream[1].map((m) => m.id)).toEqual([9])
})
