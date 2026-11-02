import { useCallback, useEffect, useRef, useState } from "react";

import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";

export type ConfigField = components["schemas"]["ConfigField"];
type Document = components["schemas"]["ConfigDocument"];
type Projection = components["schemas"]["ConfigDraftView"];
type Edits = Record<string, string>;

function scalar(field: ConfigField, text: string): unknown {
  if (text === "" || (field.kind === "boolean" && text === "default"))
    return null;
  if (field.kind === "text") return text;
  if (field.kind === "boolean") return text === "true";
  const number = Number(text);
  if (
    !Number.isSafeInteger(number) ||
    (field.minimum != null && number < field.minimum) ||
    (field.maximum != null && number > field.maximum)
  ) {
    throw new Error(`Enter a valid whole number for ${field.label}.`);
  }
  return number;
}

export function useStructuredDraft(
  document: Document | null,
  content: string,
  active: boolean,
  onChange: (text: string) => void,
  onEdit: () => void,
) {
  const latest = useRef({ document, content, onChange, onEdit });
  latest.current = { document, content, onChange, onEdit };
  const [projection, setProjection] = useState<Projection | null>(null);
  const [edits, setEdits] = useState<Edits>({});
  const editsRef = useRef<Edits>({});
  const fieldsRef = useRef<ConfigField[]>([]);
  const lastProjection = useRef<{ content: string; revision: string } | null>(
    null,
  );
  const generation = useRef(0);
  const controller = useRef<AbortController | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const mounted = useRef(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      generation.current++;
      clearTimeout(timer.current);
      controller.current?.abort();
    };
  }, []);

  const run = useCallback(async (): Promise<string | null> => {
    clearTimeout(timer.current);
    controller.current?.abort();
    const current = latest.current;
    if (
      !current.document ||
      current.document.redacted ||
      current.document.path !== "albert"
    )
      return null;
    const request = ++generation.current;
    const source = current.content;
    const revision = current.document.revision;
    const desired = editsRef.current;
    const abort = new AbortController();
    controller.current = abort;
    const deadline = setTimeout(() => abort.abort(), 15000);
    setPending(true);
    setError("");
    try {
      const changes = Object.entries(desired).map(([path, text]) => {
        const field = fieldsRef.current.find((field) => field.path === path);
        if (!field)
          throw new Error("This setting is not available in the form.");
        return { path, value: scalar(field, text) };
      });
      const result = await api("/configuration/{id}/drafts", "post", {
        path: { id: "albert" },
        body: { content: source, revision, changes },
        signal: abort.signal,
      });
      if (
        !mounted.current ||
        request !== generation.current ||
        latest.current.content !== source ||
        latest.current.document?.revision !== revision ||
        editsRef.current !== desired
      )
        return null;
      if (
        !Array.isArray(result.fields) ||
        typeof result.content !== "string" ||
        result.revision !== revision
      )
        throw new Error(
          "The structured editor is unavailable. Use Raw to edit this document.",
        );
      fieldsRef.current = result.fields;
      lastProjection.current = { content: result.content, revision };
      setProjection(result);
      editsRef.current = {};
      setEdits({});
      if (result.content !== source) current.onChange(result.content);
      return result.content;
    } catch (error) {
      if (mounted.current && request === generation.current)
        setError(errorMessage(error));
      return null;
    } finally {
      clearTimeout(deadline);
      if (mounted.current && request === generation.current) setPending(false);
    }
  }, []);

  useEffect(() => {
    generation.current++;
    controller.current?.abort();
    if (
      !active ||
      !document ||
      document.redacted ||
      document.path !== "albert"
    ) {
      setPending(false);
      return;
    }
    if (
      lastProjection.current?.content === content &&
      lastProjection.current.revision === document.revision
    )
      return;
    setPending(true);
    timer.current = setTimeout(() => void run(), 0);
    return () => clearTimeout(timer.current);
  }, [
    active,
    content,
    document?.revision,
    document?.redacted,
    document?.path,
    run,
  ]);

  const change = useCallback(
    (path: string, value: string) => {
      generation.current++;
      controller.current?.abort();
      clearTimeout(timer.current);
      const desired = { ...editsRef.current, [path]: value };
      editsRef.current = desired;
      setEdits(desired);
      setPending(true);
      setError("");
      latest.current.onEdit();
      timer.current = setTimeout(() => void run(), 300);
    },
    [run],
  );

  const flush = useCallback(
    async () =>
      Object.keys(editsRef.current).length ? run() : latest.current.content,
    [run],
  );
  return {
    fields: projection?.fields ?? [],
    edits,
    change,
    flush,
    refresh: run,
    pending,
    error,
    hasEdits: Object.keys(edits).length > 0,
    clearError: () => setError(""),
  };
}
