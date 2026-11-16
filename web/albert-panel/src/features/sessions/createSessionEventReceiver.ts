import type { Dispatch, RefObject, SetStateAction } from "react";
import type { components } from "../../api/schema";
import { mergeProgress, progressDelta } from "./progress";
import { mergeText, textDelta } from "./text";
import type { SessionState } from "./useConversation";
type SessionEvent = components["schemas"]["SessionEvent"];
export function createSessionEventReceiver({
  id,
  isStopped,
  missingRef,
  epoch,
  setEvents,
  latestState,
  setState,
  unknownAttempt,
  reconcile,
  eventIds,
}: {
  id: string;
  isStopped: () => boolean;
  missingRef: RefObject<boolean>;
  epoch: RefObject<string | null>;
  setEvents: Dispatch<SetStateAction<SessionEvent[]>>;
  latestState: RefObject<SessionState | null>;
  setState: Dispatch<SetStateAction<SessionState | null>>;
  unknownAttempt: RefObject<string | null>;
  reconcile: (reset: boolean) => Promise<void>;
  eventIds: Set<string>;
}) {
  return (event: MessageEvent) => {
    if (isStopped() || missingRef.current) return;
    try {
      const value = JSON.parse(event.data) as SessionEvent;
      if (value.session_id !== id) return;
      if (value.id && eventIds.has(value.id)) return;
      if (value.id) {
        eventIds.add(value.id);
        if (eventIds.size > 1000)
          eventIds.delete(eventIds.values().next().value!);
      }
      const reset =
        /gap|resync|reset/.test(value.kind) ||
        (epoch.current !== null && value.runtime_epoch !== epoch.current);
      setEvents((old) => [...old.slice(-49), value]);
      if (value.kind === "text" && !reset) {
        const delta = textDelta(value.data);
        if (!delta || delta.attempt_id !== value.attempt_id) {
          void reconcile(true);
          return;
        }
        if (
          latestState.current?.active_attempt?.id === delta.attempt_id ||
          latestState.current?.text?.attempt_id === delta.attempt_id
        ) {
          setState((previous) => {
            if (
              !previous ||
              (previous.active_attempt?.id !== delta.attempt_id &&
                previous.text?.attempt_id !== delta.attempt_id)
            )
              return previous;
            const next = mergeText(previous.text, delta);
            if (!next) {
              queueMicrotask(() => {
                if (!isStopped()) void reconcile(false);
              });
              return previous;
            }
            return { ...previous, text: next };
          });
          if (delta.operation === "committed") void reconcile(false);
        } else if (unknownAttempt.current !== delta.attempt_id) {
          unknownAttempt.current = delta.attempt_id;
          void reconcile(false);
        }
        return;
      }
      if (value.kind === "progress" && !reset) {
        const delta = progressDelta(value.data);
        if (!delta || delta.attempt_id !== value.attempt_id) {
          void reconcile(true);
          return;
        }
        if (latestState.current?.active_attempt?.id === delta.attempt_id) {
          setState((previous) =>
            previous?.active_attempt?.id === delta.attempt_id
              ? {
                  ...previous,
                  progress: mergeProgress(previous.progress, delta),
                }
              : previous,
          );
        } else if (
          latestState.current?.progress?.attempt_id !== delta.attempt_id &&
          unknownAttempt.current !== delta.attempt_id
        ) {
          unknownAttempt.current = delta.attempt_id;
          void reconcile(false);
        }
        return;
      }
      if (["credentials", "approvals"].includes(value.kind) && !reset) return;
      if (value.kind === "status" && !reset) return;
      void reconcile(reset);
    } catch {
      void reconcile(true);
    }
  };
}
