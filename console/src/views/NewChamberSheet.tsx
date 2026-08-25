import { useCallback, useState } from 'react'
import { HubClient, type NewChamberPayload } from '../api/hubClient'
import { useAppStore } from '../store/appStore'
import { isUnauthorized } from '../api/types'
import { Sheet } from '../components/Sheet'
import { AlertCircle } from '../components/Icon'

/**
 * Validate the form and shape the request body, or return the message to show.
 *
 * The provider block is all-or-nothing on purpose: a chamber configured with a
 * provider but no key scaffolds fine and then fails on its first wake, which is
 * a much worse place to find out. Wording matches the hub's own 400 messages so
 * client-side and server-side refusals read the same.
 */
export function buildNewChamberPayload(fields: {
  name: string
  provider: string
  apiKey: string
  model: string
  providerOpen: boolean
}): NewChamberPayload | string {
  const name = fields.name.trim()
  const provider = fields.provider.trim()
  const apiKey = fields.apiKey.trim()
  const model = fields.model.trim()
  if (!name) return 'name is empty'
  const configuring = fields.providerOpen || provider !== '' || apiKey !== '' || model !== ''
  if (!configuring) return { name, start: true }
  if (!provider) return 'api key provider is empty'
  if (!apiKey) return 'api key is empty'
  return {
    name,
    start: true,
    api_key_provider: provider,
    api_key: apiKey,
    ...(model ? { model } : {}),
  }
}

/** Create a chamber from the phone. The models.dev catalogue the control panel
 * offered is deliberately out of scope — these are plain text fields. */
export function NewChamberSheet({ onClose }: { onClose: () => void }) {
  const client = useAppStore((s) => s.client)
  const navigate = useAppStore((s) => s.navigate)
  const [name, setName] = useState('')
  const [provider, setProvider] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState('')
  const [providerOpen, setProviderOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const hub = client instanceof HubClient ? client : null
  // Dismissing mid-request would drop the outcome on the floor: a failure's
  // message would land on an unmounted sheet, and a success would still
  // navigate into a chamber the user just walked away from. The sheet stays
  // until the request answers.
  const guardedClose = useCallback(() => {
    if (!busy) onClose()
  }, [busy, onClose])

  async function create() {
    if (!hub || busy) return
    const payload = buildNewChamberPayload({ name, provider, apiKey, model, providerOpen })
    if (typeof payload === 'string') {
      setError(payload)
      return
    }
    setBusy(true)
    setError(null)
    // A completion after a logout or account switch belongs to a session that
    // no longer exists: neither its result nor its 401 may touch the new one.
    const stale = () => useAppStore.getState().client !== hub
    try {
      const { id, start_error: startError } = await hub.createChamber(payload)
      // The index changed, so re-read it rather than waiting for the `index`
      // event the hub also emits — the new chamber has to exist in the store
      // before we can navigate into it.
      const list = await hub.listChambers()
      if (stale()) return
      useAppStore.getState().setChambers(list)
      onClose()
      navigate({ name: 'conversation', chamberId: id })
      if (startError) {
        useAppStore
          .getState()
          .setAccessNotice(`Chamber was created but could not start: ${startError}`)
      }
    } catch (e) {
      if (stale()) return
      if (isUnauthorized(e)) return
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Sheet title="New chamber" label="New chamber" onClose={guardedClose}>
      {error && (
        <p className="alert" role="alert">
          <AlertCircle size={18} />
          <span className="alert-body">{error}</span>
        </p>
      )}

      <div className="group">
        <label className="row">
          Name
          <input
            className="row-input"
            value={name}
            placeholder="my-chamber"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
      </div>

      <details
        className="group group-spaced"
        open={providerOpen}
        onToggle={(e) => setProviderOpen(e.currentTarget.open)}
      >
        <summary className="row">API key provider</summary>
        <label className="row">
          Provider
          <input
            className="row-input"
            value={provider}
            placeholder="provider-id"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            onChange={(e) => setProvider(e.target.value)}
          />
        </label>
        <label className="row">
          Model
          <input
            className="row-input"
            value={model}
            placeholder="model-id"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            onChange={(e) => setModel(e.target.value)}
          />
        </label>
        <label className="row">
          API key
          <input
            className="row-input"
            type="password"
            value={apiKey}
            placeholder="sk-..."
            autoComplete="off"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            onChange={(e) => setApiKey(e.target.value)}
          />
        </label>
      </details>
      <p className="group-hint">
        The key is written into the chamber&apos;s own <code>cryo.toml</code> on the hub; the
        console never stores it.
      </p>

      <div className="sheet-action">
        <button className="btn-primary" onClick={create} disabled={busy}>
          {busy ? 'Creating and starting…' : 'Create and start'}
        </button>
      </div>
    </Sheet>
  )
}
