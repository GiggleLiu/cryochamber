import { agentOptions } from '../api/agents'

/**
 * The runner dropdown, shared by the host-wide default in Settings and by one
 * chamber's own `cryo.toml` in its controls sheet. Both pick from the same
 * list, and both save on change — there is one field, so a separate Save
 * button would only add a state the operator can leave half-done.
 */
export function AgentSelect({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string
  value: string
  disabled?: boolean
  onChange: (agent: string) => void
}) {
  return (
    <label className="row">
      {label}
      <select
        className="row-input is-select"
        aria-label={label}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      >
        {value === '' && <option value="">—</option>}
        {agentOptions(value).map((agent) => (
          <option key={agent} value={agent}>
            {agent}
          </option>
        ))}
      </select>
    </label>
  )
}
