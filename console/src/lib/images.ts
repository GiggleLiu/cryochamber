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
