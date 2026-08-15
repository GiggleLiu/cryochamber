import type { Credentials } from '../api/types'

const KEY = 'zulip-app.credentials'

export function saveCredentials(c: Credentials): void {
  localStorage.setItem(KEY, JSON.stringify(c))
}

export function loadCredentials(): Credentials | null {
  const raw = localStorage.getItem(KEY)
  if (!raw) return null
  try {
    return JSON.parse(raw) as Credentials
  } catch {
    return null
  }
}

export function clearCredentials(): void {
  localStorage.removeItem(KEY)
}
