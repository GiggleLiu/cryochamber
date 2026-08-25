import { render, screen } from '@testing-library/react'
import { StatusDot } from './StatusDot'

test.each([
  ['working', { running: true, agentRunning: true }, 'agent working', 'is-awake'],
  ['sleeping', { running: true, agentRunning: false }, 'chamber running, agent asleep', 'is-running'],
  ['stopped', { running: false, agentRunning: false }, 'chamber stopped', ''],
])('renders the known %s state', (_name, props, label, className) => {
  render(<StatusDot {...props} />)
  const dot = screen.getByLabelText(label)
  if (className) expect(dot).toHaveClass(className)
  else expect(dot).not.toHaveClass('is-awake', 'is-running', 'is-unknown')
})

test('renders an absent status as a hollow unknown dot', () => {
  render(<StatusDot />)
  expect(screen.getByLabelText('chamber status unknown')).toHaveClass('is-unknown')
})
