/** True when this bundle is running inside the Tauri shell (the "app").
 * The shell injects `__TAURI_INTERNALS__` before any script runs, so this is
 * stable from the first render. Everything multi-hub is gated on it; in a
 * browser the console stays the single-hub app the hub served. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}
