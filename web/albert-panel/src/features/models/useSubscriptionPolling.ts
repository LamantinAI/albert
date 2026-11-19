import type { Dispatch, SetStateAction } from "react";
import { useEffect } from "react";
import { api, ApiError, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
type Attempt = components["schemas"]["OpenAiLoginView"];
type Connection = components["schemas"]["SubscriptionConnectionView"];
export const pending = (attempt: Attempt | null) =>
  attempt?.phase === "preparing" || attempt?.phase === "awaiting_device_code";
export function useSubscriptionPolling({
  attempt,
  connection,
  setAttempt,
  setConnection,
  setError,
  setDone,
  resumeConnection,
}: {
  attempt: Attempt | null;
  connection: Connection | null;
  setAttempt: Dispatch<SetStateAction<Attempt | null>>;
  setConnection: Dispatch<SetStateAction<Connection | null>>;
  setError: (value: string) => void;
  setDone: (value: boolean) => void;
  resumeConnection: () => Promise<Connection>;
}) {
  useEffect(() => {
    if (!pending(attempt)) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await api("/openai-login-attempts/{id}", "get", {
          path: { id: attempt!.id },
          signal: controller.signal,
        });
        if (controller.signal.aborted) return;
        setAttempt(next);
        if (next.phase === "connected") {
          await resumeConnection();
          return;
        }
        if (!pending(next)) return;
      } catch (error) {
        if (controller.signal.aborted) return;
        if (error instanceof ApiError && error.status === 404 && attempt) {
          setAttempt({
            ...attempt,
            phase: "expired",
            verification_url: null,
            user_code: null,
            message: "This code is no longer active. Start a new sign-in.",
          });
          return;
        }
        setError(errorMessage(error));
      }
      if (!controller.signal.aborted)
        timer = setTimeout(
          poll,
          attempt?.phase === "awaiting_device_code" ? 1500 : 500,
        );
    }
    timer = setTimeout(poll, 250);
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [attempt?.id, attempt?.phase, resumeConnection]);

  useEffect(() => {
    if (connection?.phase !== "applying") return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await api("/openai-subscription", "get", {
          signal: controller.signal,
        });
        if (controller.signal.aborted) return;
        setConnection(next);
        if (next.phase === "ready") setDone(true);
        if (next.phase !== "applying") return;
      } catch (error) {
        if (!controller.signal.aborted) setError(errorMessage(error));
      }
      if (!controller.signal.aborted) timer = setTimeout(poll, 750);
    }
    timer = setTimeout(poll, 250);
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [connection?.phase]);
}
