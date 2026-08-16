/// <reference types="vitest/config" />
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const pkg = JSON.parse(
  readFileSync(fileURLToPath(new URL('./package.json', import.meta.url)), 'utf8'),
) as { version: string }

/**
 * KaTeX's CSS lists every face as woff2 → woff → ttf. Every browser the
 * console supports takes the woff2, so the other two formats are 40 files
 * (~1.2 MB) that ship inside the cryohub binary for nothing. Dropping them
 * leaves dangling fallback URLs in the CSS that only a woff2-less browser
 * would ever request.
 */
function katexWoff2Only() {
  return {
    name: 'katex-woff2-only',
    generateBundle(_: unknown, bundle: Record<string, unknown>) {
      for (const name of Object.keys(bundle)) {
        if (/KaTeX_.*\.(ttf|woff)$/.test(name)) delete bundle[name]
      }
    },
  }
}

export default defineConfig({
  plugins: [react(), katexWoff2Only()],
  // Single source of truth for the version shown in Settings.
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  server: {
    proxy: {
      // A local `cryohub start` listens here. Same-origin in dev, exactly as
      // Caddy makes it in production — and no path rewrite, because the hub
      // owns /api itself.
      '/api': { target: 'http://127.0.0.1:8765', changeOrigin: false },
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: './src/test/setup.ts',
    exclude: ['e2e/**', 'node_modules/**'],
  },
})
