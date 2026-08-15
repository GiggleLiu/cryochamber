/// <reference types="vitest/config" />
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const pkg = JSON.parse(
  readFileSync(fileURLToPath(new URL('./package.json', import.meta.url)), 'utf8'),
) as { version: string }

export default defineConfig({
  plugins: [react()],
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
