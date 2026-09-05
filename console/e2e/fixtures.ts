import type { Page, Route } from '@playwright/test'

/** Frozen wall clock for deterministic time pills / day separators. */
export const NOW = new Date('2026-08-15T14:32:00Z')
const MIN = 60_000
const HOUR = 60 * MIN

/** 1×1 transparent PNG — stands in for an app icon. */
const PNG_1PX =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='

/** A 640×360 slate placeholder so the image bubble has real proportions. */
const PLOT_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360" viewBox="0 0 640 360">
  <rect width="640" height="360" fill="#0f2318"/>
  <g stroke="#1f4a33" stroke-width="1">
    ${Array.from({ length: 7 }, (_, i) => `<line x1="60" y1="${40 + i * 45}" x2="600" y2="${40 + i * 45}"/>`).join('')}
  </g>
  <polyline fill="none" stroke="#95ec69" stroke-width="3"
    points="60,300 130,272 200,244 270,190 340,168 410,120 480,104 550,72 600,64"/>
  <polyline fill="none" stroke="#5ac8fa" stroke-width="3" stroke-dasharray="6 5"
    points="60,310 130,300 200,286 270,268 340,250 410,228 480,214 550,196 600,188"/>
  <text x="60" y="336" fill="#7c9c8a" font-family="monospace" font-size="14">rounds 0 → 400</text>
</svg>`

const LONG_AGENT_MESSAGE = `## Decoder sweep complete

I ran the **surface-17** sweep across all four noise models. Two configurations beat the current baseline; the rest are within noise.

- **MWPM + reweighting** — logical error rate \`1.8e-4\`, best overall
- **Union-find** — 3.1× faster, \`2.4e-4\`
- Neural decoder — did not converge inside the 2 h budget

The threshold estimate comes from fitting

$$p_L = A (p/p_{th})^{(d+1)/2}$$

with the fit driven by this loop:

\`\`\`python
def threshold_fit(rounds, distances=(3, 5, 7)):
    """Least-squares fit of the sub-threshold scaling ansatz."""
    samples = [simulate(d, rounds=rounds, shots=200_000) for d in distances]
    return curve_fit(ansatz, distances, samples, p0=[1.0, 0.011])
\`\`\`

| decoder | p_L | runtime | threshold |
| --- | --- | --- | --- |
| MWPM + reweighting | 1.8e-4 | 44 min | 1.09% |
| Union-find | 2.4e-4 | 14 min | 1.02% |
| Neural | — | >2 h | — |

> Full logs are on the runner under \`/scratch/sweeps/2026-08-14\`.`

/** A mailbox message, in the shape `GET /api/chambers/{id}/messages` returns. */
export interface ChamberMessage {
  id: string
  direction: string
  from: string
  subject: string
  body: string
  timestamp: string
  is_question: boolean
}

function msg(
  id: number,
  who: 'agent' | 'me' | 'peer',
  offsetMs: number,
  body: string,
): ChamberMessage {
  const senders = { agent: 'Research Agent', me: 'Jin-Guo Liu', peer: 'Mei Chen' } as const
  return {
    id: `msg-${id}`,
    direction: who === 'me' ? 'inbox' : 'outbox',
    from: senders[who],
    subject: '',
    // Zone-less ISO string, which is the shape the hub stamps.
    timestamp: new Date(NOW.getTime() + offsetMs).toISOString().replace('Z', ''),
    body,
    is_question: false,
  }
}

export const thread: ChamberMessage[] = [
  msg(101, 'me', -27 * HOUR, 'Kick off the decoder sweep when the cluster frees up.'),
  msg(102, 'agent', -26 * HOUR, 'Queued. I’ll report once all four noise models finish.'),
  msg(103, 'agent', -3 * HOUR, LONG_AGENT_MESSAGE),
  msg(
    104,
    'me',
    -2 * HOUR - 50 * MIN,
    'Nice. Can you plot logical error rate against rounds for the top two?',
  ),
  msg(
    105,
    'agent',
    -2 * HOUR - 44 * MIN,
    'Here it is — solid is MWPM, dashed is union-find.\n\n' +
      '![plot.png](/api/chambers/cham-a/files/ab_plot.png)',
  ),
  msg(106, 'me', -2 * HOUR - 40 * MIN, 'Perfect.'),
  msg(107, 'peer', -12 * MIN, '@Jin-Guo Liu should we push the distance-9 run tonight? 🚀'),
  msg(108, 'agent', -2 * MIN, 'Cluster has 6 free nodes — enough for distance 9.'),
]

export const chambers = [
  { id: 'cham-a', name: 'qec-decoders' },
  { id: 'cham-b', name: 'tensor-networks' },
  { id: 'cham-c', name: 'paper-drafts' },
  { id: 'cham-d', name: 'infra' },
]

export const TOKEN = 'ab'.repeat(16)

export interface MockOptions {
  /** Fail the message history fetch, to capture the error state. */
  failHistory?: boolean
  /** Never resolve the history fetch, to capture the loading state. */
  hangHistory?: boolean
  /** Keep the chamber index pending, to capture "Reconnecting…". */
  hangRegister?: boolean
  /** Chambers the hub reports; defaults to all four. */
  chambers?: Array<{ id: string; name: string }>
}

export async function mockHub(page: Page, opts: MockOptions = {}): Promise<void> {
  await page.clock.setFixedTime(NOW)

  await page.route('**/api/whoami', (r) =>
    r.fulfill({ json: { role: 'owner', name: 'Jin-Guo Liu', hub_version: '0.3.0' } }),
  )
  await page.route('**/api/chambers', async (r: Route) => {
    if (opts.hangRegister) return
    await r.fulfill({ json: opts.chambers ?? chambers })
  })
  // Playwright matches routes last-registered-first, so the catch-all has to be
  // registered BEFORE the chamber it is a fallback for — otherwise it shadows
  // cham-a's thread and every conversation renders empty.
  await page.route('**/api/chambers/*/messages*', (r) => r.fulfill({ json: [] }))
  await page.route('**/api/chambers/cham-a/messages*', async (r) => {
    if (opts.hangHistory) return
    if (opts.failHistory) {
      await r.fulfill({ status: 500, json: { detail: 'Server error' } })
      return
    }
    await r.fulfill({ json: thread })
  })
  await page.route('**/api/chambers/*/send', (r) =>
    r.fulfill({ json: { ok: true, id: 'inbox/2026-08-16T10-00-00_human_1.md' } }),
  )
  await page.route('**/api/chambers/*/files/**', (r) =>
    r.fulfill({ contentType: 'image/svg+xml', body: PLOT_SVG }),
  )
  // Held open the way a real connection is, so the app stays 'live'.
  await page.route('**/api/events', async () => {
    await new Promise((resolve) => setTimeout(resolve, 30_000))
  })
  await page.route('**/icons/**', (r) =>
    r.fulfill({ contentType: 'image/png', body: Buffer.from(PNG_1PX, 'base64') }),
  )
}

/** Sign in with an access token and land on the projects list. */
export async function signIn(page: Page): Promise<void> {
  await page.goto('/')
  await page.getByLabel(/access token/i).fill(TOKEN)
  await page.getByRole('button', { name: /sign in/i }).click()
}
