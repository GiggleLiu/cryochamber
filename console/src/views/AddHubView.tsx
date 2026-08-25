import { useState, type FormEvent } from 'react'
import { appRuntime, makeClientFactory, parseInviteLink } from '../lib/appBoot'
import { makeHubAccount, type HubTrust } from '../store/hubs'
import { normalizeHubUrl } from '../lib/hubKeys'
import { useAppStore } from '../store/appStore'
import { isUnauthorized } from '../api/types'
import { AlertCircle, Logo } from '../components/Icon'

/** The one thing a user can act on when a hub answers 401: the address was
 * right, the token was not. */
export const REJECTED_TOKEN_ERROR = 'The hub rejected this token'

const BAD_URL_ERROR =
  'That is not a hub address. Use the full address, like https://hub.example:8765.'

/** `normalizeHubUrl` refuses a scheme it cannot speak; it says so in the
 * language of the code, and this is the same thing said to a person. */
const BAD_SCHEME_ERROR = 'Enter an http:// or https:// hub address.'

/** The typed address as the stored account will actually record it. Parsed
 * exactly once and shared by the warning and the trust decision, so the two can
 * never disagree: `http:/hub.local` — one slash — is a hub reached in the
 * clear, and a scheme test on the raw text would call it HTTPS and store that. */
function parseAddress(raw: string): { url: string } | { error: string } | null {
  const trimmed = raw.trim()
  if (trimmed === '') return null
  try {
    return { url: normalizeHubUrl(trimmed) }
  } catch (err) {
    // `new URL` raises TypeError on garbage; a parsed URL with a scheme we do
    // not speak is normalizeHubUrl's own Error.
    return { error: err instanceof TypeError ? BAD_URL_ERROR : BAD_SCHEME_ERROR }
  }
}

/** What to put on screen for a probe that failed. */
function errorText(err: unknown): string {
  if (isUnauthorized(err)) return REJECTED_TOKEN_ERROR
  return err instanceof Error && err.message ? err.message : 'Could not reach that hub.'
}

/** Adding a hub is the app's whole onboarding: an address, a token, and — for
 * a hub reached over plain HTTP — an explicit acknowledgement that everything
 * between here and it travels in the clear. */
export function AddHubView() {
  const [link, setLink] = useState('')
  const [url, setUrl] = useState('')
  const [token, setToken] = useState('')
  const [label, setLabel] = useState('')
  const [showToken, setShowToken] = useState(false)
  const [acknowledged, setAcknowledged] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const address = parseAddress(url)
  // The parsed answer whenever there is one. The raw scheme test is only the
  // live hint while a half-typed address does not parse yet — by submit time
  // an unparseable address is refused, so nothing is ever stored on its word.
  const plainHttp =
    address && 'url' in address
      ? address.url.startsWith('http://')
      : /^http:\/\//i.test(url.trim())
  // A refused address still submits: a button that quietly will not press says
  // less than the sentence explaining what is wrong with what was typed.
  const ready = address !== null && token.trim() !== '' && (!plainHttp || acknowledged)

  /** The acknowledgement is about one address, so a changed address asks
   * again — otherwise a box ticked for one host silently covers the next. */
  function changeUrl(next: string) {
    setUrl(next)
    setAcknowledged(false)
  }

  /** A pasted invite link carries both halves — take them and drop the link,
   * so the token is only ever left in the field that masks it. */
  function takeLink(text: string) {
    const invite = parseInviteLink(text)
    if (!invite) {
      setLink(text)
      return
    }
    setLink('')
    changeUrl(invite.url)
    setToken(invite.token)
  }

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!ready || busy) return
    // Nothing is asked of an address we could not parse: no request leaves the
    // machine, and no record is written for a hub we cannot name.
    if (address === null || 'error' in address) {
      setError(address?.error ?? BAD_URL_ERROR)
      return
    }
    setBusy(true)
    setError(null)
    try {
      // Both from the same parse: the acknowledgement the user gave and the
      // trust the record keeps are about the same address.
      const trust: HubTrust = plainHttp ? { kind: 'plain-http' } : { kind: 'https' }
      const account = makeHubAccount({
        url: address.url,
        token: token.trim(),
        label: label.trim(),
        trust,
      })
      // Same factory the router uses, so the probe crosses exactly the
      // transport this hub's trust decision earned it.
      const who = await makeClientFactory(appRuntime())(account).whoami()
      await useAppStore
        .getState()
        .addHub({ ...account, role: who.role, name: who.name ?? account.name })
    } catch (err) {
      setError(errorText(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="login-screen">
      <div className="login-head">
        <Logo className="login-mark" />
        <h1>Add a hub</h1>
        <p className="login-tagline">Point the app at a chamber hub and its access token.</p>
      </div>

      {error && (
        <p className="alert login-notice" role="alert">
          <AlertCircle size={18} />
          <span className="alert-body">{error}</span>
        </p>
      )}

      <form className="login" onSubmit={submit}>
        <label className="field">
          <span>Invite link</span>
          <input
            type="text"
            autoComplete="off"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            placeholder="Paste a link to fill in both fields"
            value={link}
            onChange={(e) => takeLink(e.target.value)}
          />
        </label>
        <label className="field">
          <span>Hub address</span>
          <input
            type="text"
            inputMode="url"
            autoComplete="off"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            placeholder="https://hub.example:8765"
            value={url}
            onChange={(e) => changeUrl(e.target.value)}
          />
        </label>
        <label className="field">
          <span>Access token</span>
          <span className="token-field">
            <input
              type={showToken ? 'text' : 'password'}
              autoComplete="off"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              value={token}
              onChange={(e) => setToken(e.target.value)}
            />
            <button
              type="button"
              className="token-toggle"
              aria-pressed={showToken}
              onClick={() => setShowToken((shown) => !shown)}
            >
              {showToken ? 'Hide' : 'Show'}
            </button>
          </span>
        </label>
        <label className="field">
          <span>Label (optional)</span>
          <input
            type="text"
            autoComplete="off"
            placeholder="What to call this hub"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
          />
        </label>

        {plainHttp && (
          <div className="alert">
            <AlertCircle size={18} />
            <span className="alert-body">
              <strong>This hub is plain HTTP.</strong> The token and every message you send it
              travel unencrypted, readable by anything on the way.
              <label className="check-field">
                <input
                  type="checkbox"
                  checked={acknowledged}
                  onChange={(e) => setAcknowledged(e.target.checked)}
                />
                I understand traffic to this hub is unencrypted
              </label>
            </span>
          </div>
        )}

        <button className="btn-primary" type="submit" disabled={!ready || busy}>
          {busy ? 'Connecting…' : 'Add hub'}
        </button>
      </form>
      <p className="login-hint">
        The hub operator can print a token with <code>cryohub token owner</code>.
      </p>
    </div>
  )
}
