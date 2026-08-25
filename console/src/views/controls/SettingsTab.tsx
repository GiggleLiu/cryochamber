import { useState } from 'react'
import type { ChamberStatus } from '../../api/hubClient'
import { AgentSelect } from '../../components/AgentSelect'
import { useAppStore } from '../../store/appStore'
import { isUnauthorized } from '../../api/types'

/**
 * `cryo.toml` — the runner is editable, the rest is read-only and masked.
 *
 * The hub never sends the raw file (it can hold a provider API key), so
 * `has_config` plus `settings_rows` is all there is to read, and an unparseable
 * file is reported rather than echoed. The agent is the one exception: it is
 * the setting an operator changes often enough that "go edit a file on the
 * host" is the wrong answer, and it carries no secret. It is shown once — as
 * the dropdown — and filtered out of the read-only rows below.
 */
export function SettingsTab({
  status,
  chamberId,
  onAgentChanged,
}: {
  status: ChamberStatus
  chamberId: string
  /** Refetch the status so the rows below the dropdown catch up. */
  onAgentChanged: () => void
}) {
  const client = useAppStore((s) => s.client)
  // What `cryo.toml` says, which is what this dropdown writes back — never
  // `status.agent`, which is the CLI override when one is in force.
  const [agent, setAgent] = useState(status.config_agent)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState<{ restart: boolean; override: boolean } | null>(null)

  // The hub emits `status` every few seconds while a session runs, and the file
  // can also be edited on the host, so the dropdown has to follow it. Keyed on
  // the prop *changing* rather than on every render: the poll that lands
  // between a save and its refetch still carries the old runner, and re-reading
  // it unconditionally would snap the dropdown back to the value the operator
  // just replaced.
  const [seen, setSeen] = useState(status.config_agent)
  if (status.config_agent !== seen) {
    setSeen(status.config_agent)
    setAgent(status.config_agent)
  }

  /** Saves on change, and shows the chosen runner straight away: a select that
   * snapped back mid-request would read as the hub having refused it. A real
   * refusal restores the runner the chamber still has, next to the reason. */
  async function chooseAgent(next: string) {
    const previous = agent
    if (!client || busy || next === previous) return
    setAgent(next)
    setBusy(true)
    setError(null)
    setSaved(null)
    const stale = () => useAppStore.getState().client !== client
    try {
      const result = await client.setChamberAgent(chamberId, next)
      if (stale()) return
      setAgent(result.agent)
      setSaved({ restart: result.restart_required, override: result.override_active })
      onAgentChanged()
    } catch (e) {
      if (stale()) return
      setAgent(previous)
      if (isUnauthorized(e)) return
      setError(e instanceof Error ? e.message : 'Could not change the agent.')
    } finally {
      setBusy(false)
    }
  }

  if (!status.has_config) return <p className="tab-empty">No cryo.toml in this chamber.</p>
  const rows = status.settings_rows.filter((row) => row.key !== 'agent')
  return (
    <>
      <p className="group-label">Agent</p>
      <div className="group">
        <AgentSelect label="Agent" value={agent} disabled={!client || busy} onChange={chooseAgent} />
      </div>
      {error ? (
        <p className="group-hint" role="alert">
          {error}
        </p>
      ) : saved?.override ? (
        // The write landed, but it is not what runs: `cryo start --agent` left
        // an override in `timer.json` that outranks cryo.toml, and a restart
        // carries it along rather than dropping it.
        <p className="group-hint" role="status">
          Saved to <code>cryo.toml</code>, but this chamber was started with{' '}
          <code>cryo start --agent {status.agent}</code>, and that override wins until it is
          started again without the flag.
        </p>
      ) : saved?.restart ? (
        <p className="group-hint" role="status">
          Saved. Restart this chamber to run it — the daemon reads <code>cryo.toml</code> when it
          starts.
        </p>
      ) : (
        <p className="group-hint">
          Written to this chamber&apos;s <code>cryo.toml</code>. Saving rewrites that file, so
          comments and any keys Cryochamber does not recognise are not kept.
        </p>
      )}

      <p className="group-label">cryo.toml</p>
      {status.settings_rows.length === 0 ? (
        <p className="tab-empty">(could not parse cryo.toml — open it on disk)</p>
      ) : (
        <>
          <ul className="group">
            {rows.map((row) => (
              <li key={row.key}>
                <div className="row">
                  <span className="row-key">{row.key}</span>
                  {/* `kind` is the server's word on whether the value is a literal
                      (monospace) or a summarised section (prose). */}
                  <span className={`row-value is-wrap${row.kind === 'scalar' ? ' is-mono' : ''}`}>
                    {row.value}
                  </span>
                </div>
              </li>
            ))}
          </ul>
          <p className="group-hint">
            Read-only here. Edit <code>cryo.toml</code> in the chamber directory; any provider key
            it holds is never sent to this app.
          </p>
        </>
      )}
    </>
  )
}
