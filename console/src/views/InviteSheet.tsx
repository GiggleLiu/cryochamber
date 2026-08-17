import { useCallback, useEffect, useState } from 'react'
import { HubClient, type Invite } from '../api/hubClient'
import { ApiError, isUnauthorized } from '../api/types'
import { useAppStore } from '../store/appStore'
import { relativeTimeLabel } from '../lib/format'
import { Sheet } from '../components/Sheet'
import { AlertCircle } from '../components/Icon'

/** First unused `guest-<N>`, so an unnamed link still gets a name the owner can
 * recognise in the list — and one the hub will accept. It refuses a duplicate
 * among *all* active invites, chamber scope notwithstanding, so this must be
 * fed the whole active list and not the slice this sheet displays. */
export function defaultInviteLabel(invites: Invite[]): string {
  const taken = new Set(invites.map((i) => i.name))
  let n = 1
  while (taken.has(`guest-${n}`)) n += 1
  return `guest-${n}`
}

/**
 * What to say when the mint failed. A 4xx is the hub's considered answer about
 * this request, so telling the owner to check their connection would send them
 * to fix something that is not broken. The hub's own words win when it sent
 * any; its token route answers a bare 400 for exactly one reason — the name is
 * taken — while a silent 403 (owner rights lost mid-session) is a refusal that
 * renaming would not cure, so it must not be dressed up as one.
 */
/** The hub answers 503 on every token route while it runs in open (loopback)
 * mode: there is no access control to share, so invites cannot exist. That is
 * a configuration fact, not a connectivity problem, and is worded as one. */
export const OPEN_MODE_MESSAGE =
  'This hub runs in open mode (no authentication), so invite links cannot work. Restart it with `cryohub start` — bearer auth is the default — to enable sharing.'

function isOpenMode(e: unknown): boolean {
  return e instanceof ApiError && e.status === 503
}

function mintErrorMessage(e: unknown): string {
  if (isOpenMode(e)) return OPEN_MODE_MESSAGE
  if (e instanceof ApiError && e.status >= 400 && e.status < 500) {
    if (e.hubSaid) return e.message
    return e.status === 400
      ? 'That label is already in use — pick another.'
      : 'The hub refused to create this invite link.'
  }
  return 'Could not create the invite link. Check your connection and try again.'
}

/**
 * Sharing, the way a meeting host shares: one button that mints a link for
 * *this* chamber, and a list of who currently holds one.
 *
 * The minted link is shown once, and is copied to the clipboard in the same
 * gesture that creates it: the console never persists it, so once this sheet
 * closes the only remaining copy is the hub's own token file (0600) — not
 * somewhere an owner should have to go digging to re-send a link.
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
  const chambers = useAppStore((s) => s.chambers)
  // Every active invite, not just this chamber's: the name of one scoped
  // elsewhere is still a name this hub will refuse.
  const [active, setActive] = useState<Invite[] | null>(null)
  const [listError, setListError] = useState<string | null>(null)
  const [label, setLabel] = useState('')
  const [link, setLink] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [copyFailed, setCopyFailed] = useState(false)
  const [busy, setBusy] = useState(false)
  const [confirming, setConfirming] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const hub = client instanceof HubClient ? client : null

  /** Every invite still live. Revoked ones are not "people with access", so
   * they are not shown at all — and they free their name on the hub too. */
  const refresh = useCallback(() => {
    if (!hub) return
    hub
      .listInvites()
      .then((all) => {
        setActive(all.filter((i) => i.revoked_at === null))
        setListError(null)
      })
      .catch((e) => {
        if (isUnauthorized(e)) return
        setListError(
          isOpenMode(e)
            ? OPEN_MODE_MESSAGE
            : 'Could not load who has access. Check your connection and try again.',
        )
      })
  }, [hub])

  useEffect(refresh, [refresh])

  /** The people this sheet is about: active invites whose scope covers this
   * chamber. */
  const people = active?.filter((i) => i.chambers.includes(chamberId)) ?? null
  // Minting before the list arrives would pick a `guest-N` blind, and the hub
  // would reject the collision. A list that failed to load is different: the
  // names are simply unknown, and a 400 then says so honestly.
  const listPending = active === null && listError === null

  /** Names of the other chambers an invite also reaches. Chambers outside our
   * own scope are not in the list and are left unnamed. */
  function alsoNames(invite: Invite): string[] {
    return chambers
      .filter((c) => c.id !== chamberId && invite.chambers.includes(c.id))
      .map((c) => c.name)
  }

  async function copyLink() {
    if (!hub || busy || listPending) return
    setBusy(true)
    setError(null)
    setCopied(false)
    setCopyFailed(false)
    try {
      const name = label.trim() || defaultInviteLabel(active ?? [])
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
      if (isUnauthorized(e)) return
      setError(mintErrorMessage(e))
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
        if (isUnauthorized(e)) return
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
        <button className="btn-primary" onClick={copyLink} disabled={busy || listPending}>
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
      {listError ? (
        // The list failed, so say that here rather than leaving "Loading…" to
        // spin for ever over a list that is never coming.
        <div className="group">
          <p className="row row-muted" role="alert">{listError}</p>
        </div>
      ) : people === null || people.length === 0 ? (
        <div className="group">
          <p className="row row-muted">
            {people === null ? 'Loading…' : 'Nobody else has access. Copy a link to invite someone.'}
          </p>
        </div>
      ) : (
        <ul className="group">
          {people.map((invite) => {
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
