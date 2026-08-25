/**
 * Chamber liveness, in one glance: green while the agent is working, amber
 * while the chamber is up but the agent sleeps between wakes, gray when the
 * chamber is not running at all.
 *
 * When neither flag has arrived, a hollow ring says the status is unknown
 * without making the stronger claim that the chamber stopped. It is the same
 * dot everywhere the chamber is named, so liveness never requires opening the
 * controls sheet to read.
 */
export function StatusDot({ running, agentRunning }: { running?: boolean; agentRunning?: boolean }) {
  const unknown = running === undefined && agentRunning === undefined
  const state = unknown ? ' is-unknown' : agentRunning ? ' is-awake' : running ? ' is-running' : ''
  const label = unknown
    ? 'chamber status unknown'
    : agentRunning
    ? 'agent working'
    : running
      ? 'chamber running, agent asleep'
      : 'chamber stopped'
  return <span className={`status-dot${state}`} role="img" aria-label={label} />
}
