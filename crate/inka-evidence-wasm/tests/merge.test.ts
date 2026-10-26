// Behavioral tests for the merge binding and the inka-evidence-merge bin.
//
// The merge semantics are proven in the shared Rust crate; these pin the
// JavaScript surface: argument translation, error mapping and the CLI's
// file handling.

import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { expect, test } from "vitest";

import { EvidenceBuilder, merge } from "../pkg/inka_evidence_wasm.js";

function evidence(caseId: string, outcome?: "pass" | "skip"): string {
  const builder = new EvidenceBuilder();
  builder.provenance("source_revision", "abc123");
  builder.addTest("suite::test", "the body", [
    { case: caseId, ...(outcome === undefined ? {} : { outcome }) },
  ]);
  return builder.finish();
}

test("merges labeled inputs with options", () => {
  const merged = merge(
    [
      { label: "linux", text: evidence("default", "pass") },
      { label: "macos", text: evidence("default") },
    ],
    { provenance: { runner: "matrix-ci" }, capsulaRunId: "run-9" },
  );

  const data = JSON.parse(merged);
  expect(data.provenance).toStrictEqual({
    source_revision: "abc123",
    runner: "matrix-ci",
    capsula_run_id: "run-9",
  });
  expect(data.tests[0].executions).toStrictEqual([
    { case: "linux/default", outcome: "pass" },
    { case: "macos/default" },
  ]);
});

test("options are optional", () => {
  const merged = merge([{ text: evidence("linux", "pass") }]);

  expect(JSON.parse(merged).tests[0].executions).toStrictEqual([
    { case: "linux", outcome: "pass" },
  ]);
});

test("merge conflicts are InvalidEvidenceError", () => {
  expect(() =>
    merge([{ text: evidence("default") }, { text: evidence("default") }]),
  ).toThrowError(
    expect.objectContaining({
      name: "InvalidEvidenceError",
      message: expect.stringContaining("more than one input"),
    }),
  );
});

test("a malformed input shape is a TypeError", () => {
  const bogus = [{ text: evidence("default"), stray: true }] as unknown as [
    { text: string },
  ];

  expect(() => merge(bogus)).toThrowError(
    expect.objectContaining({
      name: "TypeError",
      message: expect.stringContaining("unknown field `stray`"),
    }),
  );
});

test("null and undefined options are accepted", () => {
  const text = evidence("linux", "pass");

  expect(JSON.parse(merge([{ text }], undefined)).v).toBe(1);
  expect(
    JSON.parse(merge([{ text }], null as unknown as undefined)).v,
  ).toBe(1);
});

test("an unknown option key is a TypeError", () => {
  const bogus = { capsulaRunID: "typo" } as unknown as { capsulaRunId: string };

  expect(() => merge([{ text: evidence("linux") }], bogus)).toThrowError(
    expect.objectContaining({
      name: "TypeError",
      message: expect.stringContaining("unknown field `capsulaRunID`"),
    }),
  );
});

test("a named input appears in merge errors", () => {
  expect(() =>
    merge([{ text: "not json", name: "linux.json" }]),
  ).toThrowError(
    expect.objectContaining({
      name: "InvalidEvidenceError",
      message: expect.stringContaining("input 1 (linux.json):"),
    }),
  );
});

test("the merge bin merges files end to end", () => {
  const scratch = mkdtempSync(join(tmpdir(), "inka-evidence-merge-"));
  const linux = join(scratch, "linux.json");
  writeFileSync(linux, evidence("default", "pass"), "utf8");
  const macos = join(scratch, "macos.json");
  writeFileSync(macos, evidence("default", "skip"), "utf8");
  const out = join(scratch, "merged.json");

  const env = { ...process.env };
  delete env.CAPSULA_RUN_ID;
  const result = spawnSync(
    process.execPath,
    [
      join(__dirname, "..", "dist", "merge-cli.js"),
      "--output",
      out,
      "--provenance",
      "runner=matrix-ci",
      `linux=${linux}`,
      `macos=${macos}`,
    ],
    { encoding: "utf8", env },
  );

  expect(result.status, result.stderr).toBe(0);
  const data = JSON.parse(readFileSync(out, "utf8"));
  expect(data.provenance).toStrictEqual({
    source_revision: "abc123",
    runner: "matrix-ci",
  });
  expect(
    data.tests[0].executions.map(
      (execution: { case: string }) => execution.case,
    ),
  ).toStrictEqual(["linux/default", "macos/default"]);
});

test("the merge bin handles option forms, help and bad inputs", () => {
  const scratch = mkdtempSync(join(tmpdir(), "inka-evidence-merge-"));
  const one = join(scratch, "one.json");
  writeFileSync(one, evidence("linux", "pass"), "utf8");
  const bin = join(__dirname, "..", "dist", "merge-cli.js");
  const out = join(scratch, "inline.json");

  // --output=PATH inline form
  const inline = spawnSync(
    process.execPath,
    [bin, `--output=${out}`, one],
    { encoding: "utf8" },
  );
  expect(inline.status, inline.stderr).toBe(0);
  expect(JSON.parse(readFileSync(out, "utf8")).v).toBe(1);

  // --help prints usage and exits zero
  const help = spawnSync(process.execPath, [bin, "--help"], {
    encoding: "utf8",
  });
  expect(help.status).toBe(0);
  expect(help.stdout).toContain("usage:");

  // unknown flags, missing values, blank labels and empty file names fail
  const unknown = spawnSync(process.execPath, [bin, "--bogus", one], {
    encoding: "utf8",
  });
  expect(unknown.status).not.toBe(0);
  expect(unknown.stderr).toContain("unknown option");

  const missingValue = spawnSync(process.execPath, [bin, one, "--output"], {
    encoding: "utf8",
  });
  expect(missingValue.status).not.toBe(0);
  expect(missingValue.stderr).toContain("--output needs a value");

  const blankLabel = spawnSync(process.execPath, [bin, `=${one}`], {
    encoding: "utf8",
  });
  expect(blankLabel.status).not.toBe(0);
  expect(blankLabel.stderr).toContain("has a blank label");

  const emptyFile = spawnSync(process.execPath, [bin, "linux="], {
    encoding: "utf8",
  });
  expect(emptyFile.status).not.toBe(0);
  expect(emptyFile.stderr).toContain("empty file name");
});

test("the merge bin reports conflicts and unreadable files", () => {
  const scratch = mkdtempSync(join(tmpdir(), "inka-evidence-merge-"));
  const one = join(scratch, "one.json");
  writeFileSync(one, evidence("default"), "utf8");
  const two = join(scratch, "two.json");
  writeFileSync(two, evidence("default"), "utf8");
  const bin = join(__dirname, "..", "dist", "merge-cli.js");

  const conflict = spawnSync(process.execPath, [bin, one, two], {
    encoding: "utf8",
  });
  expect(conflict.status).not.toBe(0);
  expect(conflict.stderr).toContain("more than one input");

  const missing = spawnSync(process.execPath, [bin, "missing.json"], {
    encoding: "utf8",
    cwd: scratch,
  });
  expect(missing.status).not.toBe(0);
  expect(missing.stderr).toContain("cannot read missing.json");
});
