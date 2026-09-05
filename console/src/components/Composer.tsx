import { useEffect, useRef, useState } from 'react'
import { useAppStore } from '../store/appStore'
import { draftKey, sendViaOutbox } from '../lib/outbox'
import { accountKey } from '../lib/account'
import { IMAGE_EXT_RE } from '../lib/images'
import { ArrowUp, Paperclip } from './Icon'
import { attachmentMarkdown, fileSize } from '../lib/attachments'

interface StagedFile {
  id: string
  name: string
  size: number
  file?: File
  preview?: string
  url?: string
  error?: string
  state: 'queued' | 'uploading' | 'ready' | 'failed'
}

function hasHardwareKeyboard(): boolean {
  return typeof window.matchMedia === 'function' && window.matchMedia('(hover: hover) and (pointer: fine)').matches
}

/** The key remounts drafts and upload state when switching conversations. */
export function Composer({ chamberId, threadId }: { chamberId: string; threadId?: string }) {
  const creds = useAppStore(s => s.creds)
  const account = creds ? accountKey(creds) : 'app'
  const key = draftKey(account, threadId ? `${chamberId}.thread.${threadId}` : chamberId)
  return <DraftComposer key={key} chamberId={chamberId} threadId={threadId} storageKey={key} />
}

function DraftComposer({ chamberId, threadId, storageKey }: { chamberId: string; threadId?: string; storageKey: string }) {
  const client = useAppStore(s => s.client)
  const [text, setText] = useState(() => localStorage.getItem(storageKey) ?? '')
  const [files, setFiles] = useState<StagedFile[]>(() => {
    try {
      const saved: unknown = JSON.parse(localStorage.getItem(`${storageKey}.files`) ?? '[]')
      return Array.isArray(saved) ? saved.filter((f): f is StagedFile =>
        f && typeof f.id === 'string' && typeof f.name === 'string' && typeof f.size === 'number'
        && typeof f.url === 'string' && f.url.startsWith('/api/chambers/') && f.state === 'ready') : []
    } catch { return [] }
  })
  const [notice, setNotice] = useState('')
  const [isDrop, setIsDrop] = useState(false)
  const ta = useRef<HTMLTextAreaElement>(null)
  const input = useRef<HTMLInputElement>(null)
  const alive = useRef(true)
  const busy = useRef(false)
  const previews = useRef(new Set<string>())
  const pending = files.some(f => f.state !== 'ready')

  useEffect(() => {
    alive.current = true
    const urls = previews.current
    return () => { alive.current = false; urls.forEach(url => URL.revokeObjectURL(url)) }
  }, [])
  useEffect(() => {
    if (text) localStorage.setItem(storageKey, text)
    else localStorage.removeItem(storageKey)
  }, [text, storageKey])
  useEffect(() => {
    const ready = files.filter(f => f.state === 'ready').map(({ id, name, size, url, state }) => ({ id, name, size, url, state }))
    if (ready.length) localStorage.setItem(`${storageKey}.files`, JSON.stringify(ready))
    else localStorage.removeItem(`${storageKey}.files`)
  }, [files, storageKey])
  useEffect(() => {
    if (!ta.current) return
    ta.current.style.height = 'auto'
    ta.current.style.height = `${ta.current.scrollHeight + ta.current.offsetHeight - ta.current.clientHeight}px`
  }, [text])

  // One upload at a time uses the existing authenticated transport on web and native.
  useEffect(() => {
    const next = files.find(f => f.state === 'queued')
    if (!next?.file || !client || busy.current) return
    busy.current = true
    setFiles(current => current.map(f => f.id === next.id ? { ...f, state: 'uploading' } : f))
    void client.uploadFile(next.file, chamberId).then(url => {
      if (alive.current) setFiles(current => current.map(f => f.id === next.id ? { ...f, url, file: undefined, state: 'ready' } : f))
    }, (error: unknown) => {
      if (alive.current) setFiles(current => current.map(f => f.id === next.id ? {
        ...f, state: 'failed', error: error instanceof Error ? error.message : 'Upload failed',
      } : f))
    }).finally(() => {
      busy.current = false
      if (alive.current) setFiles(current => [...current])
    })
  }, [files, client, chamberId])

  function addFiles(incoming: File[]) {
    setNotice('')
    const added: StagedFile[] = []
    for (const file of incoming) {
      if (file.size > 25 * 1024 * 1024) { setNotice(`${file.name} exceeds the 25 MB file limit.`); continue }
      if (files.length + added.length >= 10) { setNotice('Attach up to 10 files per message.'); break }
      const preview = IMAGE_EXT_RE.test(file.name) && !/\.svg$/i.test(file.name) && typeof URL.createObjectURL === 'function'
        ? URL.createObjectURL(file) : undefined
      if (preview) previews.current.add(preview)
      added.push({ id: crypto.randomUUID(), name: file.name, size: file.size, file, preview, state: 'queued' })
    }
    setFiles(current => [...current, ...added])
  }

  function remove(file: StagedFile) {
    setFiles(current => current.filter(f => f.id !== file.id))
    if (file.preview) { URL.revokeObjectURL(file.preview); previews.current.delete(file.preview) }
  }

  function send() {
    if (!client || pending || (!text.trim() && !files.length)) return
    const body = [text.trim(), ...files.map(f => attachmentMarkdown(f.name, f.url!, f.size))].filter(Boolean).join('\n\n')
    sendViaOutbox(chamberId, body, threadId)
    setText('')
    setFiles([])
    previews.current.forEach(url => URL.revokeObjectURL(url))
    previews.current.clear()
  }

  return <div className={`composer-dock${isDrop ? ' is-drop' : ''}`}
    onDragOver={e => { if (Array.from(e.dataTransfer.types).includes('Files')) { e.preventDefault(); setIsDrop(true) } }}
    onDragLeave={e => { if (!(e.relatedTarget instanceof Node) || !e.currentTarget.contains(e.relatedTarget)) setIsDrop(false) }}
    onDrop={e => { if (Array.from(e.dataTransfer.types).includes('Files')) { e.preventDefault(); setIsDrop(false); addFiles(Array.from(e.dataTransfer.files)) } }}>
    {notice && <p role="alert" className="composer-alert">{notice}</p>}
    {files.length > 0 && <ul className="staged-files" aria-label="Attachments">
      {files.map(file => <li key={file.id} className="staged-file">
        {file.preview ? <img src={file.preview} alt={file.name} /> : <Paperclip size={22} />}
        <div className="staged-file-info"><strong>{file.name}</strong><small>{fileSize(file.size)}</small>
          {(file.state === 'queued' || file.state === 'uploading') && <span role="status">Uploading {file.name}… <progress aria-label={`Uploading ${file.name}`} /></span>}
          {file.state === 'failed' && <><span role="alert">Could not upload {file.name}. {file.error}</span>
            <button type="button" onClick={() => setFiles(current => current.map(f => f.id === file.id ? { ...f, state: 'queued', error: undefined } : f))}>Retry {file.name}</button></>}
        </div>
        <button type="button" className="attachment-remove" aria-label={`Remove ${file.name}`} onClick={() => remove(file)}>×</button>
      </li>)}
    </ul>}
    <div className="composer">
      <button type="button" className="icon-btn" aria-label="Attach file" onClick={() => input.current?.click()}><Paperclip size={21} /></button>
      <input ref={input} type="file" aria-label="Attach file" hidden multiple onChange={e => { addFiles(Array.from(e.target.files ?? [])); e.target.value = '' }} />
      <textarea ref={ta} rows={1} value={text} aria-label={threadId ? 'Thread reply' : 'Message'} placeholder={threadId ? 'Reply in thread…' : 'Message the agent…'}
        onChange={e => setText(e.target.value)}
        onPaste={e => { const pasted = Array.from(e.clipboardData.files); if (pasted.length) { e.preventDefault(); addFiles(pasted) } }}
        onKeyDown={e => { if (!e.nativeEvent.isComposing && e.key === 'Enter' && !e.shiftKey && hasHardwareKeyboard()) { e.preventDefault(); send() } }} />
      <button type="button" className="send-btn" aria-label={threadId ? 'Send reply' : 'Send'} onClick={send} disabled={pending || (!text.trim() && !files.length)}><ArrowUp size={21} /></button>
    </div>
  </div>
}
