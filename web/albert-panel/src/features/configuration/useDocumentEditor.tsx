import { useEffect, useRef, useState } from "react";
import { api, errorMessage, requestId } from "../../api/client";
import type { components } from "../../api/schema";
import { useClientContext } from "../../app/context";
import { useUnsavedChanges } from "../../app/UnsavedChanges";
import { useDialogs } from "../../components/useDialogs";
import { useApplicationPolling } from "./useApplicationPolling";
import { useStructuredDraft } from "./useStructuredDraft";
type Document = components["schemas"]["ConfigDocument"];
type Validation = components["schemas"]["ValidationView"];
type Application = components["schemas"]["ApplyView"];
export type DocumentEditorProps = {
  id: string;
  kind: "configuration" | "prompts";
  initialContent?: string;
  preferFields?: boolean;
  onSaved?: () => void;
};
export function useDocumentEditor({
  id,
  kind,
  initialContent = "",
  preferFields = true,
  onSaved,
}: DocumentEditorProps) {
  const { refresh } = useClientContext();
  const { confirm, dialog } = useDialogs(id);
  const [document, setDocument] = useState<Document | null>(null);
  const [content, setContent] = useState("");
  const [mode, setMode] = useState<"fields" | "raw">(
    kind === "configuration" &&
      preferFields &&
      (id === "albert" || id.startsWith("connector:"))
      ? "fields"
      : "raw",
  );
  const [validation, setValidation] = useState<Validation | null>(null);
  const [application, setApplication] = useState<Application | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [reload, setReload] = useState(0);
  const [notice, setNotice] = useState("");
  const validated = useRef<string | null>(null);
  const applicationKey = useRef<{
    id: string;
    target: string;
    revision: string;
    documentId: string;
  } | null>(null);
  const structured =
    kind === "configuration" &&
    preferFields &&
    (id === "albert" || id.startsWith("connector:"));
  function invalidate() {
    setValidation(null);
    validated.current = null;
    setNotice("");
  }
  function changeContent(text: string) {
    setContent(text);
    invalidate();
  }
  const draft = useStructuredDraft(
    document,
    content,
    structured && mode === "fields",
    changeContent,
    invalidate,
  );
  useEffect(() => {
    if (document?.redacted) setMode("raw");
  }, [document?.redacted]);
  const dirty =
    document !== null && (content !== document.content || draft.hasEdits);
  const markSaved = useUnsavedChanges(
    dirty,
    JSON.stringify([content, draft.edits]),
  );
  useEffect(() => {
    if (
      structured &&
      mode === "fields" &&
      draft.error &&
      !draft.pending &&
      !draft.fields.length &&
      !draft.hasEdits
    )
      setMode("raw");
  }, [
    structured,
    mode,
    draft.error,
    draft.pending,
    draft.fields.length,
    draft.hasEdits,
  ]);
  async function switchMode(next: "fields" | "raw") {
    if (next === mode) {
      if (next === "raw") draft.clearError();
      return;
    }
    const result = next === "raw" ? await draft.flush() : await draft.refresh();
    if (result !== null) {
      setMode(next);
      draft.clearError();
    }
  }
  useEffect(() => {
    let current = true;
    setLoading(true);
    setError("");
    const request =
      kind === "configuration"
        ? api("/configuration/{id}", "get", { path: { id } })
        : api("/prompts/{id}", "get", { path: { id } });
    void request
      .then((value) => {
        if (current) {
          setDocument(value);
          setContent(value.content || initialContent);
        }
      })
      .catch((error) => {
        if (current) setError(errorMessage(error));
      })
      .finally(() => {
        if (current) setLoading(false);
      });
    return () => {
      current = false;
    };
  }, [id, kind, initialContent, reload]);
  useApplicationPolling(application, setApplication, setError);

  async function checkApplication() {
    if (application?.id) {
      setBusy(true);
      try {
        setApplication(
          await api("/configuration/applications/{id}", "get", {
            path: { id: application.id },
          }),
        );
        setError("");
      } catch (error) {
        setError(errorMessage(error));
      } finally {
        setBusy(false);
      }
    } else await apply("runtime", true);
  }

  async function validate() {
    if (!document) return;
    const candidate = structured ? await draft.flush() : content;
    if (candidate === null) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const result = await api("/configuration-validations", "post", {
        body: {
          path: document.path,
          content: candidate,
          revision: document.revision,
        },
      });
      setValidation(result);
      validated.current = result.valid ? candidate : null;
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    if (
      !document ||
      draft.hasEdits ||
      draft.pending ||
      validated.current !== content
    )
      return;
    setBusy(true);
    setError("");
    setNotice("");
    const options = {
      path: { id },
      revision: document.revision,
      body: { path: document.path, content, revision: document.revision },
    };
    try {
      const result =
        kind === "configuration"
          ? await api("/configuration/{id}", "put", options)
          : await api("/prompts/{id}", "put", options);
      markSaved();
      setDocument(result);
      setContent(result.content);
      onSaved?.();
      setValidation(null);
      validated.current = null;
      setApplication(null);
      applicationKey.current = null;
      setNotice(
        kind === "prompts"
          ? "Saved. New turns use these instructions."
          : "Saved on disk. Apply the saved revision to update the runtime.",
      );
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function apply(target = "runtime", retry = false) {
    if (!document || dirty || draft.pending) return;
    if (
      target === "process" &&
      !(await confirm({
        title: "Restart Albert?",
        description:
          "Active work and this connection will be interrupted. Unsaved temporary chats do not survive a restart.",
        confirmLabel: "Restart Albert",
        danger: true,
      }))
    )
      return;
    if (!retry || !applicationKey.current)
      applicationKey.current = {
        id: requestId(),
        target,
        revision: document.revision,
        documentId: id,
      };
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const result = await api("/configuration/applications", "post", {
        idempotencyKey: applicationKey.current.id,
        body: {
          target: applicationKey.current.target,
          revision: applicationKey.current.revision,
          client_request_id: applicationKey.current.id,
          document_id: applicationKey.current.documentId,
        },
      });
      setApplication(result);
      await refresh().catch(() =>
        setNotice(
          "The application was acknowledged. Refresh the workspace to update its client context.",
        ),
      );
    } catch (error) {
      setError(
        `${errorMessage(error)} The application outcome may be unknown. Check the same request before starting another.`,
      );
    } finally {
      setBusy(false);
    }
  }

  return {
    id,
    kind,
    dialog,
    document,
    content,
    mode,
    validation,
    application,
    busy,
    error,
    loading,
    setReload,
    notice,
    validated,
    applicationKey,
    structured,
    changeContent,
    draft,
    dirty,
    switchMode,
    checkApplication,
    validate,
    save,
    apply,
  };
}
