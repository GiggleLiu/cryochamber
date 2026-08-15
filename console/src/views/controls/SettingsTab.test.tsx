import { render, screen } from '@testing-library/react'
import { SettingsTab } from './SettingsTab'
import type { ChamberStatus } from '../../api/hubClient'

function status(overrides: Partial<ChamberStatus> = {}): ChamberStatus {
  return {
    running: false, agent_running: false, session: 1, agent: 'opencode', log_tail: '',
    daily_digests: [], next_wake: null, notes_html: '', plan_html: '',
    has_config: false, settings_rows: [], task: null, session_summary: null,
    completed: false, completion_summary: null, ...overrides,
  }
}

test('scalar rows show key and value, with a monospace value', () => {
  const { container } = render(
    <SettingsTab
      status={status({
        has_config: true,
        settings_rows: [
          { key: 'agent', value: '"opencode"', kind: 'scalar' },
          { key: 'provider', value: 'anthropic · env: ANTHROPIC_API_KEY', kind: 'section' },
        ],
      })}
    />,
  )
  expect(screen.getByText('agent')).toBeInTheDocument()
  expect(screen.getByText('"opencode"')).toHaveClass('is-mono')
  expect(screen.getByText('anthropic · env: ANTHROPIC_API_KEY')).not.toHaveClass('is-mono')
  expect(container.querySelectorAll('.settings-row')).toHaveLength(2)
})

test('no cryo.toml at all says so', () => {
  render(<SettingsTab status={status({ has_config: false, settings_rows: [] })} />)
  expect(screen.getByText('No cryo.toml in this chamber.')).toBeInTheDocument()
})

test('a present but unparseable cryo.toml is never echoed back', () => {
  render(<SettingsTab status={status({ has_config: true, settings_rows: [] })} />)
  expect(
    screen.getByText('(could not parse cryo.toml — open it on disk)'),
  ).toBeInTheDocument()
})
