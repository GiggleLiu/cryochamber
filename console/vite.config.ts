/// <reference types="vitest/config" />
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { defineConfig, type Plugin } from 'vite'
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

/**
 * Emits `precache.json` next to the build: the list of every file the service
 * worker should cache at install, plus a hash that names the cache. Hashed
 * assets already carry their content in their name, so hashing the sorted
 * filenames is enough to change the cache name whenever any asset changes.
 * `sw.js` and the manifest itself are excluded: the worker must always be
 * fetched fresh, and the manifest is what the worker fetches to learn the list.
 */
function precacheManifest(): Plugin {
  return {
    name: 'precache-manifest',
    apply: 'build',
    generateBundle(_options, bundle) {
      const files = Object.keys(bundle)
        .filter((f) => f !== 'sw.js' && f !== 'precache.json')
        .map((f) => '/' + f)
      for (const always of ['/index.html', '/manifest.webmanifest']) {
        if (!files.includes(always)) files.push(always)
      }
      files.sort()
      const hash = createHash('sha256').update(files.join('\n')).digest('hex').slice(0, 8)
      this.emitFile({
        type: 'asset',
        fileName: 'precache.json',
        source: JSON.stringify({ hash, files }),
      })
    },
  }
}

export default defineConfig({
  plugins: [react(), katexWoff2Only(), precacheManifest()],
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
