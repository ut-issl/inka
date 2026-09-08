import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import react from '@vitejs/plugin-react'
import type { Plugin } from 'vite'
import { defineConfig } from 'vitest/config'

/**
 * Path the viewer fetches its cytoscape elements from.
 *
 * In production `inka serve` answers it; in development the plugin below
 * answers it from a checked-in fixture so the UI runs without the Rust side.
 */
const ELEMENTS_PATH = '/elements.json'

/** Fixture project the dev server answers with; `aocs` unless overridden. */
const DEV_FIXTURE = process.env.INKA_FIXTURE ?? 'aocs'
const DEV_FIXTURE_FILE = fileURLToPath(
  new URL(`./fixtures/${DEV_FIXTURE}.elements.json`, import.meta.url),
)

/** Serves the development fixture at {@link ELEMENTS_PATH}; dev server only. */
function devElements(): Plugin {
  return {
    name: 'inka-dev-elements',
    apply: 'serve',
    configureServer(server) {
      server.middlewares.use(ELEMENTS_PATH, (_request, response) => {
        // Read per request so editing the fixture needs no server restart.
        try {
          const body = readFileSync(DEV_FIXTURE_FILE, 'utf8')
          response.setHeader('Content-Type', 'application/json')
          response.end(body)
        } catch (error) {
          response.statusCode = 500
          response.setHeader('Content-Type', 'application/json')
          response.end(JSON.stringify({ error: `dev fixture unreadable: ${String(error)}` }))
        }
      })
    },
  }
}

export default defineConfig({
  plugins: [react(), devElements()],
  // Relative asset URLs: `inka serve` may mount the bundle under a sub-path.
  base: './',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // The graph library is vendored into the bundle by design, so one large
    // chunk is the intended shape rather than something to warn about.
    chunkSizeWarningLimit: 900,
  },
  test: {
    globals: true,
    environment: 'jsdom',
    // Unit and component tests only; `e2e/` belongs to Playwright.
    include: ['src/**/*.test.{ts,tsx}'],
    setupFiles: ['./src/test/setup.ts'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html'],
      include: ['src/**/*.{ts,tsx}'],
      exclude: ['src/main.tsx', 'src/test/**', 'src/**/*.test.{ts,tsx}'],
    },
  },
})
