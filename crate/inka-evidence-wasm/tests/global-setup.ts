// Builds the reporter before any test runs, so a direct `npx vitest run`
// (IDE, watch mode) never tests a missing or stale dist/.

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

export function setup(): void {
  const packageRoot = join(__dirname, "..");
  if (!existsSync(join(packageRoot, "pkg", "inka_evidence_wasm.js"))) {
    throw new Error(
      "the wasm package is missing; run `npm run build` once first",
    );
  }
  const result = spawnSync(
    "./node_modules/.bin/tsc",
    ["-p", "tsconfig.build.json"],
    { cwd: packageRoot, encoding: "utf8" },
  );
  if (result.status !== 0) {
    throw new Error(`building dist/ failed:\n${result.stdout}${result.stderr}`);
  }
}
