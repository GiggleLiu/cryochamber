import '@testing-library/jest-dom/vitest'

// Node ≥ 22.4 defines an experimental global `localStorage` getter that returns
// undefined unless --localstorage-file is passed, which also makes vitest's
// jsdom environment skip populating `localStorage` from the jsdom window.
// Bind `globalThis.localStorage` to the active jsdom window's Storage,
// unconditionally and without reading the existing property (so Node's
// experimental getter never fires), then restore the original descriptor in
// afterAll so storage from a closed JSDOM instance cannot leak forward.
const originalLocalStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
const jsdomWindow = (globalThis as { jsdom?: { window: Window } }).jsdom?.window
if (jsdomWindow) {
  Object.defineProperty(globalThis, 'localStorage', {
    value: jsdomWindow.localStorage,
    configurable: true,
  })
}

afterAll(() => {
  if (originalLocalStorage) {
    Object.defineProperty(globalThis, 'localStorage', originalLocalStorage)
  } else {
    delete (globalThis as { localStorage?: unknown }).localStorage
  }
})

afterEach(() => {
  localStorage.clear()
})
