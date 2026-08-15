import type { Credentials } from '../api/types'

const KEY = 'agent-console.credentials'

export function saveCredentials(c: Credentials): void {
  localStorage.setItem(KEY, JSON.stringify(c))
}

export function loadCredentials(): Credentials | null {
  const raw = localStorage.getItem(KEY)
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as Credentials
    // A session stored by an older build could name another backend, which no
    // client here can talk to: force a re-login rather than boot into it.
    return parsed.kind === 'hub' ? parsed : null
  } catch {
    return null
  }
}

export function clearCredentials(): void {
  localStorage.removeItem(KEY)
}
