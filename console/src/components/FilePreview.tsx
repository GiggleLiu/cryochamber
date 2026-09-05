import { useEffect, useState } from 'react'
import { Sheet } from './Sheet'
import { triggerBlobDownload } from '../lib/download'

export function FilePreview({ href, name, fetchBlob, onClose }: {
  href: string; name: string; fetchBlob: (url: string) => Promise<Blob>; onClose: () => void
}) {
  const [file, setFile] = useState<{ blob: Blob; url?: string; text?: string } | null>(null)
  const [error, setError] = useState('')
  useEffect(() => {
    let active = true
    let url: string | undefined
    void fetchBlob(href).then(async blob => {
      if (!active) return
      if (/\.pdf$/i.test(name)) {
        const header = await blob.slice(0, 5).text()
        if (!active) return
        if (header !== '%PDF-' || navigator.pdfViewerEnabled === false) {
          setFile({ blob })
          setError('PDF preview is unavailable here. Download the file to open it.')
          return
        }
        // Only PDF bytes with an explicit MIME type reach the browser's PDF
        // viewer. An iframe sandbox blocks that viewer in Chrome.
        url = URL.createObjectURL(new Blob([blob], { type: 'application/pdf' }))
        setFile({ blob, url })
      } else {
        const text = await blob.slice(0, 1024 * 1024).text()
        if (active) setFile({ blob, text: text + (blob.size > 1024 * 1024 ? '\n\n[Preview truncated at 1 MB. Download the complete file.]' : '') })
      }
    }).catch(() => { if (active) setError('Could not load this file. Close the preview and try again.') })
    return () => { active = false; if (url) URL.revokeObjectURL(url) }
  }, [href, name, fetchBlob])
  return <Sheet title={name} label={`Preview ${name}`} onClose={onClose}>
    {error && <p role="alert">{error}</p>}
    {!file && !error && <p role="status">Loading preview…</p>}
    {file && <>
      <button className="message-action" onClick={() => triggerBlobDownload(file.blob, name)}>Download {name}</button>
      {file.url ? <iframe className="file-preview-frame" title={name} src={file.url} /> : file.text !== undefined && <pre className="file-preview-text">{file.text}</pre>}
    </>}
  </Sheet>
}
