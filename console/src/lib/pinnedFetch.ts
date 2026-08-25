/**
 * The transport for a hub whose certificate the user pinned.
 *
 * The WebView cannot be told "trust exactly this certificate" — it either
 * trusts the system store or refuses the connection with an error that names
 * nothing. So a pinned hub is reached from Rust instead: `pinned_fetch` for
 * ordinary requests and `pinned_sse` for the one streaming endpoint, both
 * handshaking through the verifier that compares the certificate against the
 * pinned fingerprint before a byte of the token is sent.
 *
 * What this covers is exactly what `HubClient` and `readSse` ask of `fetch`:
 * string bodies, `FormData` uploads, blob downloads, and `/api/events`. It is
 * not a general `fetch` polyfill and does not pretend to be one.
 */
import type { HubTrust } from '../store/hubs'
import { tauriChannel, tauriInvoke } from './tauri'

/** What `pinned_fetch` answers with. Bodies cross the IPC boundary as base64
 * because the bridge speaks JSON and a response can be an image. */
interface PinnedResponse {
  status: number
  headers: [string, string][]
  body_b64: string
}

/** What `pinned_sse` pushes down its channel, in the shape Rust's untagged
 * enum serializes to: an open, then chunks, then a close. */
type SseEvent = { status: number } | { chunk_b64: string } | { done: boolean }

const B64_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'

const B64_REVERSE = /* @__PURE__ */ (() => {
  const table = new Uint8Array(256).fill(0xff)
  for (let i = 0; i < B64_ALPHABET.length; i++) table[B64_ALPHABET.charCodeAt(i)] = i
  return table
})()

/**
 * Bytes → base64, written out by hand rather than through `btoa`: the usual
 * `btoa(String.fromCharCode(...bytes))` spelling passes every byte as a
 * separate argument and overflows the call stack on any real upload.
 */
export function b64encode(bytes: Uint8Array): string {
  let out = ''
  let i = 0
  for (; i + 2 < bytes.length; i += 3) {
    const v = (bytes[i] << 16) | (bytes[i + 1] << 8) | bytes[i + 2]
    out +=
      B64_ALPHABET[(v >> 18) & 63] +
      B64_ALPHABET[(v >> 12) & 63] +
      B64_ALPHABET[(v >> 6) & 63] +
      B64_ALPHABET[v & 63]
  }
  const left = bytes.length - i
  if (left === 1) {
    const v = bytes[i] << 16
    out += B64_ALPHABET[(v >> 18) & 63] + B64_ALPHABET[(v >> 12) & 63] + '=='
  } else if (left === 2) {
    const v = (bytes[i] << 16) | (bytes[i + 1] << 8)
    out +=
      B64_ALPHABET[(v >> 18) & 63] +
      B64_ALPHABET[(v >> 12) & 63] +
      B64_ALPHABET[(v >> 6) & 63] +
      '='
  }
  return out
}

/** base64 → bytes. Unknown characters (padding, stray whitespace) are skipped
 * rather than decoded as zeroes. */
export function b64decode(text: string): Uint8Array {
  const out = new Uint8Array(Math.ceil((text.length * 3) / 4))
  let acc = 0
  let bits = 0
  let written = 0
  for (let i = 0; i < text.length; i++) {
    const six = B64_REVERSE[text.charCodeAt(i) & 0xff]
    if (six === 0xff) continue
    acc = (acc << 6) | six
    bits += 6
    if (bits >= 8) {
      bits -= 8
      out[written++] = (acc >> bits) & 0xff
    }
  }
  return written === out.length ? out : out.subarray(0, written)
}

/** A `multipart/form-data` body and the content type that describes it. */
export interface MultipartBody {
  bytes: Uint8Array
  contentType: string
}

/** Neither a field name nor a filename may end a header line or a quoted
 * string; browsers percent-encode exactly these three characters. */
function headerSafe(value: string): string {
  return value.replace(/\r/g, '%0D').replace(/\n/g, '%0A').replace(/"/g, '%22')
}

function boundaryToken(): string {
  const bytes = new Uint8Array(12)
  if (typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function') {
    crypto.getRandomValues(bytes)
  } else {
    for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(Math.random() * 256)
  }
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

function concat(parts: Uint8Array[]): Uint8Array {
  let total = 0
  for (const part of parts) total += part.length
  const out = new Uint8Array(total)
  let at = 0
  for (const part of parts) {
    out.set(part, at)
    at += part.length
  }
  return out
}

/**
 * Serialize a `FormData` the way the browser would before handing it to Rust.
 * Uploads have to survive pinning too, and the IPC bridge carries bytes, not
 * form objects — so the boundary is ours to pick and ours to declare.
 */
export async function formDataToMultipart(form: FormData): Promise<MultipartBody> {
  const boundary = `----CryoFormBoundary${boundaryToken()}`
  const encoder = new TextEncoder()
  const parts: Uint8Array[] = []
  for (const [name, value] of form.entries()) {
    const head = `--${boundary}\r\nContent-Disposition: form-data; name="${headerSafe(name)}"`
    if (typeof value === 'string') {
      parts.push(encoder.encode(`${head}\r\n\r\n${value}\r\n`))
      continue
    }
    const file = value as File
    const filename = headerSafe(typeof file.name === 'string' ? file.name : 'file')
    const type = file.type || 'application/octet-stream'
    parts.push(encoder.encode(`${head}; filename="${filename}"\r\nContent-Type: ${type}\r\n\r\n`))
    parts.push(await blobBytes(file))
    parts.push(encoder.encode('\r\n'))
  }
  parts.push(encoder.encode(`--${boundary}--\r\n`))
  return { bytes: concat(parts), contentType: `multipart/form-data; boundary=${boundary}` }
}

/**
 * A blob's bytes. `arrayBuffer()` is the whole story in any WebView the app
 * runs in; the `FileReader` path is for realms whose `Blob` predates it (the
 * console's own jsdom test realm is one), so an upload is never silently
 * serialized as an empty part.
 */
async function blobBytes(blob: Blob): Promise<Uint8Array> {
  if (typeof blob.arrayBuffer === 'function') return new Uint8Array(await blob.arrayBuffer())
  const buffer = await new Promise<ArrayBuffer>((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as ArrayBuffer)
    reader.onerror = () => reject(reader.error ?? new Error('Could not read the attached file.'))
    reader.readAsArrayBuffer(blob)
  })
  return new Uint8Array(buffer)
}

/** Every header the caller set, as the pairs the command takes. */
function headerPairs(init: HeadersInit | undefined): [string, string][] {
  if (!init) return []
  if (Array.isArray(init)) return init.map(([k, v]) => [k, v])
  if (typeof Headers !== 'undefined' && init instanceof Headers) {
    const out: [string, string][] = []
    init.forEach((value, key) => out.push([key, value]))
    return out
  }
  return Object.entries(init as Record<string, string>)
}

function urlOf(input: RequestInfo | URL): string {
  if (typeof input === 'string') return input
  if (input instanceof URL) return input.toString()
  return (input as Request).url
}

/** Statuses the `Response` constructor refuses to attach a body to. */
function bodyless(status: number): boolean {
  return status === 204 || status === 205 || status === 304
}

function toHeaders(pairs: [string, string][]): Headers {
  const headers = new Headers()
  for (const [key, value] of pairs) {
    // A hub could answer with a header name this realm rejects; one odd
    // header must not turn a good response into a failed request.
    try {
      headers.append(key, value)
    } catch {
      /* skipped */
    }
  }
  return headers
}

function asError(reason: unknown): Error {
  // A command rejection arrives as the plain string the Rust side returned.
  return reason instanceof Error ? reason : new Error(String(reason))
}

function abortError(signal: AbortSignal): unknown {
  return signal.reason ?? new DOMException('The request was aborted.', 'AbortError')
}

async function bodyBytes(body: BodyInit | null | undefined): Promise<Uint8Array | null> {
  if (body === null || body === undefined) return null
  if (typeof body === 'string') return new TextEncoder().encode(body)
  if (body instanceof Uint8Array) return body
  if (body instanceof ArrayBuffer) return new Uint8Array(body)
  if (ArrayBuffer.isView(body)) {
    return new Uint8Array(body.buffer, body.byteOffset, body.byteLength)
  }
  if (body instanceof Blob) return blobBytes(body)
  // Nothing in the console sends anything else; refusing loudly beats sending
  // `[object Object]` to a hub.
  throw new Error('This request body cannot be sent to a pinned hub.')
}

/** One id per stream for the life of the window: `pinned_sse_cancel` names the
 * stream to stop, and a reused id would stop somebody else's. */
let nextStreamId = 1

/**
 * The `/api/events` half: a `Response` whose body is fed by the command's
 * channel. The status has to be known before the `Response` exists, so the
 * fetch promise is held until the `Open` message arrives.
 */
function eventsRequest(
  url: string,
  sha256: string,
  headers: [string, string][],
  signal: AbortSignal | null | undefined,
): Promise<Response> {
  const streamId = nextStreamId++
  return new Promise<Response>((resolve, reject) => {
    if (signal?.aborted) {
      reject(abortError(signal))
      return
    }
    let controller: ReadableStreamDefaultController<Uint8Array> | null = null
    let opened = false
    let finished = false

    const stopHost = () => {
      void tauriInvoke<void>('pinned_sse_cancel', { streamId }).catch(() => {})
    }
    const finish = () => {
      finished = true
      if (signal) signal.removeEventListener('abort', onAbort)
    }
    /** Whichever end failed, the other one has to hear about it: before the
     * `Response` exists that means rejecting the fetch, after it means erroring
     * the body the caller is already reading. */
    const fail = (reason: unknown) => {
      if (finished) return
      finish()
      if (opened) controller?.error(reason)
      else reject(reason)
    }
    function onAbort() {
      if (finished) return
      stopHost()
      fail(abortError(signal as AbortSignal))
    }

    const stream = new ReadableStream<Uint8Array>({
      start(c) {
        controller = c
      },
      cancel() {
        // `readSse` cancels its reader on every exit path; that is the signal
        // to let go of the connection on the Rust side too.
        if (finished) return
        finish()
        stopHost()
      },
    })
    if (signal) signal.addEventListener('abort', onAbort, { once: true })

    const channel = tauriChannel<SseEvent>((msg) => {
      if (finished) return
      if ('status' in msg) {
        opened = true
        resolve(new Response(bodyless(msg.status) ? null : stream, { status: msg.status }))
      } else if ('chunk_b64' in msg) {
        controller?.enqueue(b64decode(msg.chunk_b64))
      } else if ('done' in msg) {
        finish()
        controller?.close()
      }
    })

    tauriInvoke<void>('pinned_sse', { streamId, url, sha256, headers, onEvent: channel })
      .then(() => {
        // A clean return after `Done` is the ordinary case and has nothing
        // left to do; a clean return before it means the hub hung up.
        if (finished) return
        finish()
        if (opened) controller?.close()
        else reject(new Error('The hub closed the connection before answering.'))
      })
      .catch((reason) => fail(asError(reason)))
  })
}

/**
 * A `fetch` for one pinned hub. Nothing is asked of the shell until a request
 * is actually made — `bootApp` builds a transport for every hub before it
 * fetches anything, and a throw at construction would take the boot with it.
 *
 * A hub with any other trust belongs on the plugin fetch; asking for one here
 * fails on the first request rather than connecting to an unpinned hub through
 * a transport whose entire job is the pin.
 */
export function pinnedFetch(hub: { trust: HubTrust }): typeof fetch {
  const sha256 = hub.trust.kind === 'pinned' ? hub.trust.sha256 : null
  return async (input: RequestInfo | URL, init: RequestInit = {}): Promise<Response> => {
    if (!sha256) throw new Error('This hub has no pinned certificate.')
    const url = urlOf(input)
    const headers = headerPairs(init.headers)
    // The one streaming endpoint. Everything else is small enough to buffer,
    // and buffering is what keeps the IPC hop a single round trip — it is also
    // the only path with an `AbortSignal`: nothing in the console aborts a
    // buffered request, and `pinned_fetch` has no cancel to offer it.
    if (new URL(url, 'http://hub.invalid').pathname.endsWith('/api/events')) {
      return eventsRequest(url, sha256, headers, init.signal)
    }
    let body: Uint8Array | null
    if (init.body instanceof FormData) {
      const multipart = await formDataToMultipart(init.body)
      // The boundary is only known once the body is built, so this header can
      // only be set here — which is also why `HubClient` must not set its own.
      headers.push(['Content-Type', multipart.contentType])
      body = multipart.bytes
    } else {
      body = await bodyBytes(init.body)
    }
    const res = await tauriInvoke<PinnedResponse>('pinned_fetch', {
      req: {
        url,
        method: (init.method ?? 'GET').toUpperCase(),
        headers,
        body_b64: body ? b64encode(body) : null,
        sha256,
      },
    }).catch((reason) => {
      throw asError(reason)
    })
    const bytes = b64decode(res.body_b64 ?? '')
    return new Response(bodyless(res.status) ? null : bytes, {
      status: res.status,
      headers: toHeaders(res.headers ?? []),
    })
  }
}
