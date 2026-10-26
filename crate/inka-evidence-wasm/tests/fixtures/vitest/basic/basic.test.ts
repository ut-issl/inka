import { describe, expect, test } from "vitest";

describe("math", () => {
  test("adds", () => {
    expect(1 + 1).toBe(2);
  });

  test.each([1, 2])("positive %i", (n) => {
    expect(n).toBeGreaterThan(0);
  });
});

test("fails", () => {
  expect(true).toBe(false);
});

test.skip("skipped", () => {
  expect(1).toBe(1);
});

test.todo("todo later");
