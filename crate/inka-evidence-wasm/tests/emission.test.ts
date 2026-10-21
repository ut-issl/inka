// Behavioral tests for the `@ut-issl/inka-evidence` wasm binding.
//
// The binding wraps the Rust `inka-evidence` crate, so these tests pin the
// contract adapters rely on — the v1 envelope, the add-time rejections and
// the provenance rules — not the validation logic itself, which the Rust
// crate already proves against the JSON schema and the parser.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import Ajv2020 from "ajv/dist/2020";
import { describe, expect, test } from "vitest";

import { EvidenceBuilder, version } from "../pkg/inka_evidence_wasm.js";

const SCHEMA = JSON.parse(
  readFileSync(
    join(__dirname, "..", "..", "..", "schemas", "test-evidence-v1.schema.json"),
    "utf8",
  ),
);

function buildRepresentative(): EvidenceBuilder {
  const builder = new EvidenceBuilder();
  builder.provenance("source_revision", "0123abc");
  builder.addTest("crate::case::passes", "#[test]\nfn passes() {}\n", [
    { case: "passes", outcome: "pass" },
    { case: "declared_only" },
  ]);
  builder.addTest("crate::case::mixed", "body", [
    { case: "failing", outcome: "fail" },
    { case: "skipped", outcome: "skip" },
    { case: "cancelled", outcome: "cancelled" },
  ]);
  return builder;
}

test("finish stamps v1 and ends with one newline", () => {
  const builder = new EvidenceBuilder();
  builder.addTest("t", "b", [{ case: "e", outcome: "pass" }]);

  const text = builder.finish();

  expect(JSON.parse(text).v).toBe(1);
  expect(version()).toBe(1);
  expect(text.endsWith("\n")).toBe(true);
  expect(text.endsWith("\n\n")).toBe(false);
});

test("an emitted file conforms to the JSON schema", () => {
  const text = buildRepresentative().finish();

  const validate = new Ajv2020().compile(SCHEMA);
  expect(validate(JSON.parse(text))).toBe(true);
});

test("a missing outcome is an absent key, not null", () => {
  const builder = new EvidenceBuilder();
  builder.addTest("t", "b", [{ case: "declared_only" }]);

  const execution = JSON.parse(builder.finish()).tests[0].executions[0];

  expect(execution).toStrictEqual({ case: "declared_only" });
});

test("empty provenance is omitted entirely", () => {
  const builder = new EvidenceBuilder();
  builder.addTest("t", "b", [{ case: "e" }]);

  expect(JSON.parse(builder.finish())).not.toHaveProperty("provenance");
});

test("provenance replaces an earlier value under the same key", () => {
  const builder = new EvidenceBuilder();
  builder.provenance("source_revision", "old");
  builder.provenance("source_revision", "new");
  builder.addTest("t", "b", [{ case: "e" }]);

  expect(JSON.parse(builder.finish()).provenance).toStrictEqual({
    source_revision: "new",
  });
});

describe("a blank provenance key or value is rejected", () => {
  test.each([
    ["", "value"],
    ["  ", "value"],
    ["key", ""],
    ["key", " "],
  ])("key %j, value %j", (key, value) => {
    const builder = new EvidenceBuilder();

    expect(() => builder.provenance(key, value)).toThrowError(
      expect.objectContaining({
        name: "InvalidEvidenceError",
        message: expect.stringContaining("blank key or value"),
      }),
    );
  });
});

test("capsulaRunId fills only when nonblank and unclaimed", () => {
  const filled = new EvidenceBuilder();
  filled.capsulaRunId("run-1");
  filled.addTest("t", "b", [{ case: "e" }]);
  expect(JSON.parse(filled.finish()).provenance).toStrictEqual({
    capsula_run_id: "run-1",
  });

  const untouched = new EvidenceBuilder();
  untouched.capsulaRunId(undefined);
  untouched.capsulaRunId("   ");
  untouched.addTest("t", "b", [{ case: "e" }]);
  expect(JSON.parse(untouched.finish())).not.toHaveProperty("provenance");

  const claimed = new EvidenceBuilder();
  claimed.provenance("capsula_run_id", "explicit");
  claimed.capsulaRunId("run-2");
  claimed.addTest("t", "b", [{ case: "e" }]);
  expect(JSON.parse(claimed.finish()).provenance).toStrictEqual({
    capsula_run_id: "explicit",
  });
});

describe("invalid tests are rejected at add time", () => {
  test.each([
    ["  ", "b", [{ case: "e" }], "test id is blank"],
    ["t", "", [{ case: "e" }], "empty contract body"],
    ["t", "b", [], "declares no executions"],
    ["t", "b", [{ case: "  " }], "blank case id"],
    ["t", "b", [{ case: "e" }, { case: "e" }], "more than once"],
  ] as const)("%j / %j / %j", (id, body, executions, message) => {
    const builder = new EvidenceBuilder();

    expect(() => builder.addTest(id, body, executions)).toThrowError(
      expect.objectContaining({
        name: "InvalidEvidenceError",
        message: expect.stringContaining(message),
      }),
    );

    builder.addTest("clean", "b", [{ case: "e" }]);
    const tests = JSON.parse(builder.finish()).tests;
    expect(tests.map((t: { id: string }) => t.id)).toStrictEqual(["clean"]);
  });
});

test("a duplicate test id across calls is rejected", () => {
  const builder = new EvidenceBuilder();
  builder.addTest("t", "b", [{ case: "e" }]);

  expect(() =>
    builder.addTest("t", "other body", [{ case: "other" }]),
  ).toThrowError(expect.objectContaining({ name: "InvalidEvidenceError" }));

  expect(JSON.parse(builder.finish()).tests).toHaveLength(1);
});

test("an unknown outcome is rejected with the wire vocabulary", () => {
  const builder = new EvidenceBuilder();

  // The expected-variant list comes from the shared crate's serde enum, so
  // this match also pins that the message names the real vocabulary. The
  // spelling is only representable by defeating the generated types, which
  // is the point: the type system catches it first.
  expect(() =>
    builder.addTest("t", "b", [{ case: "e", outcome: "passed" as "pass" }]),
  ).toThrowError(
    expect.objectContaining({
      name: "TypeError",
      message: expect.stringContaining(
        "unknown variant `passed`, expected one of `pass`, `fail`, `skip`, `cancelled`",
      ),
    }),
  );
});

test("a stray field in an execution object is rejected", () => {
  const builder = new EvidenceBuilder();
  const stray = [{ case: "e", digest: "sha256:00" }] as unknown as [
    { case: string },
  ];

  expect(() => builder.addTest("t", "b", stray)).toThrowError(
    expect.objectContaining({
      name: "TypeError",
      message: expect.stringContaining("unknown field `digest`"),
    }),
  );
});

test("a non-array executions argument is rejected", () => {
  const builder = new EvidenceBuilder();
  const bogus = "not-a-list" as unknown as [];

  expect(() => builder.addTest("t", "b", bogus)).toThrowError(
    expect.objectContaining({
      name: "TypeError",
      message: expect.stringContaining("expected a sequence"),
    }),
  );
});

describe("executions with no JSON form are a TypeError, not a wasm abort", () => {
  test.each([
    ["undefined", undefined],
    ["a function", () => []],
    ["a symbol", Symbol("executions")],
  ])("%s", (_label, bogus) => {
    const builder = new EvidenceBuilder();

    expect(() =>
      builder.addTest("t", "b", bogus as unknown as []),
    ).toThrowError(
      expect.objectContaining({
        name: "TypeError",
        message: expect.stringContaining("no JSON form"),
      }),
    );

    // The failed call must not poison the builder's &mut borrow.
    builder.addTest("still-usable", "b", [{ case: "e" }]);
    expect(JSON.parse(builder.finish()).tests).toHaveLength(1);
  });
});

test("a value JSON.stringify throws on keeps its message in the TypeError", () => {
  const builder = new EvidenceBuilder();
  const bigint = [{ case: "e", outcome: 1n }] as unknown as [];

  expect(() => builder.addTest("t", "b", bigint)).toThrowError(
    expect.objectContaining({
      name: "TypeError",
      message: expect.stringContaining("not JSON-serializable"),
    }),
  );

  builder.addTest("still-usable", "b", [{ case: "e" }]);
  expect(JSON.parse(builder.finish()).tests).toHaveLength(1);
});

test("a null outcome is read as a missing result, like an absent key", () => {
  const builder = new EvidenceBuilder();
  builder.addTest("t", "b", [{ case: "e", outcome: null } as unknown as { case: string }]);

  expect(JSON.parse(builder.finish()).tests[0].executions[0]).toStrictEqual({
    case: "e",
  });
});

test("an object with toJSON that yields a valid list is accepted", () => {
  const builder = new EvidenceBuilder();
  const viaToJson = {
    toJSON: () => [{ case: "e", outcome: "pass" }],
  } as unknown as [];
  builder.addTest("t", "b", viaToJson);

  expect(JSON.parse(builder.finish()).tests[0].executions).toStrictEqual([
    { case: "e", outcome: "pass" },
  ]);
});

test("capsulaRunId treats null like undefined", () => {
  const builder = new EvidenceBuilder();
  builder.capsulaRunId(null as unknown as undefined);
  builder.addTest("t", "b", [{ case: "e" }]);

  expect(JSON.parse(builder.finish())).not.toHaveProperty("provenance");
});

test("invalid evidence errors are ordinary Error instances", () => {
  const builder = new EvidenceBuilder();

  try {
    builder.addTest("  ", "b", [{ case: "e" }]);
    expect.unreachable("a blank test id must throw");
  } catch (error) {
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).name).toBe("InvalidEvidenceError");
  }
});
