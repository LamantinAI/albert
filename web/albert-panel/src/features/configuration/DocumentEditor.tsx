import { ResourceLoading } from "../../components/ResourceLoading";
import { Button, Notice } from "../../components/ui";
import { StructuredFields } from "./StructuredFields";
import type { DocumentEditorProps } from "./useDocumentEditor";
import { useDocumentEditor } from "./useDocumentEditor";
export function DocumentEditor(props: DocumentEditorProps) {
  const {
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
  } = useDocumentEditor(props);
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
