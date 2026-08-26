import { useState } from 'react'
import type { Chamber } from '../api/types'
import { hubIdOf, isOwnerFor, unreadCount, useAppStore } from '../store/appStore'
import { useOwnerHub } from '../hooks/useOwnerHub'
import { Gear, Inbox, Plus } from '../components/Icon'
import { initial, listTimeLabel, messageSeconds, previewText, tileColor } from '../lib/format'
import { splitChamberKey } from '../lib/hubKeys'
import { NewChamberSheet } from './NewChamberSheet'
import { StatusDot } from '../components/StatusDot'

/** How the folded chambers are counted on the reveal row. Naming both kinds
 * only when both exist keeps the row from claiming an archive that is empty. */
export function foldedLabel(completed: number, archived: number): string {
  const parts: string[] = []
  if (completed > 0) parts.push(`${completed} completed`)
  if (archived > 0) parts.push(`${archived} archived`)
  return parts.join(' · ')
}

/** One hub can be saved through several scoped links. If their scopes overlap,
 * show the chamber once; admin access wins because it carries the controls the
 * joined copy cannot offer. */
export function dedupeChambers(
  chambers: Chamber[],
  hubs: Array<{ id: string; url: string }>,
  roleByHub: Record<string, 'owner' | 'invite'>,
): Chamber[] {
  const byHub = new Map(hubs.map((h) => [h.id, h]))
  const visible = new Map<string, Chamber>()
  for (const chamber of chambers) {
    const { hubId, chamberId } = splitChamberKey(chamber.id)
    const hub = byHub.get(hubId)
    const key = hub ? `${hub.url}\n${chamberId}` : chamber.id
    const previous = visible.get(key)
    const previousHubId = previous ? splitChamberKey(previous.id).hubId : ''
    if (!previous || (roleByHub[hubId] === 'owner' && roleByHub[previousHubId] !== 'owner')) {
      visible.set(key, chamber)
    }
  }
  return [...visible.values()]
}

function SkeletonList() {
  return (
    <div className="stream-list" aria-hidden="true">
      {[0, 1, 2, 3].map((i) => (
        <div className="skeleton-row" key={i}>
          <div className="skeleton skeleton-tile" />
          <div className="skeleton-lines">
            <div className="skeleton" />
            <div className="skeleton" />
          </div>
        </div>
      ))}
    </div>
  )
}

export function ProjectsView() {
  const chambers = useAppStore((s) => s.chambers)
  // Selected once, at the top: `unreadCount` is derived per row inside `card`,
  // because a selector returning a fresh map would re-render on every store
  // write whether or not any count actually moved.
  const messagesByChamber = useAppStore((s) => s.messagesByChamber)
  const lastReadByChamber = useAppStore((s) => s.lastReadByChamber)
  const selfName = useAppStore((s) => s.selfName)
  const selfNameByHub = useAppStore((s) => s.selfNameByHub)
  const connection = useAppStore((s) => s.connection)
  const mode = useAppStore((s) => s.mode)
  const hubs = useAppStore((s) => s.hubs)
  const connectionByHub = useAppStore((s) => s.connectionByHub)
  const navigate = useAppStore((s) => s.navigate)
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen)
  // Per row, never per session: a token can own one hub and be a guest on the
  // next, so the fold is asked of the hub each chamber lives on. A session-wide
  // `useIsOwner()` answers false for every app-mode row (no `hubRole`), which
  // folded nothing while Settings still offered the switch.
  const hubRole = useAppStore((s) => s.hubRole)
  const roleByHub = useAppStore((s) => s.roleByHub)
  const ownerFor = (c: Chamber) => isOwnerFor({ hubRole, roleByHub }, c.id)
  // Creating a chamber is a hub-level act: owning *any* hub is what puts the +
  // button there, which is also what gates the fold switch in Settings.
  const canCreate = useOwnerHub().isOwner
  const [newChamberOpen, setNewChamberOpen] = useState(false)
  const showCompletedArchived = useAppStore((s) => s.showCompletedArchived)
  const setShowCompletedArchived = useAppStore((s) => s.setShowCompletedArchived)
  // Every chamber this token can reach. The Completed/Archived fold below is
  // the only filing there is — a second, per-project hide switch used to live
  // in Settings and could leave a guest staring at an empty list.
  const visible = mode === 'app' ? dedupeChambers(chambers, hubs, roleByHub) : chambers
  // The folds are an owner's filing system, never a filter on anyone else's
  // list: a guest scoped to a finished chamber still sees it as a plain row,
  // so their whole list can never disappear behind a preference they cannot
  // reach. Archived wins over completed: the operator put it away on purpose,
  // and one chamber must never appear in two groups.
  const owned = visible.filter(ownerFor)
  const joined = visible.filter((c) => !ownerFor(c))
  const archivedList = owned.filter((c) => c.archived)
  const completedList = owned.filter((c) => !c.archived && c.completed)
  const ownedActive = owned.filter((c) => !c.archived && !c.completed)
  const active = visible.filter((c) => !ownerFor(c) || (!c.archived && !c.completed))
  // The lists are already empty for anything this token does not own, so the
  // switch is the only remaining question.
  const showGroups = showCompletedArchived
  // Nothing to show yet *and* still connecting means the first register is in
  // flight — show the shape of the list rather than an empty state that would
  // be contradicted a moment later. Once offline, the empty state plus the
  // reconnecting banner is the honest report.
  const loading = chambers.length === 0 && connection === 'connecting'
  // Which hub a row lives on only means something once the app holds more than
  // one: a chip that says the same word on every row is noise, and browser mode
  // never has a second hub to name.
  const showHubs = mode === 'app' && hubs.length > 1

  function card(c: Chamber) {
    // `hubIdOf`, never `c.hubId`: a row hydrated from the cache carries its hub
    // in its key alone.
    const hub = showHubs ? hubs.find((h) => h.id === hubIdOf(c)) : undefined
    // Only a hub we have actually failed to reach is unreachable. "Connecting"
    // is a call still in flight, and saying it is down would be a verdict the
    // app has not got yet.
    const unreachable = hub ? connectionByHub[hub.id] === 'offline' : false
    const count = unreadCount(
      { messagesByChamber, lastReadByChamber, selfName, selfNameByHub },
      c.id,
    )
    const last = messagesByChamber[c.id]?.at(-1)
    const preview = last ? previewText(last.body) : ''
    return (
      <li key={c.id}>
        <button
          className="stream-card"
          onClick={() => navigate({ name: 'conversation', chamberId: c.id })}
        >
          <span className="stream-tile" style={{ background: tileColor(c.name) }}>
            {initial(c.name)}
          </span>
          <span className="stream-head">
            {/* Liveness reads before the name, the same way it does in the
                conversation header — the glance that used to need the controls
                sheet. */}
            <StatusDot running={c.running} agentRunning={c.agentRunning} />
            <span className="stream-name">{c.name}</span>
            {/* Which hub, and — when that hub is down — that everything on this
                row is the last thing it said rather than the current state. */}
            {hub && (
              <span className={`hub-chip${unreachable ? ' is-unreachable' : ''}`}>
                {hub.label}
                {unreachable && ' · unreachable'}
              </span>
            )}
            {c.hasOpenQuestion && (
              <span
                className="question-badge"
                title="Open question — agent is waiting on you"
                aria-label="Open question — the agent is waiting on you"
              >
                ?
              </span>
            )}
          </span>
          {last && <span className="stream-meta">{listTimeLabel(messageSeconds(last))}</span>}
          {count > 0 && (
            <span className="unread-badge" aria-label={`${count} unread`}>
              {count}
            </span>
          )}
          <span className="stream-desc">{preview}</span>
          {/* Only a started chamber has a real schedule; a stopped one reports
              whatever was pending when it died. */}
          {c.running && c.nextWakeDisplay && (
            <span className="stream-wake">next wake {c.nextWakeDisplay}</span>
          )}
        </button>
      </li>
    )
  }

  return (
    <div className="projects">
      <header className="topbar">
        <h1>Chambers</h1>
        <div className="topbar-actions">
          {canCreate && (
            <button
              className="icon-btn"
              aria-label="New chamber"
              onClick={() => setNewChamberOpen(true)}
            >
              <Plus />
            </button>
          )}
          <button className="icon-btn" aria-label="Settings" onClick={() => setSettingsOpen(true)}>
            <Gear />
          </button>
        </div>
      </header>

      <div className="projects-scroll">
        {loading && <SkeletonList />}

        {!loading && mode !== 'app' &&
          active.length === 0 &&
          (!showGroups || completedList.length + archivedList.length === 0) && (
            <div className="empty-state">
              <Inbox size={40} />
              {/* An owner whose chambers are all completed/archived with the
                  toggle off has projects — the empty state must say where. */}
              {!showGroups && completedList.length + archivedList.length > 0 ? (
                <>
                  <h2>No active projects</h2>
                  <p>
                    {completedList.length + archivedList.length} completed or archived — turn on
                    “Show completed &amp; archived” in Settings to see them.
                  </p>
                </>
              ) : (
                <>
                  <h2>No projects yet</h2>
                  <p>Every chamber this token can reach shows up here as a project.</p>
                </>
              )}
            </div>
          )}

        {!loading && mode === 'app' && visible.length === 0 && (
          <div className="empty-state">
            <Inbox size={40} />
            <h2>No chambers yet</h2>
            <p>Add an admin or invite link to see its chambers here.</p>
          </div>
        )}

        {!loading && mode === 'app' && visible.length > 0 && (
          <>
            <h2 className="stream-category">Owned ({owned.length})</h2>
            {ownedActive.length > 0 ? (
              <ul className="stream-list">{ownedActive.map(card)}</ul>
            ) : (
              <p className="stream-category-empty">No active owned chambers.</p>
            )}

            {!showGroups && completedList.length + archivedList.length > 0 && (
              <button className="stream-reveal" onClick={() => setShowCompletedArchived(true)}>
                Show {foldedLabel(completedList.length, archivedList.length)}
              </button>
            )}

            {showGroups && completedList.length > 0 && (
              <details className="stream-group">
                <summary>Completed ({completedList.length})</summary>
                <ul className="stream-list">{completedList.map(card)}</ul>
              </details>
            )}

            {showGroups && archivedList.length > 0 && (
              <details className="stream-group">
                <summary>Archived ({archivedList.length})</summary>
                <ul className="stream-list">{archivedList.map(card)}</ul>
              </details>
            )}

            <h2 className="stream-category">Joined ({joined.length})</h2>
            {joined.length > 0 ? (
              <ul className="stream-list">{joined.map(card)}</ul>
            ) : (
              <p className="stream-category-empty">No joined chambers.</p>
            )}
          </>
        )}

        {mode !== 'app' && active.length > 0 && <ul className="stream-list">{active.map(card)}</ul>}

        {/* With the toggle off, completed and archived chambers vanish from the
            list. Saying how many are folded away — and offering the one tap
            that unfolds them — is the difference between a filter and a
            chamber that looks lost. The empty state covers the case where
            there is no active list to sit under. */}
        {mode !== 'app' && !showGroups && active.length > 0 && completedList.length + archivedList.length > 0 && (
          <button className="stream-reveal" onClick={() => setShowCompletedArchived(true)}>
            Show {foldedLabel(completedList.length, archivedList.length)}
          </button>
        )}

        {mode !== 'app' && showGroups && completedList.length > 0 && (
          <details className="stream-group">
            <summary>Completed ({completedList.length})</summary>
            <ul className="stream-list">{completedList.map(card)}</ul>
          </details>
        )}

        {mode !== 'app' && showGroups && archivedList.length > 0 && (
          <details className="stream-group">
            <summary>Archived ({archivedList.length})</summary>
            <ul className="stream-list">{archivedList.map(card)}</ul>
          </details>
        )}
      </div>

      {newChamberOpen && <NewChamberSheet onClose={() => setNewChamberOpen(false)} />}
    </div>
  )
}
