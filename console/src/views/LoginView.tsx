import { useEffect, useState, type FormEvent } from 'react'
import { loadServers } from '../api/servers'
import { signInWithHubToken } from '../lib/hubSignIn'
import { useAppStore } from '../store/appStore'
import { AlertCircle, Logo } from '../components/Icon'
import type { ServerConfig } from '../api/types'

export function LoginView() {
  const [servers, setServers] = useState<ServerConfig[] | null>(null)
  const [serverIdx, setServerIdx] = useState(0)
  const [hubToken, setHubToken] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const loginReason = useAppStore((s) => s.loginReason)

  useEffect(() => {
    loadServers().then(setServers).catch((e) => setError(e instanceof Error ? e.message : String(e)))
  }, [])

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!servers) return
    setBusy(true)
    setError(null)
    const server = servers[serverIdx]
    try {
      // The hub has no accounts: one token is the whole identity, and whoami
      // resolves the display name and the role behind it.
      await signInWithHubToken(server.prefix, hubToken.trim(), server.sendTopic ?? '')
    } catch {
      // Failures here are always "the hub said no" (HTTP 401/403); the raw
      // status is noise next to the one thing the user can act on.
      setError('That access token was not accepted. Check it and try again.')
    } finally {
      setBusy(false)
    }
  }

  if (!servers) {
    return (
      <div className="login-screen">
        <div className="login-head">
          <Logo className="login-mark" />
          <h1>Agent Console</h1>
          <p className="login-tagline">{error ?? 'Loading…'}</p>
        </div>
      </div>
    )
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
        {servers.length > 1 && (
          <label className="field">
            <span>Server</span>
            <select value={serverIdx} onChange={(e) => setServerIdx(Number(e.target.value))}>
              {servers.map((s, i) => (
                <option key={s.prefix} value={i}>{s.name}</option>
              ))}
            </select>
          </label>
        )}
        <label className="field">
          <span>Access token</span>
          <input
            type="password"
            required
            autoComplete="off"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            value={hubToken}
            onChange={(e) => setHubToken(e.target.value)}
          />
        </label>
        <button className="btn-primary" type="submit" disabled={busy}>
          {busy ? 'Signing in…' : 'Sign in'}
        </button>
      </form>
    </div>
  )
}
