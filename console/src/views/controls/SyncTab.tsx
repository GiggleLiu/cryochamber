import { useCallback, useEffect, useState } from 'react'
import { HubClient, type SyncSummary } from '../../api/hubClient'
import { useAppStore } from '../../store/appStore'
import { subscribeChamberEvents } from '../../store/chamberEvents'
import { ApiError } from '../../api/types'
import { logoutIfAuthError } from '../../lib/authGuard'
import { AlertCircle } from '../../components/Icon'

/** One card per message-sync backend, with the one control that matters. The
 * hub emits a `status` event after every action, which is what refreshes the
 * card once the daemon has actually settled. */
export function SyncTab({ chamberId }: { chamberId: string }) {
  const client = useAppStore((s) => s.client)
  const [items, setItems] = useState<SyncSummary[] | null>(null)
  const [pending, setPending] = useState<string | null>(null)
  // The tab's own error slots, never the sheet's: `loadError`/`actionError`
  // there belong to the status load and the lifecycle buttons, and a sync
  // failure must not overwrite either of them.
  //
  // Two slots, for the same reason the sheet has two: every action ends in a
  // refetch, and a shared slot let that refetch wipe the refusal the action
  // had just reported, milliseconds after the operator pressed the button.
  const [loadError, setLoadError] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const hub = client instanceof HubClient ? client : null

  const load = useCallback(async () => {
    if (!hub) return
    try {
      setItems(await hub.chamberSync(chamberId))
      setLoadError(null)
    } catch (e) {
      if (logoutIfAuthError(e)) return
      setLoadError('Could not load message sync. Check your connection and try again.')
    }
  }, [hub, chamberId])

  useEffect(() => {
    void load()
  }, [load])

  useEffect(
    () =>
      subscribeChamberEvents(chamberId, (ev) => {
        if (ev.type === 'status') void load()
      }),
    [chamberId, load],
  )

  async function toggle(item: SyncSummary) {
    if (!hub || pending) return
    setPending(item.backend)
    setActionError(null)
    try {
      await hub.syncAction(chamberId, item.backend, item.running ? 'stop' : 'start')
      // No optimistic flip: the card moves when the hub says the daemon did.
      await load()
    } catch (e) {
      if (logoutIfAuthError(e)) return
      // A refusal reaches here too now (the client throws on `{ok:false}`), so
      // the cards are refreshed before the hub's own words are shown.
      await load().catch(() => {})
      const fallback = `Could not change ${item.backend} sync. Check your connection and try again.`
      setActionError(e instanceof ApiError ? e.message || fallback : fallback)
    } finally {
      setPending(null)
    }
  }

  // One alert line: what the operator just did outranks a stale load failure.
  // And a failed *refresh* must not blank cards that are already on screen —
  // status events arrive every few seconds while a session runs, and one flaky
  // reply would otherwise flicker the whole panel into an error line and back.
  const message = actionError ?? loadError
  const alert = message ? (
    <p className="alert" role="alert">
      <AlertCircle size={18} />
      <span className="alert-body">{message}</span>
    </p>
  ) : null
  if (items === null) return alert ?? <p className="tab-empty">Loading…</p>
  if (items.length === 0) {
    return (
      <>
        {alert}
        <p className="tab-empty">No message sync is configured for this chamber.</p>
      </>
    )
  }

  return (
    <>
      {alert}
      <ul className="sync-list">
        {items.map((item) => (
          <li className="sync-card" key={item.backend}>
            <span className="sync-name">{item.backend}</span>
            <span className={`sync-badge${item.running ? '' : ' is-off'}`}>
              {item.running ? 'running' : 'off'}
            </span>
            <span className="sync-badge">{item.configured ? 'configured' : 'not configured'}</span>
            <button
              className="row-action"
              aria-label={`${item.running ? 'Stop' : 'Start'} ${item.backend} sync`}
              disabled={pending !== null}
              onClick={() => toggle(item)}
            >
              {item.running ? 'Stop' : 'Start'}
            </button>
            {item.target && <span className="sync-target">{item.target}</span>}
          </li>
        ))}
      </ul>
    </>
  )
}
