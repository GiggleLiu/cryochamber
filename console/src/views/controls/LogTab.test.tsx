import { act, render, screen } from '@testing-library/react'
import { LogTab, LOG_MAX_LINES } from './LogTab'
import { resetAppStore } from '../../store/appStore'
import { emitChamberEvent } from '../../store/chamberEvents'

beforeEach(resetAppStore)

test('starts from the status log tail', () => {
  render(<LogTab chamberId="cham-a" logTail={'first line\nsecond line'} />)
  expect(screen.getByRole('log')).toHaveTextContent('first line')
  expect(screen.getByRole('log')).toHaveTextContent('second line')
})

test('appends live log lines for this chamber only', () => {
  render(<LogTab chamberId="cham-a" logTail="first line" />)
  act(() => {
    emitChamberEvent({ type: 'log', chamberId: 'cham-a', line: 'session 5 started' })
    emitChamberEvent({ type: 'log', chamberId: 'cham-b', line: 'somebody else' })
  })
  expect(screen.getByRole('log')).toHaveTextContent('session 5 started')
  expect(screen.getByRole('log')).not.toHaveTextContent('somebody else')
})

test('an empty log says so', () => {
  render(<LogTab chamberId="cham-a" logTail="" />)
  expect(screen.getByText('No log yet.')).toBeInTheDocument()
})

test('the buffer is capped, dropping the oldest lines', () => {
  render(<LogTab chamberId="cham-a" logTail="oldest" />)
  act(() => {
    for (let i = 0; i < LOG_MAX_LINES; i += 1) {
      emitChamberEvent({ type: 'log', chamberId: 'cham-a', line: `line ${i}` })
    }
  })
  const log = screen.getByRole('log')
  expect(log.textContent?.split('\n')).toHaveLength(LOG_MAX_LINES)
  expect(log).not.toHaveTextContent('oldest')
  expect(log).toHaveTextContent(`line ${LOG_MAX_LINES - 1}`)
})

test('stays pinned to the bottom while the reader is at the bottom', () => {
  render(<LogTab chamberId="cham-a" logTail="first" />)
  const log = screen.getByRole('log')
  Object.defineProperty(log, 'scrollHeight', { value: 500, configurable: true })
  Object.defineProperty(log, 'clientHeight', { value: 100, configurable: true })
  act(() => {
    emitChamberEvent({ type: 'log', chamberId: 'cham-a', line: 'next' })
  })
  expect(log.scrollTop).toBe(500)
})

test('a reader who scrolled up is not yanked back down', () => {
  render(<LogTab chamberId="cham-a" logTail="first" />)
  const log = screen.getByRole('log')
  Object.defineProperty(log, 'scrollHeight', { value: 500, configurable: true })
  Object.defineProperty(log, 'clientHeight', { value: 100, configurable: true })
  log.scrollTop = 0
  act(() => log.dispatchEvent(new Event('scroll', { bubbles: true })))
  act(() => {
    emitChamberEvent({ type: 'log', chamberId: 'cham-a', line: 'next' })
  })
  expect(log.scrollTop).toBe(0)
})

test('a fresh status tail replaces the buffer rather than duplicating it', () => {
  const { rerender } = render(<LogTab chamberId="cham-a" logTail="first" />)
  act(() => {
    emitChamberEvent({ type: 'log', chamberId: 'cham-a', line: 'second' })
  })
  rerender(<LogTab chamberId="cham-a" logTail={'first\nsecond'} />)
  expect(screen.getByRole('log').textContent).toBe('first\nsecond')
})
