import { loadCredentials, saveCredentials, clearCredentials } from './auth'

beforeEach(() => localStorage.clear())

test('round-trips credentials', () => {
  saveCredentials({ token: 'tok', name: 'Alice', role: 'invite' })
  expect(loadCredentials()).toEqual({ token: 'tok', name: 'Alice', role: 'invite' })
})

test('migrates the pre-cutover record: apiKey→token, email→name, role placeholder', () => {
  localStorage.setItem(
    'agent-console.credentials',
    JSON.stringify({ kind: 'hub', prefix: '', email: 'Owner', apiKey: 'k1', sendTopic: '' }),
  )
  expect(loadCredentials()).toEqual({ token: 'k1', name: 'Owner', role: 'invite' })
})

test('returns null when nothing stored, on corrupt JSON, and on a record without a token', () => {
  expect(loadCredentials()).toBeNull()
  localStorage.setItem('agent-console.credentials', '{nope')
  expect(loadCredentials()).toBeNull()
  localStorage.setItem('agent-console.credentials', JSON.stringify({ name: 'x' }))
  expect(loadCredentials()).toBeNull()
})

test('clearCredentials removes the record', () => {
  saveCredentials({ token: 'tok', name: 'A', role: 'owner' })
  clearCredentials()
  expect(loadCredentials()).toBeNull()
})
