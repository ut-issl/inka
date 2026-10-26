// End-to-end tests for the Vitest evidence reporter.
//
// Each test spawns a real nested `vitest run` on a fixture directory and
// inspects the emitted evidence file, pinning the contract adapters rely
// on: identity, bodies, outcome mapping, and the explicit failures and
// suppressions for unreliable runs.

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import Ajv2020 from "ajv/dist/2020";
import { expect, test } from "vitest";

const PACKAGE_ROOT = join(__dirname, "..");
const VITEST_BIN = join(PACKAGE_ROOT, "node_modules", "vitest", "vitest.mjs");

const SCHEMA = JSON.parse(
  readFileSync(
    join(PACKAGE_ROOT, "..", "..", "schemas", "test-evidence-v1.schema.json"),
    "utf8",
  ),
);

interface Execution {
  case: string;
  outcome?: string;
}

interface EvidenceTest {
  id: string;
  body: string;
  executions: Execution[];
}

interface RunResult {
  status: number | null;
  output: string;
  evidencePath: string;
}

/** Runs a nested vitest on one fixture, returning its combined output. */
function runFixture(
  fixture: string,
  options: { args?: string[]; env?: Record<string, string> } = {},
): RunResult {
  const scratch = mkdtempSync(join(tmpdir(), "inka-evidence-vitest-"));
  const evidencePath = join(scratch, "evidence.json");
  const root = join(PACKAGE_ROOT, "tests", "fixtures", "vitest", fixture);
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    INKA_EVIDENCE_OUT: evidencePath,
    // The outer environment must not leak a run id into the fixtures.
    CAPSULA_RUN_ID: undefined,
    ...options.env,
  };
  const result = spawnSync(
    process.execPath,
    [VITEST_BIN, "run", "--root", root, ...(options.args ?? [])],
    { encoding: "utf8", env },
  );
  return {
    status: result.status,
    output: `${result.stdout}\n${result.stderr}`,
    evidencePath,
  };
}

function readEvidence(path: string): {
  raw: Record<string, unknown>;
  tests: Map<string, EvidenceTest>;
} {
  const raw = JSON.parse(readFileSync(path, "utf8")) as Record<string, unknown>;
  const validate = new Ajv2020().compile(SCHEMA);
  expect(validate(raw), JSON.stringify(validate.errors)).toBe(true);
  const tests = new Map<string, EvidenceTest>(
    (raw.tests as EvidenceTest[]).map((entry) => [entry.id, entry]),
  );
  return { raw, tests };
}

test("a full run maps identities, bodies and outcomes", () => {
  const run = runFixture("basic", { env: { CAPSULA_RUN_ID: "run-7" } });

  expect(run.status, run.output).toBe(1); // one test fails on purpose
  expect(run.output).toContain("inka-evidence: wrote");
  const { raw, tests } = readEvidence(run.evidencePath);

  expect(raw.v).toBe(1);
  expect(raw.provenance).toStrictEqual({
    source_revision: "fixture-rev",
    capsula_run_id: "run-7",
  });

  const executions = Object.fromEntries(
    [...tests.values()].map((entry) => [entry.id, entry.executions]),
  );
  expect(executions).toStrictEqual({
    "basic.test.ts > math > adds": [{ case: "node-ci", outcome: "pass" }],
    "basic.test.ts > math > positive 1": [
      { case: "node-ci", outcome: "pass" },
    ],
    "basic.test.ts > math > positive 2": [
      { case: "node-ci", outcome: "pass" },
    ],
    "basic.test.ts > fails": [{ case: "node-ci", outcome: "fail" }],
    "basic.test.ts > skipped": [{ case: "node-ci", outcome: "skip" }],
    "basic.test.ts > todo later": [{ case: "node-ci", outcome: "skip" }],
  });

  expect(tests.get("basic.test.ts > math > adds")?.body).toBe(
    'test("adds", () => {\n    expect(1 + 1).toBe(2);\n  })',
  );
  // A test.each instance keeps the whole chain, table included.
  expect(tests.get("basic.test.ts > math > positive 1")?.body).toBe(
    'test.each([1, 2])("positive %i", (n) => {\n' +
      "    expect(n).toBeGreaterThan(0);\n  })",
  );
  expect(tests.get("basic.test.ts > todo later")?.body).toBe(
    'test.todo("todo later")',
  );
});

test("a named project prefixes the test-id", () => {
  const run = runFixture("named");

  expect(run.status, run.output).toBe(0);
  const { raw, tests } = readEvidence(run.evidencePath);
  expect([...tests.keys()]).toStrictEqual([
    "[proj] named.test.ts > named passes",
  ]);
  // No case option configured: the label falls back to "default".
  expect(
    tests.get("[proj] named.test.ts > named passes")?.executions,
  ).toStrictEqual([{ case: "default", outcome: "pass" }]);
  // No explicit provenance and no capsula run id in the environment.
  expect(raw).not.toHaveProperty("provenance");
});

test("a name filter declares unmatched tests as skipped", () => {
  const run = runFixture("basic", { args: ["-t", "adds"] });

  expect(run.status, run.output).toBe(0); // the failing test is filtered out
  const { tests } = readEvidence(run.evidencePath);
  expect(tests.get("basic.test.ts > math > adds")?.executions).toStrictEqual([
    { case: "node-ci", outcome: "pass" },
  ]);
  // Filtered tests stay declared with a conservative skip, which never
  // establishes a bind.
  expect(tests.get("basic.test.ts > fails")?.executions).toStrictEqual([
    { case: "node-ci", outcome: "skip" },
  ]);
});

test("a tagged-template each table fails explicitly", () => {
  // Vitest reports untrustworthy locations for this form; the body of a
  // neighboring test must never be recorded silently.
  const run = runFixture("templateeach");

  expect(run.status, run.output).not.toBe(0);
  expect(run.output).toContain("untrustworthy");
  expect(existsSync(run.evidencePath)).toBe(false);
});

test("a bailed run leaves missing results, never passes", () => {
  const run = runFixture("bail", { args: ["--bail", "1"] });

  expect(run.status, run.output).toBe(1);
  const { tests } = readEvidence(run.evidencePath);
  expect(tests.get("bail.test.ts > first fails")?.executions).toStrictEqual([
    { case: "node-ci", outcome: "fail" },
  ]);
  // Declared by the collection, never run: the outcome key is absent.
  expect(
    tests.get("bail.test.ts > second never runs")?.executions,
  ).toStrictEqual([{ case: "node-ci" }]);
});

test("a collection error suppresses emission and clears stale files", () => {
  const scratch = mkdtempSync(join(tmpdir(), "inka-evidence-vitest-"));
  const evidencePath = join(scratch, "evidence.json");
  writeFileSync(evidencePath, '{"stale": true}', "utf8");
  const root = join(PACKAGE_ROOT, "tests", "fixtures", "vitest", "broken");
  const result = spawnSync(
    process.execPath,
    [VITEST_BIN, "run", "--root", root],
    {
      encoding: "utf8",
      env: { ...process.env, INKA_EVIDENCE_OUT: evidencePath },
    },
  );

  expect(result.status).not.toBe(0);
  expect(`${result.stdout}\n${result.stderr}`).toContain(
    "no evidence was emitted",
  );
  expect(existsSync(evidencePath), "stale evidence must be cleared").toBe(
    false,
  );
});

test("a missing task location is an explicit error", () => {
  const run = runFixture("nolocation");

  expect(run.status, run.output).not.toBe(0);
  expect(run.output).toContain("includeTaskLocation");
  expect(existsSync(run.evidencePath)).toBe(false);
});

test("duplicate test names collide and fail explicitly", () => {
  const run = runFixture("duplicate");

  expect(run.status, run.output).not.toBe(0);
  expect(run.output).toContain("more than once");
  expect(existsSync(run.evidencePath)).toBe(false);
});

test("a reporter without an output option refuses to start", () => {
  // The empty string defeats the config's `??` fallback, so the
  // constructor sees it and must reject it.
  const run = runFixture("basic", {
    env: { INKA_EVIDENCE_OUT: "" },
  });

  expect(run.status, run.output).not.toBe(0);
  expect(run.output).toContain("needs an `output` path option");
});
