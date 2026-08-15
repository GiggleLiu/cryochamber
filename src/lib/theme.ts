export const THEME_KEY = 'zulip-app.theme'

/** `''` means "follow the system", which is the default and is expressed by the
 * absence of a `data-theme` attribute — the stylesheet's media query then wins. */
export type Theme = 'light' | 'dark' | ''

function isTheme(value: string | null): value is Theme {
  return value === 'light' || value === 'dark'
}

export function readTheme(): Theme {
  try {
    const raw = localStorage.getItem(THEME_KEY)
    return isTheme(raw) ? raw : ''
  } catch {
    return ''
  }
}

export function applyTheme(theme: Theme): void {
  const root = document.documentElement
  try {
    if (theme) {
      root.dataset.theme = theme
      localStorage.setItem(THEME_KEY, theme)
    } else {
      delete root.dataset.theme
      localStorage.removeItem(THEME_KEY)
    }
  } catch {
    /* storage unavailable: the choice still applies for this session */
  }
}

/** Called before the first paint so a dark-mode user never sees a white flash. */
export function applyStoredTheme(): void {
  applyTheme(readTheme())
}
