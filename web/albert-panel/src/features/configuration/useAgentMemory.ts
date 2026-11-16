import { useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import { useMemoryDraft } from "../memory/useMemoryDraft";
export function useAgentMemory() {
  const draft = useMemoryDraft();
  const generation = useRef(0);
  function accept(view: Parameters<typeof draft.accept>[0]) {
    generation.current++;
    draft.accept(view);
    setError("");
  }
  function invalidate() {
    generation.current++;
    draft.clear();
    setError("Memory status could not be refreshed. Reload it before editing.");
  }
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  async function read(signal?: AbortSignal) {
    return api("/memory", "get", { signal });
  }
  useEffect(() => {
    const controller = new AbortController();
    const version = generation.current;
    void read(controller.signal)
      .then((view) => {
        if (!controller.signal.aborted && generation.current === version)
          accept(view);
      })
      .catch((reason) => {
        if (!controller.signal.aborted && generation.current === version)
          setError(errorMessage(reason));
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, []);
  return {
    ...draft,
    accept,
    invalidate,
    error,
    loading,
    read,
    clearError: () => setError(""),
  };
}
export type AgentMemoryDraft = ReturnType<typeof useAgentMemory>;
