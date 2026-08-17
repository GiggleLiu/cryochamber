import { accountKey } from './account'

test('accountKey fingerprints the token and never contains it', () => {
  const key = accountKey({ token: 'deadbeefdeadbeef' })
  expect(key.startsWith('hub|')).toBe(true)
  expect(key).not.toContain('deadbeef')
  expect(accountKey({ token: 'deadbeefdeadbeef' })).toBe(key)
  expect(accountKey({ token: 'other' })).not.toBe(key)
})
