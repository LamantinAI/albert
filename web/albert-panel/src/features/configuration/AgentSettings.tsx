import { useEffect } from "react";
import { useLocation } from "react-router-dom";
import { useUnsavedChanges } from "../../app/UnsavedChanges";
import { ResourceLoading } from "../../components/ResourceLoading";
import { Button, Notice } from "../../components/ui";
import { AgentActions } from "./AgentActions";
import { MemoryFields } from "../memory/MemoryFields";
import { ConfigGroup } from "./ConfigGroup";
import { AgentFields } from "./AgentFields";
import { useAgentApplication } from "./useAgentApplication";
import { useAgentDocument } from "./useAgentDocument";
import { useAgentMemory } from "./useAgentMemory";
import { useAgentSave } from "./useAgentSave";
const memoryPhases = {
  selection_pending: "Choose memory",
  saved: "Saved",
  initializing: "Initializing",
  active: "Initialized",
  restart_required: "Restart required",
  initialization_failed: "Needs attention",
};
export function AgentSettings({
  initialMode = "fields",
}: {
  initialMode?: "fields" | "raw";
}) {
  const editor = useAgentDocument(initialMode);
  const memory = useAgentMemory();
  const save = useAgentSave(editor, memory);
  const application = useAgentApplication();
  const { hash } = useLocation();
  const dirty = editor.dirty || memory.dirty;
  const busy = save.busy || application.busy || application.unresolved;
  useUnsavedChanges(
    dirty,
    JSON.stringify([
      editor.content,
      editor.draft.edits,
      memory.config,
      memory.environment,
      memory.timeout,
      memory.tokenRevision,
    ]),
  );
  useEffect(() => {
    if (application.application?.state === "applied") save.applied();
  }, [application.application?.state]);
  useEffect(() => {
    application.reset();
  }, [save.savedRevision?.revision, save.savedRevision?.document_id]);
  const revision =
    save.savedRevision ??
    (editor.document
      ? { revision: editor.document.revision, document_id: "albert" }
      : null);
  return (
    <section className="card config-editor agent-settings">
      {application.dialog}
      <div className="card-heading">
        <div
          className="view-switch"
          role="group"
          aria-label="Configuration view"
        >
          <Button
            variant="ghost"
            aria-pressed={editor.mode === "fields"}
            disabled={busy || !editor.document || editor.document.redacted}
            onClick={() => void editor.switchMode("fields")}
          >
            Fields
          </Button>
          <Button
            variant="ghost"
            aria-pressed={editor.mode === "raw"}
            disabled={busy || !editor.document}
            onClick={() => void editor.switchMode("raw")}
          >
            Raw
          </Button>
        </div>
        <span className="settings-save-state" role="status">
          {save.busy
            ? "Saving…"
            : application.busy
              ? "Checking restart…"
              : application.unresolved
                ? application.error
                  ? "Restart needs review"
                  : "Restarting…"
                : save.uncertain
                  ? "Save needs review"
                  : dirty
                    ? "Unsaved changes"
                    : save.restartRequired
                      ? "Saved · restart to apply"
                      : "Saved configuration"}
        </span>
      </div>
      {editor.error && <Notice kind="error">{editor.error}</Notice>}
      {save.error && (
        <Notice kind="error">
          {save.error}
          {save.uncertain &&
            " The save outcome needs review. Your draft is kept; nothing will be retried automatically."}
        </Notice>
      )}
      {editor.draft.error && <Notice kind="error">{editor.draft.error}</Notice>}
      {save.notice && <Notice>{save.notice}</Notice>}
      {!editor.document ? (
        editor.loading ? (
          <ResourceLoading label="Loading Agent settings…" />
        ) : (
          <Button onClick={() => void editor.reload()}>Reload settings</Button>
        )
      ) : (
        <form
          noValidate
          className="form-stack"
          onSubmit={(event) => {
            event.preventDefault();
            if (!busy) void save.save();
          }}
        >
          {editor.mode === "fields" && !editor.document.redacted ? (
            <AgentFields
              fields={editor.draft.fields}
              edits={editor.draft.edits}
              disabled={busy || editor.draft.resetting}
              version={editor.draft.resetVersion}
              onChange={editor.draft.change}
            >
              <ConfigGroup
                id="memory"
                title="Memory"
                initiallyOpen={hash === "#memory"}
                metadata={
                  memory.dirty
                    ? "Unsaved"
                    : memory.base
                      ? memoryPhases[memory.base.phase]
                      : memory.loading
                        ? "Loading…"
                        : "Unavailable"
                }
              >
                <div className="config-wide-field agent-memory">
                  {memory.error && <Notice kind="error">{memory.error}</Notice>}
                  {memory.base?.error && (
                    <Notice kind="error">{memory.base.error}</Notice>
                  )}
                  {memory.config ? (
                    <MemoryFields state={memory} disabled={busy} />
                  ) : memory.loading ? (
                    <ResourceLoading label="Loading memory…" />
                  ) : (
                    <Button
                      onClick={() =>
                        void memory
                          .read()
                          .then(memory.accept)
                          .catch(() => {})
                      }
                    >
                      Reload memory settings
                    </Button>
                  )}
                </div>
              </ConfigGroup>
            </AgentFields>
          ) : (
            <>
              {memory.dirty && (
                <Notice>
                  Your Memory changes are kept separately. Avoid editing
                  [memory] here at the same time.
                </Notice>
              )}
              <label className="field">
                <span>Agent configuration</span>
                <textarea
                  className="editor"
                  value={editor.content}
                  spellCheck={false}
                  disabled={busy}
                  readOnly={editor.document.redacted}
                  onChange={(event) => editor.setContent(event.target.value)}
                />
              </label>
              {editor.document.redacted && (
                <Notice>
                  Inline credentials are hidden. Use credential references
                  before editing this document.
                </Notice>
              )}
            </>
          )}
          <AgentActions
            save={save}
            application={application}
            dirty={dirty}
            memoryValid={!memory.dirty || memory.valid}
            memoryNeedsRetry={memory.base?.phase === "initialization_failed"}
            selectionRequired={!!memory.base?.selection_required}
            revision={revision}
            busy={busy}
          />
        </form>
      )}
    </section>
  );
}
