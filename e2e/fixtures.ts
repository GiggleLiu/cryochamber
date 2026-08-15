import type { Page, Route } from '@playwright/test'

/** Frozen wall clock for deterministic time pills / day separators. */
export const NOW = new Date('2026-08-15T14:32:00Z')
const T = Math.floor(NOW.getTime() / 1000)
const MIN = 60
const HOUR = 60 * MIN

/** 1×1 transparent PNG — stands in for an uploaded plot. */
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

const LONG_AGENT_MESSAGE = `<h2>Decoder sweep complete</h2>
<p>I ran the <strong>surface-17</strong> sweep across all four noise models. Two configurations beat the current baseline; the rest are within noise.</p>
<ul>
<li><strong>MWPM + reweighting</strong> — logical error rate <code>1.8e-4</code>, best overall</li>
<li><strong>Union-find</strong> — 3.1× faster, <code>2.4e-4</code></li>
<li>Neural decoder — did not converge inside the 2 h budget</li>
</ul>
<p>The threshold estimate comes from fitting</p>
<span class="katex-display"><span class="katex"><span class="katex-mathml"><math xmlns="http://www.w3.org/1998/Math/MathML"><semantics><mrow><msub><mi>p</mi><mi>L</mi></msub><mo>=</mo><mi>A</mi><msup><mrow><mo>(</mo><mi>p</mi><mi mathvariant="normal">/</mi><msub><mi>p</mi><mi>th</mi></msub><mo>)</mo></mrow><mrow><mo>(</mo><mi>d</mi><mo>+</mo><mn>1</mn><mo>)</mo><mi mathvariant="normal">/</mi><mn>2</mn></mrow></msup></mrow><annotation encoding="application/x-tex">p_L = A (p/p_{th})^{(d+1)/2}</annotation></semantics></math></span><span class="katex-html" aria-hidden="true"><span class="base"><span class="strut" style="height:0.5806em;vertical-align:-0.15em;"></span><span class="mord"><span class="mord mathnormal">p</span><span class="msupsub"><span class="vlist-t vlist-t2"><span class="vlist-r"><span class="vlist" style="height:0.3283em;"><span style="top:-2.55em;margin-left:0em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="sizing reset-size6 size3 mtight"><span class="mord mathnormal mtight">L</span></span></span></span><span class="vlist-s">​</span></span><span class="vlist-r"><span class="vlist" style="height:0.15em;"><span></span></span></span></span></span></span><span class="mspace" style="margin-right:0.2778em;"></span><span class="mrel">=</span><span class="mspace" style="margin-right:0.2778em;"></span></span><span class="base"><span class="strut" style="height:1.0641em;vertical-align:-0.25em;"></span><span class="mord mathnormal">A</span><span class="mopen">(</span><span class="mord mathnormal">p</span><span class="mord">/</span><span class="mord"><span class="mord mathnormal">p</span><span class="msupsub"><span class="vlist-t vlist-t2"><span class="vlist-r"><span class="vlist" style="height:0.3361em;"><span style="top:-2.55em;margin-left:0em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="sizing reset-size6 size3 mtight"><span class="mord mtight">th</span></span></span></span><span class="vlist-s">​</span></span><span class="vlist-r"><span class="vlist" style="height:0.15em;"><span></span></span></span></span></span></span><span class="mclose"><span class="mclose">)</span><span class="msupsub"><span class="vlist-t"><span class="vlist-r"><span class="vlist" style="height:0.8141em;"><span style="top:-3.063em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="sizing reset-size6 size3 mtight"><span class="mord mtight"><span class="mopen mtight">(</span><span class="mord mathnormal mtight">d</span><span class="mbin mtight">+</span><span class="mord mtight">1</span><span class="mclose mtight">)</span><span class="mord mtight">/2</span></span></span></span></span></span></span></span></span></span></span></span></span>
<p>with the fit driven by this loop:</p>
<div class="codehilite" data-code-language="Python"><pre><span></span><code><span class="k">def</span> <span class="nf">threshold_fit</span><span class="p">(</span><span class="n">rounds</span><span class="p">,</span> <span class="n">distances</span><span class="o">=</span><span class="p">(</span><span class="mi">3</span><span class="p">,</span> <span class="mi">5</span><span class="p">,</span> <span class="mi">7</span><span class="p">)):</span>
    <span class="sd">&quot;&quot;&quot;Least-squares fit of the sub-threshold scaling ansatz.&quot;&quot;&quot;</span>
    <span class="n">samples</span> <span class="o">=</span> <span class="p">[</span><span class="n">simulate</span><span class="p">(</span><span class="n">d</span><span class="p">,</span> <span class="n">rounds</span><span class="o">=</span><span class="n">rounds</span><span class="p">,</span> <span class="n">shots</span><span class="o">=</span><span class="mi">200_000</span><span class="p">)</span> <span class="k">for</span> <span class="n">d</span> <span class="ow">in</span> <span class="n">distances</span><span class="p">]</span>
    <span class="k">return</span> <span class="n">curve_fit</span><span class="p">(</span><span class="n">ansatz</span><span class="p">,</span> <span class="n">distances</span><span class="p">,</span> <span class="n">samples</span><span class="p">,</span> <span class="n">p0</span><span class="o">=</span><span class="p">[</span><span class="mf">1.0</span><span class="p">,</span> <span class="mf">0.011</span><span class="p">])</span>
</code></pre></div>
<table><thead><tr><th>decoder</th><th>p_L</th><th>runtime</th><th>threshold</th></tr></thead><tbody>
<tr><td>MWPM + reweighting</td><td>1.8e-4</td><td>44 min</td><td>1.09%</td></tr>
<tr><td>Union-find</td><td>2.4e-4</td><td>14 min</td><td>1.02%</td></tr>
<tr><td>Neural</td><td>—</td><td>&gt;2 h</td><td>—</td></tr>
</tbody></table>
<blockquote><p>Full logs are on the runner under <code>/scratch/sweeps/2026-08-14</code>.</p></blockquote>`

export interface Msg {
  id: number
  sender_full_name: string
  sender_email: string
  timestamp: number
  content: string
  stream_id: number
  subject: string
}

function msg(
  id: number,
  who: 'agent' | 'me' | 'peer',
  offsetSeconds: number,
  content: string,
): Msg {
  const senders = {
    agent: ['Research Agent', 'agent@qec.example'],
    me: ['Jin-Guo Liu', 'me@qec.example'],
    peer: ['Mei Chen', 'mei@qec.example'],
  } as const
  return {
    id,
    sender_full_name: senders[who][0],
    sender_email: senders[who][1],
    timestamp: T + offsetSeconds,
    content,
    stream_id: 1,
    subject: '',
  }
}

export const thread: Msg[] = [
  msg(101, 'me', -27 * HOUR, '<p>Kick off the decoder sweep when the cluster frees up.</p>'),
  msg(102, 'agent', -26 * HOUR, '<p>Queued. I&#39;ll report once all four noise models finish.</p>'),
  msg(103, 'agent', -3 * HOUR, LONG_AGENT_MESSAGE),
  msg(
    104,
    'me',
    -2 * HOUR - 50 * MIN,
    '<p>Nice. Can you plot logical error rate against rounds for the top two?</p>',
  ),
  msg(
    105,
    'agent',
    -2 * HOUR - 44 * MIN,
    '<p>Here it is — solid is MWPM, dashed is union-find.</p>' +
      '<div class="message_inline_image"><a href="/user_uploads/2/ab/plot.png" title="plot.png">' +
      '<img src="/user_uploads/2/ab/plot.png"></a></div>',
  ),
  msg(106, 'me', -2 * HOUR - 40 * MIN, '<p>Perfect.</p>'),
  msg(
    107,
    'peer',
    -12 * MIN,
    '<p><span class="user-mention" data-user-id="9" title="@Jin-Guo Liu">@Jin-Guo Liu</span> ' +
      'should we push the distance-9 run tonight? <span class="emoji emoji-1f680">:rocket:</span></p>',
  ),
  msg(108, 'agent', -2 * MIN, '<p>Cluster has 6 free nodes — enough for distance 9.</p>'),
]

export const subscriptions = [
  { stream_id: 1, name: 'qec-decoders', description: 'Surface-code decoder sweeps and threshold fits' },
  { stream_id: 2, name: 'tensor-networks', description: 'Contraction-order search for large PEPS' },
  { stream_id: 3, name: 'paper-drafts', description: 'Manuscript review agent' },
  { stream_id: 4, name: 'infra', description: 'Cluster babysitting, quotas, and job triage' },
]

export const users = [
  { user_id: 9, full_name: 'Jin-Guo Liu', email: 'me@qec.example' },
  { user_id: 11, full_name: 'Mei Chen', email: 'mei@qec.example' },
  { user_id: 12, full_name: 'Research Agent', email: 'agent@qec.example' },
  { user_id: 13, full_name: 'Marek Nowak', email: 'marek@qec.example' },
  { user_id: 14, full_name: 'Maya Rodriguez', email: 'maya@qec.example' },
]

export interface MockOptions {
  /** Fail the message history fetch, to capture the error state. */
  failHistory?: boolean
  /** Never resolve the history fetch, to capture the loading state. */
  hangHistory?: boolean
  /** Keep the event queue registration pending, to capture "Reconnecting…". */
  hangRegister?: boolean
}

export async function mockZulip(page: Page, opts: MockOptions = {}): Promise<void> {
  await page.clock.setFixedTime(NOW)

  await page.route('**/servers.json', (r) =>
    r.fulfill({ json: [{ name: 'QEC Harness', prefix: '/zulip/qec', sendTopic: '' }] }),
  )
  await page.route('**/zulip/qec/api/v1/fetch_api_key', (r) =>
    r.fulfill({ json: { result: 'success', api_key: 'k1', email: 'me@qec.example' } }),
  )
  await page.route('**/zulip/qec/api/v1/register', async (r: Route) => {
    if (opts.hangRegister) return
    await r.fulfill({
      json: {
        result: 'success',
        queue_id: 'q1',
        last_event_id: -1,
        subscriptions,
        unread_msgs: {
          streams: [
            { stream_id: 1, topic: '', unread_message_ids: [107, 108] },
            { stream_id: 2, topic: '', unread_message_ids: [201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212] },
            { stream_id: 4, topic: '', unread_message_ids: [301] },
          ],
        },
      },
    })
  })
  // Long-poll: answer once, then park.
  let eventCalls = 0
  await page.route('**/zulip/qec/api/v1/events**', async (r) => {
    eventCalls += 1
    if (eventCalls === 1) {
      await r.fulfill({ json: { result: 'success', events: [{ id: 0, type: 'heartbeat' }] } })
    }
  })
  await page.route('**/zulip/qec/api/v1/users', (r) =>
    r.fulfill({ json: { result: 'success', members: users } }),
  )
  await page.route('**/zulip/qec/api/v1/users/me', (r) =>
    r.fulfill({ json: { result: 'success', user_id: 9, email: 'me@qec.example' } }),
  )
  await page.route('**/zulip/qec/api/v1/messages?**', async (r) => {
    if (opts.hangHistory) return
    if (opts.failHistory) {
      await r.fulfill({ status: 500, json: { result: 'error', msg: 'Server error' } })
      return
    }
    await r.fulfill({ json: { result: 'success', messages: thread } })
  })
  await page.route('**/zulip/qec/api/v1/messages', (r) =>
    r.fulfill({ json: { result: 'success', id: 999 } }),
  )
  await page.route('**/zulip/qec/api/v1/mark_stream_as_read', (r) =>
    r.fulfill({ json: { result: 'success' } }),
  )
  await page.route('**/user_uploads/**', (r) =>
    r.fulfill({ contentType: 'image/svg+xml', body: PLOT_SVG }),
  )
  await page.route('**/icons/**', (r) =>
    r.fulfill({ contentType: 'image/png', body: Buffer.from(PNG_1PX, 'base64') }),
  )
}

/** Sign in and land on the projects list. */
export async function signIn(page: Page): Promise<void> {
  await page.goto('/')
  await page.getByLabel(/email/i).fill('me@qec.example')
  await page.getByLabel(/password/i).fill('correct-horse')
  await page.getByRole('button', { name: /sign in/i }).click()
}
