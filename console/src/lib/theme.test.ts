import { applyStoredTheme, applyTheme, readTheme, THEME_KEY } from './theme'

beforeEach(() => {
  localStorage.clear()
  delete document.documentElement.dataset.theme
})

test('an explicit theme is applied to the root and persisted', () => {
  applyTheme('dark')
  expect(document.documentElement.dataset.theme).toBe('dark')
  expect(localStorage.getItem(THEME_KEY)).toBe('dark')
  applyTheme('light')
  expect(document.documentElement.dataset.theme).toBe('light')
  expect(localStorage.getItem(THEME_KEY)).toBe('light')
})

test('the system setting clears both, so the media query decides', () => {
  applyTheme('dark')
  applyTheme('')
  expect(document.documentElement.dataset.theme).toBeUndefined()
  expect(localStorage.getItem(THEME_KEY)).toBeNull()
})

test('readTheme reports the stored choice, or system when there is none', () => {
  expect(readTheme()).toBe('')
  applyTheme('dark')
  expect(readTheme()).toBe('dark')
})

test('a stored theme is re-applied at boot', () => {
  localStorage.setItem(THEME_KEY, 'dark')
  applyStoredTheme()
  expect(document.documentElement.dataset.theme).toBe('dark')
})

test('a corrupt stored value is ignored rather than stamped on the root', () => {
  localStorage.setItem(THEME_KEY, 'neon')
  applyStoredTheme()
  expect(document.documentElement.dataset.theme).toBeUndefined()
})
