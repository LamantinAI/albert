import { expect, test } from "vitest";
import { chatReturnPath } from "./modelChoices";

test("model setup returns only to a local chat route", () => {
  expect(chatReturnPath("/chats/session-1")).toBe("/chats/session-1");
  expect(chatReturnPath("/chats?new=temporary")).toBe("/chats?new=temporary");
  for (const path of [
    null,
    "//example.test/chats",
    "https://example.test/chats",
    "/chats/../users",
    "/chats\\example",
    "http://[",
    "javascript:alert(1)",
  ]) {
    expect(chatReturnPath(path)).toBe("/chats");
  }
});
