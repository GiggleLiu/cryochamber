import { act, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { LogTab, LOG_MAX_LINES } from './LogTab'
import { resetAppStore } from '../../store/appStore'
import { emitChamberEvent } from '../../store/chamberEvents'

beforeEach(resetAppStore)

test('starts from the status log tail', () => {
  render(<LogTab chamberId="cham-a" session={7} logTail={'first line\nsecond line'} />)
  expect(screen.getByRole('log')).toHaveTextContent('first line')
  expect(screen.getByRole('log')).toHaveTextContent('second line')
})

test('appends live log lines for this chamber only', () => {
  render(<LogTab chamberId="cham-a" session={7} logTail="first line" />)
  act(() => {
    emitChamberEvent({ type: 'log', chamberId: 'cham-a', line: 'session 5 started' })
    emitChamberEvent({ type: 'log', chamberId: 'cham-b', line: 'somebody else' })
  })
  expect(screen.getByRole('log')).toHaveTextContent('session 5 started')
  expect(screen.getByRole('log')).not.toHaveTextContent('somebody else')
})

test('an empty log says so', () => {
  render(<LogTab chamberId="cham-a" session={7} logTail="" />)
  expect(screen.getByText('No log yet.')).toBeInTheDocument()
})

test('the buffer is capped, dropping the oldest lines', () => {
  render(<LogTab chamberId="cham-a" session={7} logTail="oldest" />)
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
  render(<LogTab chamberId="cham-a" session={7} logTail="first" />)
  const log = screen.getByRole('log')
  Object.defineProperty(log, 'scrollHeight', { value: 500, configurable: true })
  Object.defineProperty(log, 'clientHeight', { value: 100, configurable: true })
  act(() => {
    emitChamberEvent({ type: 'log', chamberId: 'cham-a', line: 'next' })
  })
  expect(log.scrollTop).toBe(500)
})

test('a reader who scrolled up is not yanked back down', () => {
  render(<LogTab chamberId="cham-a" session={7} logTail="first" />)
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
  const { rerender } = render(<LogTab chamberId="cham-a" session={7} logTail="first" />)
  act(() => {
    emitChamberEvent({ type: 'log', chamberId: 'cham-a', line: 'second' })
  })
  rerender(<LogTab chamberId="cham-a" session={7} logTail={'first\nsecond'} />)
  expect(screen.getByRole('log').textContent).toBe('first\nsecond')
})

test('the session number heads the log, with or without a summary', () => {
  const { rerender } = render(<LogTab chamberId="cham-a" session={7} logTail="" />)
  expect(screen.getByText('Session #7')).toBeInTheDocument()
  expect(screen.getByText('No log yet.')).toBeInTheDocument()
  rerender(<LogTab chamberId="cham-a" session={8} sessionSummary="swept the decoders" logTail="" />)
  expect(screen.getByText('Session #8')).toBeInTheDocument()
  // Prose, not a row value: a sentence squeezed into a value column truncated.
  const summary = screen.getByText('swept the decoders')
  expect(summary.closest('.row')).toBeNull()
  expect(summary).not.toHaveClass('row-value')
})

describe('last session summary', () => {
  const LONG = 'Swept the decoders and rewrote the notes. '.repeat(20).trim()

  /** jsdom lays nothing out, so the clamp has to be simulated: a body taller
   * than its box is exactly what `-webkit-line-clamp` produces in a browser. */
  function overflowing(yes: boolean) {
    const spy = vi
      .spyOn(HTMLElement.prototype, 'scrollHeight', 'get')
      .mockReturnValue(yes ? 400 : 100)
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(100)
    return spy
  }

  afterEach(() => {
    vi.restoreAllMocks()
  })

  test('a summary that fits is shown whole, with nothing to tap', () => {
    overflowing(false)
    render(<LogTab chamberId="cham-a" session={7} sessionSummary="swept the decoders" logTail="" />)
    const body = screen.getByText('swept the decoders')
    // Whole text, wrapping prose — no truncation marker of any kind.
    expect(body).toHaveTextContent('swept the decoders')
    expect(screen.queryByRole('button', { name: /show more/i })).toBeNull()
  })

  test('a summary too long for the clamp expands on tap and folds back', async () => {
    overflowing(true)
    render(<LogTab chamberId="cham-a" session={7} sessionSummary={LONG} logTail="" />)
    const body = screen.getByText(LONG)
    // Clamped, but complete: the DOM holds every word even while folded.
    expect(body.textContent).toBe(LONG)
    expect(body).not.toHaveClass('is-expanded')
    const more = screen.getByRole('button', { name: /show more/i })
    expect(more).toHaveAttribute('aria-expanded', 'false')
    await userEvent.click(more)
    expect(body).toHaveClass('is-expanded')
    const less = screen.getByRole('button', { name: /show less/i })
    expect(less).toHaveAttribute('aria-expanded', 'true')
    await userEvent.click(less)
    expect(body).not.toHaveClass('is-expanded')
  })
})

describe('recent days', () => {
  const twoDays = [
    { date: '2026-08-15', total_sessions: 4, failed_sessions: 1, latest_session: 7 },
    { date: '2026-08-14', total_sessions: 1, failed_sessions: 0, latest_session: 3 },
  ]

  test('daily digests render as a table, one row per day, in payload order', () => {
    render(<LogTab chamberId="cham-a" session={7} digests={twoDays} logTail="" />)
    const table = screen.getByRole('table', { name: 'Recent days' })
    expect(within(table).getAllByRole('columnheader').map((h) => h.textContent)).toEqual([
      'Day',
      'Sessions',
      'Failed',
    ])
    const rows = within(table).getAllByRole('row')
    // Header row plus one per day, newest first, exactly as the hub sent them.
    expect(rows).toHaveLength(3)
    expect(within(rows[1]).getAllByRole('cell').map((c) => c.textContent)).toEqual([
      '2026-08-15',
      '4',
      '1',
    ])
    expect(within(rows[2]).getAllByRole('cell').map((c) => c.textContent)).toEqual([
      '2026-08-14',
      '1',
      '0',
    ])
  })

  test('a day with failures marks the failed count, a clean day does not', () => {
    render(<LogTab chamberId="cham-a" session={7} digests={twoDays} logTail="" />)
    const rows = within(screen.getByRole('table', { name: 'Recent days' })).getAllByRole('row')
    expect(within(rows[1]).getAllByRole('cell')[2]).toHaveClass('digest-failed')
    expect(within(rows[2]).getAllByRole('cell')[2]).not.toHaveClass('digest-failed')
  })

  test('no digests, no section', () => {
    render(<LogTab chamberId="cham-a" session={7} logTail="" />)
    expect(screen.queryByRole('table')).toBeNull()
    expect(screen.queryByText('Recent days')).toBeNull()
  })
})
