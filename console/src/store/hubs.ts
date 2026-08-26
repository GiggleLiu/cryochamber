import { hubIdFor, normalizeHubUrl } from '../lib/hubKeys'

/** How the user told the app to reach this hub. `https` is silent; the other
 * two were explicit decisions at add time and are stored so the app never
 * re-asks — and, for `pinned`, so a changed certificate is refused. */
export type HubTrust =
  | { kind: 'https' }
  | { kind: 'plain-http' }
  | { kind: 'pinned'; sha256: string }

/** One remembered hub: where it is, how we authenticate, who the token is on
 * that hub, and the trust decision. The whole list is what `localStorage`
 * could never hold durably — the app persists it via the Tauri store plugin. */
export interface HubAccount {
  id: string
  url: string
  label: string
  token: string
  name: string
  role: 'owner' | 'invite'
  trust: HubTrust
}

export interface HubsBackend {
  load(): Promise<HubAccount[]>
  save(hubs: HubAccount[]): Promise<void>
}

export class MemoryHubsBackend implements HubsBackend {
  private hubs: HubAccount[] = []
  async load(): Promise<HubAccount[]> {
    return this.hubs
  }
  async save(hubs: HubAccount[]): Promise<void> {
    this.hubs = hubs
  }
}

export function makeHubAccount(input: {
  url: string
  token: string
  label?: string
  name?: string
  role?: 'owner' | 'invite'
  trust: HubTrust
}): HubAccount {
  const url = normalizeHubUrl(input.url)
  return {
    id: hubIdFor(url, input.token),
    url,
    label: input.label || new URL(url).host,
    token: input.token,
    name: input.name || 'human',
    role: input.role === 'owner' ? 'owner' : 'invite',
    trust: input.trust,
  }
}

function parseTrust(raw: unknown): HubTrust | null {
  if (!raw || typeof raw !== 'object') return null
  const t = raw as { kind?: unknown; sha256?: unknown }
  if (t.kind === 'https') return { kind: 'https' }
  if (t.kind === 'plain-http') return { kind: 'plain-http' }
  if (t.kind === 'pinned' && typeof t.sha256 === 'string' && /^[0-9a-f]{64}$/.test(t.sha256)) {
    return { kind: 'pinned', sha256: t.sha256 }
  }
  return null
}

/** Defensive parse of whatever the backend stored: malformed entries are
 * dropped, never repaired into something that could reach the wrong hub. */
export function parseHubAccounts(raw: unknown): HubAccount[] {
  if (!Array.isArray(raw)) return []
  const out: HubAccount[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const h = item as Partial<HubAccount>
    const trust = parseTrust(h.trust)
    if (
      typeof h.url !== 'string' ||
      typeof h.token !== 'string' ||
      h.token === '' ||
      typeof h.label !== 'string' ||
      trust === null
    ) {
      continue
    }
    try {
      out.push(
        makeHubAccount({
          url: h.url,
          token: h.token,
          label: h.label,
          name: typeof h.name === 'string' ? h.name : undefined,
          role: h.role === 'owner' ? 'owner' : 'invite',
          trust,
        }),
      )
    } catch {
      /* unparseable URL: drop the entry */
    }
  }
  return out
}
