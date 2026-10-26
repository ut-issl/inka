import { join } from "node:path";
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    includeTaskLocation: true,
    reporters: [
      ["default"],
      [
        join(import.meta.dirname, "../../../../dist/vitest.js"),
        {
          output: process.env.INKA_EVIDENCE_OUT ?? "evidence.json",
          case: "node-ci",
          provenance: { source_revision: "fixture-rev" },
        },
      ],
    ],
  },
});
