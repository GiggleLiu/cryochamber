/**
 * The agent commands the Console offers in its runner dropdowns, in the order
 * they appear. Mirrors the runners `resolve_agent` in `src/agent.rs` knows how
 * to launch, minus `mock`, which exists for the test harness rather than for a
 * chamber anyone would create.
 *
 * The hub accepts any parseable command, so this list is a shortcut, never a
 * limit — `agentOptions` keeps whatever is already saved alongside it.
 */
export const AGENT_CHOICES = ['pi', 'opencode', 'claude', 'codex', 'kimi'] as const

/**
 * The dropdown's options for a field currently holding `current`.
 *
 * A hand-written command (`pi --thinking high`, a path to a custom runner) is
 * legitimate and must stay selectable: a dropdown that could not represent the
 * saved value would silently rewrite it the first time anything else changed.
 */
export function agentOptions(current: string): string[] {
  const value = current.trim()
  const known: string[] = [...AGENT_CHOICES]
  if (value === '' || known.includes(value)) return known
  return [value, ...known]
}
