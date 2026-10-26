import { defineConfig } from "vitest/config";

// The fixture crates under tests/fixtures are run by the adapter tests in
// their own child vitest processes; the package's own run must not
// collect them (one of them fails on purpose).
export default defineConfig({
  test: {
    include: ["tests/**/*.test.ts"],
    exclude: ["tests/fixtures/**", "node_modules/**"],
    // The adapter tests spawn whole nested vitest runs.
    testTimeout: 120_000,
  },
});
