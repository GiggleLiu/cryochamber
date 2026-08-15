import { useCallback, useEffect, useState } from 'react'
import { HubClient, type Invite } from '../api/hubClient'
import { useAppStore } from '../store/appStore'
import { logoutIfAuthError } from '../lib/authGuard'
import { relativeTimeLabel } from '../lib/format'
import { Sheet } from '../components/Sheet'
import { AlertCircle } from '../components/Icon'

/** First unused `guest-<N>`, so an unnamed link still gets a name the owner can
 * recognise in the list — and one the hub will accept (it refuses a duplicate
 * among *active* invites, which is exactly the set passed in here). */
export function defaultInviteLabel(invites: Invite[]): string {
  const taken = new Set(invites.map((i) => i.name))
  let n = 1
  while (taken.has(`guest-${n}`)) n += 1
  return `guest-${n}`
}

/**
 * Sharing, the way a meeting host shares: one button that mints a link for
 * *this* chamber, and a list of who currently holds one.
 *
 * The minted link is shown once. The hub stores only a hash, so after this
 * sheet closes the string does not exist anywhere — which is why it is copied
 * to the clipboard in the same gesture that creates it.
 */
export function InviteSheet({
  chamberId,
  chamberName,
  onClose,
}: {
  chamberId: string
  chamberName: string
  onClose: () => void
}) {
  const client = useAppStore((s) => s.client)
  const streams = useAppStore((s) => s.streams)
  const [invites, setInvites] = useState<Invite[] | null>(null)
  const [label, setLabel] = useState('')
  const [link, setLink] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [copyFailed, setCopyFailed] = useState(false)
  const [busy, setBusy] = useState(false)
  const [confirming, setConfirming] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const hub = client instanceof HubClient ? client : null

  /** Active invites whose scope covers this chamber. Revoked ones are not
   * "people with access", so they are not shown at all. */
  const refresh = useCallback(() => {
    if (!hub) return
    hub
      .listInvites()
      .then((all) =>
        setInvites(all.filter((i) => i.revoked_at === null && i.chambers.includes(chamberId))),
      )
      .catch((e) => {
        if (logoutIfAuthError(e)) return
        setError('Could not load who has access. Check your connection and try again.')
      })
  }, [hub, chamberId])

  useEffect(refresh, [refresh])

  /** Names of the other chambers an invite also reaches, resolved through the
   * ids the last register() mapped; chambers outside our own scope simply do
   * not resolve and are left unnamed. */
  function alsoNames(invite: Invite): string[] {
    return streams
      .filter((s) => {
        const id = hub?.chamberIdFor(s.stream_id)
        return id !== undefined && id !== chamberId && invite.chambers.includes(id)
      })
      .map((s) => s.name)
  }

  async function copyLink() {
    if (!hub || busy) return
    setBusy(true)
    setError(null)
    setCopied(false)
    setCopyFailed(false)
    try {
      const name = label.trim() || defaultInviteLabel(invites ?? [])
      const { token } = await hub.createInvite(name, [chamberId])
      const minted = `${window.location.origin}/#invite=${token}`
      setLink(minted)
      setLabel('')
      refresh()
      // "Copied" is a promise that the string is on the clipboard, so it waits
      // for the write to resolve; a refusal keeps the field on screen to be
      // selected by hand.
      try {
        if (!navigator.clipboard) throw new Error('clipboard unavailable')
        await navigator.clipboard.writeText(minted)
        setCopied(true)
      } catch {
        setCopyFailed(true)
      }
    } catch (e) {
      if (logoutIfAuthError(e)) return
      setError('Could not create the invite link. Check your connection and try again.')
    } finally {
      setBusy(false)
    }
  }

  function remove(invite: Invite) {
    if (!hub) return
    setConfirming(null)
    setError(null)
    hub
      .revokeInvite(invite.name)
      .then(refresh)
      .catch((e) => {
        if (logoutIfAuthError(e)) return
        setError(`Could not remove ${invite.name}. Check your connection and try again.`)
      })
  }

  return (
    <Sheet title={`Invite to ${chamberName}`} label="Invite" onClose={onClose}>
      {error && (
        <p className="alert" role="alert">
          <AlertCircle size={18} />
          <span className="alert-body">{error}</span>
        </p>
      )}

      <div className="group">
        <label className="row">
          Who is this for?
          <input
            className="row-input"
            value={label}
            placeholder="optional"
            autoCapitalize="words"
            autoCorrect="off"
            spellCheck={false}
            onChange={(e) => setLabel(e.target.value)}
          />
        </label>
      </div>
      <div className="sheet-action">
        <button className="btn-primary" onClick={copyLink} disabled={busy}>
          {busy ? 'Creating…' : 'Copy invite link'}
        </button>
      </div>

      {link && (
        <div className="invite-copy">
          <div className="group">
            <div className="row">
              <input
                className="link-field"
                aria-label="Invite link"
                readOnly
                value={link}
                onFocus={(e) => e.currentTarget.select()}
              />
            </div>
          </div>
          <p className="group-hint" role="status">
            {copied ? 'Copied' : copyFailed ? 'Copy failed — select and copy' : ''}
          </p>
          <p className="group-hint">
            Shown once — the hub keeps only a hash, so it cannot be shown again. Lost
            link, new link.
          </p>
        </div>
      )}

      <p className="group-label">People with access</p>
      {invites === null || invites.length === 0 ? (
        <div className="group">
          <p className="row row-muted">
            {invites === null ? 'Loading…' : 'Nobody else has access. Copy a link to invite someone.'}
          </p>
        </div>
      ) : (
        <ul className="group">
          {invites.map((invite) => {
            const also = alsoNames(invite)
            return (
              <li key={invite.name}>
                <div className="row invite-row">
                  <div className="invite-main">
                    <span className="invite-name">{invite.name}</span>
                    <span className="invite-meta">
                      added {relativeTimeLabel(invite.created_at)}
                    </span>
                    {also.length > 0 && (
                      <span className="invite-scope">also: {also.join(', ')}</span>
                    )}
                  </div>
                  <button
                    className="row-action row-action-danger"
                    onClick={() => setConfirming(invite.name)}
                  >
                    Remove
                  </button>
                </div>
                {confirming === invite.name && (
                  <div className="row confirm-row">
                    <span className="confirm-question">
                      Remove {invite.name}? Their link stops working immediately.
                    </span>
                    <button className="row-action" onClick={() => setConfirming(null)}>
                      Cancel
                    </button>
                    <button
                      className="row-action row-action-danger"
                      onClick={() => remove(invite)}
                    >
                      Remove {invite.name}
                    </button>
                  </div>
                )}
              </li>
            )
          })}
        </ul>
      )}
    </Sheet>
  )
}
