import { saveCredentials, loadCredentials, clearCredentials } from './auth'
import type { Credentials } from '../api/types'

const creds: Credentials = {
  prefix: '/zulip/qec',
  email: 'a@b.c',
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
  localStorage.setItem('zulip-app.credentials', '{not json')
  expect(loadCredentials()).toBeNull()
})

test('clearCredentials removes stored value', () => {
  saveCredentials(creds)
  clearCredentials()
  expect(loadCredentials()).toBeNull()
})
