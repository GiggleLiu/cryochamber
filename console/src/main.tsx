import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
// KaTeX's stylesheet stays eager: Zulip messages arrive with server-rendered
// KaTeX markup, which needs it even when the markdown renderer never loads.
import 'katex/dist/katex.min.css'
import './styles.css'
import App from './App'
import { applyStoredTheme } from './lib/theme'

// Before the first paint: a dark-mode user must never see a white flash.
applyStoredTheme()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)

if (import.meta.env.PROD && 'serviceWorker' in navigator) {
  navigator.serviceWorker.register('/sw.js')
}
