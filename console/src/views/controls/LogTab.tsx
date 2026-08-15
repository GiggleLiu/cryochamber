import { useEffect, useRef, useState } from 'react'
import { subscribeChamberEvents } from '../../store/chamberEvents'

/** Client-side ceiling on the buffer. A long-running session emits far more
 * than this, and an unbounded <pre> is how a phone tab gets killed. */
export const LOG_MAX_LINES = 2000

/** How far from the bottom still counts as "watching the tail". */
const PIN_SLACK_PX = 30

export function LogTab({ chamberId, logTail }: { chamberId: string; logTail: string }) {
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

  if (lines.length === 0) return <p className="tab-empty">No log yet.</p>

  return (
    <pre className="log-pre" role="log" ref={preRef} onScroll={onScroll}>
      {lines.join('\n')}
    </pre>
  )
}
