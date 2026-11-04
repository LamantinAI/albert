import { expect, test } from "vitest";

import type { components } from "../../api/schema";
import { projectProgress } from "./ephemeralProgress";

function state(
  active: boolean,
  attempt = "work",
): components["schemas"]["SessionState"] {
  return {
    session: {
      id: "chat",
      title: "Chat",
      host: "host",
      kind: "permanent",
      state: "active",
      revision: 1,
      created_at: "2030-01-01T00:00:00Z",
    },
    active_attempt: active
      ? { id: attempt, state: "running", session_id: "chat" }
      : null,
    runtime_epoch: "epoch",
    status: [],
    progress: {
      attempt_id: attempt,
      started_at: "2030-01-01T00:00:00Z",
      updated_at: "2030-01-01T00:00:00Z",
      truncated: false,
      items: [],
    },
  };
}
function message(
  id: string,
  role = "assistant",
): components["schemas"]["MessageView"] {
  return { id, role, content: id, created_at: "2030-01-01T00:00:00Z" };
}

test("only work observed in this open view can decorate its new final answer", () => {
  const live = projectProgress(null, state(true), [message("old")]);
  expect(live.progress?.attempt_id).toBe("work");
  const final = state(false);
  const messages = [message("answer"), message("old")];
  const ended = projectProgress(live.view, final, messages);
  expect(ended.progress).toBe(final.progress);
  expect(ended.view?.message).toBe("answer");
  expect(projectProgress(null, final, messages).progress).toBeNull();
});

test("an interrupted attempt never decorates an old or later unrelated reply", () => {
  const live = projectProgress(null, state(true), [message("old")]);
  const ended = projectProgress(live.view, state(false), [message("old")]);
  expect(ended.progress).toBeNull();
  expect(
    projectProgress(ended.view, state(false), [message("unrelated")]).progress,
  ).toBeNull();
});

test("an exact streamed commit waits for its answer instead of decorating a linked reply", () => {
  const live = projectProgress(null, state(true), [message("old")]);
  const final = {
    ...state(false),
    text: {
      attempt_id: "work",
      generation: "one",
      state: "committed",
      offset: 0,
      text: "",
      truncated: false,
      message_ids: ["actual-answer"],
    },
  };
  const waiting = projectProgress(live.view, final, [
    message("linked-reply"),
    message("old"),
  ]);
  expect(waiting.progress).toBeNull();
  const ended = projectProgress(waiting.view, final, [
    message("actual-answer"),
    message("linked-reply"),
  ]);
  expect(ended.view?.message).toBe("actual-answer");
  expect(ended.progress).toBe(final.progress);
});

test("new messages retire the decoration and a new attempt cannot reuse old progress", () => {
  const live = projectProgress(null, state(true), [message("old")]);
  const ended = projectProgress(live.view, state(false), [message("answer")]);
  expect(
    projectProgress(ended.view, state(false), [
      message("question", "user"),
      message("answer"),
    ]).progress,
  ).toBeNull();
  const next = projectProgress(ended.view, state(true, "next"), [
    message("question", "user"),
  ]);
  expect(next.progress?.attempt_id).toBe("next");
  expect(
    projectProgress(next.view, state(false), [message("answer")]).progress,
  ).toBeNull();
  expect(
    projectProgress(
      next.view,
      { ...state(true, "next"), progress: state(false).progress },
      [message("question", "user")],
    ).progress,
  ).toBeNull();
});
