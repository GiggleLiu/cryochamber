import { useCallback, useEffect, useState } from 'react'
import { HubClient, type TodoItem } from '../../api/hubClient'
import { useAppStore } from '../../store/appStore'
import { subscribeChamberEvents } from '../../store/chamberEvents'
import { logoutIfAuthError } from '../../lib/authGuard'
import { AlertCircle } from '../../components/Icon'

/**
 * Pending first, ordered by when they are due (undated last, then by id);
 * done newest-first, which is the order the operator scans history in.
 */
export function sortTodos(items: TodoItem[]): { pending: TodoItem[]; done: TodoItem[] } {
  const pending = items
    .filter((t) => !t.done)
    .slice()
    .sort((a, b) => {
      if (a.at && b.at) return a.at < b.at ? -1 : a.at > b.at ? 1 : 0
      if (a.at) return -1
      if (b.at) return 1
      return a.id - b.id
    })
  const done = items
    .filter((t) => t.done)
    .slice()
    .sort((a, b) => b.id - a.id)
  return { pending, done }
}

/** Read-only, exactly as in the control panel: todos are the agent's own
 * schedule, and editing them behind its back is how a wake gets lost. */
export function TodosTab({ chamberId }: { chamberId: string }) {
  const client = useAppStore((s) => s.client)
  const [items, setItems] = useState<TodoItem[] | null>(null)
  // The tab's own error slot: the sheet's `loadError`/`actionError` belong to
  // the status load and the lifecycle buttons, and a todo fetch failing must
  // not overwrite either of them.
  const [error, setError] = useState<string | null>(null)
  const hub = client instanceof HubClient ? client : null

  const load = useCallback(async () => {
    if (!hub) return
    try {
      setItems(await hub.chamberTodos(chamberId))
      setError(null)
    } catch (e) {
      if (logoutIfAuthError(e)) return
      setError('Could not load todos. Check your connection and try again.')
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

  if (error) {
    return (
      <p className="alert" role="alert">
        <AlertCircle size={18} />
        <span className="alert-body">{error}</span>
      </p>
    )
  }
  if (items === null) return <p className="tab-empty">Loading…</p>
  if (items.length === 0) return <p className="tab-empty">No todos in this chamber.</p>

  const { pending, done } = sortTodos(items)
  const row = (t: TodoItem) => (
    <li className={`todo-row${t.done ? ' is-done' : ''}`} key={t.id}>
      <span className="todo-text">{t.text}</span>
      {t.at && <span className="todo-when">{t.at}</span>}
    </li>
  )

  return (
    <>
      <ul className="todo-list">{pending.map(row)}</ul>
      {done.length > 0 && (
        <details className="todo-history">
          <summary>History ({done.length})</summary>
          <ul className="todo-list">{done.map(row)}</ul>
        </details>
      )}
    </>
  )
}
