import type { components } from "../../api/schema";

type SessionState = components["schemas"]["SessionState"];
type Message = components["schemas"]["MessageView"];

export type ProgressView = {
  attempt: string;
  knownMessages: Set<string>;
  completed: boolean;
  message: string | null;
};

/** Presentation belongs only to work observed during this open chat view. */
export function projectProgress(
  view: ProgressView | null,
  snapshot: SessionState,
  descendingMessages: Message[],
) {
  const active = snapshot.active_attempt?.id;
  const progress = snapshot.progress;
  if (active) {
    const next =
      view?.attempt === active
        ? { ...view, completed: false, message: null }
        : {
            attempt: active,
            knownMessages: new Set(
              descendingMessages.map((message) => message.id),
            ),
            completed: false,
            message: null,
          };
    return {
      view: next,
      progress: progress?.attempt_id === active ? progress : null,
    };
  }
  if (!view || progress?.attempt_id !== view.attempt)
    return { view, progress: null };
  const latest = descendingMessages.find(
    (message) => message.role !== "activity",
  );
  const committed =
    snapshot.text?.attempt_id === view.attempt &&
    snapshot.text.state === "committed"
      ? snapshot.text.message_ids
      : [];
  const answer = committed.length
    ? descendingMessages.find(
        (message) =>
          message.role === "assistant" && committed.includes(message.id),
      )
    : latest;
  if (!view.completed && committed.length && !answer)
    return { view, progress: null };
  const next = view.completed
    ? view
    : {
        ...view,
        completed: true,
        message:
          answer?.role === "assistant" && !view.knownMessages.has(answer.id)
            ? answer.id
            : null,
      };
  // Never move an old decoration to a later, unrelated message.
  return {
    view: next,
    progress: latest?.id === next.message ? progress : null,
  };
}
