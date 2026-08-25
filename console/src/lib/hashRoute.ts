import type { View } from '../store/appStore'

export function viewFromHash(hash = window.location.hash): View | null {
  if (hash === '' || hash === '#' || hash === '#/') return { name: 'projects' }
  const prefix = '#/chamber/'
  if (!hash.startsWith(prefix)) return null
  const chamberId = hash.slice(prefix.length)
  return chamberId ? { name: 'conversation', chamberId } : null
}

export function hashForView(view: View): string {
  return view.name === 'projects' ? '#/' : `#/chamber/${view.chamberId}`
}

/** Push user navigation and replace automatic redirects. Store navigation
 * already applies the view, so History API writes deliberately emit no event. */
export function writeViewHash(view: View, replace = false): void {
  const hash = hashForView(view)
  if (window.location.hash === hash) return
  window.history[replace ? 'replaceState' : 'pushState'](null, '', hash)
}
