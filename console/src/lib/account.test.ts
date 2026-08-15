import { accountKey } from './account'

const base = { kind: 'hub' as const, prefix: '', sendTopic: '', email: 'Alice' }

test('two tokens with the same display name are different namespaces', () => {
  // Invite names are reusable after revocation: a later "Alice" token must
  // not inherit the old Alice's drafts, id maps, or hidden projects.
  const first = accountKey({ ...base, apiKey: 'ab'.repeat(32) })
  const second = accountKey({ ...base, apiKey: 'cd'.repeat(32) })
  expect(first).not.toBe(second)
})

test('the key never contains the token itself, and is stable', () => {
  const token = 'ab'.repeat(32)
  const key = accountKey({ ...base, apiKey: token })
  expect(key).not.toContain(token)
  expect(key).toBe(accountKey({ ...base, apiKey: token }))
})

test('the same token on two hubs is two namespaces', () => {
  const token = 'ab'.repeat(32)
  expect(accountKey({ ...base, apiKey: token, prefix: '/hub-a' })).not.toBe(
    accountKey({ ...base, apiKey: token, prefix: '/hub-b' }),
  )
})
