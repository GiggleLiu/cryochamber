import { useState } from 'react'
import { HubClient } from '../api/hubClient'
import { useAppStore } from '../store/appStore'
import { logoutIfAuthError } from '../lib/authGuard'
import { applyTheme, readTheme, type Theme } from '../lib/theme'
import { Sheet } from '../components/Sheet'

const THEMES: Array<{ value: Theme; label: string }> = [
  { value: '', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
]

/** Where this session is signed in. A hub entry carries an empty prefix — it
 * means "this origin" — so the origin is what an operator can actually read. */
export function hubLabel(prefix: string): string {
  if (prefix) return prefix
  return typeof window === 'undefined' ? '' : window.location.origin
}

/**
 * Hub-wide settings: what this token is, how the app looks, and the two
 * chamber-list preferences that are not about any one chamber. Everything that
 * belongs to a single chamber lives in its controls sheet instead.
 *
 * Owner and guest get the same sheet, in the same shell, in the same order —
 * the guest's simply has no Chambers section, because those two rows act on a
 * fold only an owner has.
 */
export function SettingsSheet() {
  const creds = useAppStore((s) => s.creds)
  const hubRole = useAppStore((s) => s.hubRole)
  const client = useAppStore((s) => s.client)
  const showCompletedArchived = useAppStore((s) => s.showCompletedArchived)
  const setShowCompletedArchived = useAppStore((s) => s.setShowCompletedArchived)
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen)
  const logout = useAppStore((s) => s.logout)
  const [theme, setTheme] = useState<Theme>(readTheme)
  const [refreshing, setRefreshing] = useState(false)
  const [refreshError, setRefreshError] = useState<string | null>(null)

  if (!creds) return null

  function chooseTheme(next: Theme) {
    applyTheme(next)
    setTheme(next)
  }

  async function refreshChambers() {
    const hub = client instanceof HubClient ? client : null
    if (!hub || refreshing) return
    setRefreshing(true)
    setRefreshError(null)
    // A completion that lands after a logout — or after another token has
    // signed in — belongs to a session that no longer exists: applying it
    // would install the previous owner's chamber list under the new account,
    // and a late 401 would sign the new session out.
    const stale = () => useAppStore.getState().client !== hub
    try {
      await hub.refreshIndex()
      // The hub also emits `index`, but re-registering here means the list is
      // already correct when the sheet closes rather than a beat later.
      const init = await hub.register()
      if (stale()) return
      useAppStore.getState().applyInitialState(init)
    } catch (e) {
      if (stale()) return
      if (logoutIfAuthError(e)) return
      setRefreshError('Could not refresh. Check your connection and try again.')
    } finally {
      setRefreshing(false)
    }
  }

  return (
    <Sheet title="Settings" label="Settings" onClose={() => setSettingsOpen(false)}>
      <p className="group-label">Account</p>
      <div className="group">
        <div className="row">
          Signed in as
          <span className="row-value">{creds.email}</span>
        </div>
        {/* A hub has no accounts — the token is the whole identity — so the
            honest thing to show is which kind of token this is. */}
        <div className="row">
          Access
          <span className="row-value">{hubRole === 'owner' ? 'Owner' : 'Guest'}</span>
        </div>
        <div className="row">
          Hub
          <span className="row-value">{hubLabel(creds.prefix)}</span>
        </div>
      </div>

      <p className="group-label">Appearance</p>
      <div className="group">
        <div className="row">
          Theme
          <div className="segmented" role="radiogroup" aria-label="Theme">
            {THEMES.map((t) => (
              <label key={t.label} className={`segment${theme === t.value ? ' is-on' : ''}`}>
                <input
                  type="radio"
                  name="theme"
                  checked={theme === t.value}
                  onChange={() => chooseTheme(t.value)}
                />
                {t.label}
              </label>
            ))}
          </div>
        </div>
      </div>

      {hubRole === 'owner' && (
        <>
          <p className="group-label">Chambers</p>
          <div className="group">
            <label className="row">
              Show completed &amp; archived
              <input
                type="checkbox"
                className="switch"
                checked={showCompletedArchived}
                onChange={(e) => setShowCompletedArchived(e.target.checked)}
              />
            </label>
            <button className="row" onClick={refreshChambers} disabled={refreshing}>
              Refresh chambers
              {/* Decorative: kept out of the button's accessible name, which
                  stays "Refresh chambers". `disabled` reports the in-flight
                  state to assistive tech. */}
              <span className="row-value" aria-hidden="true">
                {refreshing ? 'Refreshing…' : 'Re-scan the hub'}
              </span>
            </button>
          </div>
          {refreshError && (
            <p className="group-hint" role="alert">
              {refreshError}
            </p>
          )}
        </>
      )}

      <div className="group group-spaced">
        <button className="row row-danger" onClick={() => logout()}>
          Log out
        </button>
      </div>

      <p className="app-version">Agent Console v{__APP_VERSION__}</p>
    </Sheet>
  )
}
