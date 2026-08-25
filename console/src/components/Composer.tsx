import { useEffect, useRef, useState } from 'react'
import { useAppStore } from '../store/appStore'
import { draftKey, sendViaOutbox } from '../lib/outbox'
import { accountKey } from '../lib/account'
import { isUnauthorized } from '../api/types'
import { IMAGE_EXT_RE } from '../lib/images'
import { AlertCircle, ArrowUp, Paperclip } from './Icon'

/**
 * True when the device most likely has a hardware keyboard, where Enter is
 * expected to send and Shift+Enter to insert a newline. On a touch keyboard
 * Enter must stay a newline — there is no modifier to fall back on.
 */
function hasHardwareKeyboard(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false
  return window.matchMedia('(hover: hover) and (pointer: fine)').matches
}

export function Composer({ chamberId }: { chamberId: string }) {
  const client = useAppStore((s) => s.client)
  const creds = useAppStore((s) => s.creds)
  const account = creds ? accountKey(creds) : ''
  // A half-written message is the user's work: it survives leaving the project,
  // closing the tab, and the app reloading, per project.
  const [text, setText] = useState(() => localStorage.getItem(draftKey(account, chamberId)) ?? '')
  const [uploading, setUploading] = useState(false)
  const [uploadName, setUploadName] = useState('')
  const [uploadIndex, setUploadIndex] = useState(0)
  const [uploadTotal, setUploadTotal] = useState(0)
  const [uploadError, setUploadError] = useState<string | null>(null)
  const [isDrop, setIsDrop] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const pendingCaret = useRef<number | null>(null)
  const uploadingRef = useRef(false)

  // After a programmatic insert (upload link), place the caret at the end
  // of the inserted text so further typing appends naturally.
  useEffect(() => {
    const ta = textareaRef.current
    if (ta && pendingCaret.current !== null) {
      ta.setSelectionRange(pendingCaret.current, pendingCaret.current)
      pendingCaret.current = null
    }
  })

  useEffect(() => {
    const key = draftKey(account, chamberId)
    if (text) localStorage.setItem(key, text)
    else localStorage.removeItem(key)
  }, [text, account, chamberId])

  // Grow the field with its content, up to the max-height the stylesheet sets
  // (~5 lines), after which it scrolls.
  useEffect(() => {
    const ta = textareaRef.current
    if (!ta) return
    ta.style.height = 'auto'
    ta.style.height = `${ta.scrollHeight}px`
  }, [text])

  /** Insert `[name](uri)` at the caret, space-separated from surrounding text —
   * or `![name](uri)` for a picture, so the recipient sees it inline instead of
   * a filename they have to click.
   * Reads the CURRENT value via functional setState — the upload that calls
   * this resolves asynchronously, and text typed while it was pending must
   * survive. */
  function insertLink(name: string, uri: string) {
    const ta = textareaRef.current
    setText((current) => {
      const caret = pendingCaret.current ??
        (ta ? Math.min(ta.selectionStart, current.length) : current.length)
      const before = current.slice(0, caret)
      const after = current.slice(caret)
      const link = `${IMAGE_EXT_RE.test(name) ? '!' : ''}[${name}](${uri})`
      const full =
        (before.length > 0 && !/\s$/.test(before) ? ' ' : '') +
        link +
        (after.length > 0 && !/^\s/.test(after) ? ' ' : '')
      pendingCaret.current = caret + full.length
      return before + full + after
    })
  }

  async function uploadFiles(files: File[]) {
    if (files.length === 0 || !client || uploadingRef.current) return
    uploadingRef.current = true
    setUploading(true)
    setUploadTotal(files.length)
    setUploadError(null)
    try {
      for (let i = 0; i < files.length; i += 1) {
        const file = files[i]
        setUploadName(file.name)
        setUploadIndex(i + 1)
        try {
          const uri = await client.uploadFile(file, chamberId)
          insertLink(file.name, uri)
        } catch (err) {
          if (isUnauthorized(err)) return
          const detail = err instanceof Error ? err.message : String(err)
          setUploadError(`Could not upload ${file.name}. ${detail}`)
          return
        }
      }
    } finally {
      uploadingRef.current = false
      setUploading(false)
      setUploadName('')
      setUploadIndex(0)
      setUploadTotal(0)
    }
  }

  function onFilePick(e: React.ChangeEvent<HTMLInputElement>) {
    const files = Array.from(e.target.files ?? [])
    // Reset immediately so the same files can be re-picked.
    e.target.value = ''
    void uploadFiles(files)
  }

  function hasDraggedFiles(e: React.DragEvent): boolean {
    return Array.from(e.dataTransfer.types).includes('Files')
  }

  function onDragOver(e: React.DragEvent<HTMLDivElement>) {
    if (!hasDraggedFiles(e)) return
    e.preventDefault()
    setIsDrop(true)
  }

  function onDragLeave(e: React.DragEvent<HTMLDivElement>) {
    if (e.relatedTarget instanceof Node && e.currentTarget.contains(e.relatedTarget)) return
    setIsDrop(false)
  }

  function onDrop(e: React.DragEvent<HTMLDivElement>) {
    if (!hasDraggedFiles(e)) return
    e.preventDefault()
    setIsDrop(false)
    void uploadFiles(Array.from(e.dataTransfer.files))
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    // During IME composition Enter commits the candidate — never intercept it.
    if (e.nativeEvent.isComposing) return
    // Enter sends on a hardware keyboard; Shift+Enter always inserts a newline.
    if (e.key === 'Enter' && !e.shiftKey && hasHardwareKeyboard()) {
      e.preventDefault()
      send()
    }
  }

  /** Optimistic: the message becomes a pending bubble in the thread and the
   * composer empties immediately, so the next thought can be typed while the
   * last one is still in flight. Retry lives on the bubble, not here. */
  function send() {
    if (!client || !text.trim()) return
    const content = text
    setText('')
    sendViaOutbox(chamberId, content)
  }

  return (
    <div
      className={`composer-dock${isDrop ? ' is-drop' : ''}`}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
    >
      {uploadError && (
        <p className="composer-alert" role="alert">
          <AlertCircle size={15} />
          {uploadError}
        </p>
      )}
      {uploading && (
        <p className="upload-status" role="status">
          Uploading {uploadName} ({uploadIndex} of {uploadTotal})…
        </p>
      )}
      <div className="composer">
        <button
          type="button"
          className="icon-btn"
          aria-label="Attach file"
          onClick={() => fileInputRef.current?.click()}
          disabled={uploading}
        >
          <Paperclip size={21} />
        </button>
        <input
          ref={fileInputRef}
          type="file"
          aria-label="Attach file"
          hidden
          multiple
          onChange={onFilePick}
        />
        <textarea
          ref={textareaRef}
          rows={1}
          value={text}
          aria-label="Message"
          placeholder="Message the agent…"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <button
          type="button"
          className="send-btn"
          aria-label="Send"
          onClick={send}
          disabled={uploading || !text.trim()}
        >
          <ArrowUp size={21} />
        </button>
      </div>
    </div>
  )
}
