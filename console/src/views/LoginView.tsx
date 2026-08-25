import { useState, type FormEvent } from 'react'
import { signInWithHubToken } from '../lib/hubSignIn'
import { useAppStore } from '../store/appStore'
import { AlertCircle, Logo } from '../components/Icon'

export function LoginView() {
  const [hubToken, setHubToken] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [showToken, setShowToken] = useState(false)
  const loginReason = useAppStore((s) => s.loginReason)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      // The hub has no accounts: one token is the whole identity, and whoami
      // resolves the display name and the role behind it.
      await signInWithHubToken(hubToken.trim())
    } catch {
      // Failures here are always "the hub said no" (HTTP 401/403); the raw
      // status is noise next to the one thing the user can act on.
      setError('That access token was not accepted. Check it and try again.')
    } finally {
      setBusy(false)
    }
  }

  const notice = loginReason ?? error

  return (
    <div className="login-screen">
      <div className="login-head">
        <Logo className="login-mark" />
        <h1>Agent Console</h1>
        <p className="login-tagline">Every project is one conversation with its agent.</p>
      </div>

      {notice && (
        <p className="alert login-notice" role="alert">
          <AlertCircle size={18} />
          <span className="alert-body">{notice}</span>
        </p>
      )}

      <form className="login" onSubmit={submit}>
        <label className="field">
          <span>Access token</span>
          <span className="token-field">
            <input
              type={showToken ? 'text' : 'password'}
              required
              autoComplete="off"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              value={hubToken}
              onChange={(e) => setHubToken(e.target.value)}
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
        <button className="btn-primary" type="submit" disabled={busy}>
          {busy ? 'Signing in…' : 'Sign in'}
        </button>
      </form>
      <p className="login-hint">
        The hub operator can print a token with <code>cryohub token owner</code>.
      </p>
    </div>
  )
}
