import { expect, test } from "vitest";

import { resolveUiRoot } from "./paths";

test("relative workspace roots follow arbitrary proxy prefixes and deep routes", () => {
  expect(
    resolveUiRoot("../", "https://workspace.example/tools/agent/ui/chats/one"),
  ).toBe("/tools/agent/ui");
  expect(resolveUiRoot("./", "https://workspace.example/ui/")).toBe("/ui");
  expect(
    resolveUiRoot(
      "../../",
      "https://workspace.example/custom/ui/settings/documents/models",
    ),
  ).toBe("/custom/ui");
  expect(
    resolveUiRoot(
      "../",
      "https://workspace.example/ui/agents/api/v1/ui/docs/index.html",
    ),
  ).toBe("/ui/agents/api/v1/ui");
});

test("workspace metadata cannot turn a relative route into another origin", () => {
  expect(() =>
    resolveUiRoot("//untrusted.invalid/ui/", "https://workspace.example/ui/"),
  ).toThrow("Invalid workspace path");
});
