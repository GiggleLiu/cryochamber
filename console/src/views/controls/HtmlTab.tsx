import { useMemo } from 'react'
import { sanitizeHtml } from '../../components/sanitize'
import { useAppStore } from '../../store/appStore'

/**
 * `plan_html` / `notes_html` — markdown the *server* already rendered and
 * neutralized. It is sanitized again here anyway: chamber files are written by
 * an agent, i.e. untrusted, and one escaping layer is one bug away from none.
 */
export function HtmlTab({ html, empty }: { html: string; empty: string }) {
  const prefix = useAppStore((s) => s.creds?.prefix ?? '')
  const clean = useMemo(() => sanitizeHtml(html, prefix), [html, prefix])
  if (html.trim() === '') return <p className="tab-empty">{empty}</p>
  return <div className="tab-html" dangerouslySetInnerHTML={{ __html: clean }} />
}
