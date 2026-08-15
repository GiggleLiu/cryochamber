/** Shared authenticated-attachment helpers. */
import { ApiError } from '../api/errors'

/** Chamber attachment routes: /api/chambers/{id}/files/{name}. They need the
 * Authorization header, so a plain navigation would 401. */
export const HUB_FILES_RE = /^\/api\/chambers\/[^/]+\/files\//

/** Throws ApiError, not a bare Error: an attachment that comes back 401 is
 * the same revoked-credentials signal as any other request, and callers route
 * it through isAuthError to the one logout path. */
export function fetchBlob(url: string, authHeader: string): Promise<Blob> {
  return fetch(url, { headers: { Authorization: authHeader } }).then((res) => {
    if (!res.ok) throw new ApiError(`HTTP ${res.status}`, res.status)
    return res.blob()
  })
}

/** Last path segment of an attachment href (query/fragment stripped), URL-decoded. */
export function filenameFromHref(href: string): string {
  const path = href.split(/[?#]/)[0]
  const last = path.split('/').filter(Boolean).pop() ?? ''
  try {
    return decodeURIComponent(last)
  } catch {
    return last
  }
}

/** Save a blob under `name` via a temporary anchor. The object URL is revoked
 * lazily — Safari aborts the save when it is revoked synchronously. */
export function triggerBlobDownload(blob: Blob, name: string): void {
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = name
  document.body.appendChild(link)
  link.click()
  link.remove()
  setTimeout(() => URL.revokeObjectURL(url), 60_000)
}

/** Fetch an attachment with auth and save it. Throws on fetch failure. */
export async function downloadUpload(url: string, authHeader: string): Promise<void> {
  const blob = await fetchBlob(url, authHeader)
  triggerBlobDownload(blob, filenameFromHref(url))
}
