import { useEffect, useState } from 'react'
import { useAppStore } from './store/appStore'
import { loadCredentials } from './store/auth'
import { loadServers } from './api/servers'
import { useEventLoop } from './hooks/useEventLoop'
import { INVALID_INVITE_REASON, MALFORMED_INVITE_REASON, signInWithHubToken } from './lib/hubSignIn'
import { downloadUpload, filenameFromHref, HUB_FILES_RE } from './lib/download'
import { logoutIfAuthError } from './lib/authGuard'
import { LoginView } from './views/LoginView'
import { ProjectsView } from './views/ProjectsView'
import { ConversationView } from './views/ConversationView'
import { SettingsSheet } from './views/SettingsSheet'
import { ShareSheet } from './views/ShareSheet'

/** Returned by takeInviteToken for a `#invite=` fragment whose value is not a
 * usable token — the caller says so on the login screen rather than dropping
 * the user at a bare sign-in form with no explanation. */
export const MALFORMED_INVITE = Symbol('malformed-invite')

/** Invite token captured at boot from `#invite=<hex>`. Stripping comes first
 * and happens for ANY `#invite=` fragment, valid or not: a token — or a
 * mangled one that still leaks most of a token — must never survive in the
 * address bar, in history, or in anything the user might paste onward.
 * Consumed by the effect below. */
export function takeInviteToken(): string | typeof MALFORMED_INVITE | null {
  const hash = window.location.hash
  if (!hash.startsWith('#invite=')) return null
  window.history.replaceState(null, '', window.location.pathname)
  const value = hash.slice('#invite='.length)
  return /^[0-9a-f]{32,}$/.test(value) ? value : MALFORMED_INVITE
}

export default function App() {
  const creds = useAppStore((s) => s.creds)
  const client = useAppStore((s) => s.client)
  const view = useAppStore((s) => s.view)
  const connection = useAppStore((s) => s.connection)
  const settingsOpen = useAppStore((s) => s.settingsOpen)
  const shareOpen = useAppStore((s) => s.shareOpen)
  const [inviteToken] = useState<string | typeof MALFORMED_INVITE | null>(takeInviteToken)
  const [downloadNote, setDownloadNote] = useState<string | null>(null)

  useEffect(() => {
    if (!useAppStore.getState().creds) {
      const saved = loadCredentials()
      if (saved) useAppStore.getState().setCreds(saved)
    }
  }, [])

  // Opening an invite link is the whole onboarding flow: no form, no account —
  // the token in the fragment is exchanged for a session on the spot. Stored
  // credentials win, so an existing session is never silently replaced.
  useEffect(() => {
    if (!inviteToken || useAppStore.getState().creds) return
    if (inviteToken === MALFORMED_INVITE) {
      useAppStore.getState().logout(MALFORMED_INVITE_REASON)
      return
    }
    void (async () => {
      const hub = (await loadServers().catch(() => []))[0]
      if (!hub) {
        useAppStore.getState().logout(INVALID_INVITE_REASON)
        return
      }
      try {
        await signInWithHubToken(hub.prefix, inviteToken, hub.sendTopic ?? '')
      } catch {
        useAppStore.getState().logout(INVALID_INVITE_REASON)
      }
    })()
  }, [inviteToken])

  // Stored credentials carry no role (it is the hub's answer, not ours), so a
  // boot from cache re-asks once. This is also the first thing that can tell us
  // the stored token was revoked while the app was closed, so a 401 signs out
  // here rather than waiting for the event loop; other failures stay silent and
  // owner-only UI simply stays hidden.
  useEffect(() => {
    if (!client) return
    if (useAppStore.getState().hubRole) return
    client
      .whoami()
      .then((who) => useAppStore.getState().setHubRole(who.role))
      .catch((e) => logoutIfAuthError(e, INVALID_INVITE_REASON))
  }, [creds, client])

  // Last line of defense against SPA-rebooting attachment clicks: whatever
  // rendered the anchor (message body, a stale bundle, future views), a click
  // on any chamber file link downloads in place instead of navigating. Bubble-phase
  // on document, so component handlers (download/lightbox in MessageBody,
  // which call preventDefault) always win first.
  useEffect(() => {
    if (!creds || !client) return
    const origin = window.location.origin
    const onClick = (e: MouseEvent) => {
      if (e.defaultPrevented) return
      const anchor = (e.target as Element | null)?.closest?.('a')
      if (!anchor) return
      const raw = anchor.getAttribute('href') ?? ''
      const href = raw.startsWith(origin + '/') ? raw.slice(origin.length) : raw
      if (!HUB_FILES_RE.test(href)) return
      e.preventDefault()
      const name = filenameFromHref(href)
      // Chamber file paths are already absolute app paths — never re-prefix.
      downloadUpload(href, client.authHeaderValue()).catch((err) => {
        if (logoutIfAuthError(err)) return
        setDownloadNote(`Could not download ${name}. Check your connection and try again.`)
      })
    }
    document.addEventListener('click', onClick)
    return () => document.removeEventListener('click', onClick)
  }, [creds, client])

  useEventLoop()

  if (!creds) return <LoginView />

  return (
    <div className="app">
      {connection !== 'live' && (
        <div className="banner" role="status">Reconnecting</div>
      )}
      {downloadNote && (
        <div className="banner banner-info" role="status">{downloadNote}</div>
      )}
      {view.name === 'conversation' ? (
        <ConversationView streamId={view.streamId} />
      ) : (
        <ProjectsView />
      )}
      {settingsOpen && <SettingsSheet />}
      {shareOpen && <ShareSheet />}
    </div>
  )
}
