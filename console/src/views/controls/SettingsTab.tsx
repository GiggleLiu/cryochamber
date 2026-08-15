import type { ChamberStatus } from '../../api/hubClient'

/**
 * `cryo.toml`, read-only and masked. The hub never sends the raw file (it can
 * hold a provider API key) — `has_config` plus `settings_rows` is all there is,
 * and an unparseable file is reported rather than echoed.
 */
export function SettingsTab({ status }: { status: ChamberStatus }) {
  if (!status.has_config) return <p className="tab-empty">No cryo.toml in this chamber.</p>
  if (status.settings_rows.length === 0) {
    return <p className="tab-empty">(could not parse cryo.toml — open it on disk)</p>
  }
  return (
    <ul className="settings-list">
      {status.settings_rows.map((row) => (
        <li className="settings-row" key={row.key}>
          <span className="settings-key">{row.key}</span>
          {/* `kind` is the server's word on whether the value is a literal
              (monospace) or a summarised section (prose). */}
          <span className={`settings-value${row.kind === 'scalar' ? ' is-mono' : ''}`}>
            {row.value}
          </span>
        </li>
      ))}
    </ul>
  )
}
