import { defineConfig, devices } from '@playwright/test'

/**
 * End-to-end configuration.
 *
 * The graph pane can only be exercised in a real browser — cytoscape draws to a
 * canvas — so the invariants that live there (the layouts, the badges, the
 * hover description, background-tap and Esc) are covered here rather than in
 * the jsdom suite. The dev server supplies the fixture argument.
 */
export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: 'http://localhost:5173',
    viewport: { width: 1500, height: 850 },
    trace: 'on-first-retry',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    reuseExistingServer: !process.env.CI,
    stdout: 'ignore',
  },
})
