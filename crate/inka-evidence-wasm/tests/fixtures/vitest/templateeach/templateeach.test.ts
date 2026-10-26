import { expect, test } from "vitest";

test.each`
  a    | b
  ${1} | ${2}
  ${3} | ${4}
`("t $a", ({ a }: { a: number }) => {
  expect(a).toBeGreaterThan(0);
});

test("after", () => {
  expect(1).toBe(1);
});
