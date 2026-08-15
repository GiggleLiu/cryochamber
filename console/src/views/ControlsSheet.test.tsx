import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ControlsSheet, digestLine, statePillLabel } from './ControlsSheet'
import { HubClient, type ChamberStatus } from '../api/hubClient'
import { useAppStore, resetAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'
import { emitChamberEvent } from '../store/chamberEvents'
import { ApiError } from '../api/errors'
import type { Credentials } from '../api/types'

const creds: Credentials = { kind: 'hub', prefix: '', email: 'Owner', apiKey: 'k', sendTopic: '' }

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
  const client = new HubClient(creds, vi.fn())
  vi.spyOn(client, 'chamberStatus').mockResolvedValue(s)
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

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({ creds, hubRole: 'owner', client: makeHub() })
})

test('the header names the chamber, its state, and its session', async () => {
  useAppStore.setState({
    client: makeHub(status({ running: true, agent_running: true, next_wake: 'in 2 h', session_summary: 'swept the decoders' })),
  })
  renderSheet()
  expect(await screen.findByText('Working')).toBeInTheDocument()
  expect(screen.getByRole('heading', { name: 'alpha' })).toBeInTheDocument()
  expect(screen.getByText('Session #7')).toBeInTheDocument()
  expect(screen.getByText('Next wake in 2 h')).toBeInTheDocument()
  expect(screen.getByText('swept the decoders')).toBeInTheDocument()
})

test('a stopped chamber hides the wake line and a completed plan is called out', async () => {
  useAppStore.setState({ client: makeHub(status({ running: false, next_wake: 'in 2 h', completed: true })) })
  renderSheet()
  expect(await screen.findByText('Stopped')).toBeInTheDocument()
  expect(screen.queryByText(/Next wake/)).toBeNull()
  expect(screen.getByText('✓ Plan complete')).toBeInTheDocument()
})

test('state pill wording covers every state', () => {
  expect(statePillLabel(status({ running: true, agent_running: true }), false)).toBe('Working')
  expect(statePillLabel(status({ running: true, agent_running: false }), false)).toBe('Asleep')
  expect(statePillLabel(status({ running: false }), false)).toBe('Stopped')
  // Archived wins: an archived chamber is put away whatever its runtime says.
  expect(statePillLabel(status({ running: true, agent_running: true }), true)).toBe('Archived')
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
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Launch' }))
    expect(hub.lifecycle).toHaveBeenCalledWith('cham-a', 'start')
    expect(await screen.findByRole('status')).toHaveTextContent('Started')
    // No optimistic UI: the pill only moves once the refetch answers.
    await waitFor(() => expect(hub.chamberStatus).toHaveBeenCalledTimes(2))
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
    vi.mocked(hub.lifecycle).mockResolvedValue({
      ok: false, message: 'Unarchive the chamber before launching it',
    })
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Launch' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Unarchive the chamber before launching it',
    )
    expect(screen.getByRole('button', { name: 'Launch' })).toBeEnabled()
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

  test('a 401 from an action signs out', async () => {
    const hub = useAppStore.getState().client as HubClient
    vi.mocked(hub.lifecycle).mockRejectedValue(new ApiError('HTTP 401', 401))
    renderSheet()
    await screen.findByText('Stopped')
    await userEvent.click(screen.getByRole('button', { name: 'Launch' }))
    await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
    expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
  })
})

test('a status event for this chamber re-reads the detail', async () => {
  const hub = useAppStore.getState().client as HubClient
  renderSheet()
  await screen.findByText('Stopped')
  vi.mocked(hub.chamberStatus).mockResolvedValue(status({ running: true, agent_running: true }))
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  expect(await screen.findByText('Working')).toBeInTheDocument()
})

test('a status event for another chamber is ignored', async () => {
  const hub = useAppStore.getState().client as HubClient
  renderSheet()
  await screen.findByText('Stopped')
  emitChamberEvent({ type: 'status', chamberId: 'cham-b' })
  await waitFor(() => expect(hub.chamberStatus).toHaveBeenCalledTimes(1))
})

test('daily digests render under the header', async () => {
  useAppStore.setState({
    client: makeHub(
      status({
        daily_digests: [
          { date: '2026-08-15', total_sessions: 4, failed_sessions: 1, latest_session: 7 },
          { date: '2026-08-14', total_sessions: 1, failed_sessions: 0, latest_session: 3 },
        ],
      }),
    ),
  })
  renderSheet()
  expect(await screen.findByText('2026-08-15: 4 sessions, 1 failed')).toBeInTheDocument()
  expect(screen.getByText('2026-08-14: 1 session, 0 failed')).toBeInTheDocument()
})

test('digestLine pluralises the session count', () => {
  expect(digestLine({ date: '2026-08-15', total_sessions: 1, failed_sessions: 0, latest_session: 1 }))
    .toBe('2026-08-15: 1 session, 0 failed')
  expect(digestLine({ date: '2026-08-15', total_sessions: 3, failed_sessions: 2, latest_session: 9 }))
    .toBe('2026-08-15: 3 sessions, 2 failed')
})

test('a failed status load stays inline in the sheet', async () => {
  const hub = useAppStore.getState().client as HubClient
  vi.mocked(hub.chamberStatus).mockRejectedValue(new ApiError('HTTP 500', 500))
  renderSheet()
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Could not load alpha. Check your connection and try again.',
  )
  expect(screen.getByRole('dialog', { name: 'Chamber controls' })).toBeInTheDocument()
})
