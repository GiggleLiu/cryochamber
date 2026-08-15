import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ProjectsView, foldedLabel } from './ProjectsView'
import { useAppStore, resetAppStore } from '../store/appStore'

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({
    streams: [
      { stream_id: 1, name: 'alpha', description: 'Project A' },
      { stream_id: 2, name: 'beta', description: 'Project B' },
    ],
    unreadByStream: { 1: [10, 11] },
    connection: 'live',
  })
})

test('renders visible streams with unread badges', () => {
  render(<ProjectsView />)
  expect(screen.getByText('alpha')).toBeInTheDocument()
  expect(screen.getByText('Project A')).toBeInTheDocument()
  expect(screen.getByText('2')).toBeInTheDocument() // unread badge
})

test('tapping a card navigates to the conversation', async () => {
  render(<ProjectsView />)
  await userEvent.click(screen.getByRole('button', { name: /alpha/ }))
  expect(useAppStore.getState().view).toEqual({ name: 'conversation', streamId: 1 })
})

test('gear opens settings', async () => {
  render(<ProjectsView />)
  await userEvent.click(screen.getByRole('button', { name: /settings/i }))
  expect(useAppStore.getState().settingsOpen).toBe(true)
})

test('empty state message when nothing visible', () => {
  useAppStore.setState({ streams: [] })
  render(<ProjectsView />)
  expect(screen.getByText(/no projects/i)).toBeInTheDocument()
})

test('skeleton rows stand in for the list until the first register lands', () => {
  useAppStore.setState({ streams: [], connection: 'connecting' })
  const { container } = render(<ProjectsView />)
  expect(container.querySelectorAll('.skeleton-row').length).toBeGreaterThan(0)
  expect(screen.queryByText(/no projects/i)).toBeNull()
})

describe('agent status dots', () => {
  test('one dot per project the hub reported on, labelled by state', () => {
    useAppStore.setState({
      streams: [
        { stream_id: 1, name: 'alpha', description: 'A', running: true, agentRunning: true },
        { stream_id: 2, name: 'beta', description: 'B', running: true, agentRunning: false },
        { stream_id: 3, name: 'gamma', description: 'C', running: false, agentRunning: false },
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

  test('a project whose liveness is unknown carries no dot at all', () => {
    const { container } = render(<ProjectsView />)
    expect(container.querySelector('.status-dot')).toBeNull()
  })

  test('the dot reads before the name, not on the tile', () => {
    // Liveness answered at a glance is the whole point: it must sit on the
    // name line the eye already reads, ahead of the name itself.
    useAppStore.setState({
      streams: [{ stream_id: 1, name: 'alpha', description: 'A', running: true }],
    })
    const { container } = render(<ProjectsView />)
    const head = container.querySelector('.stream-head')
    expect(head?.firstElementChild).toHaveClass('status-dot')
    expect(head?.children[1]).toHaveTextContent('alpha')
  })
})

test('a cached last message replaces the description as the row preview', () => {
  useAppStore.setState({
    messagesByStream: {
      1: [
        {
          id: 5, sender_full_name: 'Agent', sender_email: 'bot@b.c',
          timestamp: 1755100000, content: '<p>Sweep <strong>finished</strong>.</p>',
          stream_id: 1, subject: '',
        },
      ],
    },
  })
  render(<ProjectsView />)
  expect(screen.getByText('Sweep finished.')).toBeInTheDocument()
  expect(screen.queryByText('Project A')).toBeNull()
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
})

describe('groups, badge and meta line', () => {
  const MIXED = [
    { stream_id: 1, name: 'alpha', description: 'A', running: true, agentRunning: true },
    { stream_id: 2, name: 'beta', description: 'B', completed: true },
    { stream_id: 3, name: 'gamma', description: 'C', archived: true },
  ]

  test('completed and archived chambers are hidden until the owner asks for them', () => {
    useAppStore.setState({ streams: MIXED, hubRole: 'owner', showCompletedArchived: false })
    render(<ProjectsView />)
    expect(screen.getByText('alpha')).toBeInTheDocument()
    expect(screen.queryByText('beta')).toBeNull()
    expect(screen.queryByText('gamma')).toBeNull()
  })

  test('with the toggle on they appear as their own collapsed groups', () => {
    useAppStore.setState({ streams: MIXED, hubRole: 'owner', showCompletedArchived: true })
    const { container } = render(<ProjectsView />)
    expect(screen.getByText('Completed (1)')).toBeInTheDocument()
    expect(screen.getByText('Archived (1)')).toBeInTheDocument()
    expect(screen.getByText('beta')).toBeInTheDocument()
    // Collapsed by default: the fold exists and is closed.
    expect(container.querySelectorAll('details.stream-group')).toHaveLength(2)
    expect(container.querySelector('details.stream-group')?.hasAttribute('open')).toBe(false)
  })

  test('a guest never sees the groups even with the flag set', () => {
    useAppStore.setState({ streams: MIXED, hubRole: 'invite', showCompletedArchived: true })
    const { container } = render(<ProjectsView />)
    expect(screen.queryByText(/^Completed/)).toBeNull()
    expect(screen.queryByText(/^Archived/)).toBeNull()
    expect(container.querySelector('details.stream-group')).toBeNull()
  })

  test('a guest still sees their completed and archived chambers as ordinary rows', () => {
    useAppStore.setState({ streams: MIXED, hubRole: 'invite', showCompletedArchived: false })
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
      streams: [{ stream_id: 1, name: 'alpha', description: 'A', hasOpenQuestion: true }],
    })
    render(<ProjectsView />)
    const badge = screen.getByTitle('Open question — agent is waiting on you')
    expect(badge).toHaveTextContent('?')
  })

  test('a running chamber shows its next wake; a stopped one does not', () => {
    useAppStore.setState({
      streams: [
        { stream_id: 1, name: 'alpha', description: 'A', running: true, nextWake: 'in 2 h' },
        { stream_id: 2, name: 'beta', description: 'B', running: false, nextWake: 'in 2 h' },
      ],
    })
    render(<ProjectsView />)
    expect(screen.getByText('next wake in 2 h')).toBeInTheDocument()
    expect(screen.getAllByText(/next wake/)).toHaveLength(1)
  })
})

describe('the folded chambers are always accounted for', () => {
  const MIXED_ACTIVE = [
    { stream_id: 1, name: 'alpha', description: 'A' },
    { stream_id: 2, name: 'beta', description: 'B', completed: true },
    { stream_id: 3, name: 'gamma', description: 'C', archived: true },
  ]

  test('a reveal row counts what the toggle is hiding', () => {
    // The old empty-state hint only fired when nothing active was left, so a
    // single active chamber was enough to make a completed one look lost.
    useAppStore.setState({
      streams: MIXED_ACTIVE,
      hubRole: 'owner',
      showCompletedArchived: false,
    })
    render(<ProjectsView />)
    expect(screen.getByRole('button', { name: /1 completed · 1 archived/ })).toBeInTheDocument()
  })

  test('tapping it unfolds them in place', async () => {
    useAppStore.setState({
      streams: MIXED_ACTIVE,
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
      streams: MIXED_ACTIVE,
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

test('an owner whose chambers are all put away is told where they went', () => {
  useAppStore.setState({
    hubRole: 'owner',
    showCompletedArchived: false,
    streams: [
      { stream_id: 1, name: 'done', description: '', completed: true },
      { stream_id: 2, name: 'old', description: '', archived: true },
    ],
  })
  render(<ProjectsView />)
  expect(screen.getByRole('heading', { name: 'No active projects' })).toBeInTheDocument()
  expect(screen.getByText(/2 completed or archived/)).toBeInTheDocument()
  expect(screen.queryByText('No projects yet')).toBeNull()
})
