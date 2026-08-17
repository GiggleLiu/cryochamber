/** Which attachments are pictures. Shared by the composer (an uploaded image
 * is inserted as an embed) and the message body (a plain link to an image is
 * upgraded to a thumbnail), so the two ends of the round trip cannot drift. */

import { HUB_FILES_RE, filenameFromHref } from './download'

export const IMAGE_EXT_RE = /\.(png|jpe?g|gif|webp|svg|avif|bmp|ico)$/i

/**
 * Upgrade `[artwork.png](/api/chambers/x/files/artwork.png)` — a plain link, as
 * produced by every chat bridge and by older composer versions — into a
 * thumbnail *inside the same anchor*. Keeping the anchor matters: the message
 * body's delegated click handler keys off it for the lightbox, and the
 * authenticated blob swap keys off the `<img src>`.
 *
 * Runs on already-sanitized HTML and only ever replaces an anchor's children
 * with an element it builds itself, so it adds no new markup surface. Anchors
 * that already wrap an image (`[![…](…)](…)`) are left alone, which also makes
 * it idempotent.
 */
export function inlineImageLinks(html: string): string {
  if (!html.includes('<a')) return html
  const doc = new DOMParser().parseFromString(html, 'text/html')
  let changed = false
  for (const a of Array.from(doc.querySelectorAll('a[href]'))) {
    const href = a.getAttribute('href') ?? ''
    if (!HUB_FILES_RE.test(href)) continue
    const name = filenameFromHref(href)
    if (!IMAGE_EXT_RE.test(name)) continue
    if (a.querySelector('img')) continue
    const img = doc.createElement('img')
    img.setAttribute('src', href)
    img.setAttribute('alt', a.textContent?.trim() || name)
    img.setAttribute('class', 'msg-thumb')
    a.textContent = ''
    a.appendChild(img)
    changed = true
  }
  return changed ? doc.body.innerHTML : html
}

/**
 * Keep the browser from fetching hub attachments on its own. An `<img src>`
 * pointing at `/api/chambers/…/files/…` is requested the moment it enters the
 * DOM — without the bearer token, so in the default (authenticated) mode it
 * 401s and paints a broken-image glyph until the blob swap in the message
 * body replaces it. This parks the URL in `data-upload-src` instead, and the
 * swap is the only thing that ever sets `src`.
 *
 * Runs on already-sanitized HTML; the attribute it adds carries a value the
 * sanitizer had already admitted as `src`, so it widens nothing. Callers only
 * apply it when they have a fetcher — with none, the plain `src` is the only
 * way the image can load at all (open mode).
 */
export function deferHubImages(html: string): string {
  if (!html.includes('<img')) return html
  const doc = new DOMParser().parseFromString(html, 'text/html')
  let changed = false
  for (const img of Array.from(doc.querySelectorAll('img[src]'))) {
    const src = img.getAttribute('src') ?? ''
    if (!HUB_FILES_RE.test(src)) continue
    img.setAttribute('data-upload-src', src)
    img.removeAttribute('src')
    changed = true
  }
  return changed ? doc.body.innerHTML : html
}
