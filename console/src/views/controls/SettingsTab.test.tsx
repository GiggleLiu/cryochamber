import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { vi } from 'vitest'
import { SettingsTab } from './SettingsTab'
import { HubClient, type ChamberStatus } from '../../api/hubClient'
import { useAppStore } from '../../store/appStore'
import { ApiError } from '../../api/types'

function status(overrides: Partial<ChamberStatus> = {}): ChamberStatus {
  return {
    running: false, agent_running: false, session: 1, agent: 'opencode', config_agent: 'opencode', log_tail: '',
    daily_digests: [], next_wake: null, notes_html: '', plan_html: '', plan_content: '',
    has_config: false, settings_rows: [], task: null, session_summary: null,
    completed: false, completion_summary: null, ...overrides,
  }
}

function renderTab(over: Partial<ChamberStatus> = {}, onAgentChanged = vi.fn()) {
  return render(
    <SettingsTab status={status(over)} chamberId="cham-a" onAgentChanged={onAgentChanged} />,
  )
}

/** An owner client whose only wired call is the agent write. */
function ownerHub() {
  const client = new HubClient({ token: 'k', fetch: vi.fn() })
  vi.spyOn(client, 'setChamberAgent').mockImplementation(async (_id, agent) => ({
    agent,
    restart_required: false,
    override_active: false,
  }))
  useAppStore.setState({ client })
  return client
}

test('scalar rows show key and value, with a monospace value', () => {
  const { container } = renderTab({
    has_config: true,
    settings_rows: [
      { key: 'agent', value: '"opencode"', kind: 'scalar' },
      { key: 'watch_dirs', value: '["messages/inbox"]', kind: 'scalar' },
      { key: 'provider', value: 'anthropic · env: ANTHROPIC_API_KEY', kind: 'section' },
    ],
  })
  expect(screen.getByText('anthropic · env: ANTHROPIC_API_KEY')).not.toHaveClass('is-mono')
  expect(screen.getByText('["messages/inbox"]')).toHaveClass('is-mono')
  // `agent` is shown once — as the dropdown — and filtered out of the
  // read-only rows, so the sheet never states it twice.
  expect(screen.queryByText('agent')).toBeNull()
  expect(screen.queryByText('"opencode"')).toBeNull()
  // Grouped rows, the same vocabulary as every other list in the app: the
  // dropdown plus the two surviving read-only rows.
  expect(container.querySelectorAll('.group .row')).toHaveLength(3)
  // The value is the thing being read here, so it wraps rather than clipping.
  expect(screen.getByText('["messages/inbox"]')).toHaveClass('is-wrap')
})

test('no cryo.toml at all says so, and offers nothing to change', () => {
  renderTab({ has_config: false, settings_rows: [] })
  expect(screen.getByText('No cryo.toml in this chamber.')).toBeInTheDocument()
  expect(screen.queryByRole('combobox', { name: 'Agent' })).not.toBeInTheDocument()
})

test('a present but unparseable cryo.toml is never echoed back', () => {
  renderTab({ has_config: true, settings_rows: [] })
  expect(screen.getByText('(could not parse cryo.toml — open it on disk)')).toBeInTheDocument()
  // Still editable: the runner is exactly what an operator would come here to
  // fix when the rest of the file cannot be read.
  expect(screen.getByRole('combobox', { name: 'Agent' })).toBeInTheDocument()
})

test('the dropdown offers the saved runner even when it is a hand-written command', () => {
  ownerHub()
  renderTab({ has_config: true, agent: 'pi --thinking high', config_agent: 'pi --thinking high' })
  const select = screen.getByRole('combobox', { name: 'Agent' })
  expect(select).toHaveValue('pi --thinking high')
  expect(
    Array.from(select.querySelectorAll('option')).map((o) => o.value),
  ).toEqual(['pi --thinking high', 'pi', 'opencode', 'claude', 'codex', 'kimi'])
})

test('choosing a runner writes it to this chamber and refetches the status', async () => {
  const client = ownerHub()
  const onAgentChanged = vi.fn()
  renderTab({ has_config: true, agent: 'opencode', config_agent: 'opencode' }, onAgentChanged)

  await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Agent' }), 'claude')

  expect(client.setChamberAgent).toHaveBeenCalledWith('cham-a', 'claude')
  await waitFor(() => expect(onAgentChanged).toHaveBeenCalled())
  expect(screen.getByRole('combobox', { name: 'Agent' })).toHaveValue('claude')
})

test('a running chamber is told the change needs a restart', async () => {
  const client = ownerHub()
  vi.mocked(client.setChamberAgent).mockResolvedValue({
    agent: 'claude',
    restart_required: true,
    override_active: false,
  })
  renderTab({ has_config: true, agent: 'opencode', running: true })

  await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Agent' }), 'claude')

  expect(await screen.findByRole('status')).toHaveTextContent('Restart this chamber')
})

test('a refused change shows the hub reason and restores the runner in force', async () => {
  const client = ownerHub()
  vi.mocked(client.setChamberAgent).mockRejectedValue(
    new ApiError(400, 'invalid agent command: Agent command is empty'),
  )
  renderTab({ has_config: true, agent: 'opencode' })

  await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Agent' }), 'claude')

  expect(await screen.findByRole('alert')).toHaveTextContent('invalid agent command')
  expect(screen.getByRole('combobox', { name: 'Agent' })).toHaveValue('opencode')
})

test('a CLI --agent override is named, because a restart will not shake it off', async () => {
  const client = ownerHub()
  vi.mocked(client.setChamberAgent).mockResolvedValue({
    agent: 'claude',
    restart_required: true,
    override_active: true,
  })
  // `agent` is the override in force; `config_agent` is what the file says and
  // what the dropdown edits.
  renderTab({ has_config: true, agent: 'codex', config_agent: 'opencode', running: true })

  const select = screen.getByRole('combobox', { name: 'Agent' })
  expect(select).toHaveValue('opencode')
  await userEvent.selectOptions(select, 'claude')

  const hint = await screen.findByRole('status')
  expect(hint).toHaveTextContent('cryo start --agent codex')
  expect(hint).toHaveTextContent('wins until it is started again without the flag')
})

test('a later status tick re-syncs the dropdown with the file', async () => {
  ownerHub()
  const { rerender } = render(
    <SettingsTab
      status={status({ has_config: true, agent: 'opencode', config_agent: 'opencode' })}
      chamberId="cham-a"
      onAgentChanged={vi.fn()}
    />,
  )
  expect(screen.getByRole('combobox', { name: 'Agent' })).toHaveValue('opencode')

  // Somebody edited cryo.toml on the host; the next poll carries the new value.
  rerender(
    <SettingsTab
      status={status({ has_config: true, agent: 'codex', config_agent: 'codex' })}
      chamberId="cham-a"
      onAgentChanged={vi.fn()}
    />,
  )
  expect(screen.getByRole('combobox', { name: 'Agent' })).toHaveValue('codex')
})

test('re-picking the runner already in force asks the hub for nothing', async () => {
  const client = ownerHub()
  renderTab({ has_config: true, agent: 'opencode', config_agent: 'opencode' })

  await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Agent' }), 'opencode')

  expect(client.setChamberAgent).not.toHaveBeenCalled()
})
