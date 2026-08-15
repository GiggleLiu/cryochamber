import { useState } from 'react'
import type { StreamSub } from '../api/types'
import { useAppStore, useIsOwner } from '../store/appStore'
import { Gear, Inbox, Plus } from '../components/Icon'
import { initial, listTimeLabel, previewText, tileColor } from '../lib/format'
import { NewChamberSheet } from './NewChamberSheet'

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
  const streams = useAppStore((s) => s.streams)
  const hidden = useAppStore((s) => s.hiddenStreams)
  const unread = useAppStore((s) => s.unreadByStream)
  const messages = useAppStore((s) => s.messagesByStream)
  const connection = useAppStore((s) => s.connection)
  const navigate = useAppStore((s) => s.navigate)
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen)
  const isOwner = useIsOwner()
  const [newChamberOpen, setNewChamberOpen] = useState(false)
  const showCompletedArchived = useAppStore((s) => s.showCompletedArchived)
  const visible = streams.filter((s) => !hidden.includes(s.stream_id))
  // Archived wins over completed: the operator put it away on purpose, and one
  // chamber must never appear in two groups.
  const archivedList = visible.filter((s) => s.archived === true)
  const completedList = visible.filter((s) => s.archived !== true && s.completed === true)
  const active = visible.filter((s) => s.archived !== true && s.completed !== true)
  const showGroups = isOwner && showCompletedArchived
  // Nothing to show yet *and* still connecting means the first register is in
  // flight — show the shape of the list rather than an empty state that would
  // be contradicted a moment later. Once offline, the empty state plus the
  // reconnecting banner is the honest report.
  const loading = streams.length === 0 && connection === 'connecting'

  function card(s: StreamSub) {
    const count = unread[s.stream_id]?.length ?? 0
    const last = messages[s.stream_id]?.at(-1)
    const preview = last ? previewText(last.content) : s.description
    return (
      <li key={s.stream_id}>
        <button
          className="stream-card"
          onClick={() => navigate({ name: 'conversation', streamId: s.stream_id })}
        >
          <span className="stream-tile" style={{ background: tileColor(s.name) }}>
            {initial(s.name)}
          </span>
          {s.running !== undefined && (
            <span
              className={`status-dot${
                s.agentRunning ? ' is-awake' : s.running ? ' is-running' : ''
              }`}
              role="img"
              aria-label={
                s.agentRunning
                  ? 'agent working'
                  : s.running
                    ? 'chamber running, agent asleep'
                    : 'chamber stopped'
              }
            />
          )}
          <span className="stream-name">{s.name}</span>
          {s.hasOpenQuestion && (
            <span className="question-badge" title="Open question — agent is waiting on you">
              ?
            </span>
          )}
          {last && <span className="stream-meta">{listTimeLabel(last.timestamp)}</span>}
          {count > 0 && (
            <span className="unread-badge" aria-label={`${count} unread`}>
              {count}
            </span>
          )}
          <span className="stream-desc">{preview}</span>
          {/* Only a started chamber has a real schedule; a stopped one reports
              whatever was pending when it died. */}
          {s.running && s.nextWake && <span className="stream-wake">next wake {s.nextWake}</span>}
        </button>
      </li>
    )
  }

  return (
    <div className="projects">
      <header className="topbar">
        <h1>Projects</h1>
        <div className="topbar-actions">
          {isOwner && (
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

        {!loading &&
          active.length === 0 &&
          (!showGroups || completedList.length + archivedList.length === 0) && (
            <div className="empty-state">
              <Inbox size={40} />
              <h2>No projects yet</h2>
              <p>Every chamber this token can reach shows up here as a project.</p>
            </div>
          )}

        {active.length > 0 && <ul className="stream-list">{active.map(card)}</ul>}

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
      </div>

      {newChamberOpen && <NewChamberSheet onClose={() => setNewChamberOpen(false)} />}
    </div>
  )
}
