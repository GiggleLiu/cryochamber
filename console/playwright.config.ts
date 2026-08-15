import { defineConfig } from '@playwright/test'

// Overridable so a git worktree can run its own suite without silently reusing
// (and testing) a dev server that another checkout already has on 5173.
const port = Number(process.env.E2E_PORT ?? 5173)

export default defineConfig({
  testDir: './e2e',
  use: { baseURL: `http://localhost:${port}` },
  webServer: {
    command: `npm run dev -- --port ${port} --strictPort`,
    url: `http://localhost:${port}`,
    reuseExistingServer: !process.env.CI,
  },
})
