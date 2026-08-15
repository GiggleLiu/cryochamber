import { saveCredentials, loadCredentials, clearCredentials } from './auth'
import type { Credentials } from '../api/types'

const creds: Credentials = {
  kind: 'hub',
  prefix: '',
  email: 'Alice',
  apiKey: 'secret',
  sendTopic: '',
}

test('round-trips credentials through localStorage', () => {
  saveCredentials(creds)
  expect(loadCredentials()).toEqual(creds)
})

test('returns null when nothing stored', () => {
  expect(loadCredentials()).toBeNull()
})

test('returns null on corrupt stored JSON', () => {
  localStorage.setItem('agent-console.credentials', '{not json')
  expect(loadCredentials()).toBeNull()
})

test('credentials for another backend are discarded, forcing a re-login', () => {
  // What an older build stored. No client here can talk to it, so booting into
  // it would strand the user in a session that fails every request.
  localStorage.setItem(
    'agent-console.credentials',
    JSON.stringify({ prefix: '/elsewhere', email: 'a@b.c', apiKey: 'k', sendTopic: '' }),
  )
  expect(loadCredentials()).toBeNull()
})

test('clearCredentials removes stored value', () => {
  saveCredentials(creds)
  clearCredentials()
  expect(loadCredentials()).toBeNull()
})
