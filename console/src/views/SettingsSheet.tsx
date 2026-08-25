import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { HubRouter } from '../api/hubRouter'
import { AgentSelect } from '../components/AgentSelect'
import { useAppStore } from '../store/appStore'
import { useOwnerHub } from '../hooks/useOwnerHub'
import type { HubAccount } from '../store/hubs'
import { isUnauthorized } from '../api/types'
import { compareVersions } from '../lib/format'
import { applyTheme, readTheme, type Theme } from '../lib/theme'
import { Sheet } from '../components/Sheet'
import { AddHubView } from './AddHubView'

const THEMES: Array<{ value: Theme; label: string }> = [
  { value: '', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
]

/** This bundle's own version, baked in at build time from the crate the
 * console ships inside. Empty when the build could not read it, which every
 * reader below treats as "unknown" rather than as a version. */
const CONSOLE_VERSION = import.meta.env.VITE_CONSOLE_VERSION

/** Where this session is signed in. The console is served by the hub it talks
 * to, so the origin is both the truth and what an operator can actually read. */
export function hubLabel(): string {
  return typeof window === 'undefined' ? '' : window.location.origin
}

/**
 * Hub-wide settings: what this token is, how the app looks, and controls that
 * affect the host rather than any one chamber. Everything chamber-specific
 * lives in its controls sheet instead.
 *
 * Owner and guest get the same sheet, in the same shell, in the same order —
 * the guest's simply has no Chambers section, because those controls act on
 * host state only an owner may change.
 *
 * App mode adds a Hubs section and turns the owner section per-hub: the app
 * holds N hubs and can own some of them, so "the hub" is a choice rather than
 * the page's origin.
 */
export function SettingsSheet() {
  // Which hub the owner rows act on, and the choice that steers it — shared
  // with the New Chamber sheet, which asks the same question of the same hubs.
  const { app, ownedHubs, ownerHubId, ownerHub, isOwner, chooseHub } = useOwnerHub()
  const creds = useAppStore((s) => s.creds)
  const hubRole = useAppStore((s) => s.hubRole)
  const hubVersion = useAppStore((s) => s.hubVersion)
  const client = useAppStore((s) => s.client)
  const hubs = useAppStore((s) => s.hubs)
  const roleByHub = useAppStore((s) => s.roleByHub)
  const versionByHub = useAppStore((s) => s.versionByHub)
  const connectionByHub = useAppStore((s) => s.connectionByHub)
  const authFailedHubs = useAppStore((s) => s.authFailedHubs)
  const removeHub = useAppStore((s) => s.removeHub)
  const showCompletedArchived = useAppStore((s) => s.showCompletedArchived)
  const setShowCompletedArchived = useAppStore((s) => s.setShowCompletedArchived)
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen)
  const logout = useAppStore((s) => s.logout)
  const [theme, setTheme] = useState<Theme>(readTheme)
  const [refreshing, setRefreshing] = useState(false)
  const [refreshError, setRefreshError] = useState<string | null>(null)
  const [defaultAgent, setDefaultAgent] = useState('')
  const [agentBusy, setAgentBusy] = useState(false)
  const [agentError, setAgentError] = useState<string | null>(null)
  const [addingHub, setAddingHub] = useState(false)

  const hubCount = hubs.length
  // Which hub the owner rows point at *now*, for a request that resolves after
  // the operator has already switched to another one.
  const selectedRef = useRef(ownerHubId)
  useLayoutEffect(() => {
    selectedRef.current = ownerHubId
  })

  useEffect(() => {
    if (!ownerHub || !isOwner) return
    // Whatever the last hub answered is not this hub's answer: the field goes
    // blank until this one speaks, rather than showing another host's runner.
    setDefaultAgent('')
    let cancelled = false
    void ownerHub.hostConfig().then(
      (config) => {
        if (cancelled || useAppStore.getState().client !== client) return
        setDefaultAgent(config.default_agent)
      },
      (error) => {
        if (cancelled || useAppStore.getState().client !== client || isUnauthorized(error)) return
        setAgentError('Could not load the host agent setting.')
      },
    )
    return () => {
      cancelled = true
    }
  }, [ownerHub, isOwner, client])

  // A hub added from inside the sheet has done its job; the form closes and
  // the new hub is already in the list behind it.
  useEffect(() => {
    setAddingHub(false)
  }, [hubCount])

  // App mode's sign-in is its hub list, not a credential.
  if (!creds && !app) return null

  function chooseTheme(next: Theme) {
    applyTheme(next)
    setTheme(next)
  }

  /** A completion that lands after a logout — or after the hub list changed —
   * belongs to a session that no longer exists: applying it would install the
   * previous owner's chamber list under the new account, and a late 401 would
   * sign the new session out. Both modes ask the same question of the store's
   * own client, which is the router in app mode and the hub in browser mode;
   * app mode also drops an answer about a hub the owner rows have left behind.
   * Browser mode's one hub is `''` on both sides, so it asks only the first. */
  function stale(hubId: string): boolean {
    return useAppStore.getState().client !== client || selectedRef.current !== hubId
  }

  async function refreshChambers() {
    const hubId = ownerHubId
    if (!ownerHub || refreshing) return
    setRefreshing(true)
    setRefreshError(null)
    try {
      await ownerHub.refreshIndex()
      // The hub also emits `index`, but re-reading here means the list is
      // already correct when the sheet closes rather than a beat later.
      // In app mode the read goes through the router, which stamps the hub on
      // every row it returns; browser mode's rows are the hub's own ids.
      const list =
        app && client instanceof HubRouter
          ? await client.listChambersFor(hubId)
          : await ownerHub.listChambers()
      if (stale(hubId)) return
      // Only this hub's rows are replaced — `''` in browser mode, which is the
      // whole list there.
      useAppStore.getState().setChambersForHub(hubId, list)
    } catch (e) {
      if (stale(hubId)) return
      if (isUnauthorized(e)) return
      setRefreshError('Could not refresh. Check your connection and try again.')
    } finally {
      setRefreshing(false)
    }
  }

  /** The dropdown saves on change, and the field shows the chosen runner
   * straight away: a select that snapped back while the request was in flight
   * would read as the hub having refused it. A real refusal restores the value
   * the hub still holds, next to the reason it gave. */
  async function chooseDefaultAgent(next: string) {
    const hub = ownerHub
    const hubId = ownerHubId
    const previous = defaultAgent
    if (!hub || agentBusy || !next.trim() || next === previous) return
    setDefaultAgent(next)
    setAgentBusy(true)
    setAgentError(null)
    try {
      const config = await hub.updateHostConfig(next)
      if (stale(hubId)) return
      setDefaultAgent(config.default_agent)
    } catch (error) {
      if (stale(hubId)) return
      setDefaultAgent(previous)
      if (isUnauthorized(error)) return
      setAgentError(
        error instanceof Error ? error.message : 'Could not save the host agent setting.',
      )
    } finally {
      setAgentBusy(false)
    }
  }

  /** Forgetting a hub throws away conversations, watermarks and unsent
   * messages that only exist on this device, so it asks first. */
  function forgetHub(hub: HubAccount) {
    const ok = window.confirm(
      `Forget ${hub.label}? Its projects and their conversations leave this device.`,
    )
    if (!ok) return
    void removeHub(hub.id)
  }

  return (
    <Sheet title="Settings" label="Settings" onClose={() => setSettingsOpen(false)}>
      {creds && (
        <>
          <p className="group-label">Account</p>
          <div className="group">
            <div className="row">
              Signed in as
              <span className="row-value">{creds.name}</span>
            </div>
            {/* A hub has no accounts — the token is the whole identity — so the
                honest thing to show is which kind of token this is. */}
            <div className="row">
              Access
              <span className="row-value">{hubRole === 'owner' ? 'Owner' : 'Guest'}</span>
            </div>
            <div className="row">
              Hub
              <span className="row-value">{hubLabel()}</span>
            </div>
          </div>
        </>
      )}

      {/* The app's own sign-in: every hub it remembers, what that hub calls
          this token, and whether the hub is answering at all. */}
      {app && (
        <>
          <p className="group-label">Hubs</p>
          <div className="group">
            {hubs.map((h) => {
              const version = versionByHub[h.id]
              const older =
                !!version &&
                CONSOLE_VERSION !== '' &&
                compareVersions(version, CONSOLE_VERSION) === -1
              return (
                <div className="row hub-row" key={h.id}>
                  <span className="hub-main">
                    <span className="hub-name">{h.label}</span>
                    <span className="hub-meta">{h.url}</span>
                    <span className="hub-meta">
                      {roleByHub[h.id] === 'owner' ? 'Owner' : 'Guest'}
                      {version ? ` · cryohub v${version}` : ''}
                    </span>
                    {connectionByHub[h.id] !== 'live' && (
                      <span className="hub-note">unreachable</span>
                    )}
                    {authFailedHubs.includes(h.id) && (
                      <span className="hub-note">sign-in failed — token revoked?</span>
                    )}
                    {older && (
                      <span className="hub-note">hub is older — some features may be missing</span>
                    )}
                  </span>
                  <button
                    className="row-action row-action-danger"
                    aria-label={`Remove ${h.label}`}
                    onClick={() => forgetHub(h)}
                  >
                    Remove
                  </button>
                </div>
              )
            })}
            <button className="row" onClick={() => setAddingHub(true)}>
              Add hub
            </button>
          </div>
        </>
      )}

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

      {isOwner && (
        <>
          <p className="group-label">Chambers</p>
          <div className="group">
            {/* These act on one host at a time. With a single owned hub the
                question has one answer and asking it would be noise. */}
            {ownedHubs.length > 1 && (
              <label className="row">
                Hub
                <select
                  className="row-input is-select"
                  aria-label="Hub"
                  value={ownerHubId}
                  onChange={(e) => chooseHub(e.target.value)}
                >
                  {ownedHubs.map((h) => (
                    <option key={h.id} value={h.id}>
                      {h.label}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <AgentSelect
              label="Default agent"
              value={defaultAgent}
              disabled={agentBusy}
              onChange={chooseDefaultAgent}
            />
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
          {agentError ? (
            <p className="group-hint" role="alert">
              {agentError}
            </p>
          ) : (
            <p className="group-hint">
              The default agent is the runner new chambers are created with. Chambers that already
              exist keep the runner in their own <code>cryo.toml</code>.
            </p>
          )}
        </>
      )}

      {creds && (
        <div className="group group-spaced">
          <button className="row row-danger" onClick={() => logout()}>
            Log out
          </button>
        </div>
      )}

      {/* Browser mode reports the hub's version, not the console's: one hub
          serves this page, and a stale bundle would report a number nobody can
          act on. The app is served by nobody, so it reports itself — each
          hub's version is on its own row above. */}
      <p className="app-version">
        {app
          ? CONSOLE_VERSION
            ? `Agent Console v${CONSOLE_VERSION}`
            : 'Agent Console'
          : hubVersion
            ? `cryohub v${hubVersion}`
            : 'cryohub'}
      </p>

      {addingHub && (
        <Sheet title="Add hub" label="Add hub" onClose={() => setAddingHub(false)}>
          <AddHubView />
        </Sheet>
      )}
    </Sheet>
  )
}
