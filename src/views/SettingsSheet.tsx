import { useState } from 'react'
import { useAppStore } from '../store/appStore'
import { applyTheme, readTheme, type Theme } from '../lib/theme'
import { Close } from '../components/Icon'

const THEMES: Array<{ value: Theme; label: string }> = [
  { value: '', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
]

export function SettingsSheet() {
  const creds = useAppStore((s) => s.creds)
  const streams = useAppStore((s) => s.streams)
  const hidden = useAppStore((s) => s.hiddenStreams)
  const hubRole = useAppStore((s) => s.hubRole)
  const toggleHidden = useAppStore((s) => s.toggleHidden)
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen)
  const setShareOpen = useAppStore((s) => s.setShareOpen)
  const logout = useAppStore((s) => s.logout)
  const [theme, setTheme] = useState<Theme>(readTheme)

  if (!creds) return null

  function chooseTheme(next: Theme) {
    applyTheme(next)
    setTheme(next)
  }

  return (
    <div className="sheet" role="dialog" aria-label="Settings" aria-modal="true">
      <header className="topbar">
        <h2>Settings</h2>
        <button
          className="icon-btn bar-end"
          aria-label="Close"
          onClick={() => setSettingsOpen(false)}
        >
          <Close />
        </button>
      </header>

      <div className="sheet-scroll">
        <p className="group-label">Account</p>
        <div className="group">
          <div className="row">
            Signed in as
            <span className="row-value">{creds.email}</span>
          </div>
          <div className="row">
            Server
            <span className="row-value">{creds.prefix}</span>
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
            <p className="group-label">People</p>
            <div className="group">
              <button
                className="row"
                onClick={() => {
                  setSettingsOpen(false)
                  setShareOpen(true)
                }}
              >
                Share access
                <span className="row-value">Invite links</span>
              </button>
            </div>
          </>
        )}

        <p className="group-label">Projects</p>
        {streams.length === 0 ? (
          <div className="group">
            <p className="row row-muted">No projects to show yet.</p>
          </div>
        ) : (
          <ul className="group">
            {streams.map((s) => (
              <li key={s.stream_id}>
                <label className="row">
                  {s.name}
                  <input
                    type="checkbox"
                    className="switch"
                    checked={!hidden.includes(s.stream_id)}
                    onChange={() => toggleHidden(s.stream_id)}
                  />
                </label>
              </li>
            ))}
          </ul>
        )}
        <p className="group-hint">Hidden projects stay subscribed on Zulip; they just leave this list.</p>

        <div className="group group-spaced">
          <button className="row row-danger" onClick={() => logout()}>Log out</button>
        </div>

        <p className="app-version">Agent Console v{__APP_VERSION__}</p>
      </div>
    </div>
  )
}
