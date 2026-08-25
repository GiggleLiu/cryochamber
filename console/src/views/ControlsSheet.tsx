import { useCallback, useEffect, useState } from 'react'
import { HubClient, type ChamberStatus, type LifecycleAction } from '../api/hubClient'
import { useAppStore } from '../store/appStore'
import { subscribeChamberEvents } from '../store/chamberEvents'
import { ApiError, isUnauthorized } from '../api/types'
import { Sheet } from '../components/Sheet'
import { AlertCircle } from '../components/Icon'
import { TodosTab } from './controls/TodosTab'
import { HtmlTab } from './controls/HtmlTab'
import { PlanTab } from './controls/PlanTab'
import { SettingsTab } from './controls/SettingsTab'
import { LogTab } from './controls/LogTab'

const SECTIONS = ['Todos', 'Plan', 'Notes', 'Settings', 'Log'] as const
export type ControlsSection = (typeof SECTIONS)[number]

/** What the hub said, when it said nothing: the panel's status words. */
const FALLBACK_MESSAGE: Record<LifecycleAction, string> = {
  start: 'started',
  stop: 'stopped',
  restart: 'restarted',
  reset: 'reset',
  archive: 'archived',
  unarchive: 'unarchived',
}

export function statePillLabel(
  status: ChamberStatus,
  archived: boolean,
): 'Working' | 'Asleep' | 'Stopped' | 'Archived' {
  // Archived is a state the operator chose explicitly, so it wins over whatever
  // the runtime is still reporting while it winds down.
  if (archived) return 'Archived'
  if (status.agent_running) return 'Working'
  if (status.running) return 'Asleep'
  return 'Stopped'
}

/**
 * Everything the legacy control panel could do to one chamber, in the
 * conversation the operator is already looking at.
 *
 * State comes from `GET /status` only — never optimistically from the button
 * that was pressed: a lifecycle action that half-succeeded must not leave the
 * UI claiming otherwise, so every action ends in a refetch.
 */
export function ControlsSheet({
  chamberId,
  chamberName,
  archived,
  onClose,
}: {
  chamberId: string
  chamberName: string
  /** From the chamber index; `GET /status` does not report it. */
  archived: boolean
  onClose: () => void
}) {
  const client = useAppStore((s) => s.client)
  const [status, setStatus] = useState<ChamberStatus | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  // Two error slots, not one. The hub emits `status` every few seconds while a
  // session runs, and each one refetches; a shared slot let a successful
  // refetch wipe the refusal an action had just reported, seconds after the
  // operator pressed the button and without them touching anything.
  const [loadError, setLoadError] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [pending, setPending] = useState(false)
  const [confirmReset, setConfirmReset] = useState(false)
  // The detail sheet on top of this one, if any. Its content mounts only while
  // it is open, so each section still fetches when first opened rather than on
  // every sheet open. `null` — where this starts — is the controls list
  // itself, which is what the sheet is opened for.
  const [section, setSection] = useState<ControlsSection | null>(null)
  const hub = client instanceof HubClient ? client : null

  const load = useCallback(async () => {
    if (!hub) return
    try {
      setStatus(await hub.chamberStatus(chamberId))
      setLoadError(null)
    } catch (e) {
      if (isUnauthorized(e)) return
      setLoadError(`Could not load ${chamberName}. Check your connection and try again.`)
    }
  }, [hub, chamberId, chamberName])

  useEffect(() => {
    void load()
  }, [load])

  // The hub already runs one event stream for the whole app; this sheet just
  // listens to the slice about its own chamber.
  useEffect(
    () =>
      subscribeChamberEvents(chamberId, (ev) => {
        if (ev.type === 'status') void load()
      }),
    [chamberId, load],
  )

  async function act(action: LifecycleAction) {
    if (!hub || pending) return
    setPending(true)
    setNotice(null)
    setActionError(null)
    setConfirmReset(false)
    try {
      const result = await hub.lifecycle(chamberId, action)
      // The refetch comes first and the verdict after it: no optimistic UI, so
      // the pill only moves once `GET /status` says it moved.
      await load()
      setNotice(result.message || FALLBACK_MESSAGE[action])
    } catch (e) {
      if (isUnauthorized(e)) return
      // A refusal reaches here too now (the client throws on `{ok:false}`), so
      // the pill must still be refreshed before the hub's own words are shown.
      await load().catch(() => {})
      // Only words the hub actually sent are worth showing; a synthesized
      // `HTTP 502` tells the operator nothing they can act on.
      setActionError(
        e instanceof ApiError && e.hubSaid
          ? e.message
          : `Could not ${action} ${chamberName}. Check your connection and try again.`,
      )
    } finally {
      setPending(false)
    }
  }

  const pill = status ? statePillLabel(status, archived) : null
  // One alert line: what the operator just did outranks a stale load failure.
  const alert = actionError ?? loadError

  return (
    <Sheet title={chamberName} label="Chamber controls" onClose={onClose}>
      {alert && (
        <p className="alert" role="alert">
          <AlertCircle size={18} />
          <span className="alert-body">{alert}</span>
        </p>
      )}
      {notice && (
        <p className="sheet-toast" role="status">
          {notice}
        </p>
      )}

      {status === null ? (
        // Nothing to say twice: a failed load has already said it in the alert.
        loadError ? null : <p className="tab-empty">Loading…</p>
      ) : (
        <>
          <p className="group-label">Status</p>
          <div className="group">
            <div className="row">
              State
              <span className={`row-value state-${pill!.toLowerCase()}`}>{pill}</span>
            </div>
            {/* Only a started chamber has a real schedule; a stopped one reports
                whatever was pending when it died, which reads as nonsense. */}
            {status.running && status.next_wake && (
              <div className="row">
                Next wake
                <span className="row-value">{status.next_wake}</span>
              </div>
            )}
            {status.completed && (
              <div className="row">
                Plan
                <span className="row-value">✓ complete</span>
              </div>
            )}
          </div>

          <p className="group-label">Actions</p>
          <div className="group">
            {archived ? (
              <button className="row" disabled={pending} onClick={() => act('unarchive')}>
                Unarchive
              </button>
            ) : (
              <>
                {status.running ? (
                  <>
                    <button className="row" disabled={pending} onClick={() => act('stop')}>
                      Stop
                    </button>
                    <button className="row" disabled={pending} onClick={() => act('restart')}>
                      Restart
                    </button>
                  </>
                ) : (
                  <>
                    <button className="row" disabled={pending} onClick={() => act('start')}>
                      Launch
                    </button>
                    <button className="row" disabled={pending} onClick={() => act('archive')}>
                      Archive
                    </button>
                  </>
                )}
                {/* Destructive and irreversible, so it asks — inline, not a
                    window.confirm, which a phone renders as a browser chrome
                    dialog over the app. */}
                <button
                  className="row row-danger"
                  disabled={pending}
                  onClick={() => setConfirmReset(true)}
                >
                  Reset…
                </button>
              </>
            )}
          </div>

          {confirmReset && (
            <div className="group">
              <div className="row confirm-row">
                <span className="confirm-question">
                  Reset {chamberName}? The session state and log are archived and the chamber
                  starts fresh.
                </span>
                <button className="row-action" onClick={() => setConfirmReset(false)}>
                  Cancel
                </button>
                <button
                  className="row-action row-action-danger"
                  disabled={pending}
                  onClick={() => act('reset')}
                >
                  Reset {chamberName}
                </button>
              </div>
            </div>
          )}

          <p className="group-label">Detail</p>
          <div className="group">
            {SECTIONS.map((name) => (
              <button key={name} className="row row-nav" onClick={() => setSection(name)}>
                {name}
                <span className="row-chevron" aria-hidden="true">
                  ›
                </span>
              </button>
            ))}
          </div>
        </>
      )}

      {/* Read in a surface of its own. A plan, a log or a parsed cryo.toml is a
          document, and inlining six of them turned this sheet into a page you
          had to scroll past to reach anything. A sheet — not a floating panel —
          because on a phone a popover over a sheet is a third layer that
          cannot scroll properly, and this one gets its own title and scroll. */}
      {section !== null && status !== null && (
        <Sheet title={section} label={`${chamberName} ${section}`} onClose={() => setSection(null)}>
          {section === 'Todos' && <TodosTab chamberId={chamberId} />}
          {section === 'Plan' && (
            <PlanTab status={status} chamberId={chamberId} onSaved={load} />
          )}
          {section === 'Notes' && (
            <HtmlTab html={status.notes_html} empty="No NOTES.md in this chamber." />
          )}
          {section === 'Settings' && (
            <SettingsTab status={status} chamberId={chamberId} onAgentChanged={load} />
          )}
          {section === 'Log' && (
            <LogTab
              chamberId={chamberId}
              session={status.session}
              sessionSummary={status.session_summary}
              digests={status.daily_digests}
              logTail={status.log_tail}
            />
          )}
        </Sheet>
      )}
    </Sheet>
  )
}
