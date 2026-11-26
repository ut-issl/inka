import { defineConfig, devices } from '@playwright/test'

/**
 * End-to-end configuration.
 *
 * The graph pane can only be exercised in a real browser — cytoscape draws to a
 * canvas — so the invariants that live there (the layouts, the badges, the
 * hover description, background-tap and Esc) are covered here rather than in
 * the jsdom suite.
 *
 * Two servers run for it, as in development: `inka serve` on the aocs fixture
 * project, and the dev server that forwards `/api` to it. Both listen on ports
 * of their own and neither is ever reused: a server left running for
 * development — an `inka serve` on another project, or a dev server that
 * forwards to one — would otherwise be tested in place of these, on whatever
 * argument it happens to hold. A port that is taken fails the run instead.
 */
const INKA_SERVE = 'http://127.0.0.1:8787'
const FRONTEND = 'http://localhost:5187'

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: FRONTEND,
    viewport: { width: 1500, height: 850 },
    trace: 'on-first-retry',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: [
    {
      command: 'cargo run --locked -p inka-cli -- serve ../tests/fixtures/aocs --port 8787',
      url: `${INKA_SERVE}/api/page`,
      reuseExistingServer: false,
      // A cold build of `inka` comes first.
      timeout: 600_000,
      stdout: 'ignore',
    },
    {
      command: 'npm run dev -- --port 5187 --strictPort',
      url: FRONTEND,
      env: { INKA_SERVE },
      reuseExistingServer: false,
      stdout: 'ignore',
    },
  ],
})
