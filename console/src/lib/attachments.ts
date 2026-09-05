import { IMAGE_EXT_RE } from './images'

export function fileSize(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** Markdown remains portable to the agent and bridges; its title carries size for file cards. */
export function attachmentMarkdown(name: string, url: string, size: number): string {
  const label = name.replace(/\\/g, '\\\\').replace(/[\[\]]/g, '\\$&').replace(/[\r\n]/g, ' ')
  const target = url.replace(/[<>\s"]/g, c => encodeURIComponent(c))
  return `${IMAGE_EXT_RE.test(name) ? '!' : ''}[${label}](<${target}> "attachment:${size}")`
}
