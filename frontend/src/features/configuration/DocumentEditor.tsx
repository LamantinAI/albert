import { useEffect, useRef, useState } from "react";

import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { useClientContext } from "../../app/context";
import { Button, Notice } from "../../components/ui";

type Document = components["schemas"]["ConfigDocument"];
type Validation = components["schemas"]["ValidationView"];
type Application = components["schemas"]["ApplyView"];

export function DocumentEditor({
  id,
  kind,
}: {
  id: string;
  kind: "configuration" | "prompts";
}) {
  const { refresh } = useClientContext();
  const [document, setDocument] = useState<Document | null>(null);
  const [content, setContent] = useState("");
  const [validation, setValidation] = useState<Validation | null>(null);
  const [application, setApplication] = useState<Application | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const validated = useRef<string | null>(null);
  const applicationKey = useRef<{
    id: string;
    target: string;
    revision: string;
    documentId: string;
  } | null>(null);
  const dirty = document !== null && content !== document.content;
  useEffect(() => {
    let current = true;
    const request =
      kind === "configuration"
        ? api("/configuration/{id}", "get", { path: { id } })
        : api("/prompts/{id}", "get", { path: { id } });
    void request
      .then((value) => {
        if (current) {
          setDocument(value);
          setContent(value.content);
        }
      })
      .catch((error) => {
        if (current) setError(errorMessage(error));
      });
    return () => {
      current = false;
    };
  }, [id, kind]);
  useEffect(() => {
    if (!dirty) return;
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirty]);
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
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const result = await api("/configuration-validations", "post", {
        body: { path: document.path, content, revision: document.revision },
      });
      setValidation(result);
      validated.current = result.valid ? content : null;
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    if (!document || validated.current !== content) return;
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
      setDocument(result);
      setContent(result.content);
      setValidation(null);
      validated.current = null;
      setApplication(null);
      applicationKey.current = null;
      setNotice(
        "Saved on disk. Apply the saved revision to update the runtime.",
      );
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function apply(target = "runtime", retry = false) {
    if (!document || dirty) return;
    if (
      target === "process" &&
      !window.confirm(
        "Restart the Albert process? Active work and this connection will be interrupted. Unsaved temporary chats do not survive a restart.",
      )
    )
      return;
    if (!retry || !applicationKey.current)
      applicationKey.current = {
        id: crypto.randomUUID(),
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
    <section className="card">
      <div className="card-heading">
        <h2>
          {id === "soul"
            ? "Persona"
            : id === "system"
              ? "Operating instructions"
              : "Document"}
        </h2>
        {document && (
          <span className="badge">
            {dirty ? "Unsaved changes" : "Saved revision"}
          </span>
        )}
      </div>
      {error && <Notice kind="error">{error}</Notice>}
      {notice && <Notice kind="success">{notice}</Notice>}
      {document ? (
        <div className="form-stack">
          <label className="field">
            <span className="field-label">{document.path}</span>
            <textarea
              className="editor"
              value={content}
              readOnly={document.redacted}
              spellCheck={false}
              onChange={(event) => {
                setContent(event.target.value);
                setValidation(null);
                validated.current = null;
                setNotice("");
              }}
            />
          </label>
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
              disabled={busy || document.redacted || !dirty}
              onClick={() => void validate()}
            >
              {busy ? "Working…" : "Validate changes"}
            </Button>
            <Button
              variant="primary"
              disabled={
                busy ||
                !dirty ||
                !validation?.valid ||
                validated.current !== content
              }
              onClick={() => void save()}
            >
              Save
            </Button>
            <Button disabled={busy || dirty} onClick={() => void apply()}>
              Apply saved configuration
            </Button>
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
          {(error || application) && applicationKey.current && (
            <Button disabled={busy} onClick={() => void checkApplication()}>
              Check application outcome
            </Button>
          )}
          {application?.state === "restart_required" && (
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
      ) : (
        <p className="muted">Loading document…</p>
      )}
    </section>
  );
}
