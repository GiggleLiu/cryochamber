/** Shared attachment helpers. Fetching is the client's job (it owns the token
 * and the 401 hook); what stays here is everything about the href and the save. */

/** Chamber attachment routes: /api/chambers/{id}/files/{name}. They need the
 * Authorization header, so a plain navigation would 401. */
export const HUB_FILES_RE = /^\/api\/chambers\/[^/]+\/files\//

/** Chamber-file route: /api/chambers/{id}/file?path=… — chamber-local
 * research artifacts (articles/, .knowledge/) served with the same auth. */
export const CHAMBER_FILE_RE = /^\/api\/chambers\/[^/]+\/file\?path=/

/** A chamber-relative link the agent put in a message (e.g.
 * `articles/review.pdf`) — a file it produced on disk, served through the
 * authenticated chamber-file route below. Anything with a scheme, or rooted
 * at `/`, `#` or `?`, is not ours to resolve: it is external or SPA-local. */
export function isChamberRelativePath(href: string): boolean {
  if (HUB_FILES_RE.test(href)) return false
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(href)) return false // scheme
  if (href.startsWith('/') || href.startsWith('#') || href.startsWith('?')) return false
  return href.length > 0
}

/** Authenticated URL for a chamber-local file at a chamber-relative path.
 *
 * The href may already carry percent-escapes (a space rendered as `%20`), so
 * it is decoded once — guarded, a bare `%` is left alone — and then each path
 * segment is re-encoded. The hub's `Query` extractor decodes once more, so
 * the on-disk name round-trips exactly: `my%20file.pdf` → `my file.pdf`. */
export function chamberFileUrl(chamberId: string, relPath: string): string {
  const pathPart = relPath.split(/[?#]/)[0]
  let decoded: string
  try {
    decoded = decodeURIComponent(pathPart)
  } catch {
    decoded = pathPart
  }
  return `/api/chambers/${chamberId}/file?path=${encodeURIComponent(decoded)}`
}

/** The authenticated URL for `href` when it is a chamber-relative path, or
 * null when it is not (external, absolute, or no chamber id known). */
export function chamberFileHref(href: string, chamberId?: string): string | null {
  if (!chamberId) return null
  if (!isChamberRelativePath(href)) return null
  return chamberFileUrl(chamberId, href)
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

/** Fetch with the given authenticated fetcher and save under the href's name. */
export async function downloadUpload(
  fetchBlob: (url: string) => Promise<Blob>,
  url: string,
): Promise<void> {
  const blob = await fetchBlob(url)
  triggerBlobDownload(blob, filenameFromHref(url))
}
