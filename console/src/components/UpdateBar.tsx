import { useAppStore } from '../store/appStore'
import { applyUpdate } from '../lib/swUpdate'

/**
 * "Update available · Reload". Shown only when a newer build's service worker
 * is installed and waiting; the tap hands control to it and the page reloads
 * once (see lib/swUpdate). Floats like the Reconnecting notice so it never
 * reflows the conversation.
 */
export function UpdateBar() {
  const updateAvailable = useAppStore((s) => s.updateAvailable)
  if (!updateAvailable) return null
  return (
    <div className="banner banner-update" role="status">
      Update available
      <button type="button" className="banner-action" onClick={() => applyUpdate()}>
        Reload
      </button>
    </div>
  )
}
