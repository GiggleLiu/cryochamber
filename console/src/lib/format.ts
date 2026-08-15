/**
 * Date and text formatting shared by the conversation and project list.
 *
 * Month and weekday names are spelled out here rather than taken from
 * `toLocaleDateString` so a label reads identically on every device — the app
 * is English-only, and a separator that silently changes shape with the host
 * locale is a bug waiting to be filed against the day-grouping logic.
 */
const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
const MONTHS = [
  'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
  'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec',
]

const DAY_MS = 24 * 60 * 60 * 1000

function hhmm(d: Date): string {
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

/** Whole calendar days between `then` and `now`, in local time. */
function daysAgo(then: Date, now: Date): number {
  const a = new Date(then.getFullYear(), then.getMonth(), then.getDate()).getTime()
  const b = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  return Math.round((b - a) / DAY_MS)
}

/**
 * The separator shown above a message that opens a new day or follows a gap.
 *
 * It names the day whenever the day changes — `Today 19:32`, `Yesterday 19:32`,
 * `Fri 19:32`, `14 Aug 19:32`, `14 Aug 2025 19:32` — and drops to a bare
 * `19:32` when `previous` puts the message on the same calendar day. Without
 * `previous`, a mid-thread gap on the current day would read as a bare time
 * directly under a "Yesterday" separator, which tells the reader nothing.
 */
export function separatorLabel(
  timestamp: number,
  now: Date = new Date(),
  previous?: number,
): string {
  const d = new Date(timestamp * 1000)
  const time = hhmm(d)
  if (previous !== undefined && daysAgo(d, new Date(previous * 1000)) === 0) return time
  const days = daysAgo(d, now)
  if (days === 0) return `Today ${time}`
  if (days === 1) return `Yesterday ${time}`
  if (days > 1 && days < 7) return `${DAYS[d.getDay()]} ${time}`
  if (d.getFullYear() === now.getFullYear()) return `${d.getDate()} ${MONTHS[d.getMonth()]} ${time}`
  return `${d.getDate()} ${MONTHS[d.getMonth()]} ${d.getFullYear()} ${time}`
}

/** The compact right-hand timestamp on a project row. */
export function listTimeLabel(timestamp: number, now: Date = new Date()): string {
  const d = new Date(timestamp * 1000)
  const days = daysAgo(d, now)
  if (days === 0) return hhmm(d)
  if (days === 1) return 'Yesterday'
  if (days > 1 && days < 7) return DAYS[d.getDay()]
  if (d.getFullYear() === now.getFullYear()) return `${d.getDate()} ${MONTHS[d.getMonth()]}`
  return `${d.getDate()}/${d.getMonth() + 1}/${String(d.getFullYear()).slice(2)}`
}

/**
 * Plain-text one-liner for a project row, from a message body. Parsed as HTML,
 * never executed: `text/html` parsing runs no scripts and loads no resources,
 * and only `textContent` is read back out.
 */
export function previewText(html: string): string {
  const doc = new DOMParser().parseFromString(html, 'text/html')
  // Script and style bodies are text nodes too; they are code, not a preview.
  for (const el of Array.from(doc.querySelectorAll('script, style'))) el.remove()
  return (doc.body.textContent ?? '').replace(/\s+/g, ' ').trim()
}

/** Deterministic tile/avatar colour, keyed on an identity string. */
const TILE_COLORS = [
  '#3f7d5c', '#4a6fa5', '#8a5a3c', '#6b5b95',
  '#2f7f8f', '#8a6d3b', '#7a4b6b', '#4d6b4d',
]

export function tileColor(key: string): string {
  let hash = 0
  for (let i = 0; i < key.length; i += 1) {
    hash = (hash * 31 + key.charCodeAt(i)) >>> 0
  }
  return TILE_COLORS[hash % TILE_COLORS.length]
}

/** First character of a display name, uppercased; falls back to the key. */
export function initial(name: string, fallback = ''): string {
  const source = name || fallback
  return source ? source[0].toUpperCase() : '?'
}

/**
 * "added 3d ago" for an ISO timestamp the hub stamped (invite creation).
 *
 * Relative up to a month, because that is the window in which "how long ago"
 * is what the reader is actually asking; past that a date is more useful than
 * "47d ago". An unparseable string is shown verbatim rather than as `NaN`.
 */
export function relativeTimeLabel(iso: string, now: Date = new Date()): string {
  const ms = Date.parse(iso)
  if (Number.isNaN(ms)) return iso
  const minutes = Math.floor((now.getTime() - ms) / 60000)
  if (minutes < 1) return 'just now'
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days}d ago`
  return listTimeLabel(Math.floor(ms / 1000), now)
}
