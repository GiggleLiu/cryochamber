import { useEffect, useMemo, useRef, useState } from 'react'
import { useAppStore, AUTH_LOGOUT_REASON } from '../store/appStore'
import { draftKey, sendViaOutbox } from '../lib/outbox'
import { accountKey } from '../lib/account'
import { isAuthError } from '../api/client'
import { AlertCircle, ArrowUp, Paperclip } from './Icon'
import type { ZulipUser } from '../api/types'

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

/** Users whose full_name case-insensitively includes the query; prefix matches
 *  rank first, capped at the panel's 8-row limit. */
export function filterUsers(users: ZulipUser[], query: string): ZulipUser[] {
  const q = query.trim().toLowerCase()
  if (!q) return users.slice(0, 8)
  const prefix = users.filter((u) => u.full_name.toLowerCase().startsWith(q))
  const rest = users.filter(
    (u) => !u.full_name.toLowerCase().startsWith(q) && u.full_name.toLowerCase().includes(q),
  )
  return [...prefix, ...rest].slice(0, 8)
}

export function Composer({ streamName, streamId }: { streamName: string; streamId: number }) {
  const client = useAppStore((s) => s.client)
  const users = useAppStore((s) => s.users)
  const setUsers = useAppStore((s) => s.setUsers)
  const creds = useAppStore((s) => s.creds)
  const account = creds ? accountKey(creds) : ''
  // A half-written message is the user's work: it survives leaving the project,
  // closing the tab, and the app reloading, per project.
  const [text, setText] = useState(() => localStorage.getItem(draftKey(account, streamId)) ?? '')
  const [uploading, setUploading] = useState(false)
  const [uploadName, setUploadName] = useState('')
  const [uploadError, setUploadError] = useState<string | null>(null)
  const [mentionOpen, setMentionOpen] = useState(false)
  const [mentionQuery, setMentionQuery] = useState('')
  const [mentionIndex, setMentionIndex] = useState(0)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const usersRequested = useRef(false)
  const pendingCaret = useRef<number | null>(null)

  // Hub has no user directory (getUsers returns []), so mentions fall back to
  // whoever has spoken in this conversation.
  const messages = useAppStore((s) => {
    const stream = s.streams.find((x) => x.name === streamName)
    return stream ? s.messagesByStream[stream.stream_id] : undefined
  })
  const candidates = useMemo<ZulipUser[]>(
    () =>
      users && users.length > 0
        ? users
        : [
            ...new Map(
              (messages ?? []).map((m) => [
                m.sender_email,
                { user_id: 0, full_name: m.sender_full_name, email: m.sender_email, is_bot: false },
              ]),
            ).values(),
          ],
    [users, messages],
  )

  const matches = useMemo(() => filterUsers(candidates, mentionQuery), [candidates, mentionQuery])
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
    const key = draftKey(account, streamId)
    if (text) localStorage.setItem(key, text)
    else localStorage.removeItem(key)
  }, [text, account, streamId])

  // Grow the field with its content, up to the max-height the stylesheet sets
  // (~5 lines), after which it scrolls.
  useEffect(() => {
    const ta = textareaRef.current
    if (!ta) return
    ta.style.height = 'auto'
    ta.style.height = `${ta.scrollHeight}px`
  }, [text])

  function fetchUsers() {
    if (!client || usersRequested.current) return
    usersRequested.current = true
    try {
      client
        .getUsers()
        .then((list) => setUsers(list))
        .catch(() => closeMention())
    } catch {
      closeMention()
    }
  }

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
      const uri = await client.uploadFile(file, streamName)
      insertLink(file.name, uri)
    } catch (err) {
      if (isAuthError(err)) {
        useAppStore.getState().logout(AUTH_LOGOUT_REASON)
        return
      }
      setUploadError(err instanceof Error ? err.message : String(err))
    } finally {
      setUploading(false)
      setUploadName('')
    }
  }

  function confirmUser(user: ZulipUser) {
    const ta = textareaRef.current
    if (!ta) return
    const caret = ta.selectionStart
    const match = mentionMatchAt(text, caret)
    if (!match) {
      closeMention()
      return
    }
    const start = match.start
    const full = `@**${user.full_name}** `
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
      if (users === null) fetchUsers()
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
      confirmUser(matches[Math.min(mentionIndex, matches.length - 1)])
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
    sendViaOutbox(streamId, streamName, content)
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
          {matches.map((u, i) => (
            <div
              // Fallback candidates all carry user_id 0; email is the stable key.
              key={u.email || u.user_id}
              role="option"
              aria-selected={i === mentionIndex}
              className={`mention-option${i === mentionIndex ? ' active' : ''}`}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => confirmUser(u)}
            >
              {u.full_name}
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
