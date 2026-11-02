import { expect, test } from "vitest";
import { mergeText, restoreText, textDelta } from "./text";
import type { TextDelta } from "./text";

const start: TextDelta = {
  attempt_id: "work",
  generation: "one",
  operation: "start",
  offset: 0,
  text: "",
  message_ids: [],
};
test("UTF-8 offsets append once and gaps require a snapshot", () => {
  const first = mergeText(null, start)!;
  const append = { ...start, operation: "append", text: "Привет" };
  const next = mergeText(first, append)!;
  expect(next.offset).toBe(12);
  expect(next.text).toBe("Привет");
  expect(mergeText(next, append)).toBe(next);
  expect(mergeText(next, { ...append, offset: 14 })).toBeNull();
  expect(textDelta({ ...append, operation: "reasoning" })).toBeNull();
});
test("retry resets generations and commit never resurrects from stale snapshots", () => {
  let value = mergeText(null, start)!;
  value = mergeText(value, { ...start, operation: "append", text: "draft" })!;
  const committed = mergeText(value, {
    ...start,
    operation: "committed",
    message_ids: ["answer"],
  })!;
  expect(restoreText(committed, value)?.state).toBe("committed");
  expect(restoreText(value, { ...committed, text: "" })?.text).toBe("draft");
  const retry = mergeText(committed, { ...start, generation: "two" })!;
  expect(retry.text).toBe("");
  expect(
    mergeText(retry, { ...start, operation: "append", text: "old" }),
  ).toBeNull();
});
