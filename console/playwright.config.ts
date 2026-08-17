import { defineConfig } from '@playwright/test'

// Overridable so a git worktree can run its own suite without silently reusing
// (and testing) a dev server that another checkout already has on 5173.
const port = Number(process.env.E2E_PORT ?? 5173)

export default defineConfig({
  testDir: './e2e',
  // In CI keep an HTML report to upload on failure; locally the list is enough.
  reporter: process.env.CI ? [['dot'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: `http://localhost:${port}`,
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
  },
  webServer: {
    command: `npm run dev -- --port ${port} --strictPort`,
    url: `http://localhost:${port}`,
    reuseExistingServer: !process.env.CI,
  },
})
