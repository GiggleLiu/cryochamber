import { useEffect, useRef, useState } from 'react'
import { sanitizeHtml } from './sanitize'
import {
  chamberFileHref,
  filenameFromHref,
  triggerBlobDownload,
  CHAMBER_FILE_RE,
  HUB_FILES_RE,
} from '../lib/download'
import { IMAGE_EXT_RE, deferHubImages, inlineImageLinks } from '../lib/images'
import { isUnauthorized } from '../api/types'

export { sanitizeHtml } from './sanitize'
export { chamberFileUrl, filenameFromHref, isChamberRelativePath } from '../lib/download'

/** The markdown renderer pulls in markdown-it and KaTeX — a third of the
 * bundle. It is imported on first need and cached at module scope, so the whole
 * app shares one download and one instance. */
type MarkdownModule = typeof import('../lib/markdown')
let markdownModule: MarkdownModule | null = null
let markdownPending: Promise<MarkdownModule> | null = null

function loadMarkdown(): Promise<MarkdownModule> {
  markdownPending ??= import('../lib/markdown')
    .then((mod) => {
      markdownModule = mod
      return mod
    })
    .catch((err: unknown) => {
      // A chunk that 404s (typically: a new build deployed under this page)
      // must not poison every later message. Forget the attempt so the next
      // mount tries again; the caller keeps showing the plain-text fallback.
      markdownPending = null
      throw err
    })
  return markdownPending
}

/** What a markdown message shows for the moment before the renderer arrives:
 * its own source, escaped. Never parsed — this is untrusted text. */
export function plainTextFallback(source: string): string {
  const escaped = source.replace(
    /[&<>"']/g,
    (c) =>
      ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] as string,
  )
  return `<p>${escaped}</p>`
}

export function MessageBody({
  source,
  fetchBlob,
  chamberId,
}: {
  /** Raw markdown, rendered and then sanitized below. */
  source: string
  /** Authenticated fetcher for chamber attachments (the signed-in client's).
   * Absent for a bubble with no session behind it — a pending send. */
  fetchBlob?: (url: string) => Promise<Blob>
  /** Hub id of the chamber the message belongs to; chamber-relative links
   * (e.g. `articles/review.pdf`) resolve against it. Absent (older call
   * sites), relative links keep their default navigation. */
  chamberId?: string
}) {
  const ref = useRef<HTMLDivElement>(null)
  // Upload path -> live object URL. Cached so innerHTML replacements and the
  // lightbox never refetch; revoked only on unmount.
  const blobCache = useRef(new Map<string, string>())
  // Whether the component is still mounted: a swap that resolves after unmount
  // must revoke the object URL it just made instead of caching it.
  const mounted = useRef(true)
  const [lightbox, setLightbox] = useState<{ src: string; alt: string } | null>(null)
  // One inline alert slot for anything the body itself could not do: an
  // attachment that would not load, a clipboard that refused.
  const [notice, setNotice] = useState<string | null>(null)
  // Re-render once the lazily-imported renderer lands.
  const [, setMarkdownLoaded] = useState(markdownModule !== null)

  useEffect(() => {
    if (markdownModule) return
    let alive = true
    loadMarkdown()
      .then(() => {
        if (alive) setMarkdownLoaded(true)
      })
      .catch(() => {
        // Stay on the fallback; a later mount retries.
      })
    return () => {
      alive = false
    }
  }, [])

  // Messages arrive as raw markdown; render first, then sanitize — the
  // sanitizer stays the single choke point for HTML entering the DOM.
  const rendered = markdownModule
    ? markdownModule.renderMarkdown(source)
    : plainTextFallback(source)
  // A plain link to an image attachment becomes an inline thumbnail; this runs
  // after the sanitizer so nothing it inserts can widen what the sanitizer let
  // through. With a fetcher in hand, hub images then lose their `src` before
  // they reach the DOM: the browser must not request them unauthenticated
  // (a 401 and a broken-image glyph until the swap below lands).
  const inlined = inlineImageLinks(sanitizeHtml(rendered), chamberId)
  const sanitized = fetchBlob ? deferHubImages(inlined, chamberId) : inlined

  // Authenticated image loading. React re-sets this div's innerHTML whenever
  // the rendered HTML changes (and dev StrictMode/remounts can do it too);
  // fresh DOM has plain upload srcs again, so a MutationObserver re-runs the
  // swap. NOTE: never attach per-node listeners inside this subtree — they are
  // silently orphaned by innerHTML replacement (that bug shipped once; clicks
  // are handled by delegation on the container below).
  useEffect(() => {
    const root = ref.current
    if (!root) return
    const decorate = () => {
      // Copy affordance on every code block. The button goes in a positioned
      // wrapper beside the <pre>, not inside it: <pre> scrolls sideways, and a
      // button in that flow would either ride away with the scroll or sit on
      // top of the code. Idempotent via a data flag, since the observer re-runs
      // after each innerHTML replacement — including the one this makes.
      for (const pre of Array.from(root.querySelectorAll('pre'))) {
        if (pre.dataset.copyWired === '1') continue
        pre.dataset.copyWired = '1'
        const wrap = document.createElement('div')
        wrap.className = 'code-block'
        pre.parentNode?.insertBefore(wrap, pre)
        wrap.appendChild(pre)
        const btn = document.createElement('button')
        btn.className = 'code-copy'
        btn.type = 'button'
        btn.textContent = 'Copy'
        wrap.appendChild(btn)
      }
      // Authenticated images. Only this half needs the fetcher, so the
      // observer itself must not be gated on it.
      if (!fetchBlob) return
      for (const img of Array.from(root.querySelectorAll('img'))) {
        // Deferred by `deferHubImages` (the usual case) or still carrying a
        // plain hub src (HTML that reached the DOM some other way).
        const src = img.dataset.uploadSrc ?? img.getAttribute('src') ?? ''
        if (!HUB_FILES_RE.test(src) && !CHAMBER_FILE_RE.test(src)) continue
        const cached = blobCache.current.get(src)
        if (cached) {
          img.dataset.uploadSrc = src
          img.setAttribute('src', cached)
          img.dataset.authSwap = 'done'
          continue
        }
        if (img.dataset.authSwap === 'pending') continue
        img.dataset.authSwap = 'pending'
        img.dataset.uploadSrc = src
        // The result lands whatever happens to this effect in the meantime,
        // on every <img> that currently wants this file — not only the node
        // the fetch was started for. It used to be dropped once the effect
        // had been cleaned up, and that raced: React re-sets innerHTML on a
        // re-render, this effect's observer fires on that change *before*
        // the effect is cleaned up and marks the fresh node pending with a
        // closure about to be disposed, and the next effect then skips the
        // node as already claimed. The thumbnail stayed blank until the
        // lightbox fetched the file itself on tap.
        fetchBlob(src)
          .then((blob) => {
            const url = URL.createObjectURL(blob)
            if (!mounted.current) {
              if (typeof URL.revokeObjectURL === 'function') URL.revokeObjectURL(url)
              return
            }
            blobCache.current.set(src, url)
            const wanting = new Set<HTMLImageElement>([img])
            for (const el of Array.from(ref.current?.querySelectorAll('img') ?? [])) {
              if (el.dataset.uploadSrc === src) wanting.add(el)
            }
            for (const el of wanting) {
              el.setAttribute('src', url)
              el.dataset.authSwap = 'done'
            }
          })
          .catch(() => {
            delete img.dataset.authSwap
          })
      }
    }
    decorate()
    const observer = new MutationObserver(decorate)
    observer.observe(root, { childList: true, subtree: true })
    return () => observer.disconnect()
  }, [fetchBlob, sanitized])

  // Revoke cached object URLs only when the component goes away for good.
  useEffect(() => {
    const cache = blobCache.current
    mounted.current = true
    return () => {
      mounted.current = false
      if (typeof URL.revokeObjectURL === 'function') {
        for (const url of cache.values()) URL.revokeObjectURL(url)
      }
      cache.clear()
    }
  }, [])

  // Close the lightbox on Escape.
  useEffect(() => {
    if (!lightbox) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setLightbox(null)
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [lightbox])

  function openLightbox(href: string, alt: string, innerImg: HTMLImageElement | null) {
    const cached = blobCache.current.get(href)
    if (cached) {
      setLightbox({ src: cached, alt })
      return
    }
    if (innerImg?.dataset.authSwap === 'done') {
      setLightbox({ src: innerImg.getAttribute('src') ?? href, alt })
      return
    }
    if (!fetchBlob) {
      setLightbox({ src: href, alt })
      return
    }
    fetchBlob(href)
      .then((blob) => {
        const url = URL.createObjectURL(blob)
        blobCache.current.set(href, url)
        setLightbox({ src: url, alt })
      })
      .catch((e) => {
        // A 401 here is not a broken image, it is a revoked session.
        if (isUnauthorized(e)) return
        setNotice(`Could not load ${alt || 'image'}. Check your connection and try again.`)
      })
  }

  async function download(href: string, name: string) {
    if (!fetchBlob) return
    try {
      const blob = await fetchBlob(href)
      triggerBlobDownload(blob, name)
      setNotice(null)
    } catch (e) {
      if (isUnauthorized(e)) return
      // A plain navigation would just hit the server without auth — surface
      // the failure instead of silently doing nothing.
      setNotice(`Could not download ${name}. Check your connection and try again.`)
    }
  }

  /** The clipboard can refuse (permission, insecure context) or not exist at
   * all, so "Copied" is only honest once the write has actually resolved. */
  async function copyToClipboard(text: string): Promise<boolean> {
    try {
      if (!navigator.clipboard) return false
      await navigator.clipboard.writeText(text)
      return true
    } catch {
      return false
    }
  }

  // Delegated click handling: lives on the React-rendered container, so it
  // survives every innerHTML replacement of the message content.
  function onClick(e: React.MouseEvent) {
    const root = ref.current
    const target = e.target as Element
    // First branch: the copy button lives inside <pre>, sometimes inside an
    // anchor's ancestry, so it has to win before the attachment handlers.
    const copyBtn = target.closest('button.code-copy')
    if (copyBtn && root?.contains(copyBtn)) {
      e.preventDefault()
      const pre = copyBtn.parentElement?.querySelector('pre')
      const code = pre?.querySelector('code') ?? pre
      void copyToClipboard(code?.textContent ?? '').then((ok) => {
        if (!ok) {
          // The label stays "Copy" — claiming otherwise would send the user off
          // to paste something that is not on their clipboard.
          setNotice('Could not copy to the clipboard. Select the code and copy it manually.')
          return
        }
        setNotice(null)
        copyBtn.textContent = 'Copied'
        setTimeout(() => {
          copyBtn.textContent = 'Copy'
        }, 1500)
      })
      return
    }
    const anchor = target.closest('a')
    if (anchor && root?.contains(anchor)) {
      const href = anchor.getAttribute('href') ?? ''
      // Chamber attachments: authenticated download / lightbox.
      if (HUB_FILES_RE.test(href)) {
        e.preventDefault()
        const name = filenameFromHref(href)
        const innerImg = anchor.querySelector('img')
        if (innerImg || IMAGE_EXT_RE.test(name)) {
          openLightbox(href, name, innerImg)
        } else {
          void download(href, name)
        }
        return
      }
      // Chamber-relative link (a file the agent produced on disk, e.g.
      // articles/review.pdf): same authenticated path, resolved against this
      // chamber. A plain navigation would 404 on the SPA route.
      // Chamber-relative link (a file the agent produced on disk, e.g.
      // articles/review.pdf): same authenticated path, resolved against this
      // chamber. A plain navigation would 404 on the SPA route.
      const localUrl = chamberFileHref(href, chamberId)
      if (localUrl) {
        e.preventDefault()
        const name = filenameFromHref(href)
        const innerImg = anchor.querySelector('img')
        if (innerImg || IMAGE_EXT_RE.test(name)) {
          openLightbox(localUrl, name, innerImg)
        } else {
          void download(localUrl, name)
        }
        return
      }
      return // external link: default new tab
    }
    const img = target.closest('img')
    if (img && root?.contains(img)) {
      const alt = img.getAttribute('alt') ?? ''
      // A hub image goes through the authenticated path, cached or not — its
      // `src` may still be empty while the swap is in flight.
      const upload = img.dataset.uploadSrc
      if (upload) openLightbox(upload, alt, img)
      else setLightbox({ src: img.getAttribute('src') ?? '', alt })
    }
  }

  return (
    <>
      <div
        className="message-body"
        ref={ref}
        onClick={onClick}
        dangerouslySetInnerHTML={{ __html: sanitized }}
      />
      {notice && (
        <p className="body-alert" role="alert">
          {notice}
        </p>
      )}
      {lightbox && (
        <div
          className="lightbox"
          role="dialog"
          aria-label={lightbox.alt || 'Image'}
          onClick={() => setLightbox(null)}
        >
          <img src={lightbox.src} alt={lightbox.alt} />
        </div>
      )}
    </>
  )
}
