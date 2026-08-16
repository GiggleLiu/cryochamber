/** Shared attachment helpers. Fetching is the client's job (it owns the token
 * and the 401 hook); what stays here is everything about the href and the save. */

/** Chamber attachment routes: /api/chambers/{id}/files/{name}. They need the
 * Authorization header, so a plain navigation would 401. */
export const HUB_FILES_RE = /^\/api\/chambers\/[^/]+\/files\//

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

/** Fetch with the given authenticated fetcher and save under the href's name. */
export async function downloadUpload(
  fetchBlob: (url: string) => Promise<Blob>,
  url: string,
): Promise<void> {
  const blob = await fetchBlob(url)
  triggerBlobDownload(blob, filenameFromHref(url))
}
