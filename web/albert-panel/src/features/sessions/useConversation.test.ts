import { describe, expect, it } from "vitest";

import { mergeMessages } from "./useConversation";
import type { Message } from "./useConversation";

const message = (id: string, content = id): Message => ({
  id,
  content,
  role: "user",
  created_at: "2026-10-26T12:00:00Z",
});

describe("history reconciliation", () => {
  it("respects journal page order even when timestamps match and public IDs sort differently", () => {
    const initial = mergeMessages(
      [],
      [message("public-a"), message("public-z")],
    );
    expect(initial.map((item) => item.id)).toEqual(["public-z", "public-a"]);
    const older = mergeMessages(
      initial,
      [message("older-b"), message("older-y")],
      true,
    );
    expect(older.map((item) => item.id)).toEqual([
      "older-y",
      "older-b",
      "public-z",
      "public-a",
    ]);
    const replay = mergeMessages(older, [
      message("new-c"),
      message("public-a", "updated"),
    ]);
    expect(replay.map((item) => item.id)).toEqual([
      "older-y",
      "older-b",
      "public-z",
      "public-a",
      "new-c",
    ]);
    expect(replay.find((item) => item.id === "public-a")?.content).toBe(
      "updated",
    );
  });
});
