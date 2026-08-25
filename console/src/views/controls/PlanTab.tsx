import { useState } from 'react'
import type { ChamberStatus } from '../../api/hubClient'
import { useAppStore } from '../../store/appStore'
import { isUnauthorized } from '../../api/types'
import { HtmlTab } from './HtmlTab'

/**
 * `plan.md` — read as rendered markdown, edited as its source.
 *
 * The plan is the operator's brief and the one chamber file they are meant to
 * write, so it is editable here while `NOTES.md` (the agent's own memory) stays
 * read-only. No restart is involved: the agent is told to read `plan.md` at the
 * top of every session, so the next wake picks the new brief up.
 *
 * Editing opens on the source the last status fetch carried; last write wins.
 * A revision token would be the honest answer to two writers, but the other
 * writer is an agent instructed to keep its state in `NOTES.md`, and pretending
 * otherwise would cost the operator a conflict dialog they would never hit.
 */
export function PlanTab({
  status,
  chamberId,
  onSaved,
}: {
  status: ChamberStatus
  chamberId: string
  /** Refetch the status so the rendered plan catches up with the source. */
  onSaved: () => void
}) {
  const client = useAppStore((s) => s.client)
  const [draft, setDraft] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function save() {
    if (!client || busy || draft === null) return
    setBusy(true)
    setError(null)
    const stale = () => useAppStore.getState().client !== client
    try {
      await client.setChamberPlan(chamberId, draft)
      if (stale()) return
      setDraft(null)
      onSaved()
    } catch (e) {
      if (stale()) return
      if (isUnauthorized(e)) return
      setError(e instanceof Error ? e.message : 'Could not save the plan.')
    } finally {
      setBusy(false)
    }
  }

  if (draft === null) {
    return (
      <>
        {client && (
          <div className="group">
            <button className="row" onClick={() => setDraft(status.plan_content)}>
              Edit plan
              <span className="row-value" aria-hidden="true">
                markdown
              </span>
            </button>
          </div>
        )}
        <HtmlTab html={status.plan_html} empty="No plan.md in this chamber." />
      </>
    )
  }

  return (
    <>
      <textarea
        className="tab-editor"
        aria-label="Plan markdown"
        value={draft}
        disabled={busy}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
      />
      <div className="group">
        <button className="row" onClick={save} disabled={busy}>
          Save plan
          <span className="row-value" aria-hidden="true">
            {busy ? 'Saving…' : 'Applies on the next wake'}
          </span>
        </button>
        <button className="row" onClick={() => setDraft(null)} disabled={busy}>
          Cancel
        </button>
      </div>
      {error && (
        <p className="group-hint" role="alert">
          {error}
        </p>
      )}
    </>
  )
}
