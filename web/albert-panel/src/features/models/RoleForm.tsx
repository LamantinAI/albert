import { Select } from "../../components/Select";
import { useId, useState } from "react";
import { errorMessage } from "../../api/client";
import { useUnsavedChanges } from "../../app/UnsavedChanges";
import { Button, Field, Notice } from "../../components/ui";
import type { ModelPool } from "./api";

export type RoleDraft = { id: string; model_id: string; description: string };
type Role = ModelPool["roles"][number];

export function RoleForm({
  pool,
  initial,
  onSave,
  onClose,
}: {
  pool: ModelPool;
  initial?: Role;
  onSave: (draft: RoleDraft) => Promise<void>;
  onClose: () => void;
}) {
  const formId = useId();
  const original = {
    id: initial?.id ?? "",
    model_id:
      initial?.models[0] ?? pool.preferred ?? pool.models[0]?.config.id ?? "",
    description: initial?.description ?? "",
  };
  const [draft, setDraft] = useState<RoleDraft>(original);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const markSaved = useUnsavedChanges(
    JSON.stringify(draft) !== JSON.stringify(original),
    JSON.stringify(draft),
  );
  function change(key: keyof RoleDraft, value: string) {
    setDraft((old) => ({ ...old, [key]: value }));
  }
  return (
    <form
      className="form-stack"
      onSubmit={(event) => {
        event.preventDefault();
        if (busy) return;
        setBusy(true);
        setError("");
        void onSave({ ...draft, id: draft.id.trim() })
          .then(() => {
            markSaved();
            onClose();
          })
          .catch((reason) => setError(errorMessage(reason)))
          .finally(() => setBusy(false));
      }}
    >
      <Field
        label="Role"
        autoFocus
        required
        maxLength={128}
        value={draft.id}
        disabled={busy || !!initial}
        onChange={(event) => change("id", event.target.value)}
        placeholder="researcher"
      />
      <Select
        label="Model"
        value={draft.model_id}
        disabled={busy}
        options={pool.models.map((model) => ({
          value: model.config.id,
          label: model.config.id,
        }))}
        onChange={(value) => change("model_id", value)}
      />

      <div className="field">
        <label htmlFor={`${formId}-description`}>Description</label>
        <textarea
          id={`${formId}-description`}
          rows={3}
          maxLength={16384}
          value={draft.description}
          disabled={busy}
          onChange={(event) => change("description", event.target.value)}
          placeholder="Which tasks should use this model?"
        />
      </div>
      {initial && (initial.models.length > 1 || initial.reasoning_effort) && (
        <Notice>
          Existing fallback models and reasoning effort will be retained. They
          remain available in Advanced settings.
        </Notice>
      )}
      {error && <Notice kind="error">{error}</Notice>}
      <footer className="dialog-actions">
        <Button type="button" onClick={onClose} disabled={busy}>
          Cancel
        </Button>
        <Button
          type="submit"
          variant="primary"
          disabled={busy || !draft.id.trim() || !draft.model_id}
        >
          {busy ? "Saving…" : "Save role"}
        </Button>
      </footer>
    </form>
  );
}
