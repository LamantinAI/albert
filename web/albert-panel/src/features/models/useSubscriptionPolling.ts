import type { Dispatch, SetStateAction } from "react";
import { useEffect } from "react";
import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
type Attempt = components["schemas"]["OpenAiLoginView"];
type Connection = components["schemas"]["SubscriptionConnectionView"];
export const pending = (attempt: Attempt | null) =>
  attempt?.phase === "awaiting_callback" || attempt?.phase === "exchanging";
export function useSubscriptionPolling({
  attempt,
  connection,
  setAttempt,
  setConnection,
  setCallback,
  setError,
  setDone,
  resumeConnection,
}: {
  attempt: Attempt | null;
  connection: Connection | null;
  setAttempt: Dispatch<SetStateAction<Attempt | null>>;
  setConnection: Dispatch<SetStateAction<Connection | null>>;
  setCallback: (value: string) => void;
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
          setCallback("");
          await resumeConnection();
          return;
        }
        if (next.phase !== "awaiting_callback" && next.phase !== "exchanging")
          return;
      } catch (error) {
        if (!controller.signal.aborted) setError(errorMessage(error));
      }
      if (!controller.signal.aborted)
        timer = setTimeout(
          poll,
          attempt?.phase === "awaiting_callback" ? 5000 : 750,
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
