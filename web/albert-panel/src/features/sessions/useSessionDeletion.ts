import { useState } from "react";
import { api, ApiError, errorMessage, requestId } from "../../api/client";
import type {
  SessionDeleteAttempt,
  SessionLifecycleTools,
} from "./sessionActionTypes";

export function useSessionDeletion(tools: SessionLifecycleTools) {
  const [pending, setPending] = useState<SessionDeleteAttempt | null>(null);
  const [writing, setWriting] = useState<SessionDeleteAttempt | null>(null);
  async function execute(attempt: SessionDeleteAttempt) {
    if (tools.mutating.current) return;
    tools.mutating.current = true;
    setWriting(attempt);
    tools.setBusy(true);
    tools.setError("");
    try {
      for (let pass = 0; pass < 2; pass++) {
        try {
          await api("/sessions/{id}", "delete", {
            path: { id: attempt.session.id },
            revision: attempt.session.revision,
            idempotencyKey: attempt.key,
          });
          if (tools.alive.current) {
            setPending(null);
            tools.finish(attempt);
          }
          return;
        } catch (reason) {
          if (!(reason instanceof ApiError) && pass === 0) continue;
          if (!tools.alive.current) return;
          const retryable =
            !(reason instanceof ApiError) || reason.status >= 500;
          setPending(retryable ? attempt : null);
          tools.setError(
            retryable
              ? `${reason instanceof ApiError ? errorMessage(reason) : "The deletion response was lost."} Retry deletion to confirm cleanup completed.`
              : errorMessage(reason),
          );
          if (reason instanceof ApiError && [409, 412].includes(reason.status))
            void tools.refreshSession(attempt.session.id).catch(() => {});
          return;
        }
      }
    } finally {
      tools.mutating.current = false;
      if (tools.alive.current) {
        setWriting(null);
        tools.setBusy(false);
      }
    }
  }
  async function remove() {
    if (pending) return;
    const value = await tools.prepare("permanent", true);
    if (!value) return;
    if (
      !(await tools.confirm({
        title: "Delete conversation?",
        description: `Deletes “${value.session.title || "Untitled conversation"}”, its attachments and managed attachment copies. Other workspace files and completed external actions remain. This cannot be undone.`,
        confirmLabel: "Delete chat",
        cancelLabel: "Keep chat",
        danger: true,
      })) ||
      !tools.alive.current
    )
      return;
    await execute({ ...value, key: requestId() });
  }
  return {
    remove,
    pending,
    deletingSession: writing?.session ?? pending?.session ?? null,
    retry: () => (pending ? execute(pending) : Promise.resolve()),
  };
}
