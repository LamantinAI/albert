import { useEffect, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { useStructuredDraft } from "./useStructuredDraft";
export type AgentDocument = components["schemas"]["ConfigDocument"];
export function useAgentDocument(initialMode: "fields" | "raw") {
  const [document, setDocument] = useState<AgentDocument | null>(null);
  const [content, setContent] = useState("");
  const [mode, setMode] = useState(initialMode);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const draft = useStructuredDraft(
    document,
    content,
    mode === "fields",
    setContent,
    () => {},
  );
  const dirty = !!document && (content !== document.content || draft.hasEdits);
  function accept(value: AgentDocument) {
    draft.reset();
    setDocument(value);
    setContent(value.content);
    setError("");
    if (value.redacted) setMode("raw");
  }
  async function read(signal?: AbortSignal) {
    return api("/configuration/{id}", "get", {
      path: { id: "albert" },
      signal,
    });
  }
  useEffect(() => {
    const controller = new AbortController();
    void read(controller.signal)
      .then((value) => {
        if (!controller.signal.aborted) accept(value);
      })
      .catch((reason) => {
        if (!controller.signal.aborted) setError(errorMessage(reason));
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, []);
  async function reload() {
    setLoading(true);
    try {
      accept(await read());
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setLoading(false);
    }
  }
  async function switchMode(next: "fields" | "raw") {
    if (next === mode) return;
    const candidate =
      next === "raw" ? await draft.flush() : await draft.refresh();
    if (candidate !== null) {
      setContent(candidate);
      setMode(next);
    }
  }
  return {
    document,
    content,
    mode,
    loading,
    error,
    draft,
    dirty,
    accept,
    read,
    reload,
    setContent,
    switchMode,
  };
}
export type AgentEditor = ReturnType<typeof useAgentDocument>;
