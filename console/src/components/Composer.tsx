import { useEffect, useMemo, useRef, useState } from 'react'
import { useAppStore } from '../store/appStore'
import { draftKey, sendViaOutbox } from '../lib/outbox'
import { accountKey } from '../lib/account'
import { isUnauthorized } from '../api/types'
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

/**
 * Returns the @-mention query immediately before `caret` in `text`, or null
 * when the caret is not right after an `@`. Letters, numbers, underscores and
 * spaces are allowed so multi-word names autocomplete.
 */
export function mentionMatchAt(
  text: string,
  caret: number,
): { start: number; query: string } | null {
  // The `@` must start the text or follow whitespace — `foo@` is an email or
  // identifier fragment, not a mention trigger.
  const m = /(^|\s)@([\p{L}\p{N}_ ]*)$/u.exec(text.slice(0, caret))
  if (!m) return null
  return { start: caret - m[2].length - 1, query: m[2] }
}

export function mentionQueryAt(text: string, caret: number): string | null {
  return mentionMatchAt(text, caret)?.query ?? null
}

/** Names case-insensitively containing the query; prefix matches rank first,
 *  capped at the panel's 8-row limit. */
export function filterNames(names: string[], query: string): string[] {
  const q = query.trim().toLowerCase()
  if (!q) return names.slice(0, 8)
  const prefix = names.filter((n) => n.toLowerCase().startsWith(q))
  const rest = names.filter((n) => !n.toLowerCase().startsWith(q) && n.toLowerCase().includes(q))
  return [...prefix, ...rest].slice(0, 8)
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
  const [uploadError, setUploadError] = useState<string | null>(null)
  const [mentionOpen, setMentionOpen] = useState(false)
  const [mentionQuery, setMentionQuery] = useState('')
  const [mentionIndex, setMentionIndex] = useState(0)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const pendingCaret = useRef<number | null>(null)

  // The hub has no user directory, so mentions are whoever has spoken in this
  // conversation — the only names the agent will recognise anyway.
  const messages = useAppStore((s) => s.messagesByChamber[chamberId])
  const candidates = useMemo<string[]>(
    () => [...new Set((messages ?? []).map((m) => m.sender).filter(Boolean))],
    [messages],
  )

  const matches = useMemo(() => filterNames(candidates, mentionQuery), [candidates, mentionQuery])
  const panelVisible = mentionOpen && matches.length > 0

  // After a programmatic insert (mention confirm), place the caret at the end
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

  function closeMention() {
    setMentionOpen(false)
    setMentionQuery('')
    setMentionIndex(0)
  }

  /** Insert `[name](uri)` at the caret, space-separated from surrounding text.
   * Reads the CURRENT value via functional setState — the upload that calls
   * this resolves asynchronously, and text typed while it was pending must
   * survive. */
  function insertLink(name: string, uri: string) {
    const ta = textareaRef.current
    setText((current) => {
      const caret = ta ? Math.min(ta.selectionStart, current.length) : current.length
      const before = current.slice(0, caret)
      const after = current.slice(caret)
      const link = `[${name}](${uri})`
      const full =
        (before.length > 0 && !/\s$/.test(before) ? ' ' : '') +
        link +
        (after.length > 0 && !/^\s/.test(after) ? ' ' : '')
      pendingCaret.current = caret + full.length
      return before + full + after
    })
  }

  async function onFilePick(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    // Reset immediately so the same file can be re-picked.
    e.target.value = ''
    if (!file || !client || uploading) return
    setUploading(true)
    setUploadName(file.name)
    setUploadError(null)
    try {
      const uri = await client.uploadFile(file, chamberId)
      insertLink(file.name, uri)
    } catch (err) {
      if (isUnauthorized(err)) return
      setUploadError(err instanceof Error ? err.message : String(err))
    } finally {
      setUploading(false)
      setUploadName('')
    }
  }

  function confirmName(name: string) {
    const ta = textareaRef.current
    if (!ta) return
    const caret = ta.selectionStart
    const match = mentionMatchAt(text, caret)
    if (!match) {
      closeMention()
      return
    }
    const start = match.start
    const full = `@**${name}** `
    pendingCaret.current = start + full.length
    setText(text.slice(0, start) + full + text.slice(caret))
    closeMention()
  }

  function onChange(e: React.ChangeEvent<HTMLTextAreaElement>) {
    const ta = e.target
    const value = ta.value
    setText(value)
    const query = mentionQueryAt(value, ta.selectionStart)
    if (query !== null) {
      setMentionOpen(true)
      setMentionQuery(query)
      setMentionIndex(0)
    } else {
      closeMention()
    }
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    // During IME composition Enter commits the candidate — never intercept it,
    // whether or not the mention panel is showing.
    if (e.nativeEvent.isComposing) return
    if (!panelVisible) {
      // Enter sends on a hardware keyboard; Shift+Enter always inserts a newline.
      if (e.key === 'Enter' && !e.shiftKey && hasHardwareKeyboard()) {
        e.preventDefault()
        send()
      }
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setMentionIndex((i) => Math.min(i + 1, matches.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setMentionIndex((i) => Math.max(i - 1, 0))
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault()
      confirmName(matches[Math.min(mentionIndex, matches.length - 1)])
    } else if (e.key === 'Escape') {
      e.preventDefault()
      closeMention()
    }
  }

  /** Optimistic: the message becomes a pending bubble in the thread and the
   * composer empties immediately, so the next thought can be typed while the
   * last one is still in flight. Retry lives on the bubble, not here. */
  function send() {
    if (!client || !text.trim()) return
    const content = text
    setText('')
    closeMention()
    sendViaOutbox(chamberId, content)
  }

  return (
    <div className="composer-dock">
      {uploadError && (
        <p className="composer-alert" role="alert">
          <AlertCircle size={15} />
          {uploadError}
        </p>
      )}
      {uploading && (
        <p className="upload-status" role="status">Uploading {uploadName}…</p>
      )}
      {panelVisible && (
        <div className="mention-panel" role="listbox" aria-label="Mention users">
          {matches.map((name, i) => (
            <div
              key={name}
              role="option"
              aria-selected={i === mentionIndex}
              className={`mention-option${i === mentionIndex ? ' active' : ''}`}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => confirmName(name)}
            >
              {name}
            </div>
          ))}
        </div>
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
          onChange={onFilePick}
        />
        <textarea
          ref={textareaRef}
          rows={1}
          value={text}
          aria-label="Message"
          placeholder="Message the agent…"
          onChange={onChange}
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
