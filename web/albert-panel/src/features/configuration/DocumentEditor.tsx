import { useEffect, useRef, useState } from "react";

import { api, errorMessage, requestId } from "../../api/client";
import type { components } from "../../api/schema";
import { useClientContext } from "../../app/context";
import { useUnsavedChanges } from "../../app/UnsavedChanges";
import { ResourceLoading } from "../../components/ResourceLoading";
import { Button, Notice } from "../../components/ui";
import { useDialogs } from "../../components/useDialogs";
import { StructuredFields } from "./StructuredFields";
import { useStructuredDraft } from "./useStructuredDraft";

type Document = components["schemas"]["ConfigDocument"];
type Validation = components["schemas"]["ValidationView"];
type Application = components["schemas"]["ApplyView"];

export function DocumentEditor({
  id,
  kind,
  initialContent = "",
  preferFields = true,
  onSaved,
}: {
  id: string;
  kind: "configuration" | "prompts";
  initialContent?: string;
  preferFields?: boolean;
  onSaved?: () => void;
}) {
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
  useEffect(() => {
    if (
      !application ||
      !["pending", "accepted", "applying", "running", "restarting"].includes(
        application.state,
      )
    )
      return;
    let stopped = false;
    let count = 0;
    const timer = window.setInterval(() => {
      if (++count > 30) {
        window.clearInterval(timer);
        return;
      }
      void api("/configuration/applications/{id}", "get", {
        path: { id: application.id },
      })
        .then((value) => {
          if (!stopped) setApplication(value);
        })
        .catch((error) => {
          if (!stopped) setError(errorMessage(error));
        });
    }, 1000);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [application?.id, application?.state]);

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
  return (
    <section className="card config-editor">
      {dialog}
      <div className="card-heading">
        {structured ? (
          <div
            className="view-switch"
            role="group"
            aria-label="Configuration view"
          >
            <Button
              variant="ghost"
              aria-pressed={mode === "fields"}
              disabled={busy || loading || !document || document.redacted}
              onClick={() => void switchMode("fields")}
            >
              Fields
            </Button>
            <Button
              variant="ghost"
              aria-pressed={mode === "raw"}
              disabled={busy || loading || !document}
              onClick={() => void switchMode("raw")}
            >
              Raw
            </Button>
          </div>
        ) : kind === "prompts" ? (
          <span className="muted">Instructions</span>
        ) : (
          <h2>Document</h2>
        )}
        {document && (
          <span className="badge">
            {dirty ? "Unsaved changes" : "Saved revision"}
          </span>
        )}
      </div>
      {error && <Notice kind="error">{error}</Notice>}
      {notice && <Notice kind="success">{notice}</Notice>}
      {draft.error && <Notice kind="error">{draft.error}</Notice>}
      {document ? (
        <div className="form-stack">
          {structured && mode === "fields" && !document.redacted ? (
            draft.fields.length ? (
              <StructuredFields
                fields={draft.fields}
                edits={draft.edits}
                disabled={busy}
                onChange={draft.change}
              />
            ) : (
              <p className="muted">Loading settings…</p>
            )
          ) : (
            <label className="field">
              <span className={kind === "prompts" ? "sr-only" : "field-label"}>
                {kind === "prompts"
                  ? id === "soul"
                    ? "Persona instructions"
                    : "Operating instructions"
                  : document.path}
              </span>
              <textarea
                className="editor"
                disabled={busy}
                value={content}
                readOnly={document.redacted}
                spellCheck={false}
                onChange={(event) => {
                  changeContent(event.target.value);
                  draft.clearError();
                }}
              />
            </label>
          )}
          {draft.pending && (
            <small className="muted" role="status">
              Updating draft…
            </small>
          )}
          {document.redacted && (
            <Notice>
              This document contains redacted values. Use the model and
              credential controls to change secrets without replacing hidden
              values.
            </Notice>
          )}
          {validation && (
            <Notice kind={validation.valid ? "success" : "error"}>
              {validation.valid
                ? validation.requires_restart
                  ? "Valid. This change requires a restart after saving."
                  : "Valid. Save this exact revision to continue."
                : validation.issues.join(" ")}
            </Notice>
          )}
          <div className="form-actions">
            <Button
              disabled={busy || draft.pending || document.redacted || !dirty}
              onClick={() => void validate()}
            >
              {busy ? "Working…" : "Validate changes"}
            </Button>
            <Button
              variant="primary"
              disabled={
                busy ||
                draft.pending ||
                draft.hasEdits ||
                !dirty ||
                !validation?.valid ||
                validated.current !== content
              }
              onClick={() => void save()}
            >
              Save
            </Button>
            {kind === "configuration" && (
              <Button
                disabled={busy || dirty || draft.pending}
                onClick={() => void apply()}
              >
                Apply saved configuration
              </Button>
            )}
          </div>
          {application && (
            <Notice
              kind={
                application.state === "failed"
                  ? "error"
                  : application.state === "applied"
                    ? "success"
                    : "info"
              }
            >
              <strong>{application.state.replaceAll("_", " ")}</strong> ·{" "}
              {application.message}
            </Notice>
          )}
          {kind === "configuration" &&
            (error || application) &&
            applicationKey.current && (
              <Button disabled={busy} onClick={() => void checkApplication()}>
                Check application outcome
              </Button>
            )}
          {kind === "configuration" &&
            application?.state === "restart_required" && (
              <Button
                variant="danger"
                disabled={busy}
                onClick={() => void apply("process")}
              >
                Restart Albert process
              </Button>
            )}
          <small className="muted">
            {kind === "prompts"
              ? "Saved prompt files are reloaded for new turns. Active attempts keep their existing instructions."
              : "Runtime application preserves the existing snapshots of active work. Topology changes may require a process restart."}
          </small>
        </div>
      ) : loading ? (
        <ResourceLoading label="Loading document…" />
      ) : (
        <Button onClick={() => setReload((value) => value + 1)}>
          Retry loading document
        </Button>
      )}
    </section>
  );
}
