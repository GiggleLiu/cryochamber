import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ProjectsView } from './ProjectsView'
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

test('hidden streams are filtered out', () => {
  useAppStore.setState({ hiddenStreams: [2] })
  render(<ProjectsView />)
  expect(screen.queryByText('beta')).toBeNull()
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
    render(<ProjectsView />)
    expect(screen.queryByText(/^Completed/)).toBeNull()
    expect(screen.queryByText(/^Archived/)).toBeNull()
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
