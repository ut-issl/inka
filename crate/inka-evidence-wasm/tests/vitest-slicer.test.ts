// Unit tests for the body slicer and its task cross-checks, on synthetic
// sources — cheaper than a nested vitest run per shape.

import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { expect, test } from "vitest";

import { SourceCache, patternMatches } from "../js/vitest";

/** Writes a synthetic source and returns its path. */
function sourceFile(name: string, text: string): string {
  const dir = mkdtempSync(join(tmpdir(), "inka-evidence-slicer-"));
  const path = join(dir, name);
  writeFileSync(path, text, "utf8");
  return path;
}

test("slices across CRLF lines and multibyte text", () => {
  const text =
    'const 日本語 = "émoji 🎌";\r\n' +
    'test("adds", () => {\r\n' +
    "  expect(1 + 1).toBe(2);\r\n" +
    "});\r\n";
  const path = sourceFile("crlf.test.ts", text);

  const body = new SourceCache().slice(
    path,
    { line: 2, column: 1 },
    "crlf.test.ts > adds",
    "adds",
  );

  expect(body).toBe('test("adds", () => {\r\n  expect(1 + 1).toBe(2);\r\n})');
});

test("slices through a comment between callee and arguments", () => {
  const text = 'test /* note */ ("a", function () {\n  check();\n});\n';
  const path = sourceFile("comment.test.ts", text);

  const body = new SourceCache().slice(
    path,
    { line: 1, column: 1 },
    "comment.test.ts > a",
    "a",
  );

  expect(body).toBe('test /* note */ ("a", function () {\n  check();\n})');
});

test("a literal name that does not match the task is an error", () => {
  const text = 'test("other", () => {});\n';
  const path = sourceFile("mismatch.test.ts", text);

  expect(() =>
    new SourceCache().slice(path, { line: 1, column: 1 }, "id", "expected"),
  ).toThrowError(/does not match the task name/);
});

test("a tagged-template each chain is rejected outright", () => {
  const text =
    'test.each`\n  a\n  ${1}\n`("t $a", ({ a }) => {\n  check(a);\n});\n';
  const path = sourceFile("tagged.test.ts", text);

  expect(() =>
    new SourceCache().slice(path, { line: 1, column: 1 }, "id", "t 1"),
  ).toThrowError(/tagged-template each/);
});

test("a call that cannot be a test declaration is an error", () => {
  const text = "register(42);\n";
  const path = sourceFile("notatest.test.ts", text);

  expect(() =>
    new SourceCache().slice(path, { line: 1, column: 1 }, "id", "name"),
  ).toThrowError(/does not look like a test declaration/);
});

test("a dynamic name is accepted as collected", () => {
  const text = 'const name = "dyn";\ntest(name, () => {\n  check();\n});\n';
  const path = sourceFile("dynamic.test.ts", text);

  const body = new SourceCache().slice(
    path,
    { line: 2, column: 1 },
    "dynamic.test.ts > dyn",
    "dyn",
  );

  expect(body).toBe("test(name, () => {\n  check();\n})");
});

test("each patterns match their formatted names", () => {
  expect(patternMatches("positive %i", "positive 1")).toBe(true);
  expect(patternMatches("t $a", "t 1")).toBe(true);
  expect(patternMatches("$#: %s end", "0: x end")).toBe(true);
  expect(patternMatches("100%% sure", "100% sure")).toBe(true);
  expect(patternMatches("v $x.y end", "v 7 end")).toBe(true);
  expect(patternMatches("exact", "other")).toBe(false);
  expect(patternMatches("v $x end", "v 1 endX")).toBe(false);
});
