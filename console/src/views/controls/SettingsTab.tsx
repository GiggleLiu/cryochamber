import type { ChamberStatus } from '../../api/hubClient'

/**
 * `cryo.toml`, read-only and masked. The hub never sends the raw file (it can
 * hold a provider API key) — `has_config` plus `settings_rows` is all there is,
 * and an unparseable file is reported rather than echoed.
 *
 * Rendered as grouped rows like every other list in the app, and the value is
 * allowed to wrap: this sheet exists to be read, so a clipped `["messages/inbox"]`
 * would defeat the point of opening it.
 */
export function SettingsTab({ status }: { status: ChamberStatus }) {
  if (!status.has_config) return <p className="tab-empty">No cryo.toml in this chamber.</p>
  if (status.settings_rows.length === 0) {
    return <p className="tab-empty">(could not parse cryo.toml — open it on disk)</p>
  }
  return (
    <>
      <p className="group-label">cryo.toml</p>
      <ul className="group">
        {status.settings_rows.map((row) => (
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
        Read-only here. Edit <code>cryo.toml</code> in the chamber directory; any provider key it
        holds is never sent to this app.
      </p>
    </>
  )
}
