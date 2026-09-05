import { useEffect, useMemo, useState } from 'react'
import { useAppStore } from '../store/appStore'
import { isUnauthorized, messageKey, type ChamberMessage } from '../api/types'
import { sortByKey } from '../api/hubClient'
import { Composer } from './Composer'
import { MessageBody } from './MessageBody'
import { retryOutboxItem } from '../lib/outbox'
import { exactTimestamp } from '../lib/format'

export function ThreadReplies({ chamberId, hubChamberId, root, revision, onRead }: {
  chamberId: string; hubChamberId: string; root: ChamberMessage; revision: string; onRead: (latest: string) => void
}) {
  const client = useAppStore(s => s.client)
  const live = useAppStore(s => s.messagesByChamber[chamberId])
  const outbox = useAppStore(s => s.outboxByChamber[chamberId])
  const [history, setHistory] = useState<ChamberMessage[]>([])
  const [loaded, setLoaded] = useState(false)
  const [error, setError] = useState('')
  const [shareError, setShareError] = useState('')
  const [retry, setRetry] = useState(0)
  const [sharing, setSharing] = useState<string | null>(null)
  const [shared, setShared] = useState<string[]>([])
  const fetchBlob = useMemo(() => client ? (url: string) => client.fetchBlobFor(chamberId, url) : undefined, [client, chamberId])
  useEffect(() => {
    let active = true
    setError('')
    void client?.getThread?.(chamberId, root.id).then(rows => {
      if (active) { setHistory(rows); setLoaded(true) }
    }, (e: unknown) => { if (active && !isUnauthorized(e)) setError('Could not load replies. Try again.') })
    return () => { active = false }
  }, [client, chamberId, root.id, revision, retry])
  const replies = useMemo(() => {
    const map = new Map([...history, ...(live ?? [])].filter(m => m.threadId === root.id).map(m => [m.id, m]))
    return sortByKey([...map.values()])
  }, [history, live, root.id])
  const latest = replies.length ? messageKey(replies[replies.length - 1]) : ''
  useEffect(() => { if (loaded && latest) onRead(latest) }, [loaded, latest, onRead])

  async function share(id: string) {
    if (!client?.shareMessage) return
    setSharing(id)
    setShareError('')
    try { await client.shareMessage(chamberId, id); setShared(ids => [...ids, id]) }
    catch (e) { if (!isUnauthorized(e)) setShareError('Could not share to the stream. Use Share to stream to try again.') }
    finally { setSharing(null) }
  }

  return <section className="thread-replies" aria-label="Thread replies">
    {error && <p role="alert">{error} <button onClick={() => setRetry(n => n + 1)}>Retry</button></p>}
    {shareError && <p role="alert">{shareError}</p>}
    {!loaded && !error && <p role="status">Loading replies…</p>}
    {replies.map(m => <article className="thread-reply" key={m.id}>
      <div className="reply-heading"><strong>{m.sender}</strong><time>{exactTimestamp(m)}</time></div>
      <MessageBody source={m.body} fetchBlob={fetchBlob} chamberId={hubChamberId} />
      <button type="button" className="message-action" disabled={sharing === m.id || shared.includes(m.id)} onClick={() => void share(m.id)}>
        {shared.includes(m.id) ? 'Shared to stream' : sharing === m.id ? 'Sharing…' : 'Share to stream'}
      </button>
    </article>)}
    {(outbox ?? []).filter(o => o.threadId === root.id && !replies.some(m => m.id === o.serverId)).map(o => <article key={o.clientId} className="thread-reply pending-reply">
      <MessageBody source={o.body} fetchBlob={fetchBlob} chamberId={hubChamberId} />
      {o.state === 'failed' ? <button className="send-failed" onClick={() => retryOutboxItem(o)}>Failed — tap to retry{o.error ? ` · ${o.error}` : ''}</button>
        : <span role="status">{o.state === 'sending' ? 'Sending…' : 'Sent'}</span>}
    </article>)}
    <Composer chamberId={chamberId} threadId={root.id} />
  </section>
}
