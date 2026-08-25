import { useEffect, useRef, useState } from 'react'
import { useAppStore } from './store/appStore'
import { loadCredentials, saveCredentials } from './store/auth'
import { purgeLegacyStorage } from './store/cache'
import { useEventLoop } from './hooks/useEventLoop'
import { INVALID_INVITE_REASON, MALFORMED_INVITE_REASON, signInWithHubToken } from './lib/hubSignIn'
import { downloadUpload, filenameFromHref, HUB_FILES_RE } from './lib/download'
import { isUnauthorized } from './api/types'
import { LoginView } from './views/LoginView'
import { ProjectsView } from './views/ProjectsView'
import { ConversationView } from './views/ConversationView'
import { SettingsSheet } from './views/SettingsSheet'
import { UpdateBar } from './components/UpdateBar'
import { ErrorBoundary } from './components/ErrorBoundary'
import { hashForView, viewFromHash } from './lib/hashRoute'

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
  const hubRole = useAppStore((s) => s.hubRole)
  const chambers = useAppStore((s) => s.chambers)
  const chambersLoaded = useAppStore((s) => s.chambersLoaded)
  const accessNotice = useAppStore((s) => s.accessNotice)
  const [inviteToken] = useState<string | typeof MALFORMED_INVITE | null>(takeInviteToken)
  const [downloadNote, setDownloadNote] = useState<string | null>(null)
  const [routeRevision, setRouteRevision] = useState(0)
  const explicitConversationRoute = useRef(viewFromHash()?.name === 'conversation')
  const chamberCount = chambers.length

  useEffect(() => {
    // Before anything reads storage: the pre-cutover build's id maps and
    // message cache are keyed on numbers this build has no use for, and a
    // stale cache under a live key would be read as this build's own.
    purgeLegacyStorage()
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
    void signInWithHubToken(inviteToken).catch(() =>
      useAppStore.getState().logout(INVALID_INVITE_REASON),
    )
  }, [inviteToken])

  // Stored credentials carry a role and a name, but both are the hub's answer
  // and both can be stale, so every boot re-asks — this call is also the
  // revocation probe (a 401 signs out through the client's own hook rather
  // than waiting for the event loop). Other failures stay silent and
  // owner-only UI simply stays hidden.
  useEffect(() => {
    if (!client) return
    client
      .whoami()
      .then((who) => {
        const s = useAppStore.getState()
        s.setHubRole(who.role)
        s.setHubVersion(who.hub_version ?? null)
        if (!s.creds) return
        if (s.creds.role === who.role && (!who.name || s.creds.name === who.name)) return
        const next = { ...s.creds, role: who.role, name: who.name ?? s.creds.name }
        saveCredentials(next)
        useAppStore.setState({ creds: next, selfName: next.name })
      })
      .catch(() => {})
    // Keyed on the client, not the credentials: a corrected name writes creds
    // back without replacing the client, and re-asking whoami for our own edit
    // would be a second round-trip that can only agree with the first.
  }, [client])

  useEffect(() => {
    const changed = () => setRouteRevision((n) => n + 1)
    window.addEventListener('hashchange', changed)
    window.addEventListener('popstate', changed)
    return () => {
      window.removeEventListener('hashchange', changed)
      window.removeEventListener('popstate', changed)
    }
  }, [])

  // The hash proposes a view; the store owns it. Conversation routes wait for
  // a completed chamber index so a cold deep link is not rejected against an
  // empty pre-boot list. Once the index has loaded, Back and Forward keep
  // working through reconnects from the last known scope.
  useEffect(() => {
    if (!creds) return
    const route = viewFromHash()
    const store = useAppStore.getState()
    if (route?.name === 'conversation') {
      explicitConversationRoute.current = true
      if (!chambersLoaded) return
      if (chambers.some((c) => c.id === route.chamberId)) {
        if (
          store.view.name !== 'conversation' ||
          store.view.chamberId !== route.chamberId
        ) {
          store.navigate(route)
        }
      } else {
        store.navigate({ name: 'projects' }, { replace: true })
      }
      return
    }
    const projects = { name: 'projects' as const }
    if (inviteToken && window.location.hash === '' && store.view.name === 'projects') return
    if (store.view.name !== 'projects' || window.location.hash !== hashForView(projects)) {
      store.navigate(projects, { replace: true })
    }
  }, [creds, chambers, chambersLoaded, inviteToken, routeRevision])

  // A guest's link is tied to one chamber, so landing them in a list of one is
  // a step that says nothing. Once per app start, and only from the default
  // view: a guest who taps Back to the list is meant to stay there, and a
  // re-register after a reconnect must not yank them out of it. An invite that
  // covers several chambers still gets the list, because then the list means
  // something.
  const landed = useRef(false)
  useEffect(() => {
    if (landed.current || hubRole !== 'invite' || chamberCount !== 1) return
    if (explicitConversationRoute.current) return
    const store = useAppStore.getState()
    if (store.view.name !== 'projects') return
    landed.current = true
    store.navigate({ name: 'conversation', chamberId: store.chambers[0].id }, { replace: true })
  }, [hubRole, chamberCount])

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
      downloadUpload((u) => client.fetchBlob(u), href).catch((err) => {
        if (isUnauthorized(err)) return
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
      <div className="banner-stack">
        {connection !== 'live' && (
          <div className="banner" role="status">Reconnecting</div>
        )}
        <UpdateBar />
        {downloadNote && (
          <div className="banner banner-info" role="status">{downloadNote}</div>
        )}
        {/* A chamber pruned from under the user leaves them on the list with no
            explanation otherwise; cleared by the next navigation. */}
        {accessNotice && (
          <div className="banner banner-info" role="status">{accessNotice}</div>
        )}
      </div>
      <ErrorBoundary>
        {view.name === 'conversation' ? (
          <ConversationView chamberId={view.chamberId} />
        ) : (
          <ProjectsView />
        )}
        {settingsOpen && <SettingsSheet />}
      </ErrorBoundary>
    </div>
  )
}
