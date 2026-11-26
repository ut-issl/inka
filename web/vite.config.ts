import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

/**
 * The `inka serve` the dev server forwards API requests to.
 *
 * The bundle never answers for the argument itself: every surface is developed
 * against the real server, so what the page shows in development is what an
 * installed `inka` would send. Start one first, for example
 * `cargo run -p inka-cli -- serve tests/fixtures/aocs` from the repository
 * root, and point `INKA_SERVE` elsewhere if it listens on another address.
 */
const INKA_SERVE = process.env.INKA_SERVE ?? 'http://127.0.0.1:8080'

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      // A recording `inka serve` answers only requests addressed to it and
      // only decisions from its own pages. In development the page is this
      // dev server's, so the proxy speaks for it: it addresses the request to
      // the real server and names that server as where the request comes from.
      '/api': { target: INKA_SERVE, changeOrigin: true, headers: { Origin: INKA_SERVE } },
    },
  },
  // Relative asset URLs: `inka serve` may mount the bundle under a sub-path.
  base: './',
  build: {
    // Next to the CLI crate: its build script embeds the bundle into `inka`.
    outDir: '../crate/inka-cli/web-dist',
    emptyOutDir: true,
    // The graph and layout libraries are vendored into the bundle by design,
    // so one large chunk is the intended shape rather than something to warn
    // about.
    chunkSizeWarningLimit: 2500,
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
