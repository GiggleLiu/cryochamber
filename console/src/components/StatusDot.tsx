/**
 * Chamber liveness, in one glance: green while the agent is working, amber
 * while the chamber is up but the agent sleeps between wakes, gray when the
 * chamber is not running at all.
 *
 * It renders nothing when the hub has not said — a hub that predates the
 * liveness fields must not have every chamber painted as stopped — and it is
 * the same dot everywhere the chamber is named, so liveness never requires
 * opening the controls sheet to read.
 */
export function StatusDot({
  running,
  agentRunning,
}: {
  running: boolean | undefined
  agentRunning: boolean | undefined
}) {
  if (running === undefined) return null
  const state = agentRunning ? ' is-awake' : running ? ' is-running' : ''
  const label = agentRunning
    ? 'agent working'
    : running
      ? 'chamber running, agent asleep'
      : 'chamber stopped'
  return <span className={`status-dot${state}`} role="img" aria-label={label} />
}
