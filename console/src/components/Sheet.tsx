import { useEffect, useLayoutEffect, useRef, type ReactNode } from 'react'
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
  const rootRef = useRef<HTMLDialogElement>(null)
  const closeRef = useRef<HTMLButtonElement>(null)
  // The Escape handler must call whatever onClose the parent passed *last*,
  // while the effect below runs only once. Every caller passes an inline
  // lambda, and the conversation re-renders on each SSE status event; keying
  // the effect on onClose re-ran it each time and refocused the Close button
  // out from under whatever the user was typing in.
  const onCloseRef = useRef(onClose)
  useLayoutEffect(() => {
    onCloseRef.current = onClose
  })
  // aria-modal promises modal behaviour: focus lands inside on open, and
  // Escape dismisses. Without both, the attribute only hides the page from
  // assistive tech while keyboard users are still stranded behind it. Both
  // are on-open concerns, so this runs once per mount.
  useEffect(() => {
    const restoreTo = document.activeElement
    const dialog = rootRef.current
    // Native modality keeps Tab and pointer input out of underlying sheets.
    dialog?.showModal()
    closeRef.current?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      // Sheets stack: a detail sheet opens over the one that listed it, and
      // both are listening on the document. Only the topmost may take the key,
      // or one Escape would close the whole stack.
      const open = document.querySelectorAll('.sheet[role="dialog"]')
      if (open[open.length - 1] !== rootRef.current) return
      e.preventDefault()
      onCloseRef.current()
    }
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('keydown', onKey)
      dialog?.close()
      // Where the focus came from is where it belongs when this closes —
      // otherwise a keyboard user lands back at the top of the document.
      if (restoreTo instanceof HTMLElement && document.contains(restoreTo)) restoreTo.focus()
    }
  }, [])
  return (
    <dialog
      className="sheet"
      ref={rootRef}
      role="dialog"
      aria-label={label}
      aria-modal="true"
      onCancel={e => {
        e.preventDefault()
        e.stopPropagation()
        onCloseRef.current()
      }}
    >
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
    </dialog>
  )
}
