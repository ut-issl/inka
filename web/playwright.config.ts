import { defineConfig, devices } from '@playwright/test'

/**
 * End-to-end configuration.
 *
 * The graph pane can only be exercised in a real browser — cytoscape draws to a
 * canvas — so the invariants that live there (the layouts, the badges, the
 * hover description, background-tap and Esc) are covered here rather than in
 * the jsdom suite.
 *
 * Two pairs of servers run for it, each pair as in development — an
 * `inka serve` and the dev server that forwards `/api` to it:
 *
 * - a read-only pair on the aocs fixture, for everything that only looks;
 * - a recording pair (`--as e2e`) on a throwaway copy of that fixture, for the
 *   tests that record decisions, so nothing they decide is written into the
 *   repository and the read-only tests never see a decision control.
 *
 * Every server listens on a port of its own and none is ever reused: a server
 * left running for development — an `inka serve` on another project, or a dev
 * server that forwards to one — would otherwise be tested in place of these,
 * on whatever argument it happens to hold. A port that is taken fails the run
 * instead.
 */
const READ_ONLY = { serve: 'http://127.0.0.1:8787', frontend: 'http://localhost:5187' }
const RECORDING = { serve: 'http://127.0.0.1:8788', frontend: 'http://localhost:5188' }

/** The dev server in front of one `inka serve`. */
function frontend(pair: typeof READ_ONLY) {
  return {
    command: `npm run dev -- --port ${new URL(pair.frontend).port} --strictPort`,
    url: pair.frontend,
    env: { INKA_SERVE: pair.serve },
    reuseExistingServer: false,
    stdout: 'ignore' as const,
  }
}

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    viewport: { width: 1500, height: 850 },
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'read-only',
      testIgnore: /recording\.spec\.ts$/,
      use: { ...devices['Desktop Chrome'], baseURL: READ_ONLY.frontend },
    },
    {
      name: 'recording',
      testMatch: /recording\.spec\.ts$/,
      // One project, mutated by each decision: its tests run in order.
      fullyParallel: false,
      use: { ...devices['Desktop Chrome'], baseURL: RECORDING.frontend },
    },
  ],
  webServer: [
    {
      command: 'cargo run --locked -p inka-cli -- serve ../tests/fixtures/aocs --port 8787',
      url: `${READ_ONLY.serve}/api/page`,
      reuseExistingServer: false,
      // A cold build of `inka` comes first.
      timeout: 600_000,
      stdout: 'ignore',
    },
    {
      command: 'node e2e/recording-server.mjs 8788 e2e',
      url: `${RECORDING.serve}/api/page`,
      reuseExistingServer: false,
      timeout: 600_000,
      stdout: 'ignore',
    },
    frontend(READ_ONLY),
    frontend(RECORDING),
  ],
})
