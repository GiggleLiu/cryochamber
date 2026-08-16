import type { Credentials } from '../api/types'

const KEY = 'agent-console.credentials'

/** A record written before credentials became `{token, name, role}`. */
interface LegacyCredentials {
  apiKey?: string
  email?: string
}

export function saveCredentials(c: Credentials): void {
  localStorage.setItem(KEY, JSON.stringify(c))
}

export function loadCredentials(): Credentials | null {
  const raw = localStorage.getItem(KEY)
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as Partial<Credentials> & LegacyCredentials
    if (typeof parsed.token === 'string' && parsed.token) {
      return {
        token: parsed.token,
        name: typeof parsed.name === 'string' ? parsed.name : 'human',
        role: parsed.role === 'owner' ? 'owner' : 'invite',
      }
    }
    // Pre-cutover record: only the token is worth carrying. The role is a
    // placeholder — App re-asks whoami at boot and corrects it.
    if (typeof parsed.apiKey === 'string' && parsed.apiKey) {
      return { token: parsed.apiKey, name: parsed.email || 'human', role: 'invite' }
    }
    return null
  } catch {
    return null
  }
}

export function clearCredentials(): void {
  localStorage.removeItem(KEY)
}
