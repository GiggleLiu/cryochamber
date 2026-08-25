import { describe, it, expect, afterEach } from 'vitest'
import { isTauri } from './env'

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown
  }
}

afterEach(() => {
  delete window.__TAURI_INTERNALS__
})

describe('isTauri', () => {
  it('is false in a plain browser', () => {
    expect(isTauri()).toBe(false)
  })

  it('is true when the Tauri runtime injected its internals', () => {
    window.__TAURI_INTERNALS__ = {}
    expect(isTauri()).toBe(true)
  })
})
