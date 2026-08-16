import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { DailyDigest } from '../../api/hubClient'
import { subscribeChamberEvents } from '../../store/chamberEvents'

/** Client-side ceiling on the buffer. A long-running session emits far more
 * than this, and an unbounded <pre> is how a phone tab gets killed. */
export const LOG_MAX_LINES = 2000

/** How far from the bottom still counts as "watching the tail". */
const PIN_SLACK_PX = 30

/**
 * The current session's number and what the agent said as it went to sleep,
 * as prose at the top of the log — the human-readable head of the machine
 * tail beneath it. It used to sit in the controls sheet's status card, where
 * it made the one screen the operator opens for the buttons scroll.
 *
 * The summary is clamped only when it is genuinely long — and then it says so
 * and opens on tap. The status payload carries no timestamp for it
 * (`session_summary` is parsed out of the current session's hibernate line),
 * so the caption is the caption alone; add the time here the day the hub
 * reports one.
 */
function SessionHead({ session, summary }: { session: number; summary: string | null }) {
  const [expanded, setExpanded] = useState(false)
  const [clamped, setClamped] = useState(false)
  const body = useRef<HTMLParagraphElement>(null)

  // Whether the clamp actually bit is a layout question, so it is asked of the
  // laid-out element rather than guessed from the character count. Deliberately
  // not re-asked on expand: once open, the box grows to fit and would report
  // "fits", taking the button that folds it back away.
  useLayoutEffect(() => {
    const el = body.current
    if (!el) return
    setClamped(el.scrollHeight > el.clientHeight + 1)
  }, [summary])

  return (
    <div className="last-session">
      <p className="last-session-caption">Session #{session}</p>
      {summary && (
        <>
          <p ref={body} className={`last-session-body${expanded ? ' is-expanded' : ''}`}>
            {summary}
          </p>
          {clamped && (
            <button
              className="last-session-more"
              aria-expanded={expanded}
              onClick={() => setExpanded((v) => !v)}
            >
              {expanded ? 'Show less' : 'Show more'}
            </button>
          )}
        </>
      )}
    </div>
  )
}

/**
 * Recent days as a table, because that is what it is: three aligned columns
 * the eye can scan down. The digests are derived from the same log that
 * scrolls below, which is why they live in this sheet and not the controls.
 */
function RecentDays({ digests }: { digests: DailyDigest[] }) {
  return (
    <table className="digest-table" aria-label="Recent days">
      <thead>
        <tr>
          <th scope="col">Day</th>
          <th scope="col">Sessions</th>
          <th scope="col">Failed</th>
        </tr>
      </thead>
      <tbody>
        {digests.map((d) => (
          <tr key={d.date}>
            <td className="digest-day">{d.date}</td>
            <td>{d.total_sessions}</td>
            {/* Only a real failure is coloured; a column of warm zeroes would
                make the one day that matters invisible. */}
            <td className={d.failed_sessions > 0 ? 'digest-failed' : undefined}>
              {d.failed_sessions}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

export function LogTab({
  chamberId,
  session,
  sessionSummary = null,
  digests = [],
  logTail,
}: {
  chamberId: string
  session: number
  sessionSummary?: string | null
  digests?: DailyDigest[]
  logTail: string
}) {
  const [lines, setLines] = useState<string[]>(() => (logTail ? logTail.split('\n') : []))
  const preRef = useRef<HTMLPreElement>(null)
  const pinnedRef = useRef(true)

  // A refreshed status carries a fresh tail that already contains whatever the
  // stream appended, so it replaces the buffer instead of being merged into it.
  useEffect(() => {
    setLines(logTail ? logTail.split('\n') : [])
  }, [logTail])

  useEffect(
    () =>
      subscribeChamberEvents(chamberId, (ev) => {
        if (ev.type !== 'log') return
        setLines((prev) => {
          const next = [...prev, ev.line]
          return next.length > LOG_MAX_LINES ? next.slice(next.length - LOG_MAX_LINES) : next
        })
      }),
    [chamberId],
  )

  useEffect(() => {
    const el = preRef.current
    if (el && pinnedRef.current) el.scrollTop = el.scrollHeight
  }, [lines])

  function onScroll() {
    const el = preRef.current
    if (!el) return
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= PIN_SLACK_PX
  }

  return (
    <>
      <SessionHead session={session} summary={sessionSummary} />
      {digests.length > 0 && (
        <>
          <p className="group-label">Recent days</p>
          <RecentDays digests={digests} />
        </>
      )}
      <p className="group-label">Log</p>
      {lines.length === 0 ? (
        <p className="tab-empty">No log yet.</p>
      ) : (
        <pre className="log-pre" role="log" ref={preRef} onScroll={onScroll}>
          {lines.join('\n')}
        </pre>
      )}
    </>
  )
}
