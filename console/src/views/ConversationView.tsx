import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { ACCESS_REVOKED_NOTICE, selfNameFor, useAppStore, useIsOwner } from '../store/appStore'
import { ApiError, isUnauthorized } from '../api/types'
import { splitChamberKey } from '../lib/hubKeys'
import { MessageBody } from '../components/MessageBody'
import { Composer } from '../components/Composer'
import { AlertCircle, ArrowDown, ChevronLeft, Dots, Message, UserPlus } from '../components/Icon'
import { StatusDot } from '../components/StatusDot'
import { exactTimestamp, initial, messageSeconds, separatorLabel, tileColor } from '../lib/format'
import { retryOutboxItem } from '../lib/outbox'
import { InviteSheet } from './InviteSheet'
import { ControlsSheet } from './ControlsSheet'

/** Messages this far apart start a new time-stamped block. */
const GAP_SECONDS = 300
/** How far from the bottom still counts as "reading the newest messages". */
const PIN_SLACK_PX = 80
const PAGE = 100

function hasFinePointer(): boolean {
  return typeof window.matchMedia === 'function' &&
    window.matchMedia('(hover: hover) and (pointer: fine)').matches
}

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

export function ConversationView({ chamberId }: { chamberId: string }) {
  const chamber = useAppStore((s) => s.chambers.find((c) => c.id === chamberId))
  const messages = useAppStore((s) => s.messagesByChamber[chamberId])
  const outbox = useAppStore((s) => s.outboxByChamber[chamberId])
  const loadedChambers = useAppStore((s) => s.loadedChambers)
  // What makes a bubble "mine" is what *this chamber's hub* calls our token:
  // two hubs can name the same person differently.
  const selfName = useAppStore((s) => selfNameFor(s, chamberId))
  const client = useAppStore((s) => s.client)
  const mode = useAppStore((s) => s.mode)
  const navigate = useAppStore((s) => s.navigate)
  // Owner of this chamber's hub — a token can own one hub and be a guest on
  // the next, so the question is only ever asked about a chamber.
  const isOwner = useIsOwner(chamberId)
  const [sheet, setSheet] = useState<'invite' | 'controls' | null>(null)
  // Memoised: MessageBody keys its decorate effect on this, and a fresh arrow
  // every render would tear down and rebuild its MutationObserver each time.
  const fetchBlob = useMemo(
    // The chamber key names the hub the file lives on; browser mode's client
    // ignores it, so the request it makes is byte-identical.
    () => (client ? (url: string) => client.fetchBlobFor(chamberId, url) : undefined),
    [client, chamberId],
  )
  // What the hub itself calls this chamber. An attachment URL built from a
  // chamber-relative link goes on the wire, and the `{hubId}:` prefix is the
  // router's own bookkeeping — never part of a path a hub serves. Asked only
  // in app mode: browser-mode ids are raw, and one that happened to start
  // `{8 hex}:` would otherwise be mistaken for a key and truncated.
  const hubChamberId = useMemo(
    () => (mode === 'app' ? splitChamberKey(chamberId).chamberId : chamberId),
    [mode, chamberId],
  )
  const [loadError, setLoadError] = useState<string | null>(null)
  const [retryToken, setRetryToken] = useState(0)
  const [showJump, setShowJump] = useState(false)
  const [hasNew, setHasNew] = useState(false)
  const [copiedMessageId, setCopiedMessageId] = useState<string | null>(null)
  const [canCopy] = useState(hasFinePointer)
  const [visibleCount, setVisibleCount] = useState(PAGE)
  const scrollRef = useRef<HTMLDivElement>(null)
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const prependHeightRef = useRef<number | null>(null)
  // Whether the reader is parked at the newest message. Kept in a ref because
  // the scroll handler and the message effect both read it without re-render.
  const pinnedRef = useRef(true)
  const loaded = messages !== undefined
  // History is loaded when the chamber is in loadedChambers, not merely when a
  // (possibly event-seeded) message list exists: after an index re-read the
  // marker is cleared so gaps over cached messages get re-fetched.
  const historyLoaded = loadedChambers.includes(chamberId)
  const messageCount = messages?.length ?? 0
  const firstVisible = Math.max(0, messageCount - visibleCount)
  const visibleMessages = messages?.slice(firstVisible)

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

  function revealEarlier() {
    const el = scrollRef.current
    if (el) prependHeightRef.current = el.scrollHeight
    setVisibleCount((count) => Math.min(messageCount, count + PAGE))
  }

  useLayoutEffect(() => {
    const before = prependHeightRef.current
    const el = scrollRef.current
    if (before === null || !el) return
    el.scrollTop += el.scrollHeight - before
    prependHeightRef.current = null
  }, [visibleCount])

  useEffect(() => {
    prependHeightRef.current = null
    setVisibleCount(PAGE)
  }, [chamberId])

  async function copyMessage(id: string, body: string) {
    if (!navigator.clipboard) return
    try {
      await navigator.clipboard.writeText(body)
      setCopiedMessageId(id)
      if (copyTimerRef.current !== null) clearTimeout(copyTimerRef.current)
      copyTimerRef.current = setTimeout(() => setCopiedMessageId(null), 1500)
    } catch {
      // Clipboard permission can be denied; leaving the label unchanged is
      // more honest than claiming the message was copied.
    }
  }

  useEffect(
    () => () => {
      if (copyTimerRef.current !== null) clearTimeout(copyTimerRef.current)
    },
    [],
  )

  // Opening a conversation lands on the newest message, with no visible glide.
  useEffect(() => {
    pinnedRef.current = true
    setShowJump(false)
    setHasNew(false)
    scrollToLatest('auto')
  }, [chamberId, scrollToLatest])

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
    if (!client || !chamber || historyLoaded) return
    setLoadError(null)
    client
      .getMessages(chamberId)
      .then((msgs) => useAppStore.getState().setMessages(chamberId, msgs))
      .catch((e) => {
        if (isUnauthorized(e)) return
        if (e instanceof ApiError && (e.status === 403 || e.status === 404)) {
          // Scope was revoked while we were looking at it: leave quietly — and
          // take the chamber with us, or it stays in the list and fails again
          // on every tap. Only the hub's own answer says that; a transport
          // failure takes the retryable path below, or an offline tap would
          // delete a cached chamber for good.
          useAppStore.getState().pruneChamber(chamberId, ACCESS_REVOKED_NOTICE)
          return
        }
        setLoadError(
          e instanceof ApiError && e.hubSaid
            ? e.message
            : 'Check your connection and try again.',
        )
      })
  }, [client, chamber, historyLoaded, chamberId, retryToken])

  // Reading a conversation is what marks it read: the watermark moves to the
  // newest message on screen, which is the whole of the unread accounting.
  useEffect(() => {
    if (loaded) useAppStore.getState().markRead(chamberId)
  }, [chamberId, loaded, messageCount])

  // The chamber can also vanish underneath us from an index re-read that no
  // longer lists it. Rendering nothing would leave a blank screen with no way
  // out, so go where the user can act — but only if the view still points
  // here: `pruneChamber` navigates as it drops the chamber, and navigating a
  // second time would clear the notice it left explaining why.
  useEffect(() => {
    if (chamber) return
    const current = useAppStore.getState().view
    if (current.name === 'conversation' && current.chamberId === chamberId) {
      navigate({ name: 'projects' }, { replace: true })
    }
  }, [chamber, chamberId, navigate])

  if (!chamber) return null

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
          <StatusDot running={chamber.running} agentRunning={chamber.agentRunning} />
          <span className="topbar-title-text">{chamber.name}</span>
        </h1>
        {/* Owner-only, and absent (not disabled) for everyone else: a guest is
            never shown a control they cannot use. */}
        {isOwner && (
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
        {/* The mailbox fetch is complete, but only a bounded window mounts. */}
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

        {firstVisible > 0 && (
          <button type="button" className="stream-reveal" onClick={revealEarlier}>
            Earlier messages ({firstVisible})
          </button>
        )}

        {visibleMessages?.map((m, i) => {
          const prev = visibleMessages[i - 1]
          const seconds = messageSeconds(m)
          const gap = !prev || seconds - messageSeconds(prev) >= GAP_SECONDS
          const isSelf = m.sender === selfName
          // Runs of messages from one sender read as one turn: the repeated
          // avatar and name are noise, so only the first of a run carries them.
          const grouped = !gap && !!prev && prev.sender === m.sender
          const rich = isRichMessage(m.body)
          return (
            <Fragment key={m.id}>
              {gap && (
                <div className="time-pill">
                  {separatorLabel(seconds, undefined, prev ? messageSeconds(prev) : undefined)}
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
                  style={{ background: tileColor(m.sender) }}
                  aria-hidden="true"
                >
                  {initial(m.sender)}
                </div>
                <div className="msg-col">
                  {!isSelf && !grouped && <div className="sender-label">{m.sender}</div>}
                  <div className="bubble" title={exactTimestamp(m)}>
                    {canCopy && (
                      <button
                        type="button"
                        className="bubble-copy"
                        onClick={() => void copyMessage(m.id, m.body)}
                      >
                        {copiedMessageId === m.id ? 'Copied' : 'Copy'}
                      </button>
                    )}
                    <MessageBody source={m.body} fetchBlob={fetchBlob} chamberId={hubChamberId} />
                  </div>
                </div>
              </div>
            </Fragment>
          )
        })}

        {/* Unconfirmed sends live after the thread: the user sees their message
            land immediately, and a failure stays theirs to retry. */}
        {(outbox ?? []).map((o) => (
          <div className="msg-row msg-self msg-pending" key={o.clientId}>
            <div className="msg-col">
              <div className="bubble">
                {/* Rendering the raw text approximates what the thread will
                    show back; it disappears the moment it does. */}
                <MessageBody source={o.body} chamberId={hubChamberId} />
              </div>
              {o.state === 'sending' ? (
                <div className="send-state">Sending…</div>
              ) : o.state === 'sent' ? (
                // Accepted, waiting for the thread to show it back.
                <div className="send-state">Sent</div>
              ) : (
                <button
                  className="send-state send-failed"
                  onClick={() => retryOutboxItem(o)}
                >
                  {/* The hub's own sentence when it gave one: "rate limited"
                      is the difference between retrying now and waiting. */}
                  Failed — tap to retry{o.error ? ` · ${o.error}` : ''}
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
      {chamber.running === false ? (
        <p className="asleep-note is-stopped" role="status">
          Chamber is not running — messages will wait in its inbox until it is started
        </p>
      ) : !chamber.agentRunning ? (
        <p className="asleep-note" role="status">
          {`Agent is asleep — messages will be read at the next wake${
            chamber.nextWakeDisplay ? ` · ${chamber.nextWakeDisplay}` : ''
          }`}
        </p>
      ) : null}

      <Composer chamberId={chamberId} />

      {sheet === 'invite' && (
        <InviteSheet
          chamberId={chamberId}
          chamberName={chamber.name}
          onClose={() => setSheet(null)}
        />
      )}

      {sheet === 'controls' && (
        <ControlsSheet
          chamberId={chamberId}
          chamberName={chamber.name}
          archived={chamber.archived}
          onClose={() => setSheet(null)}
        />
      )}
    </div>
  )
}
