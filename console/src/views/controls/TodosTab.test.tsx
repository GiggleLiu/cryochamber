import { render, screen, waitFor } from '@testing-library/react'
import { TodosTab, sortTodos } from './TodosTab'
import { HubClient, type TodoItem } from '../../api/hubClient'
import { useAppStore, resetAppStore, AUTH_LOGOUT_REASON } from '../../store/appStore'
import { emitChamberEvent } from '../../store/chamberEvents'
import { ApiError } from '../../api/errors'
import type { Credentials } from '../../api/types'

const creds: Credentials = { kind: 'hub', prefix: '', email: 'Owner', apiKey: 'k', sendTopic: '' }

function todo(id: number, overrides: Partial<TodoItem> = {}): TodoItem {
  return {
    id, text: `task-${id}`, done: false, claimed: false,
    at: '2026-08-15T18:00', created: '2026-08-14T09:00', ...overrides,
  }
}

function makeHub(items: TodoItem[]): HubClient {
  const client = new HubClient(creds, vi.fn())
  vi.spyOn(client, 'chamberTodos').mockResolvedValue(items)
  return client
}

beforeEach(() => {
  resetAppStore()
  useAppStore.setState({ creds, hubRole: 'owner' })
})

test('pending items come first, with their scheduled time', async () => {
  useAppStore.setState({
    client: makeHub([
      todo(1, { done: true, text: 'shipped it' }),
      todo(2, { at: '2026-08-15T09:00', text: 'read the log' }),
    ]),
  })
  const { container } = render(<TodosTab chamberId="cham-a" />)
  await screen.findByText('read the log')
  // Scoped to the top-level list: the History fold holds a `.todo-list` of its
  // own, which an unscoped selector would sweep up along with it.
  const rows = container.querySelectorAll(':scope > .todo-list .todo-row')
  expect(rows[0]).toHaveTextContent('read the log')
  expect(rows[0]).toHaveTextContent('2026-08-15T09:00')
  // Done items live behind the History fold, not in the main list.
  expect(rows).toHaveLength(1)
  expect(screen.getByText('History (1)')).toBeInTheDocument()
  expect(screen.getByText('shipped it')).toBeInTheDocument()
})

test('no fold at all when nothing is done', async () => {
  useAppStore.setState({ client: makeHub([todo(1)]) })
  render(<TodosTab chamberId="cham-a" />)
  await screen.findByText('task-1')
  expect(screen.queryByText(/^History/)).toBeNull()
})

test('an empty todo list says so', async () => {
  useAppStore.setState({ client: makeHub([]) })
  render(<TodosTab chamberId="cham-a" />)
  expect(await screen.findByText('No todos in this chamber.')).toBeInTheDocument()
})

test('sortTodos puts dated pending items first, undated last, done newest first', () => {
  const { pending, done } = sortTodos([
    todo(1, { at: '' }),
    todo(2, { at: '2026-08-16T10:00' }),
    todo(3, { at: '2026-08-15T10:00' }),
    todo(4, { done: true }),
    todo(5, { done: true }),
  ])
  expect(pending.map((t) => t.id)).toEqual([3, 2, 1])
  expect(done.map((t) => t.id)).toEqual([5, 4])
})

test('a status event re-reads the todos', async () => {
  const hub = makeHub([todo(1)])
  useAppStore.setState({ client: hub })
  render(<TodosTab chamberId="cham-a" />)
  await screen.findByText('task-1')
  vi.mocked(hub.chamberTodos).mockResolvedValue([todo(1), todo(2)])
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  expect(await screen.findByText('task-2')).toBeInTheDocument()
})

test('a failed load stays inline', async () => {
  const hub = makeHub([])
  vi.mocked(hub.chamberTodos).mockRejectedValue(new ApiError('HTTP 500', 500))
  useAppStore.setState({ client: hub })
  render(<TodosTab chamberId="cham-a" />)
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Could not load todos. Check your connection and try again.',
  )
})

test('a 401 signs out', async () => {
  const hub = makeHub([])
  vi.mocked(hub.chamberTodos).mockRejectedValue(new ApiError('HTTP 401', 401))
  useAppStore.setState({ client: hub })
  render(<TodosTab chamberId="cham-a" />)
  await waitFor(() => expect(useAppStore.getState().creds).toBeNull())
  expect(useAppStore.getState().loginReason).toBe(AUTH_LOGOUT_REASON)
})

test('a failed refresh keeps the loaded list on screen beside the error', async () => {
  const hub = makeHub([todo(1)])
  useAppStore.setState({ client: hub })
  render(<TodosTab chamberId="cham-a" />)
  await screen.findByText('task-1')
  vi.mocked(hub.chamberTodos).mockRejectedValueOnce(new ApiError('HTTP 500', 500))
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  expect(await screen.findByRole('alert')).toHaveTextContent(/could not load todos/i)
  expect(screen.getByText('task-1')).toBeInTheDocument()
})
