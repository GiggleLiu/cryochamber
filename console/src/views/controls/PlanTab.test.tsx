import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { vi } from 'vitest'
import { PlanTab } from './PlanTab'
import { HubClient, type ChamberStatus } from '../../api/hubClient'
import { useAppStore } from '../../store/appStore'
import { ApiError } from '../../api/types'

function status(overrides: Partial<ChamberStatus> = {}): ChamberStatus {
  return {
    running: false, agent_running: false, session: 1, agent: 'pi', config_agent: 'pi',
    log_tail: '', daily_digests: [], next_wake: null, notes_html: '',
    plan_html: '<h1>Brief</h1>', plan_content: '# Brief\n', has_config: true,
    settings_rows: [], task: null, session_summary: null,
    completed: false, completion_summary: null, ...overrides,
  }
}

function ownerHub() {
  const client = new HubClient({ token: 'k', fetch: vi.fn() })
  vi.spyOn(client, 'setChamberPlan').mockResolvedValue(undefined)
  useAppStore.setState({ client })
  return client
}

function renderTab(over: Partial<ChamberStatus> = {}, onSaved = vi.fn()) {
  return render(<PlanTab status={status(over)} chamberId="cham-a" onSaved={onSaved} />)
}

test('the plan is rendered, not editable, until Edit is pressed', () => {
  ownerHub()
  const { container } = renderTab()
  expect(container.querySelector('.tab-html')?.innerHTML).toContain('Brief')
  expect(screen.queryByRole('textbox', { name: 'Plan markdown' })).not.toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Edit plan' })).toBeInTheDocument()
})

test('an empty plan says which file is missing and still offers to write one', () => {
  ownerHub()
  renderTab({ plan_html: '', plan_content: '' })
  expect(screen.getByText('No plan.md in this chamber.')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Edit plan' })).toBeInTheDocument()
})

test('Edit opens the source — not the rendered HTML — and Save posts it', async () => {
  const client = ownerHub()
  const onSaved = vi.fn()
  renderTab({}, onSaved)

  await userEvent.click(screen.getByRole('button', { name: 'Edit plan' }))
  const editor = screen.getByRole('textbox', { name: 'Plan markdown' })
  expect(editor).toHaveValue('# Brief\n')

  await userEvent.clear(editor)
  await userEvent.type(editor, '# New brief')
  await userEvent.click(screen.getByRole('button', { name: 'Save plan' }))

  expect(client.setChamberPlan).toHaveBeenCalledWith('cham-a', '# New brief')
  // Back to the rendered view, and the sheet refetches so what is shown is the
  // hub's copy rather than the draft that was just sent.
  await waitFor(() => expect(onSaved).toHaveBeenCalled())
  expect(screen.queryByRole('textbox', { name: 'Plan markdown' })).not.toBeInTheDocument()
})

test('Cancel throws the draft away and writes nothing', async () => {
  const client = ownerHub()
  renderTab()

  await userEvent.click(screen.getByRole('button', { name: 'Edit plan' }))
  await userEvent.type(screen.getByRole('textbox', { name: 'Plan markdown' }), ' extra')
  await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))

  expect(client.setChamberPlan).not.toHaveBeenCalled()
  // Re-opening starts from the file again, not from the abandoned draft.
  await userEvent.click(screen.getByRole('button', { name: 'Edit plan' }))
  expect(screen.getByRole('textbox', { name: 'Plan markdown' })).toHaveValue('# Brief\n')
})

test('a rejected save keeps the editor open with the draft intact', async () => {
  const client = ownerHub()
  vi.mocked(client.setChamberPlan).mockRejectedValue(
    new ApiError(413, 'plan is 1048577 bytes; the limit is 1048576'),
  )
  const onSaved = vi.fn()
  renderTab({}, onSaved)

  await userEvent.click(screen.getByRole('button', { name: 'Edit plan' }))
  await userEvent.type(screen.getByRole('textbox', { name: 'Plan markdown' }), ' more')
  await userEvent.click(screen.getByRole('button', { name: 'Save plan' }))

  expect(await screen.findByRole('alert')).toHaveTextContent('the limit is 1048576')
  expect(screen.getByRole('textbox', { name: 'Plan markdown' })).toHaveValue('# Brief\n more')
  expect(onSaved).not.toHaveBeenCalled()
})

test('a guest client gets the plan to read and no way to change it', () => {
  useAppStore.setState({ client: null })
  renderTab()
  expect(screen.queryByRole('button', { name: 'Edit plan' })).not.toBeInTheDocument()
})
