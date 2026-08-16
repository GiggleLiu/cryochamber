import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import {
  HubClient,
  type ChamberStatus,
  type DailyDigest,
  type LifecycleAction,
} from '../api/hubClient'
import { useAppStore } from '../store/appStore'
import { subscribeChamberEvents } from '../store/chamberEvents'
import { ApiError, isUnauthorized } from '../api/types'
import { Sheet } from '../components/Sheet'
import { AlertCircle } from '../components/Icon'
import { TodosTab } from './controls/TodosTab'
import { HtmlTab } from './controls/HtmlTab'
import { SyncTab } from './controls/SyncTab'
import { SettingsTab } from './controls/SettingsTab'
import { LogTab } from './controls/LogTab'

const SECTIONS = ['Todos', 'Plan', 'Notes', 'Sync', 'Settings', 'Log'] as const
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
 * What the agent said as it went to sleep, as prose under the status card.
 *
 * It used to be a right-aligned `.row` value: one monospace line that clipped
 * mid-sentence and pushed its own label onto a second line. A summary is a
 * sentence or three, so it gets the full width, wraps, and is clamped only
 * when it is genuinely long — and then it says so and opens on tap.
 *
 * The status payload carries no timestamp for it (`session_summary` is parsed
 * out of the current session's hibernate line), so the caption is the caption
 * alone; add the time here the day the hub reports one.
 */
function LastSession({ summary }: { summary: string }) {
  const [expanded, setExpanded] = useState(false)
  const [clamped, setClamped] = useState(false)
  const body = useRef<HTMLParagraphElement>(null)

  // Whether the clamp actually bit is a layout question, so it is asked of the
  // laid-out element rather than guessed from the character count. Deliberately
  // not re-asked on expand: once open, the box grows to fit and would report
  // "fits", taking the button that folds it back away.
  useLayoutEffect(() => {
    const el = body.current
    if (!el) return
    setClamped(el.scrollHeight > el.clientHeight + 1)
  }, [summary])

  return (
    <div className="last-session">
      <p className="last-session-caption">Last session</p>
      <p ref={body} className={`last-session-body${expanded ? ' is-expanded' : ''}`}>
        {summary}
      </p>
      {clamped && (
        <button
          className="last-session-more"
          aria-expanded={expanded}
          onClick={() => setExpanded((v) => !v)}
        >
          {expanded ? 'Show less' : 'Show more'}
        </button>
      )}
    </div>
  )
}

/**
 * Recent days as a table, because that is what it is: three aligned columns
 * the eye can scan down. As one sentence per day ("2026-08-15: 4 sessions, 1
 * failed") the failure count was buried at the end of a muted line.
 */
function RecentDays({ digests }: { digests: DailyDigest[] }) {
  return (
    <table className="digest-table" aria-label="Recent days">
      <thead>
        <tr>
          <th scope="col">Day</th>
          <th scope="col">Sessions</th>
          <th scope="col">Failed</th>
        </tr>
      </thead>
      <tbody>
        {digests.map((d) => (
          <tr key={d.date}>
            <td className="digest-day">{d.date}</td>
            <td>{d.total_sessions}</td>
            {/* Only a real failure is coloured; a column of warm zeroes would
                make the one day that matters invisible. */}
            <td className={d.failed_sessions > 0 ? 'digest-failed' : undefined}>
              {d.failed_sessions}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  )
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
            <div className="row">
              Session
              <span className="row-value">#{status.session}</span>
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
          {status.session_summary && <LastSession summary={status.session_summary} />}

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

          {status.daily_digests.length > 0 && (
            <>
              <p className="group-label">Recent days</p>
              <RecentDays digests={status.daily_digests} />
            </>
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
            <HtmlTab html={status.plan_html} empty="No plan.md in this chamber." />
          )}
          {section === 'Notes' && (
            <HtmlTab html={status.notes_html} empty="No NOTES.md in this chamber." />
          )}
          {section === 'Sync' && <SyncTab chamberId={chamberId} />}
          {section === 'Settings' && <SettingsTab status={status} />}
          {section === 'Log' && <LogTab chamberId={chamberId} logTail={status.log_tail} />}
        </Sheet>
      )}
    </Sheet>
  )
}
