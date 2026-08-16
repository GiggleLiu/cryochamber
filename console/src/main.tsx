import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import 'katex/dist/katex.min.css'
import './styles.css'
import App from './App'
import { applyStoredTheme } from './lib/theme'
import { wireUpdateFlow } from './lib/swUpdate'
import { useAppStore } from './store/appStore'

// Before the first paint: a dark-mode user must never see a white flash.
applyStoredTheme()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)

if (import.meta.env.PROD && 'serviceWorker' in navigator) {
  navigator.serviceWorker
    .register('/sw.js')
    .then((reg) => wireUpdateFlow(reg, () => useAppStore.getState().setUpdateAvailable(true)))
    .catch(() => {
      // A failed registration only costs offline support; the app still runs.
    })
}
