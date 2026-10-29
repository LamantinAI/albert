import { useState } from "react";
import type { FormEvent } from "react";

import { errorMessage } from "../../api/client";
import { Button, Field, Notice } from "../../components/ui";
import type { Model } from "./api";

// A form draft is UI state. The API adapter validates it against generated DTOs.
export interface ModelDraft {
  id: string;
  model: string;
  baseUrl: string;
  secretName: string;
  secretValue: string;
  vision: boolean;
  tools: boolean;
  contextWindow: string;
}

export function ModelForm({
  onSave,
  label = "Add model",
  onCancel,
  initial,
}: {
  onSave: (draft: ModelDraft) => Promise<void>;
  label?: string;
  onCancel?: () => void;
  initial?: Model;
}) {
  const [draft, setDraft] = useState<ModelDraft>({
    id: initial?.config.id ?? "",
    model: initial?.config.model ?? "",
    baseUrl:
      initial?.config.base_url ??
      (initial?.config.provider === "subscription"
        ? ""
        : "https://api.openai.com/v1"),
    secretName: initial?.config.api_key_env ?? "",
    secretValue: "",
    vision: initial?.config.vision ?? true,
    tools: initial?.config.tools ?? true,
    contextWindow: initial?.config.context_window?.toString() ?? "",
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  function update<K extends keyof ModelDraft>(key: K, value: ModelDraft[K]) {
    setDraft((previous) => ({ ...previous, [key]: value }));
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    setError("");
    setBusy(true);
    try {
      await onSave(draft);
      setDraft((previous) => ({ ...previous, secretValue: "" }));
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <form className="form-stack" onSubmit={submit}>
      <div className="form-grid">
        <Field
          label="Model ID"
          disabled={!!initial}
          placeholder="primary"
          value={draft.id}
          required
          maxLength={80}
          pattern="[A-Za-z0-9_-]+"
          hint="Letters, numbers, - or _. This identifies the model in your pool."
          onChange={(event) => update("id", event.target.value)}
        />
        <Field
          label="Provider model ID"
          placeholder="Model ID from your provider"
          value={draft.model}
          required
          onChange={(event) => update("model", event.target.value)}
        />
      </div>
      <Field
        label="OpenAI-compatible API base URL"
        type="url"
        value={draft.baseUrl}
        disabled={initial?.config.provider === "subscription"}
        required={initial?.config.provider !== "subscription"}
        onChange={(event) => update("baseUrl", event.target.value)}
        hint="Use the endpoint supplied by your provider."
      />
      {initial?.config.provider === "subscription" ? (
        <Notice>
          This model uses the subscription managed by the host.{" "}
          {initial.credential_configured
            ? "Its credential is configured."
            : "The host credential needs attention."}
        </Notice>
      ) : (
        <div className="form-grid">
          <Field
            label="Credential name"
            value={draft.secretName}
            placeholder="Generated automatically"
            hint="Optional. Leave blank for a separate credential for this model."
            pattern="[A-Za-z_][A-Za-z0-9_]*"
            onChange={(event) => update("secretName", event.target.value)}
          />
          <Field
            label="API key"
            type="password"
            autoComplete="new-password"
            value={draft.secretValue}
            onChange={(event) => update("secretValue", event.target.value)}
            hint="Write only. Stored values are never sent back to this browser."
          />
        </div>
      )}
      <Field
        label="Context window (tokens)"
        type="number"
        min="1"
        value={draft.contextWindow}
        onChange={(event) => update("contextWindow", event.target.value)}
        hint="Optional. Use the provider's documented model limit."
      />
      <div className="form-actions">
        <label className="toggle-label">
          <input
            type="checkbox"
            checked={draft.tools}
            onChange={(event) => update("tools", event.target.checked)}
          />
          Tool calling
        </label>
        <label className="toggle-label">
          <input
            type="checkbox"
            checked={draft.vision}
            onChange={(event) => update("vision", event.target.checked)}
          />
          Image input
        </label>
      </div>
      {error && <Notice kind="error">{error}</Notice>}
      <div className="form-actions">
        <Button variant="primary" disabled={busy} type="submit">
          {busy ? "Saving…" : label}
        </Button>
        {onCancel && (
          <Button type="button" onClick={onCancel}>
            Cancel
          </Button>
        )}
      </div>
    </form>
  );
}
