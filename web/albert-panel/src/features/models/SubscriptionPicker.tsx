import type { components } from "../../api/schema";
import { Icon } from "../../components/Icon";
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
        <p className="subscription-connection-status">ChatGPT connected</p>
        <div className="subscription-catalog-actions">
          <Button
            variant="ghost"
            onClick={onRetry}
            disabled={busy}
            aria-label={error ? "Retry loading models" : "Refresh models"}
          >
            {error ? "Retry loading models" : "Refresh"}
          </Button>
          <Button
            variant="ghost"
            onClick={onReconnect}
            disabled={busy}
            aria-label="Reconnect OpenAI subscription"
          >
            Reconnect
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
                    <span className="subscription-unavailable">
                      {model.unavailable_reason}
                    </span>
                  )}
                </span>
              </label>
              <details className="subscription-model-details">
                <summary aria-label={`${model.name} details`}>
                  <Icon name="chevron" size={16} />
                  Details
                </summary>
                <dl>
                  <div>
                    <dt>Model ID</dt>
                    <dd>
                      <code>{model.slug}</code>
                    </dd>
                  </div>
                  {model.context_window && (
                    <div>
                      <dt>Context</dt>
                      <dd>{model.context_window.toLocaleString()} tokens</dd>
                    </div>
                  )}
                  {model.input_modalities.length > 0 && (
                    <div>
                      <dt>Input</dt>
                      <dd>{model.input_modalities.join(", ")}</dd>
                    </div>
                  )}
                  {model.reasoning_efforts.length > 0 && (
                    <div>
                      <dt>Reasoning</dt>
                      <dd>{model.reasoning_efforts.join(", ")}</dd>
                    </div>
                  )}
                </dl>
              </details>
            </div>
          ))}
        </fieldset>
      )}
    </section>
  );
}
