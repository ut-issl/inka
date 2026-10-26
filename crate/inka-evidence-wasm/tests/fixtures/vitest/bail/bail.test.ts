import { expect, test } from "vitest";

test("first fails", () => {
  expect(true).toBe(false);
});

test("second never runs", () => {
  expect(1).toBe(1);
});
