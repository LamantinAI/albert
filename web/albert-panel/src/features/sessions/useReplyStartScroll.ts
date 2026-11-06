import { useCallback, useRef } from "react";
import type { TextSnapshot } from "./text";

/** Only work observed in this view can move the viewport to its completed reply. */
export function useReplyStartScroll(
  session: string,
  activeAttempt: string | undefined,
  text: TextSnapshot | null | undefined,
) {
  const scope = useRef(session);
  const observed = useRef<string | null>(null);
  const pending = useRef<{ attempt: string; following: boolean } | null>(null);
  return useCallback(
    (node: HTMLElement, following: { current: boolean }) => {
      if (scope.current !== session) {
        scope.current = session;
        observed.current = null;
        pending.current = null;
      }
      if (activeAttempt) {
        observed.current = activeAttempt;
        pending.current = null;
        return false;
      }
      if (observed.current) {
        pending.current = {
          attempt: observed.current,
          following: following.current,
        };
        observed.current = null;
      }
      const completion = pending.current;
      if (!completion) return false;
      if (!completion.following || !following.current) {
        pending.current = null;
        return false;
      }
      if (text?.attempt_id !== completion.attempt || text.state !== "committed")
        return false;
      const answer = Array.from(
        node.querySelectorAll<HTMLElement>(
          ".message-assistant[data-message-id]",
        ),
      ).find((element) =>
        text.message_ids.includes(element.dataset.messageId ?? ""),
      );
      if (!answer) return false; // The canonical page may arrive after the terminal event.
      pending.current = null;
      const top =
        node.scrollTop +
        answer.getBoundingClientRect().top -
        node.getBoundingClientRect().top -
        16;
      following.current = false;
      node.scrollTo({ top: Math.max(0, top), behavior: "auto" });
      following.current =
        node.scrollHeight - node.scrollTop - node.clientHeight < 90;
      return true;
    },
    [session, activeAttempt, text],
  );
}
