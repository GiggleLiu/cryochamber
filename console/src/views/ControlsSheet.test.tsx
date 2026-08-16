import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ControlsSheet, statePillLabel } from './ControlsSheet'
import { HubClient, type ChamberStatus } from '../api/hubClient'
import { useAppStore, resetAppStore } from '../store/appStore'
import { emitChamberEvent } from '../store/chamberEvents'
import { ApiError } from '../api/types'
import type { Credentials } from '../api/types'

const creds: Credentials = { token: 'k', name: 'Owner', role: 'owner' }

function status(overrides: Partial<ChamberStatus> = {}): ChamberStatus {
  return {
    running: false, agent_running: false, session: 7, agent: 'opencode',
    log_tail: '', daily_digests: [], next_wake: null,
    notes_html: '', plan_html: '', has_config: false, settings_rows: [],
    task: null, session_summary: null, completed: false, completion_summary: null,
    ...overrides,
  }
}

function makeHub(s: ChamberStatus = status()): HubClient {
  const client = new HubClient({ token: creds.token, fetch: vi.fn() })
  vi.spyOn(client, 'chamberStatus').mockResolvedValue(s)
  vi.spyOn(client, 'chamberTodos').mockResolvedValue([])
  vi.spyOn(client, 'lifecycle').mockResolvedValue({ ok: true, message: 'Started' })
  return client
}

function renderSheet(archived = false) {
  return render(
    <ControlsSheet
      chamberId="cham-a"
      chamberName="alpha"
      archived={archived}
      onClose={() => {}}
    />,
  )
}

/** A promise the test decides when to settle, so the window between a POST
 * answering and the refetch answering can be inspected. */
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((r) => {
    resolve = r
  })
  return { promise, resolve }
}

/** Let every already-resolved promise and its re-render finish. */
async function settle() {
  await act(async () => {
    await Promise.resolve()
  })
}

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({ creds, hubRole: 'owner', client: makeHub() })
})

test('the status group names the chamber, its state, and its session', async () => {
  useAppStore.setState({
    client: makeHub(status({ running: true, agent_running: true, next_wake: 'in 2 h', session_summary: 'swept the decoders' })),
  })
  renderSheet()
  expect(await screen.findByText('Working')).toBeInTheDocument()
  // The sheet is titled by the chamber, the way the gear's sheet is titled by
  // what it is about.
  expect(screen.getByRole('heading', { name: 'alpha' })).toBeInTheDocument()
  expect(screen.getByText('#7')).toBeInTheDocument()
  expect(screen.getByText('in 2 h')).toBeInTheDocument()
  // The summary is prose under the card, not a right-aligned row value: a
  // sentence squeezed into the value column truncated and wrapped its label.
  const summary = screen.getByText('swept the decoders')
  expect(summary.closest('.row')).toBeNull()
  expect(screen.getByText('Last session')).toBeInTheDocument()
})

test('a stopped chamber hides the wake line and a completed plan is called out', async () => {
  useAppStore.setState({ client: makeHub(status({ running: false, next_wake: 'in 2 h', completed: true })) })
  renderSheet()
  expect(await screen.findByText('Stopped')).toBeInTheDocument()
  expect(screen.queryByText(/Next wake/)).toBeNull()
  expect(screen.getByText('✓ complete')).toBeInTheDocument()
})

test('state pill wording covers every state', () => {
  expect(statePillLabel(status({ running: true, agent_running: true }), false)).toBe('Working')
  expect(statePillLabel(status({ running: true, agent_running: false }), false)).toBe('Asleep')
  expect(statePillLabel(status({ running: false }), false)).toBe('Stopped')
  // Archived wins: an archived chamber is put away whatever its runtime says.
  expect(statePillLabel(status({ running: true, agent_running: true }), true)).toBe('Archived')
})

describe('last session summary', () => {
  const LONG = 'Swept the decoders and rewrote the notes. '.repeat(20).trim()

  /** jsdom lays nothing out, so the clamp has to be simulated: a body taller
   * than its box is exactly what `-webkit-line-clamp` produces in a browser. */
  function overflowing(yes: boolean) {
    const spy = vi
      .spyOn(HTMLElement.prototype, 'scrollHeight', 'get')
      .mockReturnValue(yes ? 400 : 100)
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(100)
    return spy
  }

  afterEach(() => {
    vi.restoreAllMocks()
  })

  test('a summary that fits is shown whole, with nothing to tap', async () => {
    overflowing(false)
    useAppStore.setState({ client: makeHub(status({ session_summary: 'swept the decoders' })) })
    renderSheet()
    const body = await screen.findByText('swept the decoders')
    // Whole text, wrapping prose — no truncation marker of any kind.
    expect(body).toHaveTextContent('swept the decoders')
    expect(body).not.toHaveClass('row-value')
    expect(screen.queryByRole('button', { name: /show more/i })).toBeNull()
  })

  test('a summary too long for the clamp expands on tap and folds back', async () => {
    overflowing(true)
    useAppStore.setState({ client: makeHub(status({ session_summary: LONG })) })
    renderSheet()
    const body = await screen.findByText(LONG)
    // Clamped, but complete: the DOM holds every word even while folded.
    expect(body.textContent).toBe(LONG)
    expect(body).not.toHaveClass('is-expanded')
    const more = screen.getByRole('button', { name: /show more/i })
    expect(more).toHaveAttribute('aria-expanded', 'false')
    await userEvent.click(more)
    expect(body).toHaveClass('is-expanded')
    const less = screen.getByRole('button', { name: /show less/i })
    expect(less).toHaveAttribute('aria-expanded', 'true')
    await userEvent.click(less)
    expect(body).not.toHaveClass('is-expanded')
  })

  test('no summary, no block', async () => {
    renderSheet()
    await screen.findByText('Stopped')
    expect(screen.queryByText('Last session')).toBeNull()
  })
})

describe('lifecycle row', () => {
  test('a stopped chamber offers Launch, Archive and Reset', async () => {
    renderSheet()
    await screen.findByText('Stopped')
    expect(screen.getByRole('button', { name: 'Launch' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Archive' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Reset…' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Stop' })).toBeNull()
  })

  test('a running chamber offers Stop, Restart and Reset', async () => {
    useAppStore.setState({ client: makeHub(status({ running: true, agent_running: true })) })
    renderSheet()
    await screen.findByText('Working')
    expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Restart' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Reset…' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Launch' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Archive' })).toBeNull()
  })

  test('an archived chamber offers Unarchive and nothing else', async () => {
    renderSheet(true)
    await screen.findByText('Archived')
    expect(screen.getByRole('button', { name: 'Unarchive' })).toBeInTheDocument()
    for (const name of ['Launch', 'Stop', 'Restart', 'Archive', 'Reset…']) {
      expect(screen.queryByRole('button', { name })).toBeNull()
    }
  })

  test('Launch posts start, re-reads status, and shows what the hub said', async () => {
    const hub = useAppStore.getState().client as HubClient
    const refetch = deferred<ChamberStatus>()
    renderSheet()
    await screen.findByText('Stopped')
    vi.mocked(hub.chamberStatus).mockReturnValue(refetch.promise)
    await userEvent.click(screen.getByRole('button', { name: 'Launch' }))
    expect(hub.lifecycle).toHaveBeenCalledWith('cham-a', 'start')
    // No optimistic UI: the POST has already answered, but until the refetch
    // does the pill still reports what the hub last said.
    await waitFor(() => expect(hub.chamberStatus).toHaveBeenCalledTimes(2))
    expect(screen.getByText('Stopped')).toBeInTheDocument()
    expect(screen.queryByRole('status')).toBeNull()
    refetch.resolve(status({ running: true, agent_running: true }))
    expect(await screen.findByText('Working')).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Started')
  })

  test('the lifecycle buttons are disabled while an action is in flight', async () => {
    const hub = useAppStore.getState().client as HubClient
    const posted = deferred<{ ok: boolean; message: string }>()
    vi.mocked(hub.lifecycle).mockReturnValue(posted.promise)
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Launch' }))
    for (const name of ['Launch', 'Archive', 'Reset…']) {
      expect(screen.getByRole('button', { name })).toBeDisabled()
    }
    posted.resolve({ ok: true, message: 'Started' })
    expect(await screen.findByRole('status')).toHaveTextContent('Started')
    expect(screen.getByRole('button', { name: 'Launch' })).toBeEnabled()
  })

  test('a message-less response falls back to the action word', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.mocked(hub.lifecycle).mockResolvedValue({ ok: true, message: '' })
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Archive' }))
    expect(await screen.findByRole('status')).toHaveTextContent('archived')
  })

  test('an ok:false response is shown as an error and the buttons come back', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.mocked(hub.lifecycle).mockRejectedValue(
      new ApiError(200, 'Unarchive the chamber before launching it', true),
    )
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Launch' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Unarchive the chamber before launching it',
    )
    expect(screen.getByRole('button', { name: 'Launch' })).toBeEnabled()
  })

  test('a refusal outlives the status events that keep arriving', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.mocked(hub.lifecycle).mockRejectedValue(
      new ApiError(200, 'Unarchive the chamber before launching it', true),
    )
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Launch' }))
    await screen.findByRole('alert')
    // The hub emits `status` every few seconds while a session runs; a
    // successful refetch must not quietly erase what the refusal said.
    emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
    await settle()
    expect(hub.chamberStatus).toHaveBeenCalledTimes(3)
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Unarchive the chamber before launching it',
    )
  })

  test('Reset asks first and only then posts', async () => {
    const hub = useAppStore.getState().client as HubClient
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Reset…' }))
    expect(hub.lifecycle).not.toHaveBeenCalled()
    expect(
      screen.getByText(
        'Reset alpha? The session state and log are archived and the chamber starts fresh.',
      ),
    ).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Reset alpha' }))
    expect(hub.lifecycle).toHaveBeenCalledWith('cham-a', 'reset')
  })

  test('cancelling the reset confirm posts nothing', async () => {
    const hub = useAppStore.getState().client as HubClient
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Reset…' }))
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(hub.lifecycle).not.toHaveBeenCalled()
    expect(screen.queryByText(/starts fresh/)).toBeNull()
  })

  test('a 401 from an action stays silent — the client already signed out', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.mocked(hub.lifecycle).mockRejectedValue(new ApiError(401, 'HTTP 401'))
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Launch' }))
    await waitFor(() => expect(hub.lifecycle).toHaveBeenCalled())
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('a transport failure is reported as one, not as words the hub never said', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.mocked(hub.lifecycle).mockRejectedValue(new ApiError(502, 'HTTP 502'))
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Launch' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      /Could not start alpha\. Check your connection/,
    )
  })
})

test('a status event for this chamber re-reads the detail', async () => {
  const hub = useAppStore.getState().client as HubClient
  renderSheet()
  await screen.findByText('Stopped')
  vi.mocked(hub.chamberStatus).mockResolvedValue(status({ running: true, agent_running: true }))
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  expect(await screen.findByText('Working')).toBeInTheDocument()
  expect(hub.chamberStatus).toHaveBeenCalledTimes(2)
})

test('a status event for another chamber is ignored', async () => {
  const hub = useAppStore.getState().client as HubClient
  renderSheet()
  await screen.findByText('Stopped')
  emitChamberEvent({ type: 'status', chamberId: 'cham-b' })
  await settle()
  expect(hub.chamberStatus).toHaveBeenCalledTimes(1)
  // …while the subscription is genuinely live: our own chamber still lands.
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  await settle()
  expect(hub.chamberStatus).toHaveBeenCalledTimes(2)
})

test('closing the sheet unsubscribes from its chamber', async () => {
  const hub = useAppStore.getState().client as HubClient
  const { unmount } = renderSheet()
  await screen.findByText('Stopped')
  unmount()
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  await settle()
  expect(hub.chamberStatus).toHaveBeenCalledTimes(1)
})

describe('recent days', () => {
  const twoDays = () =>
    makeHub(
      status({
        daily_digests: [
          { date: '2026-08-15', total_sessions: 4, failed_sessions: 1, latest_session: 7 },
          { date: '2026-08-14', total_sessions: 1, failed_sessions: 0, latest_session: 3 },
        ],
      }),
    )

  test('daily digests render as a table, one row per day, in payload order', async () => {
    useAppStore.setState({ client: twoDays() })
    renderSheet()
    const table = await screen.findByRole('table', { name: 'Recent days' })
    expect(within(table).getAllByRole('columnheader').map((h) => h.textContent)).toEqual([
      'Day',
      'Sessions',
      'Failed',
    ])
    const rows = within(table).getAllByRole('row')
    // Header row plus one per day, newest first, exactly as the hub sent them.
    expect(rows).toHaveLength(3)
    expect(within(rows[1]).getAllByRole('cell').map((c) => c.textContent)).toEqual([
      '2026-08-15',
      '4',
      '1',
    ])
    expect(within(rows[2]).getAllByRole('cell').map((c) => c.textContent)).toEqual([
      '2026-08-14',
      '1',
      '0',
    ])
  })

  test('a day with failures marks the failed count, a clean day does not', async () => {
    useAppStore.setState({ client: twoDays() })
    renderSheet()
    const table = await screen.findByRole('table', { name: 'Recent days' })
    const rows = within(table).getAllByRole('row')
    expect(within(rows[1]).getAllByRole('cell')[2]).toHaveClass('digest-failed')
    expect(within(rows[2]).getAllByRole('cell')[2]).not.toHaveClass('digest-failed')
  })

  test('no digests, no section', async () => {
    renderSheet()
    await screen.findByText('Stopped')
    expect(screen.queryByRole('table')).toBeNull()
    expect(screen.queryByText('Recent days')).toBeNull()
  })
})

test('a failed status load stays inline in the sheet', async () => {
  const hub = useAppStore.getState().client as HubClient
  vi.mocked(hub.chamberStatus).mockRejectedValue(new ApiError(500, 'HTTP 500'))
  renderSheet()
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Could not load alpha. Check your connection and try again.',
  )
  expect(screen.getByRole('dialog', { name: 'Chamber controls' })).toBeInTheDocument()
})

describe('detail sections', () => {
  /** Each detail opens a sheet of its own, listed by a row in the controls
   * sheet underneath. */
  const open = (name: string) => userEvent.click(screen.getByRole('button', { name: new RegExp(`^${name}`) }))
  const closeTop = () => userEvent.click(screen.getAllByRole('button', { name: /close/i }).at(-1)!)

  test('every section starts closed, and opening one fetches once', async () => {
    // Closed by default: the state and the actions are what the sheet is
    // opened for, and a closed section costs no request at all.
    const hub = makeHub()
    vi.spyOn(hub, 'chamberTodos').mockResolvedValue([])
    useAppStore.setState({ client: hub })
    renderSheet()
    await screen.findByText('Stopped')
    expect(hub.chamberTodos).not.toHaveBeenCalled()
    await open('Todos')
    await waitFor(() => expect(hub.chamberTodos).toHaveBeenCalledTimes(1))
  })

  test('Plan and Notes render their status HTML with their own empty copy', async () => {
    useAppStore.setState({
      client: makeHub(status({ plan_html: '<p>the plan</p>', notes_html: '' })),
    })
    renderSheet()
    await screen.findByText('Stopped')

    await open('Plan')
    expect(await screen.findByText('the plan')).toBeInTheDocument()

    await open('Notes')
    expect(await screen.findByText('No NOTES.md in this chamber.')).toBeInTheDocument()
  })

  test('a detail opens over the controls and closing returns to them', async () => {
    useAppStore.setState({
      client: makeHub(status({ plan_html: '<p>the plan</p>', notes_html: '<p>the notes</p>' })),
    })
    renderSheet()
    await screen.findByText('Stopped')
    await open('Plan')
    // Its own dialog, named for what it is reading, over the one that listed it.
    expect(await screen.findByText('the plan')).toBeInTheDocument()
    expect(screen.getByRole('dialog', { name: /alpha Plan/ })).toBeInTheDocument()
    await closeTop()
    expect(screen.queryByText('the plan')).toBeNull()
    // Back on the controls, with the actions reachable again.
    expect(screen.getByRole('button', { name: 'Launch' })).toBeInTheDocument()
    await open('Notes')
    expect(await screen.findByText('the notes')).toBeInTheDocument()
  })

  test('escape closes the detail, not the whole stack', async () => {
    // Both sheets listen on the document; one keypress must not take both.
    const onClose = vi.fn()
    useAppStore.setState({ client: makeHub(status({ plan_html: '<p>the plan</p>' })) })
    render(
      <ControlsSheet chamberId="cham-a" chamberName="alpha" archived={false} onClose={onClose} />,
    )
    await screen.findByText('Stopped')
    await open('Plan')
    await screen.findByText('the plan')
    await userEvent.keyboard('{Escape}')
    expect(screen.queryByText('the plan')).toBeNull()
    expect(onClose).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Launch' })).toBeInTheDocument()
  })

  test('the Sync, Settings and Log sections are wired to this chamber', async () => {
    const hub = makeHub(
      status({
        log_tail: 'boot line one',
        has_config: true,
        settings_rows: [{ key: 'agent', value: 'claude', kind: 'scalar' }],
      }),
    )
    vi.spyOn(hub, 'chamberSync').mockResolvedValue([
      { backend: 'zulip', configured: true, installed: true, running: false, target: 'stream', last_pushed_session: null, log_tail_path: '' },
    ])
    useAppStore.setState({ client: hub })
    renderSheet()
    await screen.findByText('Stopped')
    await open('Log')
    expect(await screen.findByText(/boot line one/)).toBeInTheDocument()
    await open('Settings')
    expect(await screen.findByText('claude')).toBeInTheDocument()
    await open('Sync')
    expect(await screen.findByRole('button', { name: 'Start zulip sync' })).toBeInTheDocument()
    expect(hub.chamberSync).toHaveBeenCalledWith('cham-a')
  })

  test('an empty plan says which file is missing', async () => {
    useAppStore.setState({ client: makeHub(status({ plan_html: '' })) })
    renderSheet()
    await screen.findByText('Stopped')
    await open('Plan')
    expect(await screen.findByText('No plan.md in this chamber.')).toBeInTheDocument()
  })
})
