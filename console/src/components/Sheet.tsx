import type { ReactNode } from 'react'
import { Close } from './Icon'

/**
 * The full-screen sheet every secondary surface uses: Settings, Invite,
 * Controls, New chamber. One shell means one close affordance, one dialog
 * role, and one scroll container — the three things each of them was
 * otherwise re-deriving slightly differently.
 */
export function Sheet({
  title,
  label,
  onClose,
  children,
}: {
  title: ReactNode
  /** Accessible name of the dialog itself, e.g. "Chamber controls". */
  label: string
  onClose: () => void
  children: ReactNode
}) {
  return (
    <div className="sheet" role="dialog" aria-label={label} aria-modal="true">
      <header className="topbar">
        <h2>{title}</h2>
        <button className="icon-btn bar-end" aria-label="Close" onClick={onClose}>
          <Close />
        </button>
      </header>
      <div className="sheet-scroll">{children}</div>
    </div>
  )
}
