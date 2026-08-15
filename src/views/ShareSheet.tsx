import { useCallback, useEffect, useState } from 'react'
import { HubClient, type Invite } from '../api/hubClient'
import { useAppStore } from '../store/appStore'
import { logoutIfAuthError } from '../lib/authGuard'
import { AlertCircle, Close } from '../components/Icon'

/** Invite links are the hub's whole access model, so the owner screen is short:
 * who can see what, plus the one-shot link for someone new. */
export function ShareSheet() {
  const client = useAppStore((s) => s.client)
  const streams = useAppStore((s) => s.streams)
  const setShareOpen = useAppStore((s) => s.setShareOpen)
  const [invites, setInvites] = useState<Invite[] | null>(null)
  const [name, setName] = useState('')
  const [checked, setChecked] = useState<number[]>([])
  const [createdLink, setCreatedLink] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [copyFailed, setCopyFailed] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const hub = client instanceof HubClient ? client : null

  // Every hub call here can be the one that discovers the owner token was
  // revoked; a 401 signs out instead of becoming another inline red line under
  // a screen that still looks authorized.
  const refresh = useCallback(() => {
    if (!hub) return
    hub
      .listInvites()
      .then(setInvites)
      .catch((e) => {
        if (logoutIfAuthError(e)) return
        setError('Could not load invites. Check your connection and try again.')
      })
  }, [hub])

  useEffect(refresh, [refresh])

  /** Project names an invite can see, resolved through the chamber ids the last
   * register() mapped. Chambers outside our own scope simply do not resolve. */
  function projectNames(invite: Invite): string {
    const names = streams
      .filter((s) => {
        const chamber = hub?.chamberIdFor(s.stream_id)
        return chamber !== undefined && invite.chambers.includes(chamber)
      })
      .map((s) => s.name)
    return names.length > 0 ? names.join(', ') : '—'
  }

  async function create() {
    if (!hub || !name.trim() || busy) return
    setBusy(true)
    setError(null)
    try {
      const chambers = checked
        .map((sid) => hub.chamberIdFor(sid))
        .filter((id): id is string => id !== undefined)
      const { token } = await hub.createInvite(name.trim(), chambers)
      // Shown once and never again: the hub stores a hash, so this string does
      // not exist anywhere else after this render.
      setCreatedLink(`${window.location.origin}/#invite=${token}`)
      setCopied(false)
      setCopyFailed(false)
      setName('')
      setChecked([])
      refresh()
    } catch (e) {
      if (logoutIfAuthError(e)) return
      setError('Could not create the invite. Is that name already in use?')
    } finally {
      setBusy(false)
    }
  }

  function revoke(invite: Invite) {
    if (!hub) return
    setError(null)
    hub
      .revokeInvite(invite.name)
      .then(refresh)
      .catch((e) => {
        if (logoutIfAuthError(e)) return
        setError(`Could not revoke ${invite.name}.`)
      })
  }

  /** "Copied" is a promise to the user that the link is on their clipboard, so
   * it waits for the write to resolve — a denied permission or a browser
   * without the API keeps the button on "Copy" and says what happened. */
  async function copy() {
    if (!createdLink) return
    try {
      if (!navigator.clipboard) throw new Error('clipboard unavailable')
      await navigator.clipboard.writeText(createdLink)
      setCopied(true)
      setCopyFailed(false)
    } catch {
      setCopied(false)
      setCopyFailed(true)
    }
  }

  return (
    <div className="sheet" role="dialog" aria-label="Share access" aria-modal="true">
      <header className="topbar">
        <h2>Share access</h2>
        <button className="icon-btn bar-end" aria-label="Close" onClick={() => setShareOpen(false)}>
          <Close />
        </button>
      </header>

      <div className="sheet-scroll">
        {error && (
          <p className="alert" role="alert">
            <AlertCircle size={18} />
            <span className="alert-body">{error}</span>
          </p>
        )}

        {createdLink && (
          <>
            <p className="group-label">New invite link</p>
            <div className="group">
              <div className="row">
                <input
                  className="link-field"
                  aria-label="Invite link"
                  readOnly
                  value={createdLink}
                  onFocus={(e) => e.currentTarget.select()}
                />
                <button className="row-action" onClick={copy}>
                  {copied ? 'Copied' : 'Copy'}
                </button>
              </div>
            </div>
            {copyFailed && (
              <p className="group-hint" role="alert">
                Could not copy — select the link above and copy it manually.
              </p>
            )}
            <p className="group-hint">
              Send this to the person you are inviting. It opens the app already signed
              in — and it is shown here once, because the hub never reveals it again.
            </p>
          </>
        )}

        <p className="group-label">Invite someone</p>
        <div className="group">
          <label className="row">
            Name
            <input
              className="row-input"
              value={name}
              autoCapitalize="words"
              autoCorrect="off"
              spellCheck={false}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          {streams.map((s) => (
            <label className="row" key={s.stream_id}>
              {s.name}
              <input
                type="checkbox"
                className="switch"
                checked={checked.includes(s.stream_id)}
                onChange={() =>
                  setChecked((prev) =>
                    prev.includes(s.stream_id)
                      ? prev.filter((id) => id !== s.stream_id)
                      : [...prev, s.stream_id],
                  )
                }
              />
            </label>
          ))}
        </div>
        <p className="group-hint">
          The link only opens the projects you tick here. Leave them all off to share
          nothing yet.
        </p>
        <div className="sheet-action">
          <button className="btn-primary" onClick={create} disabled={busy || !name.trim()}>
            {busy ? 'Creating…' : 'Create invite link'}
          </button>
        </div>

        <p className="group-label">People with access</p>
        {invites === null || invites.length === 0 ? (
          <div className="group">
            <p className="row row-muted">
              {invites === null ? 'Loading…' : 'Nobody else has a link yet.'}
            </p>
          </div>
        ) : (
          <ul className="group">
            {invites.map((invite) => (
              <li key={invite.name}>
                <div className="row invite-row">
                  <div className="invite-main">
                    <span className="invite-name">
                      {invite.name}
                      {invite.revoked_at && <span className="badge-revoked">Revoked</span>}
                    </span>
                    <span className="invite-meta">
                      {projectNames(invite)} · added {formatDate(invite.created_at)}
                    </span>
                  </div>
                  {!invite.revoked_at && (
                    <button className="row-action row-action-danger" onClick={() => revoke(invite)}>
                      Revoke
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  )
}

function formatDate(iso: string): string {
  const ms = Date.parse(iso)
  if (Number.isNaN(ms)) return iso
  return new Date(ms).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}
