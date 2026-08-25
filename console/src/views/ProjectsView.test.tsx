import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ProjectsView, foldedLabel } from './ProjectsView'
import { useAppStore, resetAppStore, type Connection } from '../store/appStore'
import { makeHubAccount } from '../store/hubs'
import type { Chamber, ChamberMessage } from '../api/types'

function chamber(id: string, name = id, extra: Partial<Chamber> = {}): Chamber {
  return {
    id,
    name,
    running: false,
    agentRunning: false,
    nextWakeDisplay: null,
    completed: false,
    archived: false,
    hasOpenQuestion: false,
    ...extra,
  }
}

function msg(n: number, chamberId = 'cham-a', sender = 'agent', body = `m${n}`): ChamberMessage {
  return {
    id: `outbox/${n}.md`,
    chamberId,
    direction: 'outbox',
    sender,
    subject: '',
    body,
    timestamp: `2026-08-15T10:0${n}:00`,
    session: null,
    isQuestion: false,
  }
}

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    chambers: [chamber('cham-a', 'alpha'), chamber('cham-b', 'beta')],
    selfName: 'me',
    connection: 'live',
  })
})

test('unread is what sits above the watermark, from anyone but us', () => {
  useAppStore.setState({
    messagesByChamber: { 'cham-a': [msg(1), msg(2, 'cham-a', 'me'), msg(3)] },
    lastReadByChamber: {},
  })
  render(<ProjectsView />)
  expect(screen.getByText('alpha')).toBeInTheDocument()
  expect(screen.getByLabelText('2 unread')).toHaveTextContent('2')
})

test('a watermark at the newest message clears the badge', () => {
  useAppStore.setState({
    messagesByChamber: { 'cham-a': [msg(1), msg(2)] },
    lastReadByChamber: { 'cham-a': '2026-08-15T10:02:00 outbox/2.md' },
  })
  render(<ProjectsView />)
  expect(screen.queryByLabelText(/unread/)).toBeNull()
})

test('tapping a card navigates to the conversation by chamber id', async () => {
  render(<ProjectsView />)
  await userEvent.click(screen.getByRole('button', { name: /alpha/ }))
  expect(useAppStore.getState().view).toEqual({ name: 'conversation', chamberId: 'cham-a' })
})

test('gear opens settings', async () => {
  render(<ProjectsView />)
  await userEvent.click(screen.getByRole('button', { name: /settings/i }))
  expect(useAppStore.getState().settingsOpen).toBe(true)
})

test('empty state message when nothing visible', () => {
  useAppStore.setState({ chambers: [] })
  render(<ProjectsView />)
  expect(screen.getByText(/no projects/i)).toBeInTheDocument()
})

test('skeleton rows stand in for the list until the first register lands', () => {
  useAppStore.setState({ chambers: [], connection: 'connecting' })
  const { container } = render(<ProjectsView />)
  expect(container.querySelectorAll('.skeleton-row').length).toBeGreaterThan(0)
  expect(screen.queryByText(/no projects/i)).toBeNull()
})

describe('agent status dots', () => {
  test('one dot per project, labelled by state', () => {
    useAppStore.setState({
      chambers: [
        chamber('cham-a', 'alpha', { running: true, agentRunning: true }),
        chamber('cham-b', 'beta', { running: true }),
        chamber('cham-c', 'gamma'),
      ],
    })
    const { container } = render(<ProjectsView />)
    expect(screen.getByLabelText('agent working')).toHaveClass('is-awake')
    expect(screen.getByLabelText('chamber running, agent asleep')).toHaveClass('is-running')
    const stopped = screen.getByLabelText('chamber stopped')
    expect(stopped).not.toHaveClass('is-awake')
    expect(stopped).not.toHaveClass('is-running')
    expect(container.querySelectorAll('.status-dot')).toHaveLength(3)
  })

  test('the dot reads before the name, not on the tile', () => {
    // Liveness answered at a glance is the whole point: it must sit on the
    // name line the eye already reads, ahead of the name itself.
    useAppStore.setState({ chambers: [chamber('cham-a', 'alpha', { running: true })] })
    const { container } = render(<ProjectsView />)
    const head = container.querySelector('.stream-head')
    expect(head?.firstElementChild).toHaveClass('status-dot')
    expect(head?.children[1]).toHaveTextContent('alpha')
  })
})

test('the last message is the row preview', () => {
  useAppStore.setState({
    messagesByChamber: {
      'cham-a': [msg(5, 'cham-a', 'agent', '<p>Sweep <strong>finished</strong>.</p>')],
    },
  })
  render(<ProjectsView />)
  expect(screen.getByText('Sweep finished.')).toBeInTheDocument()
})

describe('new chamber', () => {
  test('an owner gets a + button that opens the sheet', async () => {
    useAppStore.setState({ hubRole: 'owner' })
    render(<ProjectsView />)
    await userEvent.click(screen.getByRole('button', { name: 'New chamber' }))
    expect(await screen.findByRole('dialog', { name: 'New chamber' })).toBeInTheDocument()
  })

  test('a guest never sees the + button', () => {
    useAppStore.setState({ hubRole: 'invite' })
    render(<ProjectsView />)
    expect(screen.queryByRole('button', { name: 'New chamber' })).toBeNull()
  })

  test('a session whose role is unknown sees no + button', () => {
    render(<ProjectsView />)
    expect(screen.queryByRole('button', { name: 'New chamber' })).toBeNull()
  })

  test('in app mode, owning any hub is enough to reach the sheet', async () => {
    const hub = makeHubAccount({
      url: 'https://a.example', label: 'Alpha hub', token: 'ka', role: 'owner',
      trust: { kind: 'https' },
    })
    // App mode has no session-wide role: `hubRole` stays null and the answer
    // comes from the hubs this token owns.
    useAppStore.setState({
      mode: 'app', creds: null, hubs: [hub], roleByHub: { [hub.id]: 'owner' },
    })
    render(<ProjectsView />)
    await userEvent.click(screen.getByRole('button', { name: 'New chamber' }))
    expect(await screen.findByRole('dialog', { name: 'New chamber' })).toBeInTheDocument()
  })

  test('in app mode, a guest on every hub still sees no + button', () => {
    const hub = makeHubAccount({
      url: 'https://a.example', label: 'Alpha hub', token: 'ka', trust: { kind: 'https' },
    })
    useAppStore.setState({
      mode: 'app', creds: null, hubs: [hub], roleByHub: { [hub.id]: 'invite' },
    })
    render(<ProjectsView />)
    expect(screen.queryByRole('button', { name: 'New chamber' })).toBeNull()
  })
})

describe('groups, badge and meta line', () => {
  const MIXED = [
    chamber('cham-a', 'alpha', { running: true, agentRunning: true }),
    chamber('cham-b', 'beta', { completed: true }),
    chamber('cham-c', 'gamma', { archived: true }),
  ]

  test('completed and archived chambers are hidden until the owner asks for them', () => {
    useAppStore.setState({ chambers: MIXED, hubRole: 'owner', showCompletedArchived: false })
    render(<ProjectsView />)
    expect(screen.getByText('alpha')).toBeInTheDocument()
    expect(screen.queryByText('beta')).toBeNull()
    expect(screen.queryByText('gamma')).toBeNull()
  })

  test('with the toggle on they appear as their own collapsed groups', () => {
    useAppStore.setState({ chambers: MIXED, hubRole: 'owner', showCompletedArchived: true })
    const { container } = render(<ProjectsView />)
    expect(screen.getByText('Completed (1)')).toBeInTheDocument()
    expect(screen.getByText('Archived (1)')).toBeInTheDocument()
    expect(screen.getByText('beta')).toBeInTheDocument()
    // Collapsed by default: the fold exists and is closed.
    expect(container.querySelectorAll('details.stream-group')).toHaveLength(2)
    expect(container.querySelector('details.stream-group')?.hasAttribute('open')).toBe(false)
  })

  test('a guest never sees the groups even with the flag set', () => {
    useAppStore.setState({ chambers: MIXED, hubRole: 'invite', showCompletedArchived: true })
    const { container } = render(<ProjectsView />)
    expect(screen.queryByText(/^Completed/)).toBeNull()
    expect(screen.queryByText(/^Archived/)).toBeNull()
    expect(container.querySelector('details.stream-group')).toBeNull()
  })

  test('a guest still sees their completed and archived chambers as ordinary rows', () => {
    useAppStore.setState({ chambers: MIXED, hubRole: 'invite', showCompletedArchived: false })
    const { container } = render(<ProjectsView />)
    // The owner-only fold is not a filter for anyone else: a guest scoped to a
    // finished chamber would otherwise be left staring at an empty list.
    expect(screen.getByText('alpha')).toBeInTheDocument()
    expect(screen.getByText('beta')).toBeInTheDocument()
    expect(screen.getByText('gamma')).toBeInTheDocument()
    expect(container.querySelectorAll('ul.stream-list')).toHaveLength(1)
  })

  test('an open question is badged with an explanation', () => {
    useAppStore.setState({
      chambers: [chamber('cham-a', 'alpha', { hasOpenQuestion: true })],
    })
    render(<ProjectsView />)
    const badge = screen.getByTitle('Open question — agent is waiting on you')
    expect(badge).toHaveTextContent('?')
    expect(badge).toHaveAccessibleName('Open question — the agent is waiting on you')
  })

  test('a running chamber shows its next wake; a stopped one does not', () => {
    // A stopped chamber's `nextWakeDisplay` is already nulled at the client
    // boundary, so the list has nothing stale to print.
    useAppStore.setState({
      chambers: [
        chamber('cham-a', 'alpha', { running: true, nextWakeDisplay: 'in 2 h' }),
        chamber('cham-b', 'beta', { running: false, nextWakeDisplay: null }),
      ],
    })
    render(<ProjectsView />)
    expect(screen.getByText('next wake in 2 h')).toBeInTheDocument()
    expect(screen.getAllByText(/next wake/)).toHaveLength(1)
  })
})

describe('the folded chambers are always accounted for', () => {
  const MIXED_ACTIVE = [
    chamber('cham-a', 'alpha'),
    chamber('cham-b', 'beta', { completed: true }),
    chamber('cham-c', 'gamma', { archived: true }),
  ]

  test('a reveal row counts what the toggle is hiding', () => {
    // The old empty-state hint only fired when nothing active was left, so a
    // single active chamber was enough to make a completed one look lost.
    useAppStore.setState({
      chambers: MIXED_ACTIVE,
      hubRole: 'owner',
      showCompletedArchived: false,
    })
    render(<ProjectsView />)
    expect(screen.getByRole('button', { name: /1 completed · 1 archived/ })).toBeInTheDocument()
  })

  test('tapping it unfolds them in place', async () => {
    useAppStore.setState({
      chambers: MIXED_ACTIVE,
      hubRole: 'owner',
      showCompletedArchived: false,
    })
    render(<ProjectsView />)
    await userEvent.click(screen.getByRole('button', { name: /1 completed/ }))
    expect(useAppStore.getState().showCompletedArchived).toBe(true)
    expect(screen.getByText('Completed (1)')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /1 completed ·/ })).toBeNull()
  })

  test('a guest is never shown a fold they do not have', () => {
    useAppStore.setState({
      chambers: MIXED_ACTIVE,
      hubRole: 'invite',
      showCompletedArchived: false,
    })
    render(<ProjectsView />)
    expect(screen.queryByRole('button', { name: /completed/ })).toBeNull()
  })

  test('the row names only the kinds that exist', () => {
    expect(foldedLabel(2, 0)).toBe('2 completed')
    expect(foldedLabel(0, 3)).toBe('3 archived')
    expect(foldedLabel(2, 3)).toBe('2 completed · 3 archived')
  })
})

describe('hub chips in app mode', () => {
  const alpha = makeHubAccount({
    url: 'https://a.example',
    label: 'Alpha hub',
    token: 'ka',
    trust: { kind: 'https' },
  })
  const beta = makeHubAccount({
    url: 'https://b.example',
    label: 'Beta hub',
    token: 'kb',
    trust: { kind: 'https' },
  })

  /** Two hubs, one chamber each, keyed the way the router keys them. */
  function twoHubs(connectionByHub: Record<string, Connection>) {
    useAppStore.setState({
      mode: 'app',
      creds: null,
      hubs: [alpha, beta],
      connectionByHub,
      chambers: [
        chamber(`${alpha.id}:cham-a`, 'alpha', { hubId: alpha.id }),
        chamber(`${beta.id}:cham-b`, 'beta', { hubId: beta.id }),
      ],
    })
  }

  test('every row says which hub it lives on', () => {
    twoHubs({ [alpha.id]: 'live', [beta.id]: 'live' })
    render(<ProjectsView />)
    expect(screen.getByText('Alpha hub')).toBeInTheDocument()
    expect(screen.getByText('Beta hub')).toBeInTheDocument()
    expect(screen.queryByText(/unreachable/)).toBeNull()
  })

  test('a hub that is not live says so on its own rows only', () => {
    twoHubs({ [alpha.id]: 'live', [beta.id]: 'offline' })
    render(<ProjectsView />)
    expect(screen.getByText('Alpha hub')).toBeInTheDocument()
    expect(screen.getByText(/Beta hub · unreachable/)).toBeInTheDocument()
  })

  test('a chip that would always say the same thing is not drawn', () => {
    useAppStore.setState({
      mode: 'app',
      creds: null,
      hubs: [alpha],
      connectionByHub: { [alpha.id]: 'live' },
      chambers: [chamber(`${alpha.id}:cham-a`, 'alpha', { hubId: alpha.id })],
    })
    render(<ProjectsView />)
    expect(screen.queryByText('Alpha hub')).toBeNull()
  })

  test('browser mode has one hub and never chips a row', () => {
    render(<ProjectsView />)
    expect(document.querySelector('.hub-chip')).toBeNull()
  })
})

test('an owner whose chambers are all put away is told where they went', () => {
  useAppStore.setState({
    hubRole: 'owner',
    showCompletedArchived: false,
    chambers: [
      chamber('cham-a', 'done', { completed: true }),
      chamber('cham-b', 'old', { archived: true }),
    ],
  })
  render(<ProjectsView />)
  expect(screen.getByRole('heading', { name: 'No active projects' })).toBeInTheDocument()
  expect(screen.getByText(/2 completed or archived/)).toBeInTheDocument()
  expect(screen.queryByText('No projects yet')).toBeNull()
})
