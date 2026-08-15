import { useCallback, useEffect, useState } from 'react'
import {
  HubClient,
  type ChamberStatus,
  type DailyDigest,
  type LifecycleAction,
} from '../api/hubClient'
import { useAppStore } from '../store/appStore'
import { subscribeChamberEvents } from '../store/chamberEvents'
import { logoutIfAuthError } from '../lib/authGuard'
import { Sheet } from '../components/Sheet'
import { AlertCircle } from '../components/Icon'
import { TodosTab } from './controls/TodosTab'
import { HtmlTab } from './controls/HtmlTab'

const TABS = ['Todos', 'Plan', 'Notes'] as const
export type ControlsTab = (typeof TABS)[number]

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

export function digestLine(digest: DailyDigest): string {
  const unit = digest.total_sessions === 1 ? 'session' : 'sessions'
  return `${digest.date}: ${digest.total_sessions} ${unit}, ${digest.failed_sessions} failed`
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
  const [tab, setTab] = useState<ControlsTab>('Todos')
  const hub = client instanceof HubClient ? client : null

  const load = useCallback(async () => {
    if (!hub) return
    try {
      setStatus(await hub.chamberStatus(chamberId))
      setLoadError(null)
    } catch (e) {
      if (logoutIfAuthError(e)) return
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
      const message = result.message || FALLBACK_MESSAGE[action]
      if (result.ok) setNotice(message)
      else setActionError(message)
    } catch (e) {
      if (logoutIfAuthError(e)) return
      setActionError(`Could not ${action} ${chamberName}. Check your connection and try again.`)
    } finally {
      setPending(false)
    }
  }

  const pill = status ? statePillLabel(status, archived) : null
  // One alert line: what the operator just did outranks a stale load failure.
  const alert = actionError ?? loadError

  return (
    <Sheet title="Controls" label="Chamber controls" onClose={onClose}>
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
          <div className="controls-head">
            <h3>{chamberName}</h3>
            <span className={`state-pill state-${pill!.toLowerCase()}`}>{pill}</span>
            <span className="controls-meta">Session #{status.session}</span>
            {/* Only a started chamber has a real schedule; a stopped one reports
                whatever was pending when it died, which reads as nonsense. */}
            {status.running && status.next_wake && (
              <span className="controls-meta">Next wake {status.next_wake}</span>
            )}
            {status.completed && <span className="controls-meta">✓ Plan complete</span>}
            {status.session_summary && (
              <span className="controls-meta">{status.session_summary}</span>
            )}
          </div>

          <div className="lifecycle">
            {archived ? (
              <button
                className="lifecycle-btn is-primary"
                disabled={pending}
                onClick={() => act('unarchive')}
              >
                Unarchive
              </button>
            ) : (
              <>
                {status.running ? (
                  <>
                    <button className="lifecycle-btn" disabled={pending} onClick={() => act('stop')}>
                      Stop
                    </button>
                    <button
                      className="lifecycle-btn"
                      disabled={pending}
                      onClick={() => act('restart')}
                    >
                      Restart
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      className="lifecycle-btn is-primary"
                      disabled={pending}
                      onClick={() => act('start')}
                    >
                      Launch
                    </button>
                    <button
                      className="lifecycle-btn"
                      disabled={pending}
                      onClick={() => act('archive')}
                    >
                      Archive
                    </button>
                  </>
                )}
                {/* Destructive and irreversible, so it asks — inline, not a
                    window.confirm, which a phone renders as a browser chrome
                    dialog over the app. */}
                <button
                  className="lifecycle-btn is-danger"
                  disabled={pending}
                  onClick={() => setConfirmReset(true)}
                >
                  Reset…
                </button>
              </>
            )}
          </div>

          {confirmReset && (
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
          )}

          {status.daily_digests.length > 0 && (
            <ul className="digest-list">
              {status.daily_digests.map((d) => (
                <li className="digest-row" key={d.date}>
                  {digestLine(d)}
                </li>
              ))}
            </ul>
          )}

          <div className="tabs" role="tablist" aria-label="Chamber detail">
            {TABS.map((name) => (
              <button
                key={name}
                role="tab"
                aria-selected={tab === name}
                className={`tab${tab === name ? ' is-on' : ''}`}
                onClick={() => setTab(name)}
              >
                {name}
              </button>
            ))}
          </div>

          {/* Only the selected tab is mounted, so each one's fetch happens when
              it is first opened rather than on every sheet open. */}
          {tab === 'Todos' && <TodosTab chamberId={chamberId} />}
          {tab === 'Plan' && (
            <HtmlTab html={status.plan_html} empty="No plan.md in this chamber." />
          )}
          {tab === 'Notes' && (
            <HtmlTab html={status.notes_html} empty="No NOTES.md in this chamber." />
          )}
        </>
      )}
    </Sheet>
  )
}
