import { useState } from "react";
import { errorMessage } from "../../../api/client";
import type { components } from "../../../api/schema";
import { useUnsavedChanges } from "../../../app/UnsavedChanges";
import { Button, Field, Notice } from "../../../components/ui";
import { McpTransport } from "./McpTransport";
import { McpTools } from "./McpTools";
type Config = components["schemas"]["McpServerConfig"];
export function McpForm({
  initial,
  revision,
  phase,
  onSave,
  onSaved,
}: {
  initial?: Config;
  revision: string;
  phase: string;
  onSave: (config: Config, revision: string) => Promise<void>;
  onSaved: (config: Config) => void;
}) {
  const [draft, setDraft] = useState<Config>(
    initial ?? {
      id: "",
      name: "",
      description: "",
      enabled: true,
      connection: { transport: "http", url: "", token_env: null },
      allowed_tools: [],
    },
  );
  const [originalRevision] = useState(revision),
    [original] = useState(JSON.stringify(draft));
  const [valid, setValid] = useState(true);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const saved = useUnsavedChanges(
    JSON.stringify(draft) !== original || !valid,
    JSON.stringify(draft),
  );
  return (
    <form
      className="form-stack"
      onSubmit={(event) => {
        event.preventDefault();
        if (busy || !valid) return;
        setBusy(true);
        setError("");
        void onSave(draft, originalRevision)
          .then(() => {
            saved();
            onSaved(draft);
          })
          .catch((reason) => setError(errorMessage(reason)))
          .finally(() => setBusy(false));
      }}
    >
      <div className="form-grid">
        <Field
          label="Name"
          autoFocus
          required
          value={draft.name}
          disabled={busy}
          maxLength={256}
          onChange={(event) => setDraft({ ...draft, name: event.target.value })}
        />
        <Field
          label="Server ID"
          required
          value={draft.id}
          disabled={busy || !!initial}
          maxLength={64}
          pattern="[A-Za-z0-9_-]+"
          onChange={(event) => setDraft({ ...draft, id: event.target.value })}
        />
      </div>
      <Field
        label="Description"
        value={draft.description}
        disabled={busy}
        maxLength={4096}
        onChange={(event) =>
          setDraft({ ...draft, description: event.target.value })
        }
      />
      <label className="toggle-label">
        <input
          type="checkbox"
          checked={draft.enabled}
          disabled={busy}
          onChange={(event) =>
            setDraft({ ...draft, enabled: event.target.checked })
          }
        />
        Enabled
      </label>
      <McpTransport
        value={draft.connection}
        onValidity={setValid}
        disabled={busy}
        onChange={(connection) =>
          setDraft({ ...draft, connection, allowed_tools: [] })
        }
      />
      {initial &&
        JSON.stringify(initial.connection) !==
          JSON.stringify(draft.connection) && (
          <Notice>
            Save connection changes before reviewing access to its tools.
          </Notice>
        )}
      {initial &&
        JSON.stringify(initial.connection) ===
          JSON.stringify(draft.connection) && (
          <McpTools
            id={initial.id}
            phase={phase}
            selected={draft.allowed_tools}
            disabled={busy}
            onChange={(allowed_tools) => setDraft({ ...draft, allowed_tools })}
          />
        )}
      {error && <Notice kind="error">{error}</Notice>}
      <div className="form-actions">
        <Button type="submit" variant="primary" disabled={busy || !valid}>
          {busy ? "Saving…" : initial ? "Save changes" : "Connect server"}
        </Button>
      </div>
    </form>
  );
}
