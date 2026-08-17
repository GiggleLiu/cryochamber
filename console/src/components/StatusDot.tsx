/**
 * Chamber liveness, in one glance: green while the agent is working, amber
 * while the chamber is up but the agent sleeps between wakes, gray when the
 * chamber is not running at all.
 *
 * Both flags are booleans by the time they get here — the client boundary maps
 * an absent flag to `false` — so there is no "unknown" state to draw. It is
 * the same dot everywhere the chamber is named, so liveness never requires
 * opening the controls sheet to read.
 */
export function StatusDot({ running, agentRunning }: { running: boolean; agentRunning: boolean }) {
  const state = agentRunning ? ' is-awake' : running ? ' is-running' : ''
  const label = agentRunning
    ? 'agent working'
    : running
      ? 'chamber running, agent asleep'
      : 'chamber stopped'
  return <span className={`status-dot${state}`} role="img" aria-label={label} />
}
