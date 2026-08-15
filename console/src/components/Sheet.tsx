import { useEffect, useRef, type ReactNode } from 'react'
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
  const closeRef = useRef<HTMLButtonElement>(null)
  // aria-modal promises modal behaviour: focus lands inside on open, and
  // Escape dismisses. Without both, the attribute only hides the page from
  // assistive tech while keyboard users are still stranded behind it.
  useEffect(() => {
    closeRef.current?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onClose])
  return (
    <div className="sheet" role="dialog" aria-label={label} aria-modal="true">
      <header className="topbar">
        <h2>{title}</h2>
        <button
          ref={closeRef}
          type="button"
          className="icon-btn bar-end"
          aria-label="Close"
          onClick={onClose}
        >
          <Close />
        </button>
      </header>
      <div className="sheet-scroll">{children}</div>
    </div>
  )
}
