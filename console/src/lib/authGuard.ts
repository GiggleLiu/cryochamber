import { isUnauthorized } from '../api/types'
import { useAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'

/**
 * The single funnel every 401 goes through, wherever it surfaced: the event
 * loop, an attachment fetch, the Invite and Controls sheets, a boot-time whoami.
 *
 * A revoked token that only produced an inline "could not load" message left
 * the app looking signed in — with cached messages on screen and an SSE stream
 * still open — so every catch that can see a 401 calls this first and keeps its
 * own handling for everything else.
 *
 * Returns true when it logged out, so callers can skip their inline error path.
 */
export function logoutIfAuthError(e: unknown, reason: string = AUTH_LOGOUT_REASON): boolean {
  if (!isUnauthorized(e)) return false
  useAppStore.getState().logout(reason)
  return true
}
