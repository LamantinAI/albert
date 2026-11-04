import type { components } from "../../api/schema";
import { Button, Notice } from "../../components/ui";

type Catalog = components["schemas"]["SubscriptionCatalogView"];
export function SubscriptionPicker({
  catalog,
  selected,
  busy,
  error,
  onChange,
  onRetry,
  onReconnect,
}: {
  catalog: Catalog | null;
  selected: string;
  busy: boolean;
  error: string;
  onChange: (slug: string) => void;
  onRetry: () => void;
  onReconnect: () => void;
}) {
  return (
    <section
      className="subscription-picker"
      aria-label="Subscription model selection"
    >
      <div className="subscription-picker-toolbar">
        <p className="subscription-secondary">ChatGPT subscription connected</p>
        <div>
          <Button variant="ghost" onClick={onRetry} disabled={busy}>
            {error ? "Retry loading models" : "Refresh models"}
          </Button>
          <Button variant="ghost" onClick={onReconnect} disabled={busy}>
            Reconnect OpenAI subscription
          </Button>
        </div>
      </div>
      {error && <Notice kind="error">{error}</Notice>}
      {!catalog && !error && (
        <p role="status" className="subscription-secondary">
          Loading your available models…
        </p>
      )}
      {catalog?.models.length === 0 && (
        <Notice>
          OpenAI returned no models available for this account. Your credentials
          remain connected; retry the catalog later.
        </Notice>
      )}
      {catalog && catalog.models.length > 0 && (
        <fieldset
          className="subscription-model-list"
          role="radiogroup"
          aria-label="Available subscription models"
          disabled={busy}
        >
          <legend className="subscription-visually-hidden">
            Available subscription models
          </legend>
          {catalog.models.map((model) => (
            <div
              key={model.slug}
              className="subscription-choice"
              data-selected={selected === model.slug || undefined}
              data-disabled={!model.selectable || undefined}
            >
              <label className="subscription-choice-row">
                <input
                  type="radio"
                  name="subscription-model"
                  aria-label={model.name}
                  value={model.slug}
                  checked={selected === model.slug}
                  disabled={!model.selectable}
                  onChange={() => onChange(model.slug)}
                />
                <span className="subscription-choice-copy">
                  <strong>{model.name}</strong>
                  {model.description && <span>{model.description}</span>}
                  {model.unavailable_reason && (
                    <span className="subscription-secondary">
                      {model.unavailable_reason}
                    </span>
                  )}
                </span>
              </label>
              <details className="subscription-model-details">
                <summary>Model details</summary>
                <div>
                  <code>{model.slug}</code>
                  {model.context_window && (
                    <span>
                      {model.context_window.toLocaleString()} token context
                    </span>
                  )}
                  {model.input_modalities.length > 0 && (
                    <span>Input: {model.input_modalities.join(", ")}</span>
                  )}
                  {model.reasoning_efforts.length > 0 && (
                    <span>
                      Reasoning effort: {model.reasoning_efforts.join(", ")}
                    </span>
                  )}
                </div>
              </details>
            </div>
          ))}
        </fieldset>
      )}
    </section>
  );
}
