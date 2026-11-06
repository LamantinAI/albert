import { expect, test } from "vitest";
import { activityAppearance } from "./activityAppearance";

test("host categories choose distinct semantic icons without inspecting labels", () => {
  const expected = {
    scratchpad: "notebook",
    memory: "brain",
    skill: "skills",
    connector: "plug",
    search: "search",
    browser: "globe",
    files: "folder",
    shell: "terminal",
    sql: "database",
    calendar: "calendar",
    mail: "mail",
    image: "image",
    audio_input: "mic",
    audio_output: "volume",
    subagent: "robot",
    sessions: "chats",
    configuration: "settings",
    model: "models",
  };
  for (const [activity, icon] of Object.entries(expected))
    expect(activityAppearance({ kind: "tool", activity }).icon).toBe(icon);
  expect(activityAppearance({ kind: "tool", activity: "memory" }).label).toBe(
    "Memory",
  );
  expect(
    activityAppearance({ kind: "tool", activity: "scratchpad" }).label,
  ).toBe("Scratchpad");
});

test("legacy, unknown and malformed metadata remains generic", () => {
  for (const activity of [
    undefined,
    null,
    "unknown",
    "future_category",
    "__proto__",
    "constructor",
    7,
    {},
  ])
    expect(activityAppearance({ kind: "tool", activity })).toEqual({
      icon: "tool",
      label: "Tool",
    });
  expect(activityAppearance({ kind: "stage", activity: "memory" })).toEqual({
    icon: "plan",
    label: "Step",
  });
  expect(activityAppearance({ kind: "child", activity: "memory" })).toEqual({
    icon: "robot",
    label: "Subagent",
  });
});
