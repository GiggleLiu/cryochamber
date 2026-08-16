import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useAppStore, useIsOwner } from '../store/appStore'
import { ApiError, isUnauthorized } from '../api/types'
import { UnresolvedProjectError } from '../api/hubClient'
import { MessageBody } from '../components/MessageBody'
import { Composer } from '../components/Composer'
import { AlertCircle, ArrowDown, ChevronLeft, Dots, Message, UserPlus } from '../components/Icon'
import { StatusDot } from '../components/StatusDot'
import { initial, separatorLabel, tileColor } from '../lib/format'
import { retryOutboxItem } from '../lib/outbox'
import { InviteSheet } from './InviteSheet'
import { ControlsSheet } from './ControlsSheet'

/** Messages this far apart start a new time-stamped block. */
const GAP_SECONDS = 300
/** How far from the bottom still counts as "reading the newest messages". */
const PIN_SLACK_PX = 80

/** True when the message carries genuinely wide block content (code, tables,
 * display math) that a hugging chat bubble would clip. Images, quotes, and
 * headings stay ordinary bubbles — they fit, and users expect the avatar and
 * alignment to stay put.
 *
 * Messages are markdown source, so the question is asked of the source: a
 * fence, a table row, or a display-math block. Every detector is anchored to a
 * line start, which is what keeps inline code, a pipe in prose, and "$5 and
 * $10" out of it. */
export function isRichMessage(content: string): boolean {
  return (
    /^\s{0,3}(```|~~~)/m.test(content) ||
    /^\s{0,3}\|/m.test(content) ||
    /^\s{0,3}\$\$/m.test(content)
  )
}

/** Mirrors the real message rows so nothing shifts when the thread arrives. */
function SkeletonThread() {
  return (
    <div className="skeleton-thread" aria-hidden="true">
      {(['w-1', 'w-3'] as const).map((w, i) => (
        <div className="skeleton-msg" key={i}>
          <div className="skeleton skeleton-avatar" />
          <div className={`skeleton skeleton-bubble ${w}`} />
        </div>
      ))}
      <div className="skeleton-msg skeleton-msg-self">
        <div className="skeleton skeleton-bubble w-2" />
      </div>
    </div>
  )
}

export function ConversationView({ streamId }: { streamId: number }) {
  const stream = useAppStore((s) => s.streams.find((x) => x.stream_id === streamId))
  const messages = useAppStore((s) => s.messagesByStream[streamId])
  const outbox = useAppStore((s) => s.outboxByStream[streamId])
  const loadedStreams = useAppStore((s) => s.loadedStreams)
  const creds = useAppStore((s) => s.creds)
  const selfName = useAppStore((s) => s.selfName)
  const client = useAppStore((s) => s.client)
  const ownUserId = useAppStore((s) => s.ownUserId)
  const setOwnUserId = useAppStore((s) => s.setOwnUserId)
  const navigate = useAppStore((s) => s.navigate)
  const isOwner = useIsOwner()
  const [sheet, setSheet] = useState<'invite' | 'controls' | null>(null)
  const chamberId = client?.chamberIdFor(streamId)
  // Memoised: MessageBody keys its decorate effect on this, and a fresh arrow
  // every render would tear down and rebuild its MutationObserver each time.
  const fetchBlob = useMemo(
    () => (client ? (url: string) => client.fetchBlob(url) : undefined),
    [client],
  )
  const [loadError, setLoadError] = useState<string | null>(null)
  const [retryToken, setRetryToken] = useState(0)
  const [showJump, setShowJump] = useState(false)
  const [hasNew, setHasNew] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)
  // Whether the reader is parked at the newest message. Kept in a ref because
  // the scroll handler and the message effect both read it without re-render.
  const pinnedRef = useRef(true)
  const name = stream?.name ?? ''
  const loaded = messages !== undefined
  // History is loaded when the stream is in loadedStreams, not merely when a
  // (possibly event-seeded) message list exists: after a re-register the marker
  // is cleared so gaps over cached messages get re-fetched.
  const historyLoaded = loadedStreams.includes(streamId)
  const messageCount = messages?.length ?? 0

  const scrollToLatest = useCallback((behavior: ScrollBehavior = 'smooth') => {
    const el = scrollRef.current
    if (!el) return
    // `scrollTo(options)` is not universal; the assignment is the fallback that
    // always works, just without the animation.
    if (typeof el.scrollTo === 'function') {
      el.scrollTo({ top: el.scrollHeight, behavior })
    } else {
      el.scrollTop = el.scrollHeight
    }
    pinnedRef.current = true
    setShowJump(false)
    setHasNew(false)
  }, [])

  function onScroll() {
    const el = scrollRef.current
    if (!el) return
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight
    const pinned = distance <= PIN_SLACK_PX
    pinnedRef.current = pinned
    setShowJump(!pinned)
    if (pinned) setHasNew(false)
  }

  // Opening a conversation lands on the newest message, with no visible glide.
  useEffect(() => {
    pinnedRef.current = true
    setShowJump(false)
    setHasNew(false)
    scrollToLatest('auto')
  }, [streamId, scrollToLatest])

  // New messages follow the reader: scroll if they are at the bottom, and
  // otherwise offer the jump chip rather than yanking the viewport.
  useEffect(() => {
    if (messageCount === 0) return
    if (pinnedRef.current) {
      scrollToLatest('auto')
    } else {
      setHasNew(true)
    }
  }, [messageCount, scrollToLatest])

  // Two things move the bottom of the list after it renders: images and math
  // finishing their load (the content grows), and the composer growing with a
  // multi-line draft (the viewport shrinks). Both used to leave the reader
  // stranded above the newest message, so watch for both.
  useEffect(() => {
    const el = scrollRef.current
    if (!el || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => {
      if (pinnedRef.current) scrollToLatest('auto')
    })
    observer.observe(el)
    for (const child of Array.from(el.children)) observer.observe(child)
    return () => observer.disconnect()
  }, [messageCount, scrollToLatest])

  useEffect(() => {
    if (!client || !stream || historyLoaded) return
    setLoadError(null)
    client
      .getMessages(name)
      .then((msgs) => useAppStore.getState().setMessages(streamId, msgs))
      .catch((e) => {
        if (isUnauthorized(e)) {
          // The client already signed the app out; nothing to add here.
        } else if (e instanceof ApiError && e.status === 404 && !(e instanceof UnresolvedProjectError)) {
          // Scope was revoked while we were looking at it: leave quietly — and
          // take the project with us, or it stays in the list and fails again
          // on every tap. Only the hub's own 404 says that; a name the client
          // could not resolve (register() has not run yet — offline cold boot)
          // takes the ordinary retryable error path, or an offline tap would
          // delete a cached project for good.
          useAppStore.getState().pruneStream(streamId)
        } else {
          setLoadError(e instanceof Error ? e.message : String(e))
        }
      })
  }, [client, stream, historyLoaded, name, streamId, navigate, retryToken])

  // Fetch the signed-in user's id once per session so their own @mentions can
  // be highlighted. A 401 already signed the app out inside the client;
  // transient errors are ignored silently (mentions render unhighlighted).
  useEffect(() => {
    if (!client || ownUserId !== null) return
    client
      .getOwnUser()
      .then((me) => setOwnUserId(me.user_id))
      .catch(() => {})
  }, [client, ownUserId, setOwnUserId])

  useEffect(() => {
    if (!client || !loaded) return
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | undefined
    let attempts = 0
    const attempt = (): void => {
      attempts += 1
      client.markStreamRead(streamId).catch((e) => {
        if (cancelled) return
        if (isUnauthorized(e)) return
        // Transient failure: retry once after 3s, then give up silently.
        if (attempts < 2) timer = setTimeout(attempt, 3000)
      })
    }
    attempt()
    useAppStore.getState().clearUnread(streamId)
    return () => {
      cancelled = true
      if (timer) clearTimeout(timer)
    }
  }, [client, streamId, loaded, messageCount])

  // The project can also vanish underneath us from a re-register that no longer
  // lists it. Rendering nothing would leave a blank screen with no way out, so
  // go where the user can act.
  useEffect(() => {
    if (!stream) navigate({ name: 'projects' })
  }, [stream, navigate])

  if (!stream || !creds) return null

  return (
    <div className="conversation">
      <header className="topbar">
        <button
          className="icon-btn bar-start"
          aria-label="Back"
          onClick={() => navigate({ name: 'projects' })}
        >
          <ChevronLeft />
        </button>
        <h1>
          {/* Liveness before the name: the state the composer note explains in
              words, readable at a glance without opening the controls sheet. */}
          <StatusDot running={stream.running} agentRunning={stream.agentRunning} />
          <span className="topbar-title-text">{stream.name}</span>
        </h1>
        {/* Owner-only, and absent (not disabled) for everyone else: a guest is
            never shown a control they cannot use. `chamberId` is unknown before
            the first register(), and an invite for an unknown chamber is not a
            thing we can mint. */}
        {isOwner && chamberId && (
          <div className="topbar-actions">
            <button className="icon-btn" aria-label="Invite" onClick={() => setSheet('invite')}>
              <UserPlus />
            </button>
            <button
              className="icon-btn"
              aria-label="Chamber controls"
              onClick={() => setSheet('controls')}
            >
              <Dots />
            </button>
          </div>
        )}
      </header>

      <div className="thread">
      <div className="message-scroll" ref={scrollRef} onScroll={onScroll}>
        {/* A mailbox returns its whole history in one fetch, so there is never
            anything earlier to load. */}
        {loadError && (
          <div className="alert" role="alert">
            <AlertCircle size={18} />
            <div className="alert-body">
              <strong>Couldn’t load this conversation.</strong>
              <p className="alert-detail">{loadError}</p>
              <button className="alert-action" onClick={() => setRetryToken((n) => n + 1)}>
                Try again
              </button>
            </div>
          </div>
        )}

        {!loaded && !loadError && <SkeletonThread />}

        {loaded && messages.length === 0 && !loadError && (
          <div className="empty-state">
            <Message size={40} />
            <h2>No messages yet</h2>
            <p>Send the first instruction and this project’s agent picks it up.</p>
          </div>
        )}

        {messages?.map((m, i) => {
          const prev = messages[i - 1]
          const gap = !prev || m.timestamp - prev.timestamp >= GAP_SECONDS
          const isSelf = m.sender_email === selfName
          // Runs of messages from one sender read as one turn: the repeated
          // avatar and name are noise, so only the first of a run carries them.
          const grouped = !gap && !!prev && prev.sender_email === m.sender_email
          const rich = isRichMessage(m.content)
          return (
            <Fragment key={m.id}>
              {gap && (
                <div className="time-pill">
                  {separatorLabel(m.timestamp, undefined, prev?.timestamp)}
                </div>
              )}
              <div
                className={
                  `msg-row ${isSelf ? 'msg-self' : 'msg-other'}` +
                  (grouped ? ' msg-grouped' : '') +
                  (rich ? ' msg-rich' : '')
                }
              >
                <div
                  className={`avatar${grouped ? ' avatar-hidden' : ''}`}
                  style={{ background: tileColor(m.sender_email) }}
                  aria-hidden="true"
                >
                  {initial(m.sender_full_name, m.sender_email)}
                </div>
                <div className="msg-col">
                  {!isSelf && !grouped && <div className="sender-label">{m.sender_full_name}</div>}
                  <div className="bubble">
                    <MessageBody
                      source={m.content}
                      fetchBlob={fetchBlob}
                      selfUserId={ownUserId ?? undefined}
                    />
                  </div>
                </div>
              </div>
            </Fragment>
          )
        })}

        {/* Unconfirmed sends live after the thread: the user sees their message
            land immediately, and a failure stays theirs to retry. */}
        {(outbox ?? []).map((o) => (
          <div className="msg-row msg-self msg-pending" key={o.localId}>
            <div className="msg-col">
              <div className="bubble">
                {/* Rendering the raw text approximates what the server will
                    echo back; it disappears the moment it does. */}
                <MessageBody source={o.content} />
              </div>
              {o.state === 'sending' ? (
                <div className="send-state">Sending…</div>
              ) : o.state === 'sent' ? (
                // Accepted, waiting for the thread to show it back.
                <div className="send-state">Sent</div>
              ) : (
                <button
                  className="send-state send-failed"
                  onClick={() => retryOutboxItem(o, name)}
                >
                  Failed — tap to retry
                </button>
              )}
            </div>
          </div>
        ))}
      </div>

      {showJump && (
        <button
          className={`jump-latest${hasNew ? ' has-new' : ''}`}
          onClick={() => scrollToLatest()}
        >
          <ArrowDown size={16} />
          {hasNew ? 'New messages' : 'Latest'}
        </button>
      )}
      </div>

      {/* Persistent, and the composer stays enabled: queuing for a sleeping
          agent is the intended way to use it, so this states where the message
          goes rather than standing in its way. */}
      {stream.running === false ? (
        <p className="asleep-note is-stopped" role="status">
          Chamber is not running — messages will wait in its inbox until it is started
        </p>
      ) : stream.agentRunning === false ? (
        <p className="asleep-note" role="status">
          {`Agent is asleep — messages will be read at the next wake${
            stream.nextWake ? ` · ${stream.nextWake}` : ''
          }`}
        </p>
      ) : null}

      <Composer streamName={name} streamId={streamId} />

      {sheet === 'invite' && chamberId && (
        <InviteSheet
          chamberId={chamberId}
          chamberName={stream.name}
          onClose={() => setSheet(null)}
        />
      )}

      {sheet === 'controls' && chamberId && (
        <ControlsSheet
          chamberId={chamberId}
          chamberName={stream.name}
          archived={stream.archived === true}
          onClose={() => setSheet(null)}
        />
      )}
    </div>
  )
}
